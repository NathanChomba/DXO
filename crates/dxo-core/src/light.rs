//! Light bar effects: what the light does on top of its colour.
//!
//! The colour itself comes from the controller's setting (player colour, the
//! profile's colour, or its own). This turns that into what is shown right
//! now: steady, breathing, pulsing, cycling, or a battery gauge, optionally
//! flashing brighter with rumble.

use crate::output::Rgb;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LightMode {
    #[default]
    Static,
    /// Fades gently up and down.
    Breathing,
    /// A short bright blink, then dim.
    Pulse,
    /// Cycles through the colours.
    Rainbow,
    /// Green when full, through yellow, to red when nearly empty.
    Battery,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LightConfig {
    pub mode: LightMode,
    /// Cycles per second for the moving modes (0.05..=4).
    pub speed: f32,
    /// 0..=1; the pad has no separate brightness, so the colour is scaled.
    pub brightness: f32,
    /// Flash towards white while the controller rumbles.
    pub flash_on_rumble: bool,
}

impl Default for LightConfig {
    fn default() -> Self {
        Self { mode: LightMode::Static, speed: 0.25, brightness: 1.0, flash_on_rumble: false }
    }
}

/// What the effect needs to know about the pad this frame.
#[derive(Debug, Clone, Copy, Default)]
pub struct LightInput {
    /// Seconds since the pad connected.
    pub t: f32,
    pub battery: Option<u8>,
    pub charging: bool,
    /// Strongest motor right now, 0..=255.
    pub rumble: u8,
}

impl LightConfig {
    /// Does the colour change by itself over time? (The engine then refreshes
    /// the light on a timer instead of only on change.)
    pub fn animated(&self, charging: bool) -> bool {
        match self.mode {
            LightMode::Static => false,
            LightMode::Battery => charging,
            _ => true,
        }
    }

    pub fn render(&self, base: Rgb, i: LightInput) -> Rgb {
        let speed = self.speed.clamp(0.05, 4.0);
        let phase = (i.t * speed).fract(); // 0..1 through one cycle
        let wave = 0.5 - 0.5 * (phase * std::f32::consts::TAU).cos(); // 0 -> 1 -> 0
        let (mut c, mut level) = match self.mode {
            LightMode::Static => (base, 1.0),
            LightMode::Breathing => (base, 0.12 + 0.88 * wave),
            LightMode::Pulse => (base, if phase < 0.12 { 1.0 } else { 0.15 }),
            LightMode::Rainbow => (hue(phase * 360.0), 1.0),
            LightMode::Battery => match i.battery {
                // 0 % = red (0 deg), 50 % = yellow, 100 % = green (120 deg)
                Some(p) => (hue(p.min(100) as f32 * 1.2), if i.charging { 0.35 + 0.65 * wave } else { 1.0 }),
                None => (base, 1.0),
            },
        };
        if self.flash_on_rumble && i.rumble > 0 {
            let k = i.rumble as f32 / 255.0;
            let lift = |v: u8| (v as f32 + (255.0 - v as f32) * k).round() as u8;
            c = Rgb(lift(c.0), lift(c.1), lift(c.2));
            level = level.max(k);
        }
        let k = level * self.brightness.clamp(0.0, 1.0);
        let dim = |v: u8| (v as f32 * k).round().clamp(0.0, 255.0) as u8;
        Rgb(dim(c.0), dim(c.1), dim(c.2))
    }
}

/// A fully saturated colour for a hue in degrees.
fn hue(deg: f32) -> Rgb {
    let h = deg.rem_euclid(360.0) / 60.0;
    let x = 1.0 - (h % 2.0 - 1.0).abs();
    let (r, g, b) = match h as u32 {
        0 => (1.0, x, 0.0),
        1 => (x, 1.0, 0.0),
        2 => (0.0, 1.0, x),
        3 => (0.0, x, 1.0),
        4 => (x, 0.0, 1.0),
        _ => (1.0, 0.0, x),
    };
    let to = |v: f32| (v * 255.0).round() as u8;
    Rgb(to(r), to(g), to(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLUE: Rgb = Rgb(0, 64, 255);

    fn at(t: f32) -> LightInput {
        LightInput { t, ..Default::default() }
    }

    #[test]
    fn static_is_the_colour_scaled_by_brightness() {
        let c = LightConfig::default();
        assert_eq!(c.render(BLUE, at(3.3)), BLUE);
        let half = LightConfig { brightness: 0.5, ..Default::default() };
        assert_eq!(half.render(BLUE, at(0.0)), Rgb(0, 32, 128));
        assert!(!c.animated(false));
    }

    #[test]
    fn breathing_rises_and_falls_but_never_goes_black() {
        let c = LightConfig { mode: LightMode::Breathing, speed: 1.0, ..Default::default() };
        let low = c.render(BLUE, at(0.0));
        let high = c.render(BLUE, at(0.5));
        assert_eq!(high, BLUE, "peak is the full colour");
        assert!(low.2 > 0 && low.2 < 60, "trough is dim, not off: {low:?}");
        assert_eq!(c.render(BLUE, at(1.0)), low, "one cycle per second");
        assert!(c.animated(false));
    }

    #[test]
    fn battery_gauge_goes_green_to_red() {
        let c = LightConfig { mode: LightMode::Battery, ..Default::default() };
        let b = |p| c.render(BLUE, LightInput { battery: Some(p), ..Default::default() });
        assert_eq!(b(100), Rgb(0, 255, 0));
        assert_eq!(b(50), Rgb(255, 255, 0));
        assert_eq!(b(0), Rgb(255, 0, 0));
        assert!(b(20).0 == 255 && b(20).1 < 120, "low = red-orange: {:?}", b(20));
        // a pad that doesn't report its battery keeps its normal colour
        assert_eq!(c.render(BLUE, at(0.0)), BLUE);
        // only moves (breathes) while charging
        assert!(!c.animated(false) && c.animated(true));
    }

    #[test]
    fn rumble_flashes_towards_white() {
        let c = LightConfig { flash_on_rumble: true, ..Default::default() };
        assert_eq!(c.render(BLUE, at(0.0)), BLUE, "no rumble, no change");
        assert_eq!(c.render(BLUE, LightInput { rumble: 255, ..Default::default() }), Rgb(255, 255, 255));
        let mid = c.render(BLUE, LightInput { rumble: 128, ..Default::default() });
        assert!(mid.0 > 100 && mid.0 < 150);
        // a dim breathing trough still flashes
        let b = LightConfig { mode: LightMode::Breathing, flash_on_rumble: true, speed: 1.0, ..Default::default() };
        assert_eq!(b.render(BLUE, LightInput { t: 0.0, rumble: 255, ..Default::default() }), Rgb(255, 255, 255));
    }

    #[test]
    fn rainbow_walks_the_hues() {
        let c = LightConfig { mode: LightMode::Rainbow, speed: 1.0, ..Default::default() };
        assert_eq!(c.render(BLUE, at(0.0)), Rgb(255, 0, 0));
        assert_eq!(c.render(BLUE, at(1.0 / 3.0)), Rgb(0, 255, 0));
        assert_eq!(c.render(BLUE, at(2.0 / 3.0)), Rgb(0, 0, 255));
    }
}
