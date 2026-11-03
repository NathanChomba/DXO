//! M5: per-button bindings, the Mappings page's engine.
//!
//! A binding takes one physical button and decides what it does:
//!   * `Same`     - stays itself (useful with turbo / toggle)
//!   * `Button`   - becomes another pad button
//!   * `Key`      - presses a keyboard key (sent as a scancode, so DirectInput
//!                  games and anti-cheat-friendly titles see it too)
//!   * `Mouse`    - clicks a mouse button or turns the wheel
//!   * `Disabled` - does nothing
//! plus `toggle` (press once = hold, press again = release) and `turbo`
//! (rapid repeat while held).
//!
//! `mapping::apply` stays a pure function; this `Runner` holds the state that
//! turbo/toggle need and turns key/mouse actions into `InputEvent`s.

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
    /// Source button name (`mapping::button_from_name`).
    pub button: String,
    pub action: Action,
    pub turbo: bool,
    /// Presses per second while turbo is active.
    pub turbo_hz: f32,
    pub toggle: bool,
}

impl Default for Binding {
    fn default() -> Self {
        Self { button: String::new(), action: Action::Same, turbo: false, turbo_hz: 10.0, toggle: false }
    }
}

/// The normal bindings plus the optional shift layer.
#[derive(Debug, Clone, Copy)]
pub struct Layers<'a> {
    pub base: &'a [Binding],
    /// Button that switches to `shift` while held; "" = no shift layer.
    pub shift_button: &'a str,
    pub shift: &'a [Binding],
}

impl<'a> Layers<'a> {
    pub fn base(b: &'a [Binding]) -> Self {
        Self { base: b, shift_button: "", shift: &[] }
    }

