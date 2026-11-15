//! Milestone 7: DXO's on-screen keyboard, driven by the controller and typing
//! into whatever app is in front.
//!
//! This file is the keyboard itself: the layout, where the highlight is, and
//! what each pad button does. It never touches Windows. The worker that owns
//! the keyboard feeds it the RAW pad state and sends whatever it returns as
//! real keystrokes; the UI only draws what `view()` and `layout_json()` say.
//! Running here (not in the UI) means presses are read at the pad's full
//! report rate and nothing is missed while the window is still appearing.
//!
//! Pad shortcuts (the Xbox / Windows gamepad-keyboard habits):
//!   D-pad / left stick  move the highlight      Cross / A      press the key
//!   Square / X          Backspace               Triangle / Y   Space
//!   Options / Menu      Enter                   Circle / B     close
//!   L1 / R1             move the text cursor    L3             Shift (twice = Caps)
//!   L2                  symbols
//! PS is the chord modifier: while it is held the keyboard ignores the pad.

use crate::state::{Buttons, PadState};
use std::time::{Duration, Instant};

pub const VK_BACK: u16 = 0x08;
pub const VK_TAB: u16 = 0x09;
pub const VK_RETURN: u16 = 0x0D;
pub const VK_ESCAPE: u16 = 0x1B;
pub const VK_SPACE: u16 = 0x20;
pub const VK_LEFT: u16 = 0x25;
pub const VK_RIGHT: u16 = 0x27;

/// Columns in every row (keys may span more than one).
pub const COLS: u8 = 11;
pub const ROWS: u8 = 5;

/// L2 is analogue: it counts as "pressed" past ON and "released" below OFF.
const L2_ON: f32 = 0.6;
const L2_OFF: f32 = 0.3;
/// Pseudo button bit for L2 inside this module (the parsers don't set L2_DIGITAL).
const L2_BIT: u32 = 1 << 31;

const STICK_ON: f32 = 0.6;
const STICK_OFF: f32 = 0.4;
const MOVE_FIRST: Duration = Duration::from_millis(350);
const MOVE_REPEAT: Duration = Duration::from_millis(110);
const KEY_FIRST: Duration = Duration::from_millis(420);
const KEY_REPEAT: Duration = Duration::from_millis(70);

