//! System chords: hold PS (Guide) as a modifier and the other buttons trigger
//! DXO or Windows actions instead of reaching the game. A quick PS tap on its
//! own still passes through. Runs in the per-pad worker, so it works while you
//! game with DXO sitting in the tray.
//!
//! Which button does what is a table the Chords page edits (`set_map`).
//! Defaults:
//!   PS + Share         -> power off the controller (Bluetooth)
//!   PS + D-pad Up/Down -> volume up / down
//!   PS + D-pad Left    -> mute
//!   PS + Touchpad      -> screenshot (Win+PrtScn, saved to Pictures\Screenshots)
//!   PS + L3            -> show / hide the DXO window
//!   PS + Options       -> show / hide the on-screen keyboard
//!   PS + R3            -> the quick menu (radial)

use crate::bindings::{self, InputEvent, LaunchRequest};
use crate::mapping::button_from_name;
use crate::state::{Buttons, PadState};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

// Windows virtual-key codes.
const VK_VOLUME_MUTE: u16 = 0xAD;
const VK_VOLUME_DOWN: u16 = 0xAE;
const VK_VOLUME_UP: u16 = 0xAF;
const VK_MEDIA_NEXT: u16 = 0xB0;
const VK_MEDIA_PREV: u16 = 0xB1;
const VK_MEDIA_PLAY_PAUSE: u16 = 0xB3;
const VK_LWIN: u16 = 0x5B;
const VK_SNAPSHOT: u16 = 0x2C; // PrtScn

/// A held PS released within this, with nothing else pressed, is a tap that
/// still reaches the game (so Steam/Guide keeps working).
const TAP: Duration = Duration::from_millis(300);
const TAP_PULSE: Duration = Duration::from_millis(80);
const VOL_REPEAT: Duration = Duration::from_millis(120);

/// What a chord (or a quick-menu item) does.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum SysAction {
    #[default]
    None,
    PowerOff,
    VolumeUp,
    VolumeDown,
    Mute,
    Screenshot,
    ToggleWindow,
    ToggleKeyboard,
    RadialMenu,
    MediaPlayPause,
    MediaNext,
    MediaPrev,
    /// Keys pressed together, by virtual-key code (e.g. Ctrl+Shift+Esc).
    Keys { keys: Vec<u16> },
    /// Open a program, file, folder or web address.
    Launch {
        target: String,
        #[serde(default)]
        args: String,
    },
}

impl SysAction {
    /// Worth repeating while the button is held.
    fn repeats(&self) -> bool {
        matches!(self, SysAction::VolumeUp | SysAction::VolumeDown)
    }
}

/// One row of the chord table: PS + `button` -> `action`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SysBinding {
    /// Button name as in profiles: "share", "options", "l3", "up"...
    pub button: String,
    pub action: SysAction,
}

/// The chords DXO ships with.
pub fn default_map() -> Vec<SysBinding> {
    [
        ("share", SysAction::PowerOff),
        ("up", SysAction::VolumeUp),
        ("down", SysAction::VolumeDown),
        ("left", SysAction::Mute),
        ("touchpad", SysAction::Screenshot),
        ("l3", SysAction::ToggleWindow),
        ("options", SysAction::ToggleKeyboard),
        ("r3", SysAction::RadialMenu),
    ]
    .into_iter()
    .map(|(b, a)| SysBinding { button: b.into(), action: a })
    .collect()
}

/// Actions the worker carries out (it holds the Bluetooth MAC, the window
/// flag and the overlays). Key presses are sent from here directly.
#[derive(Debug, Default, Clone)]
pub struct ChordFired {
    pub power_off: bool,
    pub toggle_window: bool,
    pub toggle_keyboard: bool,
    pub toggle_radial: bool,
    pub launches: Vec<LaunchRequest>,
}

