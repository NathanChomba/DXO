//! M5: per-button bindings, the Mappings page's engine.
//!
//! A binding takes one physical button and decides what it does:
//!   * `Same`     - stays itself (useful with turbo / toggle)
//!   * `Button`   - becomes another pad button
//!   * `Key`      - presses a keyboard key (sent as a scancode, so DirectInput
//!                  games and anti-cheat-friendly titles see it too)
//!   * `Mouse`    - clicks a mouse button or turns the wheel
//!   * `Disabled` - does nothing
//!   * `Macro`    - plays a sequence of presses
//!   * `Launch`   - opens an app, a folder, a file or a web address
//! plus `toggle` (press once = hold, press again = release) and `turbo`
//! (rapid repeat while held).
//!
//! On top of that (0.3):
//!   * tap / hold / double-tap: a binding can carry a second action for a long
//!     press (`hold`) and a third for two quick presses (`double_tap`);
//!   * dual-stage triggers: the sources "l2_soft" / "l2_full" (and r2) fire at
//!     a light pull and at a full pull, so one trigger can aim and then shoot;
//!   * chords: two buttons held together fire their own action.
//!
//! `mapping::apply` stays a pure function; this `Runner` holds the state that
//! all of the above need and turns key/mouse actions into `InputEvent`s.

use crate::mapping::button_from_name;
use crate::state::{Buttons, PadState};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Back,
    Forward,
    WheelUp,
    WheelDown,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum Action {
    #[default]
    Same,
    Button { to: String },
    /// Windows virtual-key code (0x41 = A, 0x20 = Space, ...)
    Key { vk: u16 },
    Mouse { button: MouseButton },
    Disabled,
    /// Play a sequence of presses. Starts when the button goes down and plays
    /// to the end; with `repeat` it loops for as long as the button is held.
    Macro {
        steps: Vec<MacroStep>,
        #[serde(default)]
        repeat: bool,
    },
    /// Open something once per press: a program (`C:\\...\\Discord.exe`), a
    /// folder, a file, or a web address. `args` are passed to a program.
    Launch {
        target: String,
        #[serde(default)]
        args: String,
    },
}

/// One thing a macro step presses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum Press {
    Button { to: String },
    Key { vk: u16 },
    Mouse { button: MouseButton },
}

/// Press everything in `presses` together for `hold_ms`, then wait `gap_ms`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MacroStep {
    pub presses: Vec<Press>,
    pub hold_ms: u32,
    pub gap_ms: u32,
}

impl Default for MacroStep {
    fn default() -> Self {
        Self { presses: vec![], hold_ms: 50, gap_ms: 50 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Binding {
    /// Source button name (`source_from_name`): a pad button, or a trigger
    /// stage ("l2_soft", "l2_full", "r2_soft", "r2_full").
    pub button: String,
    /// What a normal press does (with `hold` / `double_tap` set: a short tap).
    pub action: Action,
    pub turbo: bool,
    /// Presses per second while turbo is active.
    pub turbo_hz: f32,
    pub toggle: bool,
    /// What holding the button does (longer than the profile's `hold_ms`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hold: Option<Action>,
    /// What two quick presses do (within the profile's `double_tap_ms`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub double_tap: Option<Action>,
}

impl Default for Binding {
    fn default() -> Self {
        Self {
            button: String::new(),
            action: Action::Same,
            turbo: false,
            turbo_hz: 10.0,
            toggle: false,
            hold: None,
            double_tap: None,
        }
    }
}

/// Two (or more) buttons held together fire `action`; while they are, the
/// game sees none of them. Not the same thing as the PS-button system chords
/// in `chords.rs`: these belong to the profile.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ChordBinding {
    pub buttons: Vec<String>,
    pub action: Action,
}

/// The timings and thresholds a profile sets for its bindings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Timing {
    pub hold_ms: u32,
    pub double_ms: u32,
    /// Trigger pull (0..=1) where the "soft" stage starts.
    pub soft: f32,
    /// Trigger pull where the "full" stage starts.
    pub full: f32,
}

impl Default for Timing {
    fn default() -> Self {
        Self { hold_ms: 250, double_ms: 250, soft: 0.12, full: 0.9 }
    }
}

/// The normal bindings plus the optional shift layer and chords.
#[derive(Debug, Clone, Copy)]
pub struct Layers<'a> {
    pub base: &'a [Binding],
    /// Button that switches to `shift` while held; "" = no shift layer.
    pub shift_button: &'a str,
    pub shift: &'a [Binding],
    pub chords: &'a [ChordBinding],
    pub timing: Timing,
}

impl<'a> Layers<'a> {
    pub fn base(b: &'a [Binding]) -> Self {
        Self { base: b, shift_button: "", shift: &[], chords: &[], timing: Timing::default() }
    }