    pub fn of(p: &'a crate::profile::Profile) -> Self {
        Self { base: &p.bindings, shift_button: &p.shift_button, shift: &p.shift_bindings }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEvent {
    Key { vk: u16, down: bool },
    Mouse { button: MouseButton, down: bool },
    /// Wheel notches, + = up.
    Wheel(i32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Out {
    Key(u16),
    Mouse(MouseButton),
}

/// (layer, binding index): 0 = normal, 1 = shift.
type Slot = (u8, usize);

#[derive(Debug)]
struct Playing {
    start: Instant,
    step: Option<usize>,
}

/// A quick tap of the shift button (shorter than this, nothing else pressed)
/// is passed to the game as a normal press.
const SHIFT_TAP: Duration = Duration::from_millis(300);
const TAP_PULSE: Duration = Duration::from_millis(80);

#[derive(Debug, Default)]
pub struct Runner {
    started: Option<Instant>,
    prev_raw: u32,
    latched: HashMap<(u8, String), bool>,
    prev_active: HashMap<Slot, bool>,
    held: HashSet<Out>,
    macros: HashMap<Slot, Playing>,
    shift_down_at: Option<Instant>,
    shift_used: bool,
    tap_until: Option<Instant>,
    /// Bindings the state above belongs to; a different set resets it.
    last: (Vec<Binding>, String, Vec<Binding>),
}

impl Runner {
    /// Apply the bindings to `mapped` (the pad state games will see) and return
    /// the keyboard/mouse events to send now.
    pub fn process(&mut self, layers: Layers, raw: &PadState, mapped: &mut PadState, now: Instant) -> Vec<InputEvent> {
        let mut events = vec![];
        if self.last.0 != layers.base || self.last.1 != layers.shift_button || self.last.2 != layers.shift {
            // Profile changed: let go of anything the old bindings held.
            events.extend(self.release_all());
            self.last = (layers.base.to_vec(), layers.shift_button.to_string(), layers.shift.to_vec());
        }
        let t = now.duration_since(*self.started.get_or_insert(now)).as_secs_f32();
        let orig = *mapped;
        let mut want: HashSet<Out> = HashSet::new();

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
            mapped.buttons.set(sb, tap);
            set_analog(mapped, sb, if tap { 1.0 } else { 0.0 });
        }

        let shifted = |name: &str| shift_on && layers.shift.iter().any(|b| b.button == name);
        let mut list: Vec<(Slot, &Binding)> = vec![];
        for (i, b) in layers.base.iter().enumerate() {
            let skip = shifted(&b.button) || shift_src.is_some() && button_from_name(&b.button) == shift_src;
            if skip {
                self.prev_active.insert((0, i), false);
            } else {
                list.push(((0, i), b));
            }
        }
        if shift_on {
            for (i, b) in layers.shift.iter().enumerate() {
                if button_from_name(&b.button) != shift_src {
                    list.push(((1, i), b));
                }
            }
        } else {
            for i in 0..layers.shift.len() {
                self.prev_active.insert((1, i), false);
            }
        }

        // ---- bindings -------------------------------------------------------
        for (slot, b) in list {
            let Some(src) = button_from_name(&b.button) else { continue };
            let pressed = raw.buttons.has(src);
            let was = self.prev_raw & src != 0;

            let mut active = pressed;
            if b.toggle {
                let l = self.latched.entry((slot.0, b.button.clone())).or_insert(false);
                if pressed && !was {
                    *l = !*l;
                }
                active = *l;
            }
            if b.turbo && active {
                let hz = b.turbo_hz.clamp(1.0, 30.0);
                active = ((t * hz * 2.0) as u64) % 2 == 0; // on for half of each cycle
            }
            let rising = active && !self.prev_active.get(&slot).copied().unwrap_or(false);
            self.prev_active.insert(slot, active);

            // The source no longer reaches the game as itself...
            mapped.buttons.set(src, false);
            set_analog(mapped, src, 0.0);

            // ...unless the action says so.
            match &b.action {
                Action::Same => {
                    if active {
                        mapped.buttons.set(src, true);
                        let a = analog(&orig, src).filter(|_| pressed && !b.toggle && !b.turbo).unwrap_or(1.0);
                        set_analog(mapped, src, a);
                    }
                }
                Action::Button { to } => {
                    if active {
                        press(&Press::Button { to: to.clone() }, rising, mapped, &mut want, &mut events);
                    }
                }
                Action::Key { vk } => {
                    if active && *vk != 0 {
                        want.insert(Out::Key(*vk));
                    }
                }
                Action::Mouse { button } => {
                    if active {
                        press(&Press::Mouse { button: *button }, rising, mapped, &mut want, &mut events);
                    }
                }
                Action::Disabled => {}
                Action::Macro { steps, repeat } => {
                    if rising {
                        self.macros.insert(slot, Playing { start: now, step: None });
                    }
                    let total: u64 = steps.iter().map(|s| s.hold_ms as u64 + s.gap_ms as u64).sum();
                    let Some(p) = self.macros.get_mut(&slot) else { continue };
                    let mut el = now.duration_since(p.start).as_millis() as u64;
                    if total == 0 {
                        self.macros.remove(&slot);
                        continue;
                    }
                    if el >= total {
                        if *repeat && active {
                            p.start += Duration::from_millis(total * (el / total));
                            el %= total;
                            p.step = None;
                        } else {
                            self.macros.remove(&slot);
                            continue;
                        }
                    }
                    let mut at = 0u64;
                    for (n, st) in steps.iter().enumerate() {
                        if el >= at && el < at + st.hold_ms as u64 {
                            let entering = p.step != Some(n);
                            p.step = Some(n);
                            for pr in &st.presses {
                                press(pr, entering, mapped, &mut want, &mut events);
                            }
                            break;
                        }
                        at += st.hold_ms as u64 + st.gap_ms as u64;
                    }
                }
            }
        }
        self.prev_raw = raw.buttons.0;

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

    /// Release every key / mouse button we're holding (disconnect, profile
    /// switch, shutdown). Never leave a key stuck down.
    pub fn release_all(&mut self) -> Vec<InputEvent> {
        let ev = self.held.drain().map(release).collect();
        self.latched.clear();
        self.prev_active.clear();
        self.macros.clear();
        self.shift_down_at = None;
        self.tap_until = None;
        ev
    }
}

/// Hold one press this frame. Wheel turns only fire on `rising`.
fn press(p: &Press, rising: bool, mapped: &mut PadState, want: &mut HashSet<Out>, events: &mut Vec<InputEvent>) {
    match p {
        Press::Button { to } => {
            if let Some(dst) = button_from_name(to) {
                mapped.buttons.set(dst, true);
                set_analog(mapped, dst, 1.0);
            }
        }
        Press::Key { vk } => {
            if *vk != 0 {
                want.insert(Out::Key(*vk));
            }
        }
        Press::Mouse { button: MouseButton::WheelUp } => {
            if rising {
                events.push(InputEvent::Wheel(1));
            }
        }
        Press::Mouse { button: MouseButton::WheelDown } => {
            if rising {
                events.push(InputEvent::Wheel(-1));
            }
        }
        Press::Mouse { button } => {
            want.insert(Out::Mouse(*button));
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
        let l = Layers { base: &base, shift_button: "ps", shift: &shift };
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