/// Carry out one action: keys are sent straight away, everything that needs
/// the worker is noted in `fired`. Shared by chords and the quick menu.
pub fn perform(action: &SysAction, fired: &mut ChordFired) {
    match action {
        SysAction::None => {}
        SysAction::PowerOff => fired.power_off = true,
        SysAction::ToggleWindow => fired.toggle_window = true,
        SysAction::ToggleKeyboard => fired.toggle_keyboard = true,
        SysAction::RadialMenu => fired.toggle_radial = true,
        SysAction::VolumeUp => tap_key(VK_VOLUME_UP),
        SysAction::VolumeDown => tap_key(VK_VOLUME_DOWN),
        SysAction::Mute => tap_key(VK_VOLUME_MUTE),
        SysAction::MediaPlayPause => tap_key(VK_MEDIA_PLAY_PAUSE),
        SysAction::MediaNext => tap_key(VK_MEDIA_NEXT),
        SysAction::MediaPrev => tap_key(VK_MEDIA_PREV),
        SysAction::Screenshot => press_together(&[VK_LWIN, VK_SNAPSHOT]),
        SysAction::Keys { keys } => press_together(keys),
        SysAction::Launch { target, args } => {
            if !target.trim().is_empty() {
                fired.launches.push(LaunchRequest { target: target.trim().to_string(), args: args.trim().to_string() });
            }
        }
    }
}

#[derive(Debug)]
pub struct Chords {
    /// (button bit, action). PS itself and unknown names are left out.
    map: Vec<(u32, SysAction)>,
    prev: u32,
    home_at: Option<Instant>,
    used: bool,
    tap_until: Option<Instant>,
    /// A repeating action: (button bit, next time).
    repeat: Option<(u32, Instant)>,
}

impl Default for Chords {
    fn default() -> Self {
        let mut c = Chords { map: Vec::new(), prev: 0, home_at: None, used: false, tap_until: None, repeat: None };
        c.set_map(&default_map());
        c
    }
}

impl Chords {
    /// Replace the chord table. A button listed twice keeps its first action.
    pub fn set_map(&mut self, map: &[SysBinding]) {
        self.map.clear();
        for b in map {
            let Some(bit) = button_from_name(&b.button) else { continue };
            if bit == Buttons::HOME || b.action == SysAction::None || self.map.iter().any(|(x, _)| *x == bit) {
                continue;
            }
            self.map.push((bit, b.action.clone()));
        }
    }

    /// Read the RAW state, carry out key actions, hide the combo from `mapped`
    /// (what the game sees), and return the higher-level actions for the worker.
    pub fn process(&mut self, raw: &PadState, mapped: &mut PadState, now: Instant) -> ChordFired {
        let mut fired = ChordFired::default();
        // The triggers count as buttons here once pulled most of the way.
        let mut b = raw.buttons.0;
        if raw.l2 > 0.6 {
            b |= Buttons::L2_DIGITAL;
        }
        if raw.r2 > 0.6 {
            b |= Buttons::R2_DIGITAL;
        }
        let home = b & Buttons::HOME != 0;

        if home {
            if self.home_at.is_none() {
                self.home_at = Some(now);
                self.used = false;
            }
            for (bit, action) in &self.map {
                let down = b & bit != 0;
                if down && self.prev & bit == 0 {
                    perform(action, &mut fired);
                    self.used = true;
                    if action.repeats() {
                        self.repeat = Some((*bit, now + VOL_REPEAT));
                    }
                } else if down && action.repeats() {
                    // Volume ramps while the button is held.
                    if let Some((rb, next)) = self.repeat {
                        if rb == *bit && now >= next {
                            perform(action, &mut fired);
                            self.repeat = Some((*bit, now + VOL_REPEAT));
                        }
                    }
                }
            }
            if self.repeat.is_some_and(|(rb, _)| b & rb == 0) {
                self.repeat = None;
            }
        } else {
            if let Some(t0) = self.home_at.take() {
                if !self.used && now.duration_since(t0) < TAP {
                    self.tap_until = Some(now + TAP_PULSE);
                }
            }
            self.repeat = None;
        }

        // ---- what the game sees ----
        let tap = self.tap_until.is_some_and(|u| now < u);
        if !tap {
            self.tap_until = None;
        }
        if home {
            for (bit, _) in &self.map {
                mapped.buttons.set(*bit, false);
                if *bit == Buttons::L2_DIGITAL {
                    mapped.l2 = 0.0;
                }
                if *bit == Buttons::R2_DIGITAL {
                    mapped.r2 = 0.0;
                }
            }
        }
        mapped.buttons.set(Buttons::HOME, tap); // held = hidden; quick tap = a Guide pulse

        self.prev = b;
        fired
    }
}

/// Press and release one key.
fn tap_key(vk: u16) {
    bindings::send(&[InputEvent::Key { vk, down: true }, InputEvent::Key { vk, down: false }]);
}

