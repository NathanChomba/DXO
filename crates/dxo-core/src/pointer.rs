//! Touchpad and stick as mouse.
//!
//! * Touchpad: slide a finger to move the cursor, like a laptop trackpad.
//!   With `touchpad_buttons`, pressing the pad clicks: one finger = left,
//!   two fingers = right. The press then no longer reaches the game.
//!   Two fingers together scroll (up, down and sideways, the content follows
//!   the fingers); pinching them apart or together zooms (Ctrl + wheel).
//!   Sony's touchpads report two fingers at most, so there are no
//!   three-finger gestures.
//! * Stick: one stick moves the cursor; games stop seeing that stick.

use crate::bindings::{InputEvent, MouseButton};
use crate::profile::Curve;
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
    /// Slide two fingers to scroll.
    pub two_finger_scroll: bool,
    /// Multiplier on scrolling speed.
    pub scroll_speed: f32,
    /// Pinch two fingers to zoom (sends Ctrl + wheel).
    pub pinch_zoom: bool,
    /// "" = off, "left" or "right".
    pub stick: String,
    /// Pixels per second with the stick pushed all the way.
    pub stick_speed: f32,
    /// How cursor speed grows as the stick is pushed out. The default
    /// (power 2) is slow near the centre and fast at the edge; a Bezier
    /// curve makes it fully editable.
    pub stick_curve: Curve,
    /// Hand the touchpad and stick back to the game while a full-screen
    /// game or video is in front (gyro aiming is not affected).
    pub suspend_in_fullscreen: bool,
}

impl Default for MouseConfig {
    fn default() -> Self {
        Self {
            touchpad: false,
            touchpad_speed: 1.0,
            touchpad_buttons: true,
            two_finger_scroll: true,
            scroll_speed: 1.0,
            pinch_zoom: true,
            stick: String::new(),
            stick_speed: 1200.0,
            stick_curve: Curve::Power { exponent: 2.0 },
            suspend_in_fullscreen: true,
        }
    }
}

impl MouseConfig {
    pub fn is_off(&self) -> bool {
        !self.touchpad && self.stick.is_empty()
    }
}

/// What two fingers on the pad are doing. Decided once per touch, so a
/// scroll never turns into a zoom half-way through.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Gesture {
    #[default]
    Undecided,
    Scroll,
    Pinch,
    /// It was a gesture that is switched off.
    Ignore,
}

const VK_CONTROL: u16 = 0x11;
/// Finger travel (touchpad units) before a two-finger touch is called a scroll or a pinch.
const GESTURE_DECIDE: f32 = 24.0;
/// Wheel units per touchpad unit at scroll speed 1 (120 = one notch).
const SCROLL_GAIN: f32 = 3.0;
/// Scroll is sent in steps of this many wheel units: fine enough to feel
/// smooth, coarse enough for apps that only act on a good part of a notch.
const SCROLL_STEP: i32 = 30;
/// Change in finger spacing (touchpad units) per zoom step.
const PINCH_STEP: f32 = 90.0;

/// Per-pad state: last finger position, sub-pixel remainders, held click.
#[derive(Debug, Default)]
pub struct Pointer {
    /// Both fingers last frame: (id, x, y) each.
    two: Option<[(u8, u16, u16); 2]>,
    gesture: Gesture,
    travel: f32,
    spread: f32,
    scroll_acc: (f32, f32),
    pinch_acc: f32,
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

