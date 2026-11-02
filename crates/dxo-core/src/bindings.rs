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
use std::time::Instant;

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

#[derive(Debug, Default)]
pub struct Runner {
    started: Option<Instant>,
    prev_raw: u32,
    latched: HashMap<String, bool>,
    prev_active: HashMap<usize, bool>,
    held: HashSet<Out>,
    /// Bindings the state above belongs to; a different set resets it.
    last: Vec<Binding>,
}

impl Runner {
    /// Apply `bindings` to `mapped` (the pad state games will see) and return
    /// the keyboard/mouse events to send now.
    pub fn process(&mut self, bindings: &[Binding], raw: &PadState, mapped: &mut PadState, now: Instant) -> Vec<InputEvent> {
        let mut events = vec![];
        if self.last.as_slice() != bindings {
            // Profile changed: let go of anything the old bindings held.
            events.extend(self.release_all());
            self.last = bindings.to_vec();
        }
        let t = now.duration_since(*self.started.get_or_insert(now)).as_secs_f32();
        let orig = *mapped;
        let mut want: HashSet<Out> = HashSet::new();

        for (i, b) in bindings.iter().enumerate() {
            let Some(src) = button_from_name(&b.button) else { continue };
            let pressed = raw.buttons.has(src);
            let was = self.prev_raw & src != 0;

            let mut active = pressed;
            if b.toggle {
                let l = self.latched.entry(b.button.clone()).or_insert(false);
                if pressed && !was {
                    *l = !*l;
                }
                active = *l;
            }
            if b.turbo && active {
                let hz = b.turbo_hz.clamp(1.0, 30.0);
                active = ((t * hz * 2.0) as u64) % 2 == 0; // on for half of each cycle
            }
            let rising = active && !self.prev_active.get(&i).copied().unwrap_or(false);
            self.prev_active.insert(i, active);

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
                    if let (true, Some(dst)) = (active, button_from_name(to)) {
                        mapped.buttons.set(dst, true);
                        set_analog(mapped, dst, 1.0);
                    }
                }
                Action::Key { vk } => {
                    if active && *vk != 0 {
                        want.insert(Out::Key(*vk));
                    }
                }
                Action::Mouse { button: MouseButton::WheelUp } => {
                    if rising {
                        events.push(InputEvent::Wheel(1));
                    }
                }
                Action::Mouse { button: MouseButton::WheelDown } => {
                    if rising {
                        events.push(InputEvent::Wheel(-1));
                    }
                }
                Action::Mouse { button } => {
                    if active {
                        want.insert(Out::Mouse(*button));
                    }
                }
                Action::Disabled => {}
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
        ev
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
    use std::time::Duration;

    fn pad(buttons: &[u32]) -> PadState {
        let mut s = PadState::default();
        for b in buttons {
            s.buttons.set(*b, true);
        }
        s
    }

    fn run(r: &mut Runner, b: &[Binding], raw: &PadState, t: Instant) -> (PadState, Vec<InputEvent>) {
        let mut m = *raw;
        let ev = r.process(b, raw, &mut m, t);
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

    #[test]
    fn toml_round_trip() {
        let mut p = crate::profile::Profile::default();
        p.bindings = vec![
            Binding { button: "cross".into(), action: Action::Key { vk: 0x20 }, turbo: true, ..Default::default() },
            Binding { button: "r2".into(), action: Action::Mouse { button: MouseButton::Left }, ..Default::default() },
            Binding { button: "l1".into(), action: Action::Same, toggle: true, ..Default::default() },
            Binding { button: "square".into(), action: Action::Button { to: "circle".into() }, ..Default::default() },
        ];
        let text = toml::to_string_pretty(&p).unwrap();
        let back: crate::profile::Profile = toml::from_str(&text).unwrap();
        assert_eq!(back.bindings, p.bindings);
        let json = serde_json::to_string(&p).unwrap();
        let back: crate::profile::Profile = serde_json::from_str(&json).unwrap();
        assert_eq!(back.bindings, p.bindings);
    }
}
