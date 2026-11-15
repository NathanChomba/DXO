//! M3/M5: profiles. One TOML file per profile; `match_process` makes it a
//! game profile that `foreground` switches to automatically (M5).

use crate::error::{Error, Result};
use crate::output::Rgb;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum Curve {
    #[default]
    Linear,
    /// out = t^exponent  (>1 = finer aim near centre)
    Power { exponent: f32 },
    /// Linear blend between linear and cubic, 0..=1
    Smooth { amount: f32 },
    /// Editable curve, the same shape as CSS `cubic-bezier(x1, y1, x2, y2)`:
    /// it runs from (0,0) to (1,1) and the two handles bend it. Low y1 = fine
    /// control near the centre; high y2 = fast at the edge.
    Bezier { x1: f32, y1: f32, x2: f32, y2: f32 },
}

impl Curve {
    pub fn apply(self, t: f32) -> f32 {
        match self {
            Curve::Linear => t,
            Curve::Power { exponent } => t.powf(exponent.max(0.1)),
            Curve::Smooth { amount } => {
                let a = amount.clamp(0.0, 1.0);
                t * (1.0 - a) + t * t * t * a
            }
            Curve::Bezier { x1, y1, x2, y2 } => bezier(t, x1, y1, x2, y2),
        }
    }
}

