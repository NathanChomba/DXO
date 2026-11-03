//! System chords: hold PS (Guide) as a modifier and the other buttons trigger
//! DXO or Windows actions instead of reaching the game. A quick PS tap on its
//! own still passes through. Runs in the per-pad worker, so it works while you
//! game with DXO sitting in the tray.
//!
//! Defaults (fixed for now; a Chords page can make them assignable later):
//!   PS + Share         -> power off the controller (Bluetooth)
//!   PS + D-pad Up/Down -> volume up / down
//!   PS + D-pad Left    -> mute
//!   PS + Touchpad      -> screenshot (Win+PrtScn, saved to Pictures\Screenshots)
//!   PS + L3            -> show / hide the DXO window

use crate::bindings::{self, InputEvent};
use crate::state::{Buttons, PadState};
use std::time::{Duration, Instant};

// Windows virtual-key codes.
const VK_VOLUME_MUTE: u16 = 0xAD;
const VK_VOLUME_DOWN: u16 = 0xAE;
const VK_VOLUME_UP: u16 = 0xAF;
const VK_LWIN: u16 = 0x5B;
const VK_SNAPSHOT: u16 = 0x2C; // PrtScn

/// A held PS released within this, with nothing else pressed, is a tap that
/// still reaches the game (so Steam/Guide keeps working).
const TAP: Duration = Duration::from_millis(300);
const TAP_PULSE: Duration = Duration::from_millis(80);
const VOL_REPEAT: Duration = Duration::from_millis(120);

/// Buttons the modifier swallows while held, so the combo never reaches the game.
const SWALLOW: [u32; 6] = [
    Buttons::SELECT,
    Buttons::TOUCHPAD,
    Buttons::L3,
    Buttons::DPAD_UP,
    Buttons::DPAD_DOWN,
    Buttons::DPAD_LEFT,
];

/// Actions the worker carries out (it holds the Bluetooth MAC and the window
/// flag). Key presses are sent from here directly.
#[derive(Debug, Default, Clone, Copy)]
pub struct ChordFired {
    pub power_off: bool,
    pub toggle_window: bool,
}

#[derive(Debug, Default)]
pub struct Chords {
    prev: u32,
    home_at: Option<Instant>,
    used: bool,
    tap_until: Option<Instant>,
    vol_next: Option<Instant>,
}

impl Chords {
    /// Read the RAW state, carry out key actions, hide the combo from `mapped`
    /// (what the game sees), and return the higher-level actions for the worker.
    pub fn process(&mut self, raw: &PadState, mapped: &mut PadState, now: Instant) -> ChordFired {
        let mut fired = ChordFired::default();
        let b = raw.buttons.0;
        let home = b & Buttons::HOME != 0;
        let edge = |bit: u32| (b & bit != 0) && (self.prev & bit == 0);

        if home {
            if self.home_at.is_none() {
                self.home_at = Some(now);
                self.used = false;
            }
            if edge(Buttons::SELECT) {
                fired.power_off = true;
                self.used = true;
            }
            if edge(Buttons::L3) {
                fired.toggle_window = true;
                self.used = true;
            }
            if edge(Buttons::TOUCHPAD) {
                screenshot();
                self.used = true;
            }
            if edge(Buttons::DPAD_LEFT) {
                tap_key(VK_VOLUME_MUTE);
                self.used = true;
            }

            // Volume ramps while Up/Down is held.
            let vol = if raw.buttons.has(Buttons::DPAD_UP) {
                Some(VK_VOLUME_UP)
            } else if raw.buttons.has(Buttons::DPAD_DOWN) {
                Some(VK_VOLUME_DOWN)
            } else {
                None
            };
            match vol {
                Some(vk) if self.vol_next.is_none_or(|t| now >= t) => {
                    tap_key(vk);
                    self.vol_next = Some(now + VOL_REPEAT);
                    self.used = true;
                }
                Some(_) => {}
                None => self.vol_next = None,
            }
        } else {
            if let Some(t0) = self.home_at.take() {
                if !self.used && now.duration_since(t0) < TAP {
                    self.tap_until = Some(now + TAP_PULSE);
                }
            }
            self.vol_next = None;
        }

        // ---- what the game sees ----
        let tap = self.tap_until.is_some_and(|u| now < u);
        if !tap {
            self.tap_until = None;
        }
        if home {
            for bit in SWALLOW {
                mapped.buttons.set(bit, false);
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

/// Win+PrtScn: capture the whole screen straight to a PNG in
/// Pictures\Screenshots — no overlay, no region to draw, no further clicks.
fn screenshot() {
    bindings::send(&[
        InputEvent::Key { vk: VK_LWIN, down: true },
        InputEvent::Key { vk: VK_SNAPSHOT, down: true },
        InputEvent::Key { vk: VK_SNAPSHOT, down: false },
        InputEvent::Key { vk: VK_LWIN, down: false },
    ]);
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
