//! Flick stick: the right stick stops being a camera *speed* and becomes a
//! camera *direction*.
//!
//!   * Flick: push the stick towards an angle and the view snaps round by that
//!     much (right = a quarter turn right, back = an about-turn).
//!   * Turn: keep it pushed out and sweep it round the edge; the view follows
//!     the sweep one-to-one.
//!
//! The view turns by moving the mouse, so DXO has to know how many pixels of
//! mouse movement make one full turn in the game (`px_per_turn`; it depends on
//! the game's sensitivity). The Motion page has a "test spin" to find it.
//! Pair it with gyro aiming: stick for big turns, gyro for the fine aim.
//!
//! Design after Jibb Smart's write-up (jibbsmart.com) and JoyShockMapper.

use crate::state::PadState;
use serde::{Deserialize, Serialize};
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FlickConfig {
    pub enabled: bool,
    /// Mouse pixels for one full 360-degree turn in the game.
    pub px_per_turn: f32,
    /// How long a flick takes to play out, milliseconds (0 = instant).
    pub flick_ms: u32,
    /// How far out (0..=1) the stick must be pushed before it counts.
    pub threshold: f32,
}

impl Default for FlickConfig {
    fn default() -> Self {
        Self { enabled: false, px_per_turn: 6000.0, flick_ms: 100, threshold: 0.9 }
    }
}

/// Sweeps smaller than this per report (degrees) are mostly stick noise and
/// are smoothed; bigger ones pass straight through.
const SMOOTH_BELOW: f32 = 0.5;
const DIRECT_ABOVE: f32 = 2.0;
/// Time constant of that smoothing, seconds.
const SMOOTH_TAU: f32 = 0.05;

#[derive(Debug, Default)]
pub struct FlickStick {
    out: bool,
    last_angle: f32,
    /// The flick in progress: total degrees and how much of it has played.
    flick_total: f32,
    flick_played: f32,
    flick_t: f32,
    /// Small sweeps waiting to be fed out smoothly, degrees.
    smooth: f32,
    frac: f32,
    last_t: Option<Instant>,
}

impl FlickStick {
    /// Read the right stick from `mapped`, hide it from the game, and return
    /// the horizontal mouse move (pixels) for this report.
    pub fn step(&mut self, cfg: &FlickConfig, mapped: &mut PadState, now: Instant) -> i32 {
        let dt = self.last_t.map_or(0.0, |t| now.duration_since(t).as_secs_f32().min(0.05));
        self.last_t = Some(now);
        let (x, y) = (mapped.rx, mapped.ry);
        (mapped.rx, mapped.ry) = (0.0, 0.0);

        let thr = cfg.threshold.clamp(0.3, 1.0);
        let mag = (x * x + y * y).sqrt();
        let out = if self.out { mag >= thr - 0.1 } else { mag >= thr };
        let mut turn = 0.0f32; // degrees to turn this report, + = right

        if out {
            let angle = x.atan2(y).to_degrees(); // 0 = forward, 90 = right, 180 = back
            if !self.out {
                // Just pushed out: flick to that angle.
                self.flick_total = angle;
                self.flick_played = 0.0;
                self.flick_t = 0.0;
            } else {
                let mut d = angle - self.last_angle;
                if d > 180.0 {
                    d -= 360.0;
                } else if d < -180.0 {
                    d += 360.0;
                }
                let direct = ((d.abs() - SMOOTH_BELOW) / (DIRECT_ABOVE - SMOOTH_BELOW)).clamp(0.0, 1.0);
                turn += d * direct;
                self.smooth += d * (1.0 - direct);
            }
            self.last_angle = angle;
        }
        self.out = out;

        // Play out the flick with an ease-out, so it's quick but not a jump cut.
        if self.flick_played != self.flick_total {
            self.flick_t += dt;
            let dur = cfg.flick_ms.min(500) as f32 / 1000.0;
            let p = if dur <= 0.0 { 1.0 } else { (self.flick_t / dur).min(1.0) };
            let eased = 1.0 - (1.0 - p) * (1.0 - p);
            let target = if p >= 1.0 { self.flick_total } else { self.flick_total * eased };
            turn += target - self.flick_played;
            self.flick_played = target;
        }

        // Feed the smoothed part out gradually; what goes in always comes out.
        if self.smooth != 0.0 {
            let k = if dt > 0.0 { 1.0 - (-dt / SMOOTH_TAU).exp() } else { 0.0 };
            let part = self.smooth * k;
            turn += part;
            self.smooth -= part;
            if self.smooth.abs() < 0.01 {
                turn += self.smooth;
                self.smooth = 0.0;
            }
        }

        self.frac += turn / 360.0 * cfg.px_per_turn.clamp(100.0, 200_000.0);
        let px = self.frac.trunc();
        self.frac -= px;
        px as i32
    }

