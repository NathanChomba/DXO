//! The starter community hub in `dxo-profiles/` must stay loadable: every
//! entry in index.json has the required fields, a known category, and points
//! at a profile file the engine can read.

use std::path::PathBuf;

use dxo_core::profile::Profile;

/// Keep in step with `ProfileTransfer.Categories` in the app.
const CATEGORIES: &[&str] = &[
    "Shooter", "Racing", "Action & adventure", "Sports", "Fighting", "Platformer",
    "Strategy", "Flight & simulation", "Emulation", "Desktop", "Other",
];

#[test]
fn every_hub_profile_loads() {
    let hub = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../dxo-profiles");
    let text = std::fs::read_to_string(hub.join("index.json")).expect("index.json");
    let index: serde_json::Value = serde_json::from_str(&text).expect("index.json is JSON");
    let entries = index["profiles"].as_array().expect("a \"profiles\" list");
    assert!(!entries.is_empty());

    for e in entries {
        let name = e["name"].as_str().expect("name");
        let url = e["url"].as_str().expect("url");
        let category = e["category"].as_str().unwrap_or("Other");
        assert!(CATEGORIES.contains(&category), "{name}: unknown category {category}");
        assert!(e["game"].as_str().is_some_and(|g| !g.is_empty()), "{name}: no game");

        let profile = Profile::load(&hub.join(url)).unwrap_or_else(|err| panic!("{name}: {err}"));
        assert_eq!(profile.name, name, "{url}: name differs from index.json");
        assert!(!profile.match_process.is_empty(), "{name}: no game exe");
        // Hub profiles never open programs (the app strips them anyway).
        let text = std::fs::read_to_string(hub.join(url)).unwrap();
        assert!(!text.contains("launch"), "{name}: contains a launch action");
    }
}