        // ---- touchpad: two fingers scroll or zoom ---------------------------
        let (t0, t1) = (raw.touch[0], raw.touch[1]);
        let two = cfg.touchpad && t0.active && t1.active && (cfg.two_finger_scroll || cfg.pinch_zoom);
        if two {
            if let Some(prev) = self.two.filter(|p| p[0].0 == t0.id && p[1].0 == t1.id) {
                let d0 = (t0.x as f32 - prev[0].1 as f32, t0.y as f32 - prev[0].2 as f32);
                let d1 = (t1.x as f32 - prev[1].1 as f32, t1.y as f32 - prev[1].2 as f32);
                let avg = ((d0.0 + d1.0) / 2.0, (d0.1 + d1.1) / 2.0);
                let gap = |a: (f32, f32), b: (f32, f32)| (a.0 - b.0).hypot(a.1 - b.1);
                let spread = gap((t0.x as f32, t0.y as f32), (t1.x as f32, t1.y as f32))
                    - gap((prev[0].1 as f32, prev[0].2 as f32), (prev[1].1 as f32, prev[1].2 as f32));
                if self.gesture == Gesture::Undecided {
                    self.travel += avg.0.hypot(avg.1);
                    self.spread += spread.abs();
                    if self.travel.max(self.spread) >= GESTURE_DECIDE {
                        // Fingers moving together = scroll; moving apart or together = pinch.
                        let pinch = self.spread > self.travel * 1.5;
                        self.gesture = match (pinch, cfg.pinch_zoom, cfg.two_finger_scroll) {
                            (true, true, _) => Gesture::Pinch,
                            (false, _, true) => Gesture::Scroll,
                            _ => Gesture::Ignore,
                        };
                    }
                }
                match self.gesture {
                    Gesture::Scroll => {
                        // The content follows the fingers, like a phone or a laptop trackpad.
                        let k = SCROLL_GAIN * cfg.scroll_speed.clamp(0.1, 5.0);
                        self.scroll_acc.0 -= avg.0 * k;
                        self.scroll_acc.1 += avg.1 * k;
                        let sx = (self.scroll_acc.0 as i32 / SCROLL_STEP) * SCROLL_STEP;
                        let sy = (self.scroll_acc.1 as i32 / SCROLL_STEP) * SCROLL_STEP;
                        if sx != 0 || sy != 0 {
                            self.scroll_acc.0 -= sx as f32;
                            self.scroll_acc.1 -= sy as f32;
                            out.events.push(InputEvent::Scroll { dx: sx, dy: sy });
                        }
                    }
                    Gesture::Pinch => {
                        self.pinch_acc += spread;
                        while self.pinch_acc.abs() >= PINCH_STEP {
                            let dir = if self.pinch_acc > 0.0 { 1 } else { -1 }; // apart = zoom in
                            self.pinch_acc -= dir as f32 * PINCH_STEP;
                            out.events.push(InputEvent::Key { vk: VK_CONTROL, down: true });
                            out.events.push(InputEvent::Wheel(dir));
                            out.events.push(InputEvent::Key { vk: VK_CONTROL, down: false });
                        }
                    }
                    Gesture::Undecided | Gesture::Ignore => {}
                }
            }
            self.two = Some([(t0.id, t0.x, t0.y), (t1.id, t1.x, t1.y)]);
        } else if self.two.take().is_some() {
            self.end_gesture();
        }