    /// Flick stick switched off or the pad went away: forget where it was.
    pub fn reset(&mut self) {
        *self = FlickStick::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn stick(angle_deg: f32, mag: f32) -> PadState {
        let mut s = PadState::default();
        let a = angle_deg.to_radians();
        s.rx = a.sin() * mag;
        s.ry = a.cos() * mag;
        s
    }

    /// Hold the stick at each (angle, magnitude) for one 4 ms report; total pixels.
    fn play(f: &mut FlickStick, cfg: &FlickConfig, t0: Instant, from_ms: u64, frames: &[(f32, f32)]) -> i32 {
        let mut px = 0;
        for (i, (a, m)) in frames.iter().enumerate() {
            let mut s = stick(*a, *m);
            px += f.step(cfg, &mut s, t0 + Duration::from_millis(from_ms + i as u64 * 4));
            assert_eq!((s.rx, s.ry), (0.0, 0.0), "the game never sees the stick");
        }
        px
    }

    const CFG: FlickConfig = FlickConfig { enabled: true, px_per_turn: 3600.0, flick_ms: 100, threshold: 0.9 };

    #[test]
    fn flick_right_is_a_quarter_turn() {
        let mut f = FlickStick::default();
        let t0 = Instant::now();
        // centred, then pushed right and held for 200 ms
        let mut frames = vec![(0.0, 0.0); 3];
        frames.extend(vec![(90.0, 1.0); 50]);
        let px = play(&mut f, &CFG, t0, 0, &frames);
        assert!((px - 900).abs() <= 1, "90 degrees of 3600 px/turn = 900 px, got {px}");
    }

    #[test]
    fn flick_back_left_turns_left() {
        let mut f = FlickStick::default();
        let mut frames = vec![(0.0, 0.0); 2];
        frames.extend(vec![(-135.0, 1.0); 50]);
        let px = play(&mut f, &CFG, Instant::now(), 0, &frames);
        assert!((px + 1350).abs() <= 1, "{px}");
    }

    #[test]
    fn pushing_forward_does_not_turn() {
        let mut f = FlickStick::default();
        let mut frames = vec![(0.0, 0.0); 2];
        frames.extend(vec![(0.0, 1.0); 50]);
        assert_eq!(play(&mut f, &CFG, Instant::now(), 0, &frames), 0);
    }

    #[test]
    fn sweeping_round_the_edge_turns_one_to_one() {
        let mut f = FlickStick::default();
        let t0 = Instant::now();
        // push forward (no flick), then sweep 0 -> 180 degrees over 180 reports, then hold
        let mut frames = vec![(0.0, 0.0), (0.0, 1.0)];
        frames.extend((1..=180).map(|i| (i as f32, 1.0)));
        frames.extend(vec![(180.0, 1.0); 100]);
        let px = play(&mut f, &CFG, t0, 0, &frames);
        assert!((px - 1800).abs() <= 2, "half a turn = 1800 px, got {px}");
    }

    #[test]
    fn sweep_across_the_back_does_not_spin_the_long_way() {
        let mut f = FlickStick::default();
        // start pointing back-right (170), sweep through 180 to back-left (-170)
        let mut frames = vec![(0.0, 0.0), (170.0, 1.0)];
        frames.extend(vec![(170.0, 1.0); 60]); // let the flick finish
        let flick = play(&mut f, &CFG, Instant::now(), 0, &frames);
        assert!((flick - 1700).abs() <= 1);
        let sweep: Vec<(f32, f32)> = [175.0, 180.0, -175.0, -170.0].iter().map(|a| (*a, 1.0)).chain(vec![(-170.0, 1.0); 100]).collect();
        let px = play(&mut f, &CFG, Instant::now() + Duration::from_secs(1), 0, &sweep);
        assert!((px - 200).abs() <= 2, "20 degrees right = 200 px, got {px}");
    }

    #[test]
    fn letting_go_and_pushing_again_flicks_again() {
        let mut f = FlickStick::default();
        let t0 = Instant::now();
        let mut frames = vec![(0.0, 0.0)];
        frames.extend(vec![(90.0, 1.0); 40]);
        frames.extend(vec![(0.0, 0.0); 10]);
        frames.extend(vec![(90.0, 1.0); 40]);
        assert!((play(&mut f, &CFG, t0, 0, &frames) - 1800).abs() <= 2);
    }

    #[test]
    fn half_pushed_stick_is_ignored() {
        let mut f = FlickStick::default();
        let frames = vec![(90.0, 0.5); 50];
        assert_eq!(play(&mut f, &CFG, Instant::now(), 0, &frames), 0);
    }

    #[test]
    fn jitter_at_the_edge_does_not_drift() {
        let mut f = FlickStick::default();
        let mut frames = vec![(0.0, 0.0), (0.0, 1.0)];
        // an 8-bit stick held still flickers by a fraction of a degree
        frames.extend((0..500).map(|i| (if i % 2 == 0 { 0.3 } else { -0.3 }, 1.0)));
        frames.extend(vec![(0.0, 1.0); 100]);
        let px = play(&mut f, &CFG, Instant::now(), 0, &frames);
        assert!(px.abs() <= 3, "noise must cancel out, got {px}");
    }
}