/// y for a given x on the cubic Bezier (0,0) (x1,y1) (x2,y2) (1,1).
/// x1/x2 are clamped to 0..=1 so x only ever grows along the curve, which
/// lets a plain bisection find the point (20 steps = better than 1e-6).
pub fn bezier(x: f32, x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let (x1, x2) = (x1.clamp(0.0, 1.0), x2.clamp(0.0, 1.0));
    let at = |t: f32, a: f32, b: f32| {
        let u = 1.0 - t;
        3.0 * u * u * t * a + 3.0 * u * t * t * b + t * t * t
    };
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..20 {
        let mid = (lo + hi) * 0.5;
        if at(mid, x1, x2) < x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    at((lo + hi) * 0.5, y1, y2).clamp(0.0, 1.0)
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StickConfig {
    pub deadzone: f32,
    pub outer: f32,
    pub curve: Curve,
    pub invert_y: bool,
    /// Size of the GAME's built-in deadzone to compensate for (0 = off).
    /// See `mapping::anti_deadzone`.
    pub anti_deadzone: f32,
}

impl Default for StickConfig {
    fn default() -> Self {
        Self { deadzone: 0.08, outer: 0.98, curve: Curve::Linear, invert_y: false, anti_deadzone: 0.0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VirtualTarget {
    Xbox360,
    /// M6+: virtual DS4 via ViGEm (needs output-report forwarding)
    DualShock4,
    /// Do not create a virtual pad (useful for gyro-only/emulator use)
    None,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GyroConfig {
    /// DSU/CemuHook motion server for emulators (Cemu, Dolphin, Yuzu forks...).
    pub cemuhook_server: bool,
    pub cemuhook_port: u16,
    /// Turn the controller to move the mouse.
    pub to_mouse: bool,
    /// Multiplier on DXO's base speed (10 px per degree turned).
    pub sensitivity: f32,
    /// When gyro aiming is live: "" = always, "touch" = finger on the
    /// touchpad, or a button name from `mapping::button_from_name` ("l2", "r1"...).
    pub mouse_hold: String,
    /// Turning slower than this (degrees/second) is ignored: kills hand tremor.
    pub mouse_deadzone: f32,
    pub invert_x: bool,
    pub invert_y: bool,
    /// Ratchet: hold this button to pause gyro aiming while you bring the
    /// controller back to a comfortable position (like lifting a mouse).
    /// "" = off; names as for `mouse_hold`.
    pub ratchet: String,
    /// Jitter filter, 0 = off .. 1 = strongest. Slow movement is smoothed,
    /// quick turns pass straight through, so it adds no lag to a flick.
    pub smoothing: f32,
    /// Which way is "sideways": the controller's own axes, or the player's
    /// (turning about gravity, however the pad is tilted).
    pub space: GyroSpace,
}

/// What a sideways turn is measured against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum GyroSpace {
    /// The controller's own yaw axis. Simple; changes feel if you tilt the pad.
    #[default]
    Local,
    /// Yaw about gravity: turning your whole body or the pad left/right always
    /// aims left/right, whatever angle you hold it at. Uses the accelerometer.
    Player,
}

impl Default for GyroConfig {
    fn default() -> Self {
        Self {
            cemuhook_server: false,
            cemuhook_port: 26760,
            to_mouse: false,
            sensitivity: 1.0,
            mouse_hold: "l2".into(), // aim-down-sights = gyro aim, the classic setup
            mouse_deadzone: 1.0,
            invert_x: false,
            invert_y: false,
            ratchet: String::new(),
            smoothing: 0.0,
            space: GyroSpace::Local,
        }
    }
}

/// Rumble shaping: what the motors do with the strength a game asks for.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RumbleShape {
    /// Low-rumble boost: the weakest non-zero rumble plays at least this
    /// strong (0..=1). Small motors don't spin at all below about 0.15.
    pub floor: f32,
    /// Ceiling: nothing plays stronger than this (0..=1).
    pub ceiling: f32,
    /// Response: 1 = as sent, below 1 lifts quiet rumble, above 1 tames it.
    pub curve: f32,
}

impl Default for RumbleShape {
    fn default() -> Self {
        Self { floor: 0.0, ceiling: 1.0, curve: 1.0 }
    }
}

impl RumbleShape {
    /// Shape one motor value (0..=255). Zero always stays zero.
    pub fn apply(&self, v: u8) -> u8 {
        if v == 0 {
            return 0;
        }
        let floor = self.floor.clamp(0.0, 1.0);
        let ceiling = self.ceiling.clamp(floor, 1.0);
        let t = (v as f32 / 255.0).powf(self.curve.clamp(0.2, 5.0));
        ((floor + (ceiling - floor) * t) * 255.0).round().clamp(1.0, 255.0) as u8
    }
}

/// Game sound turned into vibration (needs an engine built with the
/// `audio-haptics` feature; otherwise this is ignored).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioHaptics {
    pub mode: AudioHapticsMode,
    /// How strongly sound drives the motors, 0..=4.
    pub gain: f32,
}

impl Default for AudioHaptics {
    fn default() -> Self {
        Self { mode: AudioHapticsMode::Off, gain: 1.0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AudioHapticsMode {
    #[default]
    Off,
    /// Bass in whatever is playing drives the rumble motors (any controller).
    Rumble,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub name: String,
    /// Executable names (case-insensitive), e.g. ["eldenring.exe"]. Empty = manual only.
    pub match_process: Vec<String>,
    pub target: VirtualTarget,
    pub left_stick: StickConfig,
    pub right_stick: StickConfig,
    pub trigger_deadzone: f32,
    /// (from, to) pairs using names from `mapping::button_from_name`.
    /// Older, simpler form; the Mappings page writes `bindings` instead.
    pub remap: Vec<(String, String)>,
    /// Per-button actions: other button, key, mouse, turbo, toggle.
    pub bindings: Vec<crate::bindings::Binding>,
    pub lightbar: [u8; 3],
    pub player_leds: u8,
    pub hide_physical: bool,
    /// Play vibration that games send to the virtual pad on the real controller.
    pub rumble_passthrough: bool,
    /// Multiplier on game rumble: 0.0 = off, 1.0 = as sent, up to 2.0 = boosted.
    pub rumble_strength: f32,
    /// Swap the heavy (left) and light (right) motors.
    pub rumble_swap: bool,
    /// Send sound to the controller's own speaker / headset jack (USB only).
    pub controller_audio: bool,
    /// Controller speaker + headset volume, 0..=100.
    pub audio_volume: u8,
    pub gyro: GyroConfig,
    /// Hold this button (e.g. "ps") for a second set of bindings. "" = off.
    /// A quick tap on its own still reaches the game.
    pub shift_button: String,
    /// Bindings used while the shift button is held. Buttons not listed here
    /// keep their normal binding.
    pub shift_bindings: Vec<crate::bindings::Binding>,
    /// Touchpad and stick as mouse.
    pub mouse: crate::pointer::MouseConfig,
    /// Two buttons pressed together do something else (see `bindings::ChordBinding`).
    pub chords: Vec<crate::bindings::ChordBinding>,
    /// Tap / hold / double-tap: holding a button this long (ms) is a "hold".
    pub hold_ms: u32,
    /// ...and a second press within this long (ms) is a "double-tap".
    pub double_tap_ms: u32,
    /// Dual-stage triggers: how far a trigger is pulled (0..=1) before its
    /// "soft pull" binding starts, and before its "full pull" binding does.
    pub trigger_soft_point: f32,
    pub trigger_full_point: f32,
    /// Adaptive-trigger effects (DualSense). Off = a normal trigger.
    pub trigger_left: crate::output::TriggerEffect,
    pub trigger_right: crate::output::TriggerEffect,
    /// Light bar effect on top of the colour.
    pub light: crate::light::LightConfig,
    pub rumble_shape: RumbleShape,
    pub audio_haptics: AudioHaptics,
    /// Flick stick (right stick turns the camera by its angle).
    pub flick: crate::flick::FlickConfig,
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            name: "Default".into(),
            match_process: vec![],
            target: VirtualTarget::Xbox360,
            left_stick: StickConfig::default(),
            right_stick: StickConfig::default(),
            trigger_deadzone: 0.02,
            remap: vec![],
            bindings: vec![],
            lightbar: [0, 64, 255],
            player_leds: 0b00100,
            hide_physical: true,
            rumble_passthrough: true,
            rumble_strength: 1.0,
            rumble_swap: false,
            controller_audio: false,
            audio_volume: 70,
            gyro: GyroConfig::default(),
            shift_button: String::new(),
            shift_bindings: vec![],
            mouse: Default::default(),
            chords: vec![],
            hold_ms: 250,
            double_tap_ms: 250,
            trigger_soft_point: 0.12,
            trigger_full_point: 0.9,
            trigger_left: Default::default(),
            trigger_right: Default::default(),
            light: Default::default(),
            rumble_shape: Default::default(),
            audio_haptics: Default::default(),
            flick: Default::default(),
        }
    }
}

impl Profile {
    pub fn lightbar_rgb(&self) -> Rgb {
        Rgb(self.lightbar[0], self.lightbar[1], self.lightbar[2])
    }

    pub fn load(path: &Path) -> Result<Profile> {
        let text = std::fs::read_to_string(path)?;
        toml::from_str(&text).map_err(|e| Error::Profile(format!("{}: {e}", path.display())))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let text = toml::to_string_pretty(self).map_err(|e| Error::Profile(e.to_string()))?;
        std::fs::write(path, text)?;
        Ok(())
    }

    /// Load every *.toml in a directory. Bad files are logged and skipped.
    pub fn load_dir(dir: &Path) -> Vec<Profile> {
        let Ok(entries) = std::fs::read_dir(dir) else { return vec![] };
        let mut out = vec![];
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "toml") {
                match Profile::load(&p) {
                    Ok(pr) => out.push(pr),
                    Err(err) => log::warn!("skipping profile: {err}"),
                }
            }
        }
        out
    }

    pub fn matches(&self, exe_name: &str) -> bool {
        self.match_process.iter().any(|m| m.eq_ignore_ascii_case(exe_name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_toml() {
        let p = Profile { name: "Test".into(), match_process: vec!["game.exe".into()], ..Default::default() };
        let text = toml::to_string_pretty(&p).unwrap();
        let back: Profile = toml::from_str(&text).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn shipped_profiles_parse() {
        let d: Profile = toml::from_str(include_str!("../../../profiles/default.toml")).unwrap();
        assert_eq!(d.name, "Default");
        let g: Profile = toml::from_str(include_str!("../../../profiles/example-game.toml")).unwrap();
        assert!(g.matches("EldenRing.exe"));
        assert_eq!(g.right_stick.deadzone, 0.06);
    }

    #[test]
    fn json_roundtrip_for_the_ui() {
        let mut p = Profile::default();
        p.remap = vec![("cross".into(), "circle".into())];
        p.right_stick.curve = Curve::Power { exponent: 1.5 };
        let j = serde_json::to_string(&p).unwrap();
        assert!(j.contains("\"type\":\"power\""));
        let back: Profile = serde_json::from_str(&j).unwrap();
        assert_eq!(p, back);
        // partial JSON from the UI falls back to defaults like TOML does
        let mini: Profile = serde_json::from_str(r#"{"name":"Mini"}"#).unwrap();
        assert_eq!(mini.target, VirtualTarget::Xbox360);
    }

    #[test]
    fn bezier_curve_shapes() {
        // straight handles = a straight line
        for t in [0.0, 0.25, 0.5, 0.9, 1.0] {
            assert!((bezier(t, 0.3, 0.3, 0.7, 0.7) - t).abs() < 1e-3, "{t}");
        }
        // "precision": slow near the centre, catches up at the edge
        let c = Curve::Bezier { x1: 0.6, y1: 0.05, x2: 0.9, y2: 0.5 };
        assert!(c.apply(0.3) < 0.1);
        assert!((c.apply(1.0) - 1.0).abs() < 1e-3);
        assert_eq!(c.apply(0.0), 0.0);
        // always rising
        let mut prev = 0.0;
        for i in 0..=50 {
            let v = c.apply(i as f32 / 50.0);
            assert!(v + 1e-4 >= prev);
            prev = v;
        }
    }

    #[test]
    fn rumble_shaping() {
        let flat = RumbleShape::default();
        assert_eq!((flat.apply(0), flat.apply(128), flat.apply(255)), (0, 128, 255));
        let s = RumbleShape { floor: 0.2, ceiling: 0.8, curve: 1.0 };
        assert_eq!(s.apply(0), 0, "silence stays silent");
        assert_eq!(s.apply(1), 52, "weakest rumble is lifted to the floor (20 % = 51)");
        assert_eq!(s.apply(255), 204, "strongest is capped at the ceiling");
        let quiet_boost = RumbleShape { curve: 0.5, ..Default::default() };
        assert!(quiet_boost.apply(64) > 64);
    }

    #[test]
    fn new_fields_round_trip() {
        use crate::output::TriggerEffect;
        let mut p = Profile::default();
        p.left_stick.anti_deadzone = 0.15;
        p.right_stick.curve = Curve::Bezier { x1: 0.5, y1: 0.0, x2: 1.0, y2: 0.5 };
        p.gyro.ratchet = "r1".into();
        p.gyro.smoothing = 0.5;
        p.gyro.space = GyroSpace::Player;
        p.trigger_right = TriggerEffect::Bow { start: 1, end: 6, strength: 6, snap: 8 };
        p.trigger_left = TriggerEffect::Feedback { start: 2, strength: 4 };
        p.light.mode = crate::light::LightMode::Breathing;
        p.light.flash_on_rumble = true;
        p.rumble_shape.floor = 0.2;
        p.audio_haptics.mode = AudioHapticsMode::Rumble;
        p.flick.enabled = true;
        p.mouse.stick_curve = Curve::Bezier { x1: 0.6, y1: 0.1, x2: 0.9, y2: 0.6 };
        let text = toml::to_string_pretty(&p).unwrap();
        let back: Profile = toml::from_str(&text).unwrap();
        assert_eq!(p, back);
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("\"type\":\"bow\""), "{json}");
        let back: Profile = serde_json::from_str(&json).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn v02_profile_still_loads() {
        // A profile written by DXO 0.2 (no 0.3 fields at all).
        let old = r#"
            name = "Old"
            remap = [["cross", "circle"]]
            [left_stick]
            deadzone = 0.1
            outer = 0.95
            invert_y = false
            [left_stick.curve]
            type = "power"
            exponent = 1.5
            [gyro]
            to_mouse = true
            mouse_hold = "l2"
            [mouse]
            stick = "right"
        "#;
        let p: Profile = toml::from_str(old).unwrap();
        assert_eq!(p.left_stick.anti_deadzone, 0.0);
        assert_eq!(p.gyro.smoothing, 0.0);
        assert_eq!(p.hold_ms, 250);
        assert_eq!(p.trigger_right, crate::output::TriggerEffect::Off);
        assert!(p.chords.is_empty());
    }

    #[test]
    fn partial_toml_uses_defaults() {
        let p: Profile = toml::from_str("name = \"Mini\"\nlightbar = [255, 0, 0]").unwrap();
        assert_eq!(p.name, "Mini");
        assert_eq!(p.target, VirtualTarget::Xbox360);
    }
}
