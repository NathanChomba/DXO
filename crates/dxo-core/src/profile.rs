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

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GyroConfig {
    pub cemuhook_server: bool,
    pub cemuhook_port: u16,
    /// M5: gyro -> right stick / mouse
    pub to_mouse: bool,
    pub sensitivity: f32,
}

impl Default for GyroConfig {
    fn default() -> Self {
        Self { cemuhook_server: false, cemuhook_port: 26760, to_mouse: false, sensitivity: 1.0 }
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
    /// (from, to) pairs using names from `mapping::button_from_name`
    pub remap: Vec<(String, String)>,
    pub lightbar: [u8; 3],
    pub player_leds: u8,
    pub hide_physical: bool,
    pub gyro: GyroConfig,
    /// Disconnect a Bluetooth pad after this many idle minutes (0 = never). M6.
    pub idle_disconnect_min: u32,
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
            lightbar: [0, 64, 255],
            player_leds: 0b00100,
            hide_physical: true,
            gyro: GyroConfig::default(),
            idle_disconnect_min: 15,
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
    fn partial_toml_uses_defaults() {
        let p: Profile = toml::from_str("name = \"Mini\"\nlightbar = [255, 0, 0]").unwrap();
        assert_eq!(p.name, "Mini");
        assert_eq!(p.target, VirtualTarget::Xbox360);
    }
}