    pub fn of(p: &'a crate::profile::Profile) -> Self {
        Self {
            base: &p.bindings,
            shift_button: &p.shift_button,
            shift: &p.shift_bindings,
            chords: &p.chords,
            timing: Timing {
                hold_ms: p.hold_ms.clamp(100, 2000),
                double_ms: p.double_tap_ms.clamp(100, 1000),
                soft: p.trigger_soft_point.clamp(0.02, 0.8),
                full: p.trigger_full_point.clamp(0.3, 1.0),
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEvent {
    Key { vk: u16, down: bool },
    Mouse { button: MouseButton, down: bool },
    /// Wheel notches, + = up.
    Wheel(i32),
    /// Smooth scrolling in wheel units (120 = one notch): + dy = up, + dx = right.
    Scroll { dx: i32, dy: i32 },
    /// One UTF-16 unit typed as text, whatever the PC's keyboard layout is
    /// (the on-screen keyboard).
    Unicode { unit: u16, down: bool },
}

/// Something a `Launch` action asked to open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchRequest {
    pub target: String,
    pub args: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Out {
    Key(u16),
    Mouse(MouseButton),
}

// ---- trigger stages: extra "buttons" that exist only inside the runner ------

const L2_SOFT: u32 = 1 << 24;
const L2_FULL: u32 = 1 << 25;
const R2_SOFT: u32 = 1 << 26;
const R2_FULL: u32 = 1 << 27;
const STAGE_MASK: u32 = L2_SOFT | L2_FULL | R2_SOFT | R2_FULL;
/// A stage lets go this far below the point where it engaged, so a finger
/// resting right on the threshold doesn't chatter.
const STAGE_SLACK: f32 = 0.04;

/// A binding's source: a real button, or a trigger stage.
pub fn source_from_name(name: &str) -> Option<u32> {
    Some(match name.to_ascii_lowercase().as_str() {
        "l2_soft" => L2_SOFT,
        "l2_full" => L2_FULL,
        "r2_soft" => R2_SOFT,
        "r2_full" => R2_FULL,
        other => return button_from_name(other),
    })
}

/// The real trigger a stage belongs to (other sources are themselves).
fn real_of(src: u32) -> u32 {
    match src {
        L2_SOFT | L2_FULL => Buttons::L2_DIGITAL,
        R2_SOFT | R2_FULL => Buttons::R2_DIGITAL,
        other => other,
    }
}

/// (layer, binding index, which action). Layer: 0 = normal, 1 = shift,
/// 2 = chords. Action: 0 = press/tap, 1 = hold, 2 = double-tap.
type Slot = (u8, usize, u8);

#[derive(Debug)]
struct Playing {
    start: Instant,
    step: Option<usize>,
}

/// Where one button is in the tap / hold / double-tap dance.
#[derive(Debug, Default)]
struct TapState {
    prev: bool,
    down_at: Option<Instant>,
    /// Held long enough: the hold action is on until release.
    holding: bool,
    /// A short press ended here; waiting to see if a second one follows.
    pending_tap: Option<Instant>,
    /// The press in progress is the second of a double-tap.
    second: bool,
    tap_until: Option<Instant>,
    double_until: Option<Instant>,
}

impl TapState {
    /// Returns (tap, hold, double): which of the three actions is on right now.
    fn step(&mut self, pressed: bool, now: Instant, t: Timing, has_double: bool) -> (bool, bool, bool) {
        let hold_after = Duration::from_millis(t.hold_ms as u64);
        let double_within = Duration::from_millis(t.double_ms as u64);
        let rising = pressed && !self.prev;
        let falling = !pressed && self.prev;
        self.prev = pressed;

        if rising {
            let follows = self.pending_tap.take().map(|t0| now.duration_since(t0) <= double_within);
            match follows {
                Some(true) if has_double => {
                    self.second = true;
                    self.double_until = Some(now + TAP_PULSE); // long enough for a game to notice
                }
                Some(_) => {
                    self.second = false;
                    self.tap_until = Some(now + TAP_PULSE); // the earlier tap was just a tap
                }
                None => self.second = false,
            }
            self.down_at = Some(now);
            self.holding = false;
        }
        if pressed && !self.second && !self.holding {
            if self.down_at.is_some_and(|t0| now.duration_since(t0) >= hold_after) {
                self.holding = true;
            }
        }
        if falling {
            if self.second {
                self.second = false;
            } else if self.holding {
                self.holding = false;
            } else if has_double {
                self.pending_tap = Some(now); // might still become a double-tap
            } else {
                self.tap_until = Some(now + TAP_PULSE);
            }
            self.down_at = None;
        }
        // Nobody pressed again in time: it was a single tap after all.
        if !pressed && self.pending_tap.is_some_and(|t0| now.duration_since(t0) > double_within) {
            self.pending_tap = None;
            self.tap_until = Some(now + TAP_PULSE);
        }
        let tap = self.tap_until.is_some_and(|u| now < u);
        if !tap {
            self.tap_until = None;
        }
        let double_pulse = self.double_until.is_some_and(|u| now < u);
        if !double_pulse {
            self.double_until = None;
        }
        (tap, self.holding, (pressed && self.second) || double_pulse)
    }
}

/// A quick tap of the shift button (shorter than this, nothing else pressed)
/// is passed to the game as a normal press.
const SHIFT_TAP: Duration = Duration::from_millis(300);
const TAP_PULSE: Duration = Duration::from_millis(80);

#[derive(Debug, Default)]
pub struct Runner {
    started: Option<Instant>,
    /// Raw buttons last frame (shift-layer bookkeeping).
    prev_raw: u32,
    /// Sources last frame as the bindings saw them (stages added, chord buttons removed).
    prev_eff: u32,
    /// Trigger stages currently engaged.
    stages: u32,
    latched: HashMap<(u8, String), bool>,
    prev_active: HashMap<Slot, bool>,
    taps: HashMap<(u8, usize), TapState>,
    held: HashSet<Out>,
    macros: HashMap<Slot, Playing>,
    /// Buttons a chord has claimed: hidden from everything else until released.
    chord_claimed: u32,
    launches: Vec<LaunchRequest>,
    shift_down_at: Option<Instant>,
    shift_used: bool,
    tap_until: Option<Instant>,
    /// Bindings the state above belongs to; a different set resets it.
    last: Option<(Vec<Binding>, String, Vec<Binding>, Vec<ChordBinding>, Timing)>,
}

/// The pad state and outputs one frame is building up.
struct Frame<'a> {
    orig: PadState,
    mapped: &'a mut PadState,
    want: HashSet<Out>,
    events: Vec<InputEvent>,
    now: Instant,
}

impl Runner {
    /// Apply the bindings to `mapped` (the pad state games will see) and return
    /// the keyboard/mouse events to send now.
    pub fn process(&mut self, layers: Layers, raw: &PadState, mapped: &mut PadState, now: Instant) -> Vec<InputEvent> {
        let mut carried = vec![];
        let same = self.last.as_ref().is_some_and(|l| {
            l.0 == layers.base && l.1 == layers.shift_button && l.2 == layers.shift && l.3 == layers.chords && l.4 == layers.timing
        });
        if !same {
            // Profile changed: let go of anything the old bindings held.
            carried = self.release_all();
            self.last = Some((
                layers.base.to_vec(),
                layers.shift_button.to_string(),
                layers.shift.to_vec(),
                layers.chords.to_vec(),
                layers.timing,
            ));
        }
        let t = now.duration_since(*self.started.get_or_insert(now)).as_secs_f32();
        let mut f = Frame { orig: *mapped, mapped, want: HashSet::new(), events: carried, now };

        // ---- trigger stages ---------------------------------------------------
        let tm = layers.timing;
        let stage = |bit: u32, v: f32, at: f32, stages: &mut u32| {
            let on = if *stages & bit != 0 { v >= at - STAGE_SLACK } else { v >= at };
            if on {
                *stages |= bit;
            } else {
                *stages &= !bit;
            }
        };
        stage(L2_SOFT, raw.l2, tm.soft, &mut self.stages);
        stage(L2_FULL, raw.l2, tm.full.max(tm.soft + 0.05), &mut self.stages);
        stage(R2_SOFT, raw.r2, tm.soft, &mut self.stages);
        stage(R2_FULL, raw.r2, tm.full.max(tm.soft + 0.05), &mut self.stages);
        let src_bits = raw.buttons.0 | self.stages;

        // ---- shift layer ----------------------------------------------------
        let shift_src = button_from_name(layers.shift_button);
        let mut shift_on = false;
        if let Some(sb) = shift_src {
            let down = raw.buttons.has(sb);
            match (down, self.shift_down_at) {
                (true, None) => {
                    self.shift_down_at = Some(now);
                    self.shift_used = false;
                }
                (false, Some(t0)) => {
                    if !self.shift_used && now.duration_since(t0) < SHIFT_TAP {
                        self.tap_until = Some(now + TAP_PULSE);
                    }
                    self.shift_down_at = None;
                }
                _ => {}
            }
            if down && raw.buttons.0 & !self.prev_raw & !sb != 0 {
                self.shift_used = true; // pressed something else while holding it
            }
            shift_on = down;
            let tap = self.tap_until.is_some_and(|u| now < u);
            if !tap {
                self.tap_until = None;
            }
            f.mapped.buttons.set(sb, tap);
            set_analog(f.mapped, sb, if tap { 1.0 } else { 0.0 });
        }

        let shifted = |name: &str| shift_on && layers.shift.iter().any(|b| b.button == name);
        let mut list: Vec<((u8, usize), &Binding)> = vec![];
        let mut idle: Vec<(u8, usize)> = vec![]; // bindings not in play this frame
        for (i, b) in layers.base.iter().enumerate() {
            if shifted(&b.button) || shift_src.is_some() && source_from_name(&b.button) == shift_src {
                idle.push((0, i));
            } else {
                list.push(((0, i), b));
            }
        }
        for (i, b) in layers.shift.iter().enumerate() {
            if shift_on && source_from_name(&b.button) != shift_src {
                list.push(((1, i), b));
            } else {
                idle.push((1, i));
            }
        }
        for key in idle {
            for sub in 0..3 {
                self.prev_active.insert((key.0, key.1, sub), false);
            }
            self.taps.remove(&key);
        }

        // A trigger with stage bindings is no longer an ordinary trigger: the
        // game only gets what those bindings give it.
        for (trigger, mask) in [(Buttons::L2_DIGITAL, L2_SOFT | L2_FULL), (Buttons::R2_DIGITAL, R2_SOFT | R2_FULL)] {
            if list.iter().any(|(_, b)| source_from_name(&b.button).is_some_and(|s| s & mask != 0)) {
                f.mapped.buttons.set(trigger, false);
                set_analog(f.mapped, trigger, 0.0);
            }
        }

        // ---- chords -----------------------------------------------------------
        // Buttons stay claimed until they are let go, so releasing one half of a
        // chord doesn't suddenly press the other half in the game.
        self.chord_claimed &= src_bits;
        for (i, c) in layers.chords.iter().enumerate() {
            let bits: Vec<u32> = c.buttons.iter().filter_map(|n| source_from_name(n)).collect();
            let valid = bits.len() >= 2 && bits.len() == c.buttons.len();
            let down = valid && bits.iter().all(|b| src_bits & b != 0);
            if down {
                for b in &bits {
                    self.chord_claimed |= b;
                }
            }
            self.exec((2, i, 0), &c.action, down, 0, false, &mut f);
        }
        let claimed = self.chord_claimed;
        for bit in 0..32 {
            let b = 1u32 << bit;
            if claimed & b != 0 {
                let real = real_of(b);
                f.mapped.buttons.set(real, false);
                set_analog(f.mapped, real, 0.0);
            }
        }
        let eff = src_bits & !claimed;

        // ---- bindings -------------------------------------------------------
        for (key, b) in list {
            let Some(src) = source_from_name(&b.button) else { continue };
            let pressed = eff & src != 0;
            let was = self.prev_eff & src != 0;

            // The source no longer reaches the game as itself; the action below
            // decides what it does instead. (Stage sources were handled above.)
            if src & STAGE_MASK == 0 {
                f.mapped.buttons.set(src, false);
                set_analog(f.mapped, src, 0.0);
            }

            if b.hold.is_some() || b.double_tap.is_some() {
                let (tap, hold, double) =
                    self.taps.entry(key).or_default().step(pressed, now, layers.timing, b.double_tap.is_some());
                // No hold action: a long press simply holds the normal action.
                let main = tap || (hold && b.hold.is_none());
                self.exec((key.0, key.1, 0), &b.action, main, src, false, &mut f);
                if let Some(a) = &b.hold {
                    self.exec((key.0, key.1, 1), a, hold, src, false, &mut f);
                }
                if let Some(a) = &b.double_tap {
                    self.exec((key.0, key.1, 2), a, double, src, false, &mut f);
                }
                continue;
            }

            let mut active = pressed;
            if b.toggle {
                let l = self.latched.entry((key.0, b.button.clone())).or_insert(false);
                if pressed && !was {
                    *l = !*l;
                }
                active = *l;
            }
            if b.turbo && active {
                let hz = b.turbo_hz.clamp(1.0, 30.0);
                active = ((t * hz * 2.0) as u64) % 2 == 0; // on for half of each cycle
            }
            let live = pressed && !b.toggle && !b.turbo; // analog value follows the finger
            self.exec((key.0, key.1, 0), &b.action, active, src, live, &mut f);
        }
        self.prev_raw = raw.buttons.0;
        self.prev_eff = eff;
        f.mapped.buttons.0 &= !STAGE_MASK;

        let Frame { want, mut events, .. } = f;
        for o in self.held.difference(&want) {
            events.push(release(*o));
        }
        for o in want.difference(&self.held) {
            events.push(match *o {
                Out::Key(vk) => InputEvent::Key { vk, down: true },
                Out::Mouse(button) => InputEvent::Mouse { button, down: true },
            });
        }
        self.held = want;
        events
    }

    /// Carry out one action. `src` is the source button (0 for a chord);
    /// `live` = for `Same` on a trigger, pass its real analog value through
    /// (otherwise it counts as fully pressed).
    fn exec(&mut self, slot: Slot, action: &Action, active: bool, src: u32, live: bool, f: &mut Frame) {
        let rising = active && !self.prev_active.get(&slot).copied().unwrap_or(false);
        self.prev_active.insert(slot, active);
        match action {
            Action::Same => {
                if active && src != 0 {
                    let real = real_of(src);
                    f.mapped.buttons.set(real, true);
                    // A stage keeps the trigger analog; a plain trigger too while it's simply held.
                    let follow = live || src & STAGE_MASK != 0;
                    let a = analog(&f.orig, real).filter(|_| follow).unwrap_or(1.0);
                    set_analog(f.mapped, real, a);
                }
            }
            Action::Button { to } => {
                if active {
                    press(&Press::Button { to: to.clone() }, rising, f);
                }
            }
            Action::Key { vk } => {
                if active && *vk != 0 {
                    f.want.insert(Out::Key(*vk));
                }
            }
            Action::Mouse { button } => {
                if active {
                    press(&Press::Mouse { button: *button }, rising, f);
                }
            }
            Action::Disabled => {}
            Action::Launch { target, args } => {
                if rising && !target.trim().is_empty() {
                    self.launches.push(LaunchRequest { target: target.trim().to_string(), args: args.trim().to_string() });
                }
            }
            Action::Macro { steps, repeat } => {
                if rising {
                    self.macros.insert(slot, Playing { start: f.now, step: None });
                }
                let total: u64 = steps.iter().map(|s| s.hold_ms as u64 + s.gap_ms as u64).sum();
                let Some(p) = self.macros.get_mut(&slot) else { return };
                let mut el = f.now.duration_since(p.start).as_millis() as u64;
                if total == 0 {
                    self.macros.remove(&slot);
                    return;
                }
                if el >= total {
                    if *repeat && active {
                        p.start += Duration::from_millis(total * (el / total));
                        el %= total;
                        p.step = None;
                    } else {
                        self.macros.remove(&slot);
                        return;
                    }
                }
                let mut at = 0u64;
                for (n, st) in steps.iter().enumerate() {
                    if el >= at && el < at + st.hold_ms as u64 {
                        let entering = p.step != Some(n);
                        p.step = Some(n);
                        for pr in &st.presses {
                            press(pr, entering, f);
                        }
                        break;
                    }
                    at += st.hold_ms as u64 + st.gap_ms as u64;
                }
            }
        }
    }

    /// Things `Launch` actions asked to open since the last call.
    pub fn take_launches(&mut self) -> Vec<LaunchRequest> {
        std::mem::take(&mut self.launches)
    }

    /// Release every key / mouse button we're holding (disconnect, profile
    /// switch, shutdown). Never leave a key stuck down.
    pub fn release_all(&mut self) -> Vec<InputEvent> {
        let ev = self.held.drain().map(release).collect();
        self.latched.clear();
        self.prev_active.clear();
        self.taps.clear();
        self.macros.clear();
        self.chord_claimed = 0;
        self.shift_down_at = None;
        self.tap_until = None;
        ev
    }
}

/// Hold one press this frame. Wheel turns only fire on `rising`.
fn press(p: &Press, rising: bool, f: &mut Frame) {
    match p {
        Press::Button { to } => {
            if let Some(dst) = button_from_name(to) {
                f.mapped.buttons.set(dst, true);
                set_analog(f.mapped, dst, 1.0);
            }
        }
        Press::Key { vk } => {
            if *vk != 0 {
                f.want.insert(Out::Key(*vk));
            }
        }
        Press::Mouse { button: MouseButton::WheelUp } => {
            if rising {
                f.events.push(InputEvent::Wheel(1));
            }
        }
        Press::Mouse { button: MouseButton::WheelDown } => {
            if rising {
                f.events.push(InputEvent::Wheel(-1));
            }
        }
        Press::Mouse { button } => {
            f.want.insert(Out::Mouse(*button));
        }
    }
}

fn release(o: Out) -> InputEvent {
    match o {
        Out::Key(vk) => InputEvent::Key { vk, down: false },
        Out::Mouse(button) => InputEvent::Mouse { button, down: false },
    }
}

/// Triggers are buttons AND analog axes; keep both in step.
fn analog(s: &PadState, b: u32) -> Option<f32> {
    match b {
        Buttons::L2_DIGITAL => Some(s.l2),
        Buttons::R2_DIGITAL => Some(s.r2),
        _ => None,
    }
}

fn set_analog(s: &mut PadState, b: u32, v: f32) {
    match b {
        Buttons::L2_DIGITAL => s.l2 = v,
        Buttons::R2_DIGITAL => s.r2 = v,
        _ => {}
    }
}

// ---- launching -----------------------------------------------------------------

/// Open what a `Launch` action named. Never blocks the caller.
///
/// DXO runs as administrator, and anything it started directly would be an
/// administrator too. So a plain target (a program, folder, file or web
/// address, no arguments) is handed to Explorer, which opens it as the
/// signed-in user, exactly like a double-click. Only a target WITH arguments
/// is started by DXO itself, because Explorer can't pass arguments on.
#[cfg(windows)]
pub fn launch(req: &LaunchRequest) {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let result = if req.args.is_empty() {
        Command::new("explorer.exe").arg(&req.target).spawn()
    } else {
        // `start` resolves programs on PATH and file associations; raw_arg keeps
        // the user's own quoting of the arguments.
        Command::new("cmd.exe")
            .args(["/C", "start", "", req.target.as_str()])
            .raw_arg(&req.args)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
    };
    match result {
        Ok(_) => log::info!("launched {}", req.target),
        Err(e) => log::warn!("launch {}: {e}", req.target),
    }
}

#[cfg(not(windows))]
pub fn launch(_req: &LaunchRequest) {}

// ---- sending -----------------------------------------------------------------

#[cfg(windows)]
pub fn send(events: &[InputEvent]) {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
    if events.is_empty() {
        return;
    }
    let mut inputs: Vec<INPUT> = Vec::with_capacity(events.len());
    let mouse = |flags: u32, data: u32| INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 { mi: MOUSEINPUT { dx: 0, dy: 0, mouseData: data, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
    };
    for e in events {
        match *e {
            InputEvent::Key { vk, down } => {
                // Scancodes reach games that ignore virtual keys (DirectInput).
                let sc = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC_EX) };
                let mut flags = KEYEVENTF_SCANCODE;
                if matches!(sc >> 8, 0xE0 | 0xE1) {
                    flags |= KEYEVENTF_EXTENDEDKEY;
                }
                if !down {
                    flags |= KEYEVENTF_KEYUP;
                }
                inputs.push(INPUT {
                    r#type: INPUT_KEYBOARD,
                    Anonymous: INPUT_0 {
                        ki: KEYBDINPUT { wVk: vk, wScan: (sc & 0xFF) as u16, dwFlags: flags, time: 0, dwExtraInfo: 0 },
                    },
                });
            }
            InputEvent::Mouse { button, down } => {
                let (d, u, data) = match button {
                    MouseButton::Left => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, 0),
                    MouseButton::Right => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, 0),
                    MouseButton::Middle => (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, 0),
                    MouseButton::Back => (MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, 1), // XBUTTON1
                    MouseButton::Forward => (MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, 2), // XBUTTON2
                    MouseButton::WheelUp | MouseButton::WheelDown => continue,
                };
                inputs.push(mouse(if down { d } else { u }, data));
            }
            InputEvent::Wheel(n) => inputs.push(mouse(MOUSEEVENTF_WHEEL, (n * 120) as u32)),
            InputEvent::Scroll { dx, dy } => {
                if dy != 0 {
                    inputs.push(mouse(MOUSEEVENTF_WHEEL, dy as u32));
                }
                if dx != 0 {
                    inputs.push(mouse(MOUSEEVENTF_HWHEEL, dx as u32));
                }
            }
            InputEvent::Unicode { unit, down } => inputs.push(INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: 0,
                        wScan: unit,
                        dwFlags: KEYEVENTF_UNICODE | if down { 0 } else { KEYEVENTF_KEYUP },
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            }),
        }
    }
    unsafe {
        SendInput(inputs.len() as u32, inputs.as_ptr(), std::mem::size_of::<INPUT>() as i32);
    }
}