/// Press keys in order, then let go in reverse (a shortcut such as Win+PrtScn:
/// the whole screen straight to a PNG in Pictures\Screenshots).
fn press_together(keys: &[u16]) {
    let mut ev: Vec<InputEvent> = keys.iter().take(6).map(|&vk| InputEvent::Key { vk, down: true }).collect();
    ev.extend(keys.iter().take(6).rev().map(|&vk| InputEvent::Key { vk, down: false }));
    bindings::send(&ev);
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

    #[test]
    fn ps_share_powers_off_and_is_hidden_from_the_game() {
        let mut c = Chords::default();
        let t = Instant::now();
        let raw = pad(&[Buttons::HOME, Buttons::SELECT]);
        let mut m = raw;
        let fired = c.process(&raw, &mut m, t);
        assert!(fired.power_off);
        assert!(!m.buttons.has(Buttons::HOME), "game must not see PS");
        assert!(!m.buttons.has(Buttons::SELECT), "game must not see Share");
    }

    #[test]
    fn ps_options_toggles_the_keyboard_and_is_hidden_from_the_game() {
        let mut c = Chords::default();
        let t = Instant::now();
        let raw = pad(&[Buttons::HOME, Buttons::START]);
        let mut m = raw;
        let fired = c.process(&raw, &mut m, t);
        assert!(fired.toggle_keyboard);
        assert!(!m.buttons.has(Buttons::START), "game must not see Options");
        // still held on the next report: fires once, not every report
        let mut m = raw;
        assert!(!c.process(&raw, &mut m, t + Duration::from_millis(8)).toggle_keyboard);
    }

    #[test]
    fn the_table_can_be_changed() {
        let mut c = Chords::default();
        c.set_map(&[
            SysBinding { button: "cross".into(), action: SysAction::ToggleWindow },
            SysBinding { button: "cross".into(), action: SysAction::PowerOff }, // second entry for a button is ignored
            SysBinding { button: "share".into(), action: SysAction::None },     // nothing assigned
            SysBinding { button: "ps".into(), action: SysAction::Mute },        // the modifier can't be a chord
            SysBinding { button: "circle".into(), action: SysAction::Launch { target: " notepad.exe ".into(), args: String::new() } },
            SysBinding { button: "r3".into(), action: SysAction::RadialMenu },
        ]);
        let t = Instant::now();
        let raw = pad(&[Buttons::HOME, Buttons::SOUTH, Buttons::SELECT, Buttons::EAST, Buttons::R3]);
        let mut m = raw;
        let fired = c.process(&raw, &mut m, t);
        assert!(fired.toggle_window && !fired.power_off && fired.toggle_radial);
        assert_eq!(fired.launches, vec![LaunchRequest { target: "notepad.exe".into(), args: String::new() }]);
        assert!(!m.buttons.has(Buttons::SOUTH), "a bound button is hidden from the game");
        assert!(m.buttons.has(Buttons::SELECT), "an unbound one is not");
    }

    #[test]
    fn default_table_round_trips_as_json() {
        let json = serde_json::to_string(&default_map()).unwrap();
        assert!(json.contains(r#"{"button":"options","action":{"type":"toggle_keyboard"}}"#));
        let back: Vec<SysBinding> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, default_map());
        let keys: SysAction = serde_json::from_str(r#"{"type":"keys","keys":[17,16,27]}"#).unwrap();
        assert_eq!(keys, SysAction::Keys { keys: vec![17, 16, 27] });
    }

    #[test]
    fn quick_ps_tap_passes_through() {
        let mut c = Chords::default();
        let t = Instant::now();
        // press PS alone
        let down = pad(&[Buttons::HOME]);
        c.process(&down, &mut down.clone(), t);
        // release soon after
        let up = PadState::default();
        let mut m = up;
        c.process(&up, &mut m, t + Duration::from_millis(100));
        assert!(m.buttons.has(Buttons::HOME), "a quick tap reaches the game");
        // and the pulse ends
        let mut m2 = PadState::default();
        c.process(&up, &mut m2, t + Duration::from_millis(300));
        assert!(!m2.buttons.has(Buttons::HOME));
    }

    #[test]
    fn holding_ps_with_a_chord_is_not_a_tap() {
        let mut c = Chords::default();
        let t = Instant::now();
        c.process(&pad(&[Buttons::HOME, Buttons::L3]), &mut pad(&[Buttons::HOME, Buttons::L3]), t);
        let mut m = PadState::default();
        c.process(&PadState::default(), &mut m, t + Duration::from_millis(50));
        assert!(!m.buttons.has(Buttons::HOME), "used as a chord, so no tap-through");
    }
}
