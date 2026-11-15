//! M3/M5: transforms between the physical state and what the virtual pad sees:
//! deadzones, response curves, button remaps.

use crate::profile::{Curve, Profile};
use crate::state::{Buttons, PadState};

/// Radial deadzone + outer clamp + response curve on one stick.
pub fn shape_stick(x: f32, y: f32, inner: f32, outer: f32, curve: Curve) -> (f32, f32) {
    let mag = (x * x + y * y).sqrt();
    if mag <= inner || mag == 0.0 {
        return (0.0, 0.0);
    }
    let outer = outer.max(inner + 0.01);
    let t = ((mag - inner) / (outer - inner)).clamp(0.0, 1.0);
    let scaled = curve.apply(t);
    (x / mag * scaled, y / mag * scaled)
}

/// Anti-deadzone: many games ignore the first 10-25 % of stick travel. Start
/// our output at `anti` instead of 0 so the smallest push already clears the
/// game's own deadzone. `anti` is the size of the game's deadzone (0 = off).
pub fn anti_deadzone(x: f32, y: f32, anti: f32) -> (f32, f32) {
    let anti = anti.clamp(0.0, 0.9);
    let mag = (x * x + y * y).sqrt();
    if anti <= 0.0 || mag <= 0.0 {
        return (x, y);
    }
    let out = (anti + mag * (1.0 - anti)).min(1.0);
    (x / mag * out, y / mag * out)
}

pub fn shape_trigger(v: f32, deadzone: f32) -> f32 {
    if v <= deadzone {
        0.0
    } else {
        ((v - deadzone) / (1.0 - deadzone).max(0.01)).clamp(0.0, 1.0)
    }
}

/// Apply a profile to a raw state. Pure function: easy to unit test and safe
/// to call from the 1 kHz loop.
pub fn apply(p: &Profile, raw: &PadState) -> PadState {
    let mut s = *raw;
    (s.lx, s.ly) = shape_stick(raw.lx, raw.ly, p.left_stick.deadzone, p.left_stick.outer, p.left_stick.curve);
    (s.rx, s.ry) = shape_stick(raw.rx, raw.ry, p.right_stick.deadzone, p.right_stick.outer, p.right_stick.curve);
    (s.lx, s.ly) = anti_deadzone(s.lx, s.ly, p.left_stick.anti_deadzone);
    (s.rx, s.ry) = anti_deadzone(s.rx, s.ry, p.right_stick.anti_deadzone);
    if p.left_stick.invert_y {
        s.ly = -s.ly;
    }
    if p.right_stick.invert_y {
        s.ry = -s.ry;
    }
    s.l2 = shape_trigger(raw.l2, p.trigger_deadzone);
    s.r2 = shape_trigger(raw.r2, p.trigger_deadzone);

    if !p.remap.is_empty() {
        let pairs: Vec<(u32, u32)> = p
            .remap
            .iter()
            .filter_map(|(f, t)| Some((button_from_name(f)?, button_from_name(t)?)))
            .collect();
        // Pass 1: remove every remapped source. Pass 2: OR sources into
        // targets. Two passes make swaps (A<->B) work.
        let mut out = Buttons(raw.buttons.0);
        for &(f, _) in &pairs {
            out.set(f, false);
        }
        for &(f, t) in &pairs {
            if raw.buttons.has(f) {
                out.set(t, true);
            }
        }
        s.buttons = out;
    }
    s
}

pub fn button_from_name(name: &str) -> Option<u32> {
    Some(match name.to_ascii_lowercase().as_str() {
        "south" | "cross" | "a" => Buttons::SOUTH,
        "east" | "circle" | "b" => Buttons::EAST,
        "west" | "square" | "x" => Buttons::WEST,
        "north" | "triangle" | "y" => Buttons::NORTH,
        "l1" | "lb" => Buttons::L1,
        "r1" | "rb" => Buttons::R1,
        "l3" | "ls" => Buttons::L3,
        "r3" | "rs" => Buttons::R3,
        "select" | "create" | "share" | "back" => Buttons::SELECT,
        "start" | "options" => Buttons::START,
        "home" | "ps" | "guide" => Buttons::HOME,
        "touchpad" => Buttons::TOUCHPAD,
        "l2" | "lt" => Buttons::L2_DIGITAL,
        "r2" | "rt" => Buttons::R2_DIGITAL,
        "mute" => Buttons::MUTE,
        "up" => Buttons::DPAD_UP,
        "down" => Buttons::DPAD_DOWN,
        "left" => Buttons::DPAD_LEFT,
        "right" => Buttons::DPAD_RIGHT,
        "paddle_l" => Buttons::PADDLE_L,
        "paddle_r" => Buttons::PADDLE_R,
        "fn_l" => Buttons::FN_L,
        "fn_r" => Buttons::FN_R,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deadzone_zeroes_small_input() {
        assert_eq!(shape_stick(0.05, 0.0, 0.1, 1.0, Curve::Linear), (0.0, 0.0));
    }

    #[test]
    fn full_tilt_stays_full() {
        let (x, _) = shape_stick(1.0, 0.0, 0.1, 0.95, Curve::Linear);
        assert!((x - 1.0).abs() < 1e-5);
    }

    #[test]
    fn anti_deadzone_starts_past_the_games_deadzone() {
        // at rest: still exactly zero (no drift is invented)
        assert_eq!(anti_deadzone(0.0, 0.0, 0.2), (0.0, 0.0));
        // the smallest push lands just outside a 20 % game deadzone
        let (x, _) = anti_deadzone(0.01, 0.0, 0.2);
        assert!(x > 0.2 && x < 0.22, "{x}");
        // full tilt stays full, direction is kept
        let (x, y) = anti_deadzone(0.0, -1.0, 0.2);
        assert!(x.abs() < 1e-6 && (y + 1.0).abs() < 1e-6);
        // through the profile: deadzone first, then anti-deadzone
        let mut p = Profile::default();
        p.left_stick.anti_deadzone = 0.15;
        let mut raw = PadState::default();
        raw.lx = 0.03; // inside DXO's own deadzone: stays zero
        assert_eq!(apply(&p, &raw).lx, 0.0);
        raw.lx = 0.2;
        assert!(apply(&p, &raw).lx > 0.15);
    }

    #[test]
    fn remap_swaps() {
        let mut p = Profile::default();
        p.remap = vec![("cross".into(), "circle".into()), ("circle".into(), "cross".into())];
        let mut raw = PadState::default();
        raw.buttons.set(Buttons::SOUTH, true);
        let s = apply(&p, &raw);
        assert!(s.buttons.has(Buttons::EAST));
        assert!(!s.buttons.has(Buttons::SOUTH));
    }
}
