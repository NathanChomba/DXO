//! Milestone 7: the quick menu, a ring of actions around a centre.
//!
//! Like the on-screen keyboard, this file is only the menu itself: which
//! items there are, which one is pointed at, and what the pad does. The
//! worker that owns the menu feeds it RAW pad state and carries out the
//! action it returns; the UI draws `view()` and `layout_json()`.
//!
//! Pad:
//!   either stick        point at an item (the first item is at the top,
//!                       the rest go round clockwise)
//!   D-pad left / right  step round the ring
//!   Cross / A, R3       do the pointed-at item
//!   let go of a stick   does the item it was pointing at
//!   Circle / B          close without doing anything
//! Volume items keep the menu open so they can be pressed again; every other
//! item closes it.

use crate::chords::SysAction;
use crate::state::{Buttons, PadState};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// A stick counts as pointing past ON and as let go below OFF.
const POINT_ON: f32 = 0.6;
const POINT_OFF: f32 = 0.3;
const STEP_FIRST: Duration = Duration::from_millis(350);
const STEP_REPEAT: Duration = Duration::from_millis(140);
/// Most items the ring holds.
pub const MAX_ITEMS: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RadialItem {
    pub label: String,
    pub action: SysAction,
}

/// The menu DXO ships with.
pub fn default_items() -> Vec<RadialItem> {
    [
        ("Play / pause", SysAction::MediaPlayPause),
        ("Next track", SysAction::MediaNext),
        ("Volume up", SysAction::VolumeUp),
        ("Keyboard", SysAction::ToggleKeyboard),
        ("Screenshot", SysAction::Screenshot),
        ("Mute", SysAction::Mute),
        ("Volume down", SysAction::VolumeDown),
        ("Previous track", SysAction::MediaPrev),
    ]
    .into_iter()
    .map(|(l, a)| RadialItem { label: l.into(), action: a })
    .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RadialView {
    pub open: bool,
    /// Pointed-at item; 255 = none.
    pub selected: u8,
    pub count: u8,
    /// Goes up whenever the items change, so the UI knows to redraw them.
    pub rev: u32,
    /// Goes up every time an item is carried out (flash it).
    pub fired: u32,
}

#[derive(Debug)]
pub struct Radial {
    items: Vec<RadialItem>,
    open: bool,
    selected: Option<usize>,
    /// The stick is what chose `selected`, so letting it go carries it out.
    by_stick: bool,
    prev: u32,
    step_dir: i8,
    step_next: Option<Instant>,
    rev: u32,
    fired: u32,
}

impl Default for Radial {
    fn default() -> Self {
        Radial {
            items: default_items(),
            open: false,
            selected: None,
            by_stick: false,
            prev: u32::MAX,
            step_dir: 0,
            step_next: None,
            rev: 1,
            fired: 0,
        }
    }
}

impl Radial {
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Replace the items. Empty labels and "none" actions are dropped; an
    /// empty list brings the defaults back.
    pub fn set_items(&mut self, items: Vec<RadialItem>) {
        let mut items: Vec<RadialItem> = items
            .into_iter()
            .filter(|i| !i.label.trim().is_empty() && !matches!(i.action, SysAction::None | SysAction::RadialMenu))
            .take(MAX_ITEMS)
            .collect();
        if items.is_empty() {
            items = default_items();
        }
        self.items = items;
        self.selected = None;
        self.rev = self.rev.wrapping_add(1);
    }

    pub fn items(&self) -> &[RadialItem] {
        &self.items
    }

    pub fn open(&mut self) {
        if self.open {
            return;
        }
        self.open = true;
        self.selected = None;
        self.by_stick = false;
        self.prev = u32::MAX; // buttons held right now wait for a release
        self.step_dir = 0;
        self.step_next = None;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.selected = None;
        self.by_stick = false;
    }

    pub fn view(&self) -> RadialView {
        RadialView {
            open: self.open,
            selected: self.selected.map_or(255, |i| i as u8),
            count: self.items.len() as u8,
            rev: self.rev,
            fired: self.fired,
        }
    }

    /// `{"items":[{"label":"Mute","type":"mute"}, ...]}`: `type` is the
    /// action's kind, so the UI can pick an icon.
    pub fn layout_json(&self) -> String {
        let items: Vec<serde_json::Value> = self
            .items
            .iter()
            .map(|i| {
                let kind = serde_json::to_value(&i.action).ok().and_then(|v| v.get("type").cloned()).unwrap_or_default();
                serde_json::json!({ "label": i.label, "type": kind })
            })
            .collect();
        serde_json::json!({ "items": items }).to_string()
    }

    /// Feed one RAW report. Returns the action to carry out, if any.
    pub fn step(&mut self, raw: &PadState, now: Instant) -> Option<SysAction> {
        if !self.open || self.items.is_empty() {
            return None;
        }
        let b = raw.buttons.0;
        // PS held: the chord layer owns the pad.
        if b & Buttons::HOME != 0 {
            self.prev = b;
            self.step_dir = 0;
            return None;
        }
        let fresh = self.prev == u32::MAX; // just opened
        let prev = self.prev;
        let edge = |bit: u32| !fresh && b & bit != 0 && prev & bit == 0;
        let n = self.items.len();
        let mut act: Option<usize> = None;

        // ---- sticks: whichever is pushed further points ----
        let (lm, rm) = (raw.lx.hypot(raw.ly), raw.rx.hypot(raw.ry));
        let (x, y, mag) = if rm > lm { (raw.rx, raw.ry, rm) } else { (raw.lx, raw.ly, lm) };
        if mag >= POINT_ON {
            // Angle clockwise from straight up, in turns; item 0 is centred on the top.
            let turns = (x.atan2(y) / std::f32::consts::TAU).rem_euclid(1.0);
            self.selected = Some(((turns * n as f32 + 0.5) as usize) % n);
            self.by_stick = true;
        } else if mag < POINT_OFF && self.by_stick {
            self.by_stick = false;
            act = self.selected; // flick and let go
        }

        // ---- D-pad: step round the ring ----
        let dir: i8 = if b & Buttons::DPAD_RIGHT != 0 {
            1
        } else if b & Buttons::DPAD_LEFT != 0 {
            -1
        } else {
            0
        };
        if fresh && dir != 0 {
            // held from before the menu opened: wait for a release
        } else if dir == 0 {
            self.step_dir = 0;
            self.step_next = None;
        } else if dir != self.step_dir {
            self.step_dir = dir;
            self.step_next = Some(now + STEP_FIRST);
            self.step_by(dir);
        } else if self.step_next.is_some_and(|t| now >= t) {
            self.step_next = Some(now + STEP_REPEAT);
            self.step_by(dir);
        }

        // ---- buttons ----
        if edge(Buttons::EAST) {
            self.close();
            self.prev = b;
            return None;
        }
        if edge(Buttons::SOUTH) || edge(Buttons::R3) {
            self.by_stick = false; // the button did it; letting the stick go must not do it again
            act = self.selected;
        }
        self.prev = b;

        let i = act?;
        let action = self.items.get(i)?.action.clone();
        self.fired = self.fired.wrapping_add(1);
        if !matches!(action, SysAction::VolumeUp | SysAction::VolumeDown) {
            self.close();
        }
        Some(action)
    }

    fn step_by(&mut self, dir: i8) {
        let n = self.items.len();
        self.by_stick = false;
        self.selected = Some(match self.selected {
            None => 0,
            Some(i) if dir > 0 => (i + 1) % n,
            Some(i) => (i + n - 1) % n,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pad(bits: &[u32]) -> PadState {
        let mut s = PadState::default();
        for b in bits {
            s.buttons.set(*b, true);
        }
        s
    }

    fn stick(x: f32, y: f32) -> PadState {
        let mut s = PadState::default();
        (s.rx, s.ry) = (x, y);
        s
    }

    fn open() -> (Radial, Instant) {
        let mut r = Radial::default();
        let t = Instant::now();
        r.open();
        assert_eq!(r.step(&PadState::default(), t), None);
        (r, t)
    }

    #[test]
    fn stick_points_clockwise_from_the_top() {
        let (mut r, t) = open();
        assert_eq!(r.view().count, 8);
        r.step(&stick(0.0, 1.0), t);
        assert_eq!(r.view().selected, 0, "up = first item");
        r.step(&stick(1.0, 0.0), t);
        assert_eq!(r.view().selected, 2, "right = a quarter of the way round");
        r.step(&stick(0.0, -1.0), t);
        assert_eq!(r.view().selected, 4);
        r.step(&stick(-1.0, 0.0), t);
        assert_eq!(r.view().selected, 6);
        r.step(&stick(-0.1, 1.0), t);
        assert_eq!(r.view().selected, 0, "just left of the top is still the first item");
    }

    #[test]
    fn letting_the_stick_go_does_the_item_and_closes() {
        let (mut r, t) = open();
        assert_eq!(r.step(&stick(0.0, -1.0), t), None);
        assert_eq!(r.step(&stick(0.0, -0.45), t), None, "easing off is not letting go");
        assert_eq!(r.step(&PadState::default(), t), Some(SysAction::Screenshot));
        assert!(!r.is_open());
        assert_eq!(r.view().fired, 1);
    }

    #[test]
    fn cross_does_it_and_volume_keeps_the_menu_open() {
        let (mut r, t) = open();
        r.step(&stick(1.0, 0.0), t); // Volume up
        let mut held = stick(1.0, 0.0);
        held.buttons.set(Buttons::SOUTH, true);
        assert_eq!(r.step(&held, t), Some(SysAction::VolumeUp));
        assert!(r.is_open(), "press it again for more volume");
        r.step(&stick(1.0, 0.0), t);
        assert_eq!(r.step(&held, t), Some(SysAction::VolumeUp));
        // letting the stick go afterwards must not fire a third time
        assert_eq!(r.step(&PadState::default(), t), None);
    }

    #[test]
    fn dpad_steps_and_circle_closes() {
        let (mut r, t) = open();
        r.step(&pad(&[Buttons::DPAD_RIGHT]), t);
        assert_eq!(r.view().selected, 0, "first press lands on the first item");
        r.step(&PadState::default(), t);
        r.step(&pad(&[Buttons::DPAD_LEFT]), t);
        assert_eq!(r.view().selected, 7, "wraps");
        r.step(&pad(&[Buttons::DPAD_LEFT]), t + Duration::from_millis(360));
        assert_eq!(r.view().selected, 6, "repeats while held");
        r.step(&PadState::default(), t);
        assert_eq!(r.step(&pad(&[Buttons::EAST]), t), None);
        assert!(!r.is_open());
    }

    #[test]
    fn buttons_held_at_open_and_ps_do_nothing() {
        let mut r = Radial::default();
        let t = Instant::now();
        r.open();
        // opened with PS + R3: R3 is still down when PS is let go
        assert_eq!(r.step(&pad(&[Buttons::HOME, Buttons::R3]), t), None);
        assert_eq!(r.step(&pad(&[Buttons::R3]), t), None);
        assert!(r.is_open());
        r.step(&PadState::default(), t);
        r.step(&pad(&[Buttons::DPAD_RIGHT]), t);
        r.step(&PadState::default(), t);
        assert_eq!(r.step(&pad(&[Buttons::R3]), t), Some(SysAction::MediaPlayPause));
    }

    #[test]
    fn items_can_be_replaced_and_fall_back_to_defaults() {
        let mut r = Radial::default();
        let rev = r.view().rev;
        r.set_items(vec![
            RadialItem { label: "Discord".into(), action: SysAction::Launch { target: "discord.exe".into(), args: String::new() } },
            RadialItem { label: "".into(), action: SysAction::Mute },
            RadialItem { label: "Nothing".into(), action: SysAction::None },
        ]);
        assert_eq!(r.items().len(), 1);
        assert_ne!(r.view().rev, rev);
        let v: serde_json::Value = serde_json::from_str(&r.layout_json()).unwrap();
        assert_eq!(v["items"][0]["label"], "Discord");
        assert_eq!(v["items"][0]["type"], "launch");
        r.set_items(vec![]);
        assert_eq!(r.items(), default_items().as_slice());
    }
}
