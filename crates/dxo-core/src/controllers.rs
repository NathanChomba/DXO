//! Per-controller settings, remembered by the controller's own identity.
//!
//! Each physical controller keeps:
//!   * a name ("Caesar's PS4"),
//!   * its default profile (used whenever no game rule applies),
//!   * what it emulates (Xbox 360 / DualShock 4 / native) and whether the real
//!     pad is hidden. Changed by hand only, never by a game switching profiles,
//!   * per-game overrides: "in eldenring.exe, use profile X".
//!
//! Stored in `controllers.toml` next to the profiles folder.

use crate::output::Rgb;
use crate::profile::{Profile, VirtualTarget};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ControllerSettings {
    /// Stable hardware id (the key in controllers.toml). Filled in on load.
    #[serde(skip_serializing)]
    pub id: String,
    /// What the user calls it; empty = the product name.
    pub name: String,
    /// Profile used when no game rule applies; empty = the saved "Default".
    pub default_profile: String,
    /// What games see. Only ever changed by hand.
    pub target: VirtualTarget,
    pub hide_physical: bool,
    /// exe name (case-insensitive) -> profile name, for this controller only.
    pub game_profiles: BTreeMap<String, String>,
    /// Lightbar colour: "" = its player colour (P1 blue, P2 red, P3 green,
    /// P4 pink), "profile" = the active profile's colour, "#rrggbb" = fixed.
    pub lightbar: String,
    /// Bluetooth only: turn the controller off after this many minutes
    /// without input. 0 = never.
    pub idle_disconnect_min: u32,
}

impl Default for ControllerSettings {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            default_profile: String::new(),
            target: VirtualTarget::Xbox360,
            hide_physical: true,
            game_profiles: BTreeMap::new(),
            lightbar: String::new(),
            idle_disconnect_min: 0,
        }
    }
}

/// PS4-style player colours, players 1-4.
pub const PLAYER_COLOURS: [Rgb; 4] = [Rgb(0, 64, 255), Rgb(255, 0, 32), Rgb(0, 200, 40), Rgb(255, 64, 160)];

impl ControllerSettings {
    /// The lightbar colour this controller shows in `slot` with `profile` active.
    pub fn lightbar_rgb(&self, slot: usize, profile: &Profile) -> Rgb {
        let v = self.lightbar.trim();
        if v.eq_ignore_ascii_case("profile") {
            return profile.lightbar_rgb();
        }
        if let Some(c) = parse_hex(v) {
            return c;
        }
        PLAYER_COLOURS[slot.min(PLAYER_COLOURS.len() - 1)]
    }
}

