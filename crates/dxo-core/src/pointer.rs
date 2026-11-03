//! Touchpad and stick as mouse.
//!
//! * Touchpad: slide a finger to move the cursor, like a laptop trackpad.
//!   With `touchpad_buttons`, pressing the pad clicks: one finger = left,
//!   two fingers = right. The press then no longer reaches the game.
//! * Stick: one stick moves the cursor; games stop seeing that stick.

use crate::bindings::{InputEvent, MouseButton};
use crate::state::{Buttons, PadState};
use serde::{Deserialize, Serialize};
use std::time::Instant;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MouseConfig {
    pub touchpad: bool,
    /// Multiplier on the base speed (about half a pixel per touchpad unit).
    pub touchpad_speed: f32,
    pub touchpad_buttons: bool,
    /// "" = off, "left" or "right".
    pub stick: String,
    /// Pixels per second with the stick pushed all the way.
    pub stick_speed: f32,
}

impl Default for MouseConfig {
    fn default() -> Self {
        Self { touchpad: false, touchpad_speed: 1.0, touchpad_buttons: true, stick: String::new(), stick_speed: 1200.0 }
    }
}

impl MouseConfig {
    pub fn is_off(&self) -> bool {
        !self.touchpad && self.stick.is_empty()
    }
}

/// Per-pad state: last finger position, sub-pixel remainders, held click.
#[derive(Debug, Default)]
pub struct Pointer {
    finger: Option<(u8, u16, u16)>,
    frac: (f32, f32),
    last_t: Option<Instant>,
    click: Option<MouseButton>,
}

/// What one frame produces: a cursor move and any click changes.
#[derive(Debug, Default, PartialEq)]
pub struct PointerOut {
    pub dx: i32,
    pub dy: i32,
    pub events: Vec<InputEvent>,
}

impl Pointer {
    pub fn step(&mut self, cfg: &MouseConfig, raw: &PadState, mapped: &mut PadState, now: Instant) -> PointerOut {
        let mut out = PointerOut::default();
        let dt = self.last_t.map_or(0.0, |t| now.duration_since(t).as_secs_f32().min(0.05));
        self.last_t = Some(now);
        let (mut fx, mut fy) = (0.0f32, 0.0f32);

        // ---- touchpad -----------------------------------------------------
        let t = raw.touch[0];
        if cfg.touchpad && t.active {
            if let Some((_, x, y)) = self.finger.filter(|f| f.0 == t.id) {
                let k = 0.5 * cfg.touchpad_speed.clamp(0.1, 5.0);
                fx += (t.x as f32 - x as f32) * k;
                fy += (t.y as f32 - y as f32) * k;
            }
            self.finger = Some((t.id, t.x, t.y));
        } else {
            self.finger = None;
        }

        // ---- touchpad click ----------------------------------------------
        let pressed = cfg.touchpad && cfg.touchpad_buttons && raw.buttons.has(Buttons::TOUCHPAD);
        if cfg.touchpad && cfg.touchpad_buttons {
            mapped.buttons.set(Buttons::TOUCHPAD, false);
        }
        match (pressed, self.click) {
            (true, None) => {
                let b = if raw.touch[1].active { MouseButton::Right } else { MouseButton::Left };
                out.events.push(InputEvent::Mouse { button: b, down: true });
                self.click = Some(b);
            }
            (false, Some(b)) => {
                out.events.push(InputEvent::Mouse { button: b, down: false });
                self.click = None;
            }
            _ => {}
        }

        // ---- stick --------------------------------------------------------
        let stick = match cfg.stick.as_str() {
            "left" => Some((mapped.lx, mapped.ly)),
            "right" => Some((mapped.rx, mapped.ry)),
            _ => None,
        };
        if let Some((x, y)) = stick {
            // squared response: fine control near the centre, fast at the edge
            let v = cfg.stick_speed.clamp(50.0, 5000.0) * dt;
            fx += x * x.abs() * v;
            fy -= y * y.abs() * v; // stick up = cursor up
            if cfg.stick == "left" {
                (mapped.lx, mapped.ly) = (0.0, 0.0);
            } else {
                (mapped.rx, mapped.ry) = (0.0, 0.0);
            }
        }

        self.frac.0 += fx;
        self.frac.1 += fy;
        out.dx = self.frac.0.trunc() as i32;
        out.dy = self.frac.1.trunc() as i32;
        self.frac.0 -= out.dx as f32;
        self.frac.1 -= out.dy as f32;
        out
    }

    /// Let go of a held click (profile change, disconnect).
    pub fn release(&mut self) -> Vec<InputEvent> {
        self.finger = None;
        self.click.take().map(|b| vec![InputEvent::Mouse { button: b, down: false }]).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn touch(x: u16, y: u16) -> PadState {
        let mut s = PadState::default();
        s.touch[0].active = true;
        s.touch[0].x = x;
        s.touch[0].y = y;
        s
    }

    #[test]
    fn touchpad_moves_by_delta() {
        let cfg = MouseConfig { touchpad: true, ..Default::default() };
        let mut p = Pointer::default();
        let t = Instant::now();
        let mut m = touch(100, 100);
        assert_eq!(p.step(&cfg, &touch(100, 100), &mut m, t).dx, 0, "first touch only sets the anchor");
        let o = p.step(&cfg, &touch(140, 80), &mut m, t);
        assert_eq!((o.dx, o.dy), (20, -10));
        // lift the finger: the next touch elsewhere must not jump
        p.step(&cfg, &PadState::default(), &mut m, t);
        assert_eq!(p.step(&cfg, &touch(1500, 900), &mut m, t).dx, 0);
    }

    #[test]
    fn touchpad_click_left_and_right() {
        let cfg = MouseConfig { touchpad: true, ..Default::default() };
        let mut p = Pointer::default();
        let t = Instant::now();
        let mut s = touch(10, 10);
        s.buttons.set(Buttons::TOUCHPAD, true);
        let mut m = s;
        assert_eq!(p.step(&cfg, &s, &mut m, t).events, vec![InputEvent::Mouse { button: MouseButton::Left, down: true }]);
        assert!(!m.buttons.has(Buttons::TOUCHPAD), "game no longer sees the pad press");
        let up = PadState::default();
        assert_eq!(p.step(&cfg, &up, &mut up.clone(), t).events, vec![InputEvent::Mouse { button: MouseButton::Left, down: false }]);
        s.touch[1].active = true;
        assert_eq!(p.step(&cfg, &s, &mut s.clone(), t).events[0], InputEvent::Mouse { button: MouseButton::Right, down: true });
    }

    #[test]
    fn stick_moves_and_is_hidden_from_games() {
        let cfg = MouseConfig { stick: "right".into(), stick_speed: 1000.0, ..Default::default() };
        let mut p = Pointer::default();
        let t0 = Instant::now();
        let mut s = PadState::default();
        s.rx = 1.0;
        let mut m = s;
        p.step(&cfg, &s, &mut m, t0);
        let mut m = s;
        let o = p.step(&cfg, &s, &mut m, t0 + Duration::from_millis(10));
        assert_eq!(o.dx, 10, "1000 px/s for 10 ms");
        assert_eq!(m.rx, 0.0);
    }
}