/// What the worker sends to Windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OskOut {
    /// A character, typed as text (independent of the PC's keyboard layout).
    Char(char),
    /// A key by virtual-key code (Enter, Backspace, arrows...), pressed and released.
    Key(u16),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyKind {
    Char(char),
    Vk(u16),
    Shift,
    Symbols,
    Close,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key {
    pub label: String,
    pub kind: KeyKind,
    /// First column and width, in columns.
    pub col: u8,
    pub span: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Shift {
    #[default]
    Off,
    /// The next letter only.
    Once,
    /// Caps lock.
    Lock,
}

/// What the UI needs to draw the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OskView {
    pub open: bool,
    /// 0 lower case, 1 upper case, 2 symbols. The key labels change with it.
    pub layer: u8,
    /// 0 off, 1 next letter, 2 caps lock.
    pub shift: u8,
    pub row: u8,
    /// Index of the highlighted key within its row.
    pub key: u8,
    /// Counts every key typed, so the UI can flash the key that was pressed.
    pub presses: u32,
}

const LETTERS: [&str; 4] = ["1234567890", "qwertyuiop", "asdfghjkl'", "zxcvbnm,.?"];
const SYMBOLS: [&str; 4] = ["1234567890", "!@#$%^&*()", "-_=+[]{};:", "/\\|\"<>~`,."];

/// The key grid for one layer. Every layer has the same shape, so the
/// highlight stays where it is when Shift or symbols change the labels.
pub fn layout(symbols: bool, upper: bool) -> Vec<Vec<Key>> {
    let table = if symbols { &SYMBOLS } else { &LETTERS };
    let side = [("\u{232B}", VK_BACK), ("Enter", VK_RETURN), ("Tab", VK_TAB), ("Esc", VK_ESCAPE)];
    let mut rows: Vec<Vec<Key>> = Vec::with_capacity(ROWS as usize);
    for (r, chars) in table.iter().enumerate() {
        let mut row: Vec<Key> = chars
            .chars()
            .enumerate()
            .map(|(i, c)| {
                let c = if upper && !symbols { c.to_ascii_uppercase() } else { c };
                Key { label: c.to_string(), kind: KeyKind::Char(c), col: i as u8, span: 1 }
            })
            .collect();
        let (label, vk) = side[r];
        row.push(Key { label: label.into(), kind: KeyKind::Vk(vk), col: 10, span: 1 });
        rows.push(row);
    }
    rows.push(vec![
        Key { label: "\u{21E7}".into(), kind: KeyKind::Shift, col: 0, span: 2 },
        Key { label: if symbols { "abc".into() } else { "&123".into() }, kind: KeyKind::Symbols, col: 2, span: 2 },
        Key { label: "Space".into(), kind: KeyKind::Vk(VK_SPACE), col: 4, span: 4 },
        Key { label: "\u{2190}".into(), kind: KeyKind::Vk(VK_LEFT), col: 8, span: 1 },
        Key { label: "\u{2192}".into(), kind: KeyKind::Vk(VK_RIGHT), col: 9, span: 1 },
        Key { label: "\u{2715}".into(), kind: KeyKind::Close, col: 10, span: 1 },
    ]);
    rows
}

/// Keys worth repeating while held.
fn repeats(vk: u16) -> bool {
    matches!(vk, VK_BACK | VK_LEFT | VK_RIGHT | VK_SPACE)
}

#[derive(Debug)]
pub struct Osk {
    open: bool,
    symbols: bool,
    shift: Shift,
    row: u8,
    key: u8,
    /// The column the player is "in": kept through vertical moves so going
    /// down onto Space and back up returns to the same letter.
    col: u8,
    presses: u32,
    /// Buttons last step (plus L2_BIT), for edge detection.
    prev: u32,
    l2_down: bool,
    dir: u8,
    dir_next: Option<Instant>,
    /// A key repeating while its button is held: (what, the button, next time).
    held: Option<(OskOut, u32, Instant)>,
}

impl Default for Osk {
    fn default() -> Self {
        Osk {
            open: false,
            symbols: false,
            shift: Shift::Off,
            row: 1,
            key: 0,
            col: 0,
            presses: 0,
            prev: u32::MAX,
            l2_down: false,
            dir: 0,
            dir_next: None,
            held: None,
        }
    }
}

impl Osk {
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Show the keyboard. Buttons already held (the ones that opened it) do
    /// nothing until they are let go and pressed again.
    pub fn open(&mut self) {
        if self.open {
            return;
        }
        self.open = true;
        self.symbols = false;
        self.shift = Shift::Off;
        self.prev = u32::MAX;
        self.l2_down = true;
        self.dir = 0;
        self.dir_next = None;
        self.held = None;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.held = None;
        self.dir = 0;
    }

    pub fn view(&self) -> OskView {
        OskView {
            open: self.open,
            layer: if self.symbols { 2 } else if self.shift != Shift::Off { 1 } else { 0 },
            shift: match self.shift {
                Shift::Off => 0,
                Shift::Once => 1,
                Shift::Lock => 2,
            },
            row: self.row,
            key: self.key,
            presses: self.presses,
        }
    }

    fn rows(&self) -> Vec<Vec<Key>> {
        layout(self.symbols, self.shift != Shift::Off)
    }

    /// The current layer as JSON for the UI:
    /// `{"cols":11,"rows":[[{"label":"q","col":0,"span":1,"kind":"char"}, ...], ...]}`.
    /// kind: "char", "key" (Enter, Backspace...), "shift", "symbols", "close".
    pub fn layout_json(&self) -> String {
        let rows: Vec<Vec<serde_json::Value>> = self
            .rows()
            .iter()
            .map(|row| {
                row.iter()
                    .map(|k| {
                        let kind = match k.kind {
                            KeyKind::Char(_) => "char",
                            KeyKind::Vk(_) => "key",
                            KeyKind::Shift => "shift",
                            KeyKind::Symbols => "symbols",
                            KeyKind::Close => "close",
                        };
                        serde_json::json!({ "label": k.label, "col": k.col, "span": k.span, "kind": kind })
                    })
                    .collect()
            })
            .collect();
        serde_json::json!({ "cols": COLS, "rows": rows }).to_string()
    }

    /// Feed one RAW pad report. Returns what to type, in order.
    pub fn step(&mut self, raw: &PadState, now: Instant) -> Vec<OskOut> {
        let mut out = Vec::new();
        if !self.open {
            return out;
        }
        // L2 as a button, with hysteresis so a half-held trigger doesn't chatter.
        self.l2_down = if self.l2_down { raw.l2 > L2_OFF } else { raw.l2 >= L2_ON };
        let b = raw.buttons.0 & !L2_BIT | if self.l2_down { L2_BIT } else { 0 };

        // PS held: the chord layer owns the pad (PS + D-pad is volume, not typing).
        if b & Buttons::HOME != 0 {
            self.prev = b;
            self.dir = 0;
            self.held = None;
            return out;
        }
        let prev = self.prev;
        let edge = |bit: u32| b & bit != 0 && prev & bit == 0;

        // ---- move the highlight: D-pad first, then the left stick ----
        let mut dir = 0u8;
        if b & Buttons::DPAD_UP != 0 {
            dir = 1;
        } else if b & Buttons::DPAD_DOWN != 0 {
            dir = 2;
        } else if b & Buttons::DPAD_LEFT != 0 {
            dir = 3;
        } else if b & Buttons::DPAD_RIGHT != 0 {
            dir = 4;
        } else if prev != u32::MAX {
            let gate = if self.dir == 0 { STICK_ON } else { STICK_OFF };
            if raw.lx.abs().max(raw.ly.abs()) >= gate {
                dir = if raw.lx.abs() > raw.ly.abs() {
                    if raw.lx > 0.0 { 4 } else { 3 }
                } else if raw.ly > 0.0 {
                    1 // +Y = up
                } else {
                    2
                };
            }
        }
        // A direction held from before the keyboard opened waits for a release.
        let dpad = Buttons::DPAD_UP | Buttons::DPAD_DOWN | Buttons::DPAD_LEFT | Buttons::DPAD_RIGHT;
        if prev == u32::MAX && b & dpad != 0 {
            dir = 0;
        }
        if dir == 0 {
            self.dir = 0;
            self.dir_next = None;
        } else if dir != self.dir {
            self.dir = dir;
            self.dir_next = Some(now + MOVE_FIRST);
            self.move_highlight(dir);
        } else if self.dir_next.is_some_and(|t| now >= t) {
            self.dir_next = Some(now + MOVE_REPEAT);
            self.move_highlight(dir);
        }

        // ---- buttons ----
        if prev != u32::MAX || b == 0 {
            if edge(Buttons::EAST) {
                self.close();
                self.prev = b;
                return out;
            }
            if edge(L2_BIT) {
                self.symbols = !self.symbols;
            }
            if edge(Buttons::L3) {
                self.cycle_shift();
            }
            if edge(Buttons::SOUTH) {
                self.press_highlighted(&mut out, now);
                if !self.open {
                    self.prev = b;
                    return out;
                }
            }
            for (bit, vk) in [
                (Buttons::WEST, VK_BACK),
                (Buttons::NORTH, VK_SPACE),
                (Buttons::START, VK_RETURN),
                (Buttons::L1, VK_LEFT),
                (Buttons::R1, VK_RIGHT),
            ] {
                if edge(bit) {
                    self.type_out(OskOut::Key(vk), &mut out);
                    // Enter never repeats: a held Options must not send a form twice.
                    self.held = (repeats(vk) && vk != VK_SPACE).then_some((OskOut::Key(vk), bit, now + KEY_FIRST));
                }
            }
        }

        // ---- repeat while the button stays down ----
        if let Some((what, bit, next)) = self.held {
            if b & bit == 0 {
                self.held = None;
            } else if now >= next {
                self.type_out(what, &mut out);
                self.held = Some((what, bit, now + KEY_REPEAT));
            }
        }

        self.prev = b;
        out
    }

    /// A key clicked with the mouse or tapped on a touch screen: move the
    /// highlight there and press it once.
    pub fn press_at(&mut self, row: u8, key: u8, now: Instant) -> Vec<OskOut> {
        let mut out = Vec::new();
        if !self.open {
            return out;
        }
        let rows = layout(false, false);
        let Some(k) = rows.get(row as usize).and_then(|r| r.get(key as usize)) else { return out };
        self.row = row;
        self.key = key;
        self.col = k.col + (k.span - 1) / 2;
        self.press_highlighted(&mut out, now);
        self.held = None; // a click is one press; only a held pad button repeats
        out
    }

    fn type_out(&mut self, what: OskOut, out: &mut Vec<OskOut>) {
        out.push(what);
        self.presses = self.presses.wrapping_add(1);
    }

    fn cycle_shift(&mut self) {
        self.shift = match self.shift {
            Shift::Off => Shift::Once,
            Shift::Once => Shift::Lock,
            Shift::Lock => Shift::Off,
        };
    }

    fn press_highlighted(&mut self, out: &mut Vec<OskOut>, now: Instant) {
        let rows = self.rows();
        let Some(k) = rows.get(self.row as usize).and_then(|r| r.get(self.key as usize)) else { return };
        match k.kind {
            KeyKind::Char(c) => {
                self.type_out(OskOut::Char(c), out);
                if self.shift == Shift::Once && !self.symbols {
                    self.shift = Shift::Off;
                }
            }
            KeyKind::Vk(vk) => {
                self.type_out(OskOut::Key(vk), out);
                self.held = repeats(vk).then_some((OskOut::Key(vk), Buttons::SOUTH, now + KEY_FIRST));
            }
            KeyKind::Shift => self.cycle_shift(),
            KeyKind::Symbols => self.symbols = !self.symbols,
            KeyKind::Close => self.close(),
        }
    }

    /// 1 up, 2 down, 3 left, 4 right. Every edge wraps round.
    fn move_highlight(&mut self, dir: u8) {
        let rows = layout(false, false); // the shape is the same on every layer
        match dir {
            1 | 2 => {
                self.row = if dir == 1 { (self.row + ROWS - 1) % ROWS } else { (self.row + 1) % ROWS };
                let row = &rows[self.row as usize];
                self.key = row.iter().position(|k| self.col >= k.col && self.col < k.col + k.span).unwrap_or(0) as u8;
            }
            _ => {
                let n = rows[self.row as usize].len() as u8;
                self.key = if dir == 3 { (self.key + n - 1) % n } else { (self.key + 1) % n };
                let k = &rows[self.row as usize][self.key as usize];
                self.col = k.col + (k.span - 1) / 2;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pad(bits: &[u32]) -> PadState {
        let mut s = PadState::default();
        for b in bits {
            s.buttons.set(*b, true);
        }
        s
    }

    /// An open keyboard that has already seen the pad at rest.
    fn open() -> (Osk, Instant) {
        let mut k = Osk::default();
        let t = Instant::now();
        k.open();
        assert!(k.step(&PadState::default(), t).is_empty());
        (k, t)
    }

    /// Press and release one button; returns what was typed.
    fn tap(k: &mut Osk, bit: u32, t: Instant) -> Vec<OskOut> {
        let out = k.step(&pad(&[bit]), t);
        k.step(&PadState::default(), t + Duration::from_millis(30));
        out
    }

    #[test]
    fn every_row_fills_the_grid_exactly() {
        for (symbols, upper) in [(false, false), (false, true), (true, false)] {
            let rows = layout(symbols, upper);
            assert_eq!(rows.len(), ROWS as usize);
            for row in rows {
                let mut next = 0;
                for k in &row {
                    assert_eq!(k.col, next, "keys sit edge to edge");
                    next += k.span;
                }
                assert_eq!(next, COLS);
            }
        }
    }

    #[test]
    fn cross_types_the_highlighted_key() {
        let (mut k, t) = open();
        // starts on "q"
        assert_eq!(tap(&mut k, Buttons::SOUTH, t), vec![OskOut::Char('q')]);
        tap(&mut k, Buttons::DPAD_RIGHT, t);
        assert_eq!(tap(&mut k, Buttons::SOUTH, t), vec![OskOut::Char('w')]);
        tap(&mut k, Buttons::DPAD_DOWN, t);
        assert_eq!(tap(&mut k, Buttons::SOUTH, t), vec![OskOut::Char('s')]);
        assert_eq!(k.view().presses, 3);
    }

    #[test]
    fn shortcut_buttons() {
        let (mut k, t) = open();
        assert_eq!(tap(&mut k, Buttons::WEST, t), vec![OskOut::Key(VK_BACK)]);
        assert_eq!(tap(&mut k, Buttons::NORTH, t), vec![OskOut::Key(VK_SPACE)]);
        assert_eq!(tap(&mut k, Buttons::START, t), vec![OskOut::Key(VK_RETURN)]);
        assert_eq!(tap(&mut k, Buttons::L1, t), vec![OskOut::Key(VK_LEFT)]);
        assert_eq!(tap(&mut k, Buttons::R1, t), vec![OskOut::Key(VK_RIGHT)]);
        assert!(k.is_open());
        assert!(tap(&mut k, Buttons::EAST, t).is_empty());
        assert!(!k.is_open(), "Circle closes the keyboard");
    }

    #[test]
    fn shift_is_one_letter_then_caps_then_off() {
        let (mut k, t) = open();
        tap(&mut k, Buttons::L3, t);
        assert_eq!(k.view().layer, 1);
        assert_eq!(tap(&mut k, Buttons::SOUTH, t), vec![OskOut::Char('Q')]);
        assert_eq!(tap(&mut k, Buttons::SOUTH, t), vec![OskOut::Char('q')], "one letter only");
        tap(&mut k, Buttons::L3, t);
        tap(&mut k, Buttons::L3, t);
        assert_eq!(k.view().shift, 2, "twice = caps lock");
        assert_eq!(tap(&mut k, Buttons::SOUTH, t), vec![OskOut::Char('Q')]);
        assert_eq!(tap(&mut k, Buttons::SOUTH, t), vec![OskOut::Char('Q')]);
        tap(&mut k, Buttons::L3, t);
        assert_eq!(k.view().shift, 0);
    }

    #[test]
    fn l2_switches_to_symbols_and_back() {
        let (mut k, t) = open();
        let mut pulled = PadState::default();
        pulled.l2 = 1.0;
        k.step(&pulled, t);
        k.step(&PadState::default(), t);
        assert_eq!(k.view().layer, 2);
        assert_eq!(tap(&mut k, Buttons::SOUTH, t), vec![OskOut::Char('!')]);
        // a trigger resting half-way does not flip it back and forth
        pulled.l2 = 0.45;
        k.step(&pulled, t);
        assert_eq!(k.view().layer, 2);
        pulled.l2 = 1.0;
        k.step(&pulled, t);
        assert_eq!(k.view().layer, 0);
    }

    #[test]
    fn backspace_repeats_while_held_but_enter_does_not() {
        let (mut k, t) = open();
        let square = pad(&[Buttons::WEST]);
        assert_eq!(k.step(&square, t).len(), 1);
        assert!(k.step(&square, t + Duration::from_millis(200)).is_empty(), "not yet");
        assert_eq!(k.step(&square, t + Duration::from_millis(430)).len(), 1);
        assert_eq!(k.step(&square, t + Duration::from_millis(505)).len(), 1);
        assert!(k.step(&PadState::default(), t + Duration::from_millis(520)).is_empty());
        assert!(k.step(&PadState::default(), t + Duration::from_millis(900)).is_empty(), "stops on release");

        let options = pad(&[Buttons::START]);
        assert_eq!(k.step(&options, t + Duration::from_secs(2)).len(), 1);
        assert!(k.step(&options, t + Duration::from_secs(4)).is_empty());
    }

    #[test]
    fn buttons_held_when_it_opens_do_nothing_until_released() {
        let mut k = Osk::default();
        let t = Instant::now();
        k.open();
        // Opened by Cross on a text box: Cross is still down.
        assert!(k.step(&pad(&[Buttons::SOUTH]), t).is_empty());
        assert!(k.step(&pad(&[Buttons::SOUTH]), t + Duration::from_millis(600)).is_empty());
        k.step(&PadState::default(), t + Duration::from_millis(700));
        assert_eq!(k.step(&pad(&[Buttons::SOUTH]), t + Duration::from_millis(800)), vec![OskOut::Char('q')]);
    }

    #[test]
    fn ps_held_hands_the_pad_to_the_chord_layer() {
        let (mut k, t) = open();
        let before = k.view();
        // PS + D-pad Up is volume: the highlight must not move, nothing is typed.
        assert!(k.step(&pad(&[Buttons::HOME, Buttons::DPAD_UP, Buttons::SOUTH]), t).is_empty());
        assert_eq!(k.view(), before);
        // PS let go first, Cross still down: still nothing.
        assert!(k.step(&pad(&[Buttons::SOUTH]), t + Duration::from_millis(20)).is_empty());
    }

    #[test]
    fn highlight_wraps_and_remembers_its_column_through_space() {
        let (mut k, t) = open();
        // q -> left wraps to Enter at the end of the row
        tap(&mut k, Buttons::DPAD_LEFT, t);
        assert_eq!(tap(&mut k, Buttons::SOUTH, t), vec![OskOut::Key(VK_RETURN)]);
        tap(&mut k, Buttons::DPAD_RIGHT, t); // back on q
        for _ in 0..6 {
            tap(&mut k, Buttons::DPAD_RIGHT, t); // u (column 6)
        }
        assert_eq!(tap(&mut k, Buttons::SOUTH, t), vec![OskOut::Char('u')]);
        for _ in 0..3 {
            tap(&mut k, Buttons::DPAD_DOWN, t); // j, m, Space
        }
        assert_eq!(tap(&mut k, Buttons::SOUTH, t), vec![OskOut::Key(VK_SPACE)]);
        tap(&mut k, Buttons::DPAD_UP, t);
        assert_eq!(tap(&mut k, Buttons::SOUTH, t), vec![OskOut::Char('m')], "back up into the same column");
        // up from the top row wraps to the bottom row
        for _ in 0..4 {
            tap(&mut k, Buttons::DPAD_UP, t);
        }
        assert_eq!(k.view().row, 4);
    }

    #[test]
    fn stick_moves_once_then_repeats() {
        let (mut k, t) = open();
        let mut s = PadState::default();
        s.lx = 1.0;
        k.step(&s, t);
        assert_eq!(k.view().key, 1);
        k.step(&s, t + Duration::from_millis(100));
        assert_eq!(k.view().key, 1, "held: waits before repeating");
        k.step(&s, t + Duration::from_millis(360));
        assert_eq!(k.view().key, 2);
        k.step(&s, t + Duration::from_millis(480));
        assert_eq!(k.view().key, 3);
    }

    #[test]
    fn a_clicked_key_is_pressed_once_and_takes_the_highlight() {
        let (mut k, t) = open();
        assert_eq!(k.press_at(2, 3, t), vec![OskOut::Char('f')]);
        assert_eq!((k.view().row, k.view().key), (2, 3));
        assert_eq!(k.press_at(0, 10, t), vec![OskOut::Key(VK_BACK)]);
        assert!(k.step(&PadState::default(), t + Duration::from_secs(2)).is_empty(), "no repeat from a click");
        assert!(k.press_at(9, 9, t).is_empty(), "outside the grid");
        k.press_at(4, 5, t);
        assert!(!k.is_open(), "the close key");
        assert!(k.press_at(1, 0, t).is_empty());
    }

    #[test]
    fn close_key_and_layout_json() {
        let (mut k, t) = open();
        let v: serde_json::Value = serde_json::from_str(&k.layout_json()).unwrap();
        assert_eq!(v["cols"], 11);
        assert_eq!(v["rows"][1][0]["label"], "q");
        assert_eq!(v["rows"][4][2]["span"], 4);
        assert_eq!(v["rows"][4][5]["kind"], "close");
        tap(&mut k, Buttons::DPAD_UP, t); // row 0
        tap(&mut k, Buttons::DPAD_UP, t); // wraps to the bottom row
        tap(&mut k, Buttons::DPAD_LEFT, t); // Shift -> wraps to the close key
        tap(&mut k, Buttons::SOUTH, t);
        assert!(!k.is_open());
        assert!(k.step(&pad(&[Buttons::SOUTH]), t).is_empty(), "closed: types nothing");
    }
}