fn parse_hex(s: &str) -> Option<Rgb> {
    let h = s.strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    let b = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some(Rgb(b(0)?, b(2)?, b(4)?))
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct File {
    controllers: BTreeMap<String, ControllerSettings>,
}

/// All controllers DXO has seen, saved to disk on every change.
#[derive(Debug, Default)]
pub struct ControllerStore {
    path: Option<PathBuf>,
    map: BTreeMap<String, ControllerSettings>,
}

impl ControllerStore {
    pub fn load(path: Option<PathBuf>) -> ControllerStore {
        let mut map = BTreeMap::new();
        if let Some(p) = &path {
            if let Ok(text) = std::fs::read_to_string(p) {
                match toml::from_str::<File>(&text) {
                    Ok(f) => map = f.controllers,
                    Err(e) => log::warn!("{}: {e}", p.display()),
                }
            }
        }
        for (id, c) in map.iter_mut() {
            c.id = id.clone();
        }
        ControllerStore { path, map }
    }

    /// Settings for this controller, creating them on first sight.
    pub fn get_or_create(&mut self, id: &str, product: &str) -> ControllerSettings {
        if !self.map.contains_key(id) {
            let c = ControllerSettings { id: id.to_string(), name: product.to_string(), ..Default::default() };
            self.map.insert(id.to_string(), c);
            self.save();
        }
        self.map[id].clone()
    }

    pub fn get(&self, id: &str) -> Option<ControllerSettings> {
        self.map.get(id).cloned()
    }

    pub fn set(&mut self, c: ControllerSettings) {
        if c.id.is_empty() {
            return;
        }
        self.map.insert(c.id.clone(), c);
        self.save();
    }

    fn save(&self) {
        let Some(p) = &self.path else { return };
        let f = File { controllers: self.map.clone() };
        match toml::to_string_pretty(&f) {
            Ok(text) => {
                if let Err(e) = std::fs::write(p, text) {
                    log::warn!("save {}: {e}", p.display());
                }
            }
            Err(e) => log::warn!("controllers.toml: {e}"),
        }
    }
}

/// The profile a controller should run while `exe` is in front (None or a
/// non-game = desktop): its own rule for that game, else the game's profile,
/// else its default. Emulation and hiding always come from the controller.
pub fn effective_profile(c: &ControllerSettings, exe: Option<&str>, profiles: &[Profile], fallback: &Profile) -> Profile {
    let by_name = |n: &str| profiles.iter().find(|p| p.name == n).cloned();
    let mut p = exe
        .and_then(|e| c.game_profiles.iter().find(|(k, _)| k.eq_ignore_ascii_case(e)).and_then(|(_, n)| by_name(n)))
        .or_else(|| exe.and_then(|e| profiles.iter().find(|p| p.matches(e)).cloned()))
        .or_else(|| (!c.default_profile.is_empty()).then(|| by_name(&c.default_profile)).flatten())
        .unwrap_or_else(|| fallback.clone());
    p.target = c.target;
    p.hide_physical = c.hide_physical;
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prof(name: &str, games: &[&str]) -> Profile {
        let mut p = Profile::default();
        p.name = name.into();
        p.match_process = games.iter().map(|s| s.to_string()).collect();
        p
    }

    #[test]
    fn resolution_order() {
        let profiles = vec![prof("Default", &[]), prof("Souls", &["eldenring.exe"]), prof("Gyro", &[])];
        let fallback = Profile::default();
        let mut c = ControllerSettings { default_profile: "Gyro".into(), target: VirtualTarget::DualShock4, ..Default::default() };

        // desktop / unknown app -> the controller's default
        assert_eq!(effective_profile(&c, None, &profiles, &fallback).name, "Gyro");
        assert_eq!(effective_profile(&c, Some("chrome.exe"), &profiles, &fallback).name, "Gyro");
        // a game with a profile -> that profile
        assert_eq!(effective_profile(&c, Some("EldenRing.exe"), &profiles, &fallback).name, "Souls");
        // this controller's own rule beats the game's profile
        c.game_profiles.insert("eldenring.exe".into(), "Default".into());
        assert_eq!(effective_profile(&c, Some("eldenring.exe"), &profiles, &fallback).name, "Default");
        // emulation always comes from the controller, never the profile
        assert_eq!(effective_profile(&c, Some("eldenring.exe"), &profiles, &fallback).target, VirtualTarget::DualShock4);
    }

    #[test]
    fn lightbar_choice() {
        let p = Profile::default();
        let mut c = ControllerSettings::default();
        assert_eq!(c.lightbar_rgb(1, &p), PLAYER_COLOURS[1]);
        c.lightbar = "profile".into();
        assert_eq!(c.lightbar_rgb(1, &p), p.lightbar_rgb());
        c.lightbar = "#FF8000".into();
        assert_eq!(c.lightbar_rgb(3, &p), Rgb(255, 128, 0));
        c.lightbar = "#nope".into();
        assert_eq!(c.lightbar_rgb(9, &p), PLAYER_COLOURS[3]);
    }

    #[test]
    fn store_round_trip() {
        let dir = std::env::temp_dir().join(format!("dxo-ctl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("controllers.toml");
        let mut s = ControllerStore::load(Some(path.clone()));
        let mut c = s.get_or_create("sony-a4b1c2d3e4f5", "Wireless Controller");
        c.name = "Caesar's PS4".into();
        c.target = VirtualTarget::None;
        c.game_profiles.insert("eldenring.exe".into(), "Souls".into());
        s.set(c.clone());
        let back = ControllerStore::load(Some(path)).get("sony-a4b1c2d3e4f5").unwrap();
        assert_eq!(back, c);
        let _ = std::fs::remove_dir_all(dir);
    }
}