#[cfg(not(windows))]
pub fn send(_events: &[InputEvent]) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn pad(buttons: &[u32]) -> PadState {
        let mut s = PadState::default();
        for b in buttons {
            s.buttons.set(*b, true);
        }
        s
    }

    fn run(r: &mut Runner, b: &[Binding], raw: &PadState, t: Instant) -> (PadState, Vec<InputEvent>) {
        let mut m = *raw;
        let ev = r.process(Layers::base(b), raw, &mut m, t);
        (m, ev)
    }

    #[test]
    fn key_press_and_release() {
        let b = vec![Binding { button: "cross".into(), action: Action::Key { vk: 0x20 }, ..Default::default() }];
        let mut r = Runner::default();
        let t = Instant::now();
        let (m, ev) = run(&mut r, &b, &pad(&[Buttons::SOUTH]), t);
        assert!(!m.buttons.has(Buttons::SOUTH), "game must not also see Cross");
        assert_eq!(ev, vec![InputEvent::Key { vk: 0x20, down: true }]);
        let (_, ev) = run(&mut r, &b, &pad(&[Buttons::SOUTH]), t);
        assert!(ev.is_empty(), "held key is not re-sent");
        let (_, ev) = run(&mut r, &b, &pad(&[]), t);
        assert_eq!(ev, vec![InputEvent::Key { vk: 0x20, down: false }]);
    }

    #[test]
    fn toggle_latches() {
        let b = vec![Binding { button: "l1".into(), toggle: true, ..Default::default() }];
        let mut r = Runner::default();
        let t = Instant::now();
        assert!(run(&mut r, &b, &pad(&[Buttons::L1]), t).0.buttons.has(Buttons::L1));
        assert!(run(&mut r, &b, &pad(&[]), t).0.buttons.has(Buttons::L1), "stays on after release");
        run(&mut r, &b, &pad(&[Buttons::L1]), t);
        assert!(!run(&mut r, &b, &pad(&[]), t).0.buttons.has(Buttons::L1), "second press turns it off");
    }

    #[test]
    fn turbo_pulses() {
        let b = vec![Binding { button: "square".into(), turbo: true, turbo_hz: 10.0, ..Default::default() }];
        let mut r = Runner::default();
        let t0 = Instant::now();
        let raw = pad(&[Buttons::WEST]);
        let mut ons = 0;
        for ms in (0..1000).step_by(5) {
            if run(&mut r, &b, &raw, t0 + Duration::from_millis(ms)).0.buttons.has(Buttons::WEST) {
                ons += 1;
            }
        }
        assert!((90..=110).contains(&ons), "about half of 200 frames, got {ons}");
    }

    #[test]
    fn trigger_to_mouse_clears_analog() {
        let b = vec![Binding { button: "r2".into(), action: Action::Mouse { button: MouseButton::Left }, ..Default::default() }];
        let mut r = Runner::default();
        let mut raw = pad(&[Buttons::R2_DIGITAL]);
        raw.r2 = 0.9;
        let (m, ev) = run(&mut r, &b, &raw, Instant::now());
        assert_eq!(m.r2, 0.0);
        assert_eq!(ev, vec![InputEvent::Mouse { button: MouseButton::Left, down: true }]);
    }

    #[test]
    fn changing_bindings_releases_held_keys() {
        let b = vec![Binding { button: "cross".into(), action: Action::Key { vk: 0x57 }, ..Default::default() }];
        let mut r = Runner::default();
        let t = Instant::now();
        run(&mut r, &b, &pad(&[Buttons::SOUTH]), t);
        let (_, ev) = run(&mut r, &[], &pad(&[Buttons::SOUTH]), t);
        assert_eq!(ev, vec![InputEvent::Key { vk: 0x57, down: false }]);
    }

    fn macro_binding(steps: Vec<MacroStep>, repeat: bool) -> Vec<Binding> {
        vec![Binding { button: "r1".into(), action: Action::Macro { steps, repeat }, ..Default::default() }]
    }

    #[test]
    fn macro_plays_steps_in_order() {
        let steps = vec![
            MacroStep { presses: vec![Press::Key { vk: 0x11 }, Press::Key { vk: 0x43 }], hold_ms: 40, gap_ms: 20 }, // Ctrl+C
            MacroStep { presses: vec![Press::Button { to: "cross".into() }], hold_ms: 40, gap_ms: 0 },
        ];
        let b = macro_binding(steps, false);
        let mut r = Runner::default();
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        let (m, ev) = run(&mut r, &b, &pad(&[Buttons::R1]), ms(0));
        assert!(!m.buttons.has(Buttons::R1));
        assert_eq!(ev.len(), 2, "Ctrl and C go down together");
        // let go of R1: the macro keeps playing
        let (_, ev) = run(&mut r, &b, &pad(&[]), ms(45));
        assert_eq!(ev.len(), 2, "both released in the gap");
        let (m, _) = run(&mut r, &b, &pad(&[]), ms(70));
        assert!(m.buttons.has(Buttons::SOUTH), "second step presses Cross");
        let (m, _) = run(&mut r, &b, &pad(&[]), ms(120));
        assert!(!m.buttons.has(Buttons::SOUTH), "finished");
    }

    #[test]
    fn repeating_macro_loops_while_held() {
        let steps = vec![MacroStep { presses: vec![Press::Button { to: "square".into() }], hold_ms: 10, gap_ms: 10 }];
        let b = macro_binding(steps, true);
        let mut r = Runner::default();
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        run(&mut r, &b, &pad(&[Buttons::R1]), ms(0));
        assert!(run(&mut r, &b, &pad(&[Buttons::R1]), ms(45)).0.buttons.has(Buttons::WEST), "third loop");
        run(&mut r, &b, &pad(&[]), ms(55));
        assert!(!run(&mut r, &b, &pad(&[]), ms(85)).0.buttons.has(Buttons::WEST), "stops after release");
    }

    #[test]
    fn shift_layer_swaps_bindings() {
        let base = vec![Binding { button: "cross".into(), action: Action::Key { vk: 0x20 }, ..Default::default() }];
        let shift = vec![Binding { button: "cross".into(), action: Action::Key { vk: 0x52 }, ..Default::default() }];
        let l = Layers { shift_button: "ps", shift: &shift, ..Layers::base(&base) };
        let mut r = Runner::default();
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        let go = |r: &mut Runner, s: PadState, t| {
            let mut m = s;
            let ev = r.process(l, &s, &mut m, t);
            (m, ev)
        };
        assert_eq!(go(&mut r, pad(&[Buttons::SOUTH]), ms(0)).1, vec![InputEvent::Key { vk: 0x20, down: true }]);
        go(&mut r, pad(&[]), ms(10));
        let (m, ev) = go(&mut r, pad(&[Buttons::HOME, Buttons::SOUTH]), ms(20));
        assert!(!m.buttons.has(Buttons::HOME), "held shift button is hidden from the game");
        assert_eq!(ev, vec![InputEvent::Key { vk: 0x52, down: true }]);
        go(&mut r, pad(&[]), ms(30));
        // a quick tap on its own still reaches the game
        go(&mut r, pad(&[Buttons::HOME]), ms(1000));
        let (m, _) = go(&mut r, pad(&[]), ms(1100));
        assert!(m.buttons.has(Buttons::HOME), "tap passes through");
        let (m, _) = go(&mut r, pad(&[]), ms(1300));
        assert!(!m.buttons.has(Buttons::HOME));
    }

    // ---- 0.3: stages, tap/hold/double, chords, launch --------------------

    fn key(button: &str, vk: u16) -> Binding {
        Binding { button: button.into(), action: Action::Key { vk }, ..Default::default() }
    }

    fn keys_down(ev: &[InputEvent]) -> Vec<u16> {
        ev.iter().filter_map(|e| if let InputEvent::Key { vk, down: true } = e { Some(*vk) } else { None }).collect()
    }

    fn keys_up(ev: &[InputEvent]) -> Vec<u16> {
        ev.iter().filter_map(|e| if let InputEvent::Key { vk, down: false } = e { Some(*vk) } else { None }).collect()
    }

    #[test]
    fn dual_stage_trigger_aims_then_fires() {
        // soft pull = right mouse (aim), full pull = left mouse (fire)
        let b = vec![
            Binding { button: "r2_soft".into(), action: Action::Mouse { button: MouseButton::Right }, ..Default::default() },
            Binding { button: "r2_full".into(), action: Action::Mouse { button: MouseButton::Left }, ..Default::default() },
        ];
        let mut r = Runner::default();
        let t = Instant::now();
        let pull = |v: f32| {
            let mut s = PadState::default();
            s.r2 = v;
            s.buttons.set(Buttons::R2_DIGITAL, v > 0.0);
            s
        };
        let (m, ev) = run(&mut r, &b, &pull(0.05), t);
        assert!(ev.is_empty(), "a brush of the trigger does nothing");
        assert_eq!(m.r2, 0.0, "the game no longer sees the trigger itself");
        assert!(!m.buttons.has(Buttons::R2_DIGITAL));
        let (_, ev) = run(&mut r, &b, &pull(0.5), t);
        assert_eq!(ev, vec![InputEvent::Mouse { button: MouseButton::Right, down: true }], "soft pull aims");
        let (_, ev) = run(&mut r, &b, &pull(1.0), t);
        assert_eq!(ev, vec![InputEvent::Mouse { button: MouseButton::Left, down: true }], "full pull fires, still aiming");
        // easing off just below the full point keeps firing (no chatter)...
        assert!(run(&mut r, &b, &pull(0.88), t).1.is_empty());
        // ...further back stops firing but keeps aiming
        let (_, ev) = run(&mut r, &b, &pull(0.6), t);
        assert_eq!(ev, vec![InputEvent::Mouse { button: MouseButton::Left, down: false }]);
        let (_, ev) = run(&mut r, &b, &pull(0.0), t);
        assert_eq!(ev, vec![InputEvent::Mouse { button: MouseButton::Right, down: false }]);
    }

    #[test]
    fn stage_same_keeps_the_trigger_analog() {
        // the trigger works as normal, and a full pull ALSO presses a key
        let b = vec![Binding { button: "l2_soft".into(), ..Default::default() }, key("l2_full", 0x51)];
        let mut r = Runner::default();
        let t = Instant::now();
        let mut s = PadState::default();
        s.l2 = 0.5;
        s.buttons.set(Buttons::L2_DIGITAL, true);
        let (m, ev) = run(&mut r, &b, &s, t);
        assert_eq!(m.l2, 0.5);
        assert!(m.buttons.has(Buttons::L2_DIGITAL));
        assert!(ev.is_empty());
        assert_eq!(m.buttons.0 & STAGE_MASK, 0, "internal stage bits never leak to the virtual pad");
        s.l2 = 1.0;
        let (m, ev) = run(&mut r, &b, &s, t);
        assert_eq!(m.l2, 1.0);
        assert_eq!(keys_down(&ev), vec![0x51]);
    }

    #[test]
    fn untouched_trigger_is_unaffected_by_the_other_ones_stages() {
        let b = vec![key("r2_full", 0x51)];
        let mut r = Runner::default();
        let mut s = PadState::default();
        s.l2 = 0.7;
        s.buttons.set(Buttons::L2_DIGITAL, true);
        let (m, _) = run(&mut r, &b, &s, Instant::now());
        assert_eq!(m.l2, 0.7);
        assert!(m.buttons.has(Buttons::L2_DIGITAL));
    }

    fn timed() -> Vec<Binding> {
        vec![Binding {
            button: "cross".into(),
            action: Action::Key { vk: 0x41 },           // tap  = A
            hold: Some(Action::Key { vk: 0x42 }),       // hold = B
            double_tap: Some(Action::Key { vk: 0x43 }), // double = C
            ..Default::default()
        }]
    }

    #[test]
    fn tap_fires_after_the_double_tap_window() {
        let b = timed();
        let mut r = Runner::default();
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        let down = pad(&[Buttons::SOUTH]);
        let up = pad(&[]);
        let (m, ev) = run(&mut r, &b, &down, ms(0));
        assert!(ev.is_empty(), "nothing yet: could still become a hold or a double");
        assert!(!m.buttons.has(Buttons::SOUTH));
        assert!(run(&mut r, &b, &up, ms(60)).1.is_empty(), "released: waiting for a second press");
        assert!(run(&mut r, &b, &up, ms(200)).1.is_empty());
        assert_eq!(keys_down(&run(&mut r, &b, &up, ms(320)).1), vec![0x41], "window over: it was a tap");
        assert_eq!(keys_up(&run(&mut r, &b, &up, ms(420)).1), vec![0x41], "tap is a short pulse");
    }

    #[test]
    fn hold_fires_while_held() {
        let b = timed();
        let mut r = Runner::default();
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        let down = pad(&[Buttons::SOUTH]);
        run(&mut r, &b, &down, ms(0));
        assert!(run(&mut r, &b, &down, ms(200)).1.is_empty());
        assert_eq!(keys_down(&run(&mut r, &b, &down, ms(260)).1), vec![0x42], "held past hold_ms");
        assert!(run(&mut r, &b, &down, ms(900)).1.is_empty(), "stays down");
        let ev = run(&mut r, &b, &pad(&[]), ms(1000)).1;
        assert_eq!(keys_up(&ev), vec![0x42]);
        assert!(keys_down(&ev).is_empty());
        assert!(run(&mut r, &b, &pad(&[]), ms(1400)).1.is_empty(), "a hold is not followed by a tap");
    }

    #[test]
    fn double_tap_fires_instead_of_two_taps() {
        let b = timed();
        let mut r = Runner::default();
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        let down = pad(&[Buttons::SOUTH]);
        let up = pad(&[]);
        run(&mut r, &b, &down, ms(0));
        run(&mut r, &b, &up, ms(50));
        assert_eq!(keys_down(&run(&mut r, &b, &down, ms(150)).1), vec![0x43], "second press in time = double");
        let mut all = vec![];
        for t in [160, 200, 260, 400, 700] {
            all.extend(run(&mut r, &b, &up, ms(t)).1);
        }
        assert_eq!(keys_up(&all), vec![0x43]);
        assert!(keys_down(&all).is_empty(), "no stray tap (A) afterwards");
    }

    #[test]
    fn tap_only_binding_without_double_fires_on_release() {
        // tap = A, hold = B, no double-tap: the tap doesn't have to wait
        let mut b = timed();
        b[0].double_tap = None;
        let mut r = Runner::default();
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        run(&mut r, &b, &pad(&[Buttons::SOUTH]), ms(0));
        assert_eq!(keys_down(&run(&mut r, &b, &pad(&[]), ms(40)).1), vec![0x41]);
    }

    #[test]
    fn long_press_holds_the_normal_action_when_there_is_no_hold_action() {
        // only a double-tap action: holding the button holds the normal one
        let b = vec![Binding { button: "r1".into(), double_tap: Some(Action::Key { vk: 0x43 }), ..Default::default() }];
        let mut r = Runner::default();
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        let down = pad(&[Buttons::R1]);
        assert!(!run(&mut r, &b, &down, ms(0)).0.buttons.has(Buttons::R1));
        assert!(run(&mut r, &b, &down, ms(300)).0.buttons.has(Buttons::R1), "long press = R1 held");
        assert!(run(&mut r, &b, &down, ms(800)).0.buttons.has(Buttons::R1));
        assert!(!run(&mut r, &b, &pad(&[]), ms(900)).0.buttons.has(Buttons::R1));
    }

    #[test]
    fn chord_fires_and_hides_both_buttons() {
        let chords = vec![ChordBinding { buttons: vec!["l1".into(), "r1".into()], action: Action::Key { vk: 0x70 } }];
        let base = vec![key("r1", 0x45)];
        let l = Layers { chords: &chords, ..Layers::base(&base) };
        let mut r = Runner::default();
        let t = Instant::now();
        let go = |r: &mut Runner, s: PadState| {
            let mut m = s;
            let ev = r.process(l, &s, &mut m, t);
            (m, ev)
        };
        // L1 alone is just L1
        let (m, ev) = go(&mut r, pad(&[Buttons::L1]));
        assert!(m.buttons.has(Buttons::L1));
        assert!(ev.is_empty());
        // both: the chord's key, and neither button (nor R1's own binding)
        let (m, ev) = go(&mut r, pad(&[Buttons::L1, Buttons::R1]));
        assert!(!m.buttons.has(Buttons::L1) && !m.buttons.has(Buttons::R1));
        assert_eq!(keys_down(&ev), vec![0x70]);
        // let go of R1 only: chord ends, L1 stays hidden until it's released too
        let (m, ev) = go(&mut r, pad(&[Buttons::L1]));
        assert_eq!(keys_up(&ev), vec![0x70]);
        assert!(keys_down(&ev).is_empty());
        assert!(!m.buttons.has(Buttons::L1), "no stray L1 after the chord");
        go(&mut r, pad(&[]));
        // afterwards both work on their own again
        assert!(go(&mut r, pad(&[Buttons::L1])).0.buttons.has(Buttons::L1));
        go(&mut r, pad(&[]));
        assert_eq!(keys_down(&go(&mut r, pad(&[Buttons::R1])).1), vec![0x45]);
    }

    #[test]
    fn half_a_chord_definition_is_ignored() {
        let chords = vec![
            ChordBinding { buttons: vec!["l1".into()], action: Action::Key { vk: 0x70 } },
            ChordBinding { buttons: vec!["l1".into(), "nonsense".into()], action: Action::Key { vk: 0x71 } },
        ];
        let l = Layers { chords: &chords, ..Layers::base(&[]) };
        let mut r = Runner::default();
        let s = pad(&[Buttons::L1]);
        let mut m = s;
        assert!(r.process(l, &s, &mut m, Instant::now()).is_empty());
        assert!(m.buttons.has(Buttons::L1));
    }

    #[test]
    fn launch_fires_once_per_press() {
        let b = vec![Binding {
            button: "touchpad".into(),
            action: Action::Launch { target: " C:\\Apps\\Discord.exe ".into(), args: String::new() },
            ..Default::default()
        }];
        let mut r = Runner::default();
        let t = Instant::now();
        let (m, ev) = run(&mut r, &b, &pad(&[Buttons::TOUCHPAD]), t);
        assert!(ev.is_empty());
        assert!(!m.buttons.has(Buttons::TOUCHPAD));
        run(&mut r, &b, &pad(&[Buttons::TOUCHPAD]), t); // still held: no second launch
        let l = r.take_launches();
        assert_eq!(l, vec![LaunchRequest { target: "C:\\Apps\\Discord.exe".into(), args: String::new() }]);
        assert!(r.take_launches().is_empty());
        run(&mut r, &b, &pad(&[]), t);
        run(&mut r, &b, &pad(&[Buttons::TOUCHPAD]), t);
        assert_eq!(r.take_launches().len(), 1, "pressing again launches again");
    }

    #[test]
    fn empty_launch_target_does_nothing() {
        let b = vec![Binding {
            button: "cross".into(),
            action: Action::Launch { target: "   ".into(), args: "x".into() },
            ..Default::default()
        }];
        let mut r = Runner::default();
        run(&mut r, &b, &pad(&[Buttons::SOUTH]), Instant::now());
        assert!(r.take_launches().is_empty());
    }

    #[test]
    fn edge_fn_buttons_are_mappable() {
        let b = vec![key("fn_l", 0x31), key("paddle_r", 0x32)];
        let mut r = Runner::default();
        let (_, ev) = run(&mut r, &b, &pad(&[Buttons::FN_L, Buttons::PADDLE_R]), Instant::now());
        let mut k = keys_down(&ev);
        k.sort();
        assert_eq!(k, vec![0x31, 0x32]);
    }

    #[test]
    fn new_binding_fields_round_trip() {
        let mut p = crate::profile::Profile::default();
        p.bindings = timed();
        p.bindings.push(Binding {
            button: "share".into(),
            action: Action::Launch { target: "https://example.com".into(), args: String::new() },
            ..Default::default()
        });
        p.chords = vec![ChordBinding { buttons: vec!["l1".into(), "r1".into()], action: Action::Key { vk: 0x70 } }];
        let text = toml::to_string_pretty(&p).unwrap();
        let back: crate::profile::Profile = toml::from_str(&text).unwrap();
        assert_eq!(back.bindings, p.bindings);
        assert_eq!(back.chords, p.chords);
        let json = serde_json::to_string(&p).unwrap();
        let back: crate::profile::Profile = serde_json::from_str(&json).unwrap();
        assert_eq!(back.bindings, p.bindings);
        // what the UI sends for "no hold action": the key is simply absent, or null
        let b: Binding = serde_json::from_str(r#"{"button":"cross","action":{"type":"same"},"hold":null}"#).unwrap();
        assert_eq!(b.hold, None);
    }

    #[test]
    fn toml_round_trip() {
        let mut p = crate::profile::Profile::default();
        p.bindings = vec![
            Binding { button: "cross".into(), action: Action::Key { vk: 0x20 }, turbo: true, ..Default::default() },
            Binding { button: "r2".into(), action: Action::Mouse { button: MouseButton::Left }, ..Default::default() },
            Binding { button: "l1".into(), action: Action::Same, toggle: true, ..Default::default() },
            Binding { button: "square".into(), action: Action::Button { to: "circle".into() }, ..Default::default() },
            Binding {
                button: "r3".into(),
                action: Action::Macro {
                    steps: vec![MacroStep { presses: vec![Press::Key { vk: 0x11 }, Press::Mouse { button: MouseButton::Left }], hold_ms: 30, gap_ms: 5 }],
                    repeat: true,
                },
                ..Default::default()
            },
        ];
        p.shift_button = "ps".into();
        p.shift_bindings = vec![p.bindings[0].clone()];
        let text = toml::to_string_pretty(&p).unwrap();
        let back: crate::profile::Profile = toml::from_str(&text).unwrap();
        assert_eq!(back.bindings, p.bindings);
        assert_eq!(back.shift_bindings, p.shift_bindings);
        let json = serde_json::to_string(&p).unwrap();
        let back: crate::profile::Profile = serde_json::from_str(&json).unwrap();
        assert_eq!(back.bindings, p.bindings);
    }
}
