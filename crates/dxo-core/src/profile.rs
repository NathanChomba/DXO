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
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StickConfig {
    pub deadzone: f32,
    pub outer: f32,
    pub curve: Curve,
    pub invert_y: bool,
}

impl Default for StickConfig {
    fn default() -> Self {
        Self { deadzone: 0.08, outer: 0.98, curve: Curve::Linear, invert_y: false }
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
        }
    }
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
    fn partial_toml_uses_defaults() {
        let p: Profile = toml::from_str("name = \"Mini\"\nlightbar = [255, 0, 0]").unwrap();
        assert_eq!(p.name, "Mini");
        assert_eq!(p.target, VirtualTarget::Xbox360);
    }
}