        // ---- touchpad: one finger moves the cursor ---------------------------
        let t = raw.touch[0];
        if two {
            self.finger = None; // the cursor stays put; no jump when a finger lifts
        } else if cfg.touchpad && t.active {
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
            // The curve shapes how far the stick is pushed (not each axis on its
            // own), so diagonals move as fast as straight lines.
            let v = cfg.stick_speed.clamp(50.0, 5000.0) * dt;
            let mag = (x * x + y * y).sqrt().min(1.0);
            if mag > 0.0 {
                let k = cfg.stick_curve.apply(mag) / mag * v;
                fx += x * k;
                fy -= y * k; // stick up = cursor up
            }
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

    fn end_gesture(&mut self) {
        self.gesture = Gesture::Undecided;
        self.travel = 0.0;
        self.spread = 0.0;
        self.scroll_acc = (0.0, 0.0);
        self.pinch_acc = 0.0;
    }

    /// Let go of a held click (profile change, disconnect).
    pub fn release(&mut self) -> Vec<InputEvent> {
        self.finger = None;
        self.two = None;
        self.end_gesture();
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

    fn two(a: (u16, u16), b: (u16, u16)) -> PadState {
        let mut s = touch(a.0, a.1);
        s.touch[1].active = true;
        s.touch[1].id = 1;
        s.touch[1].x = b.0;
        s.touch[1].y = b.1;
        s
    }

    #[test]
    fn two_fingers_scroll_and_the_cursor_stays_put() {
        let cfg = MouseConfig { touchpad: true, ..Default::default() };
        let mut p = Pointer::default();
        let t = Instant::now();
        let mut m = PadState::default();
        assert_eq!(p.step(&cfg, &two((500, 300), (700, 300)), &mut m, t), PointerOut::default());
        // both fingers slide down 40 units: content follows = wheel up, one notch
        let o = p.step(&cfg, &two((500, 340), (700, 340)), &mut m, t);
        assert_eq!((o.dx, o.dy), (0, 0));
        assert_eq!(o.events, vec![InputEvent::Scroll { dx: 0, dy: 120 }]);
        // sideways: fingers right = scroll left
        let o = p.step(&cfg, &two((520, 340), (720, 340)), &mut m, t);
        assert_eq!(o.events, vec![InputEvent::Scroll { dx: -60, dy: 0 }]);
        // one finger lifts: no cursor jump
        assert_eq!(p.step(&cfg, &touch(520, 340), &mut m, t).dx, 0);
    }

    #[test]
    fn pinching_zooms_and_does_not_scroll() {
        let cfg = MouseConfig { touchpad: true, ..Default::default() };
        let mut p = Pointer::default();
        let t = Instant::now();
        let mut m = PadState::default();
        p.step(&cfg, &two((800, 500), (1000, 500)), &mut m, t);
        // fingers move apart by 100 in total: one zoom-in step
        let o = p.step(&cfg, &two((750, 500), (1050, 500)), &mut m, t);
        assert_eq!(
            o.events,
            vec![
                InputEvent::Key { vk: 0x11, down: true },
                InputEvent::Wheel(1),
                InputEvent::Key { vk: 0x11, down: false },
            ]
        );
        // and back together: zoom out
        let o = p.step(&cfg, &two((800, 500), (1000, 500)), &mut m, t);
        assert!(o.events.contains(&InputEvent::Wheel(-1)));
    }

    #[test]
    fn gestures_can_be_switched_off() {
        let cfg = MouseConfig { touchpad: true, two_finger_scroll: false, pinch_zoom: false, ..Default::default() };
        let mut p = Pointer::default();
        let t = Instant::now();
        let mut m = PadState::default();
        p.step(&cfg, &two((500, 300), (700, 300)), &mut m, t);
        // with both off, the first finger just moves the cursor as before
        let o = p.step(&cfg, &two((540, 300), (740, 300)), &mut m, t);
        assert_eq!(o.dx, 20);
        assert!(o.events.is_empty());
        // scroll off, pinch on: a two-finger slide does nothing at all
        let cfg = MouseConfig { touchpad: true, two_finger_scroll: false, ..Default::default() };
        let mut p = Pointer::default();
        p.step(&cfg, &two((500, 300), (700, 300)), &mut m, t);
        assert_eq!(p.step(&cfg, &two((500, 400), (700, 400)), &mut m, t), PointerOut::default());
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
    fn stick_curve_shapes_the_speed() {
        let run = |curve: Curve, push: f32| {
            let cfg = MouseConfig { stick: "right".into(), stick_speed: 1000.0, stick_curve: curve, ..Default::default() };
            let mut p = Pointer::default();
            let t0 = Instant::now();
            let mut s = PadState::default();
            s.rx = push;
            p.step(&cfg, &s, &mut s.clone(), t0);
            p.step(&cfg, &s, &mut s.clone(), t0 + Duration::from_millis(50)).dx
        };
        assert_eq!(run(Curve::Linear, 0.5), 25, "half push = half speed");
        assert_eq!(run(Curve::Power { exponent: 2.0 }, 0.5), 12, "default: a quarter speed at half push");
        let precise = Curve::Bezier { x1: 0.7, y1: 0.0, x2: 1.0, y2: 0.4 };
        assert!(run(precise, 0.5) < 8, "a precision curve is slower still near the centre");
        assert_eq!(run(precise, 1.0), 50, "every curve reaches full speed at the edge");
    }

    #[test]
    fn diagonal_is_as_fast_as_straight() {
        let cfg = MouseConfig { stick: "left".into(), stick_speed: 1000.0, ..Default::default() };
        let mut p = Pointer::default();
        let t0 = Instant::now();
        let mut s = PadState::default();
        (s.lx, s.ly) = (0.70710677, 0.70710677);
        p.step(&cfg, &s, &mut s.clone(), t0);
        let o = p.step(&cfg, &s, &mut s.clone(), t0 + Duration::from_millis(50));
        let dist = ((o.dx * o.dx + o.dy * o.dy) as f32).sqrt();
        assert!((dist - 50.0).abs() < 1.5, "1000 px/s for 50 ms along the diagonal, got {dist}");
        assert!(o.dy < 0, "stick up = cursor up");
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
