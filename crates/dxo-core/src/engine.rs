//! The engine: one MANAGER thread plus one WORKER thread per controller.
//!
//!   manager:  scans for pads (1 Hz) -> opens each new one -> hands it to a worker
//!             owns HidApi, the profile list, game auto-switch, HidHide setup
//!   worker:   physical pad --(parse)--> PadState --(profile/bindings)--> virtual pad
//!                  ^                                  \--> CemuHook (slot 0 only)
//!                  \--(output report)-- lightbar / rumble / triggers / audio
//!
//! Up to four controllers, each in a stable SLOT (0..3 = player 1..4). A slot
//! remembers its pad's path, so a pad that drops out and comes back (Bluetooth
//! hiccup, our own hide-restart) gets its slot, profile and settings back.
//!
//! The UI talks to one SELECTED slot at a time (`select`): every older API call
//! (`snapshot`, `set_profile`, `set_virtual`...) acts on it, so pages written
//! for one controller keep working; `pads()` lists them all.

use crate::bindings::{self, Layers, Runner};
use crate::chords::{self, ChordFired, Chords, SysBinding};
use crate::pointer::Pointer;
use crate::controllers::{effective_profile, ControllerSettings, ControllerStore};
use crate::device::{self, Device, Kind};
use crate::error::{Error, Result};
use crate::flick::FlickStick;
use crate::foreground::Watcher;
use crate::gyro::{CalState, Calibrator, CemuHookServer, GyroMouse};
use crate::hidhide::{self, HidHide};
use crate::light::LightInput;
use crate::mapping;
use crate::osk::{Osk, OskOut, OskView};
use crate::output::{self, OutputState};
use crate::profile::{AudioHapticsMode, Profile, VirtualTarget};
use crate::radial::{Radial, RadialItem, RadialView};
use crate::state::{BatteryStatus, PadState, Transport};
use crate::virtual_pad::{self, NullPad, VirtualPad};
use hidapi::HidApi;
use std::collections::{HashMap, HashSet, VecDeque};
use std::ffi::CString;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Most controllers DXO drives at once (XInput's limit too).
pub const MAX_PADS: usize = 4;

#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub connected: bool,
    pub kind: Option<Kind>,
    pub product: String,
    pub raw: PadState,
    pub mapped: PadState,
    pub profile_name: String,
    pub last_error: Option<String>,
    pub reports_per_sec: u32,
    /// HidHide is currently hiding the physical pad from other apps.
    pub pad_hidden: bool,
    /// A virtual controller is plugged in for games.
    pub virtual_active: bool,
    pub motion: MotionStatus,
}

/// Live motion readings + gyro feature status for the Motion page.
#[derive(Debug, Clone, Copy, Default)]
pub struct MotionStatus {
    /// Calibrated angular velocity, degrees/second (pitch, yaw, roll).
    pub gyro_dps: [f32; 3],
    /// Acceleration in g (x, y, z); about 1 g total at rest.
    pub accel_g: [f32; 3],
    pub cal_state: CalState,
    /// CemuHook server is open on `cemu_port`.
    pub cemu_listening: bool,
    /// CemuHook is switched on but the port couldn't be opened (in use?).
    pub cemu_failed: bool,
    pub cemu_port: u16,
    pub cemu_clients: u8,
    /// Gyro is moving the mouse right now (enabled + hold condition met).
    pub mouse_active: bool,
}

#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub profiles_dir: Option<PathBuf>,
    pub include_generic: bool,
    /// Create the virtual pad + HidHide. Off = read-only (M1/M2 testing).
    pub enable_virtual: bool,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self { profiles_dir: None, include_generic: false, enable_virtual: true }
    }
}

/// Diagnostics page data: report arrival times and the latest raw report.
#[derive(Debug, Clone, Default)]
pub struct Diag {
    /// Gap between consecutive input reports as DXO received them, microseconds (newest last).
    pub intervals_us: VecDeque<u32>,
    pub last_report: Vec<u8>,
    /// Total reports since connect.
    pub reports: u64,
}

impl Diag {
    pub const KEEP: usize = 500;
}

/// One controller slot as the UI lists it.
#[derive(Debug, Clone)]
pub struct PadSummary {
    pub slot: usize,
    pub snapshot: Snapshot,
    pub profile: Profile,
    pub selected: bool,
    /// This controller's saved settings (name, default profile, emulation, game rules).
    pub controller: Option<ControllerSettings>,
}

/// Everything about one slot that the UI and its worker share.
struct Pad {
    slot: usize,
    snapshot: Mutex<Snapshot>,
    /// UI-requested overrides layered on top of the profile (lightbar, triggers)
    output_override: Mutex<Option<OutputState>>,
    profile: Mutex<Profile>,
    /// UI asked for a fresh connect (e.g. "Fix double input").
    reconnect: AtomicBool,
    /// UI asked for a manual gyro calibration.
    calibrate_gyro: AtomicBool,
    diag: Mutex<Diag>,
    /// A worker is driving this slot right now.
    active: AtomicBool,
    /// Hardware id of the controller in this slot (key into the controller store).
    controller_id: Mutex<Option<String>>,
    /// The device this slot was last used by, and when it went away.
    path: Mutex<Option<CString>>,
    gone_at: Mutex<Option<Instant>>,
}

impl Pad {
    fn new(slot: usize) -> Pad {
        Pad {
            slot,
            snapshot: Mutex::new(Snapshot::default()),
            output_override: Mutex::new(None),
            profile: Mutex::new(Profile::default()),
            reconnect: AtomicBool::new(false),
            calibrate_gyro: AtomicBool::new(false),
            diag: Mutex::new(Diag::default()),
            active: AtomicBool::new(false),
            controller_id: Mutex::new(None),
            path: Mutex::new(None),
            gone_at: Mutex::new(None),
        }
    }
}

struct Shared {
    running: AtomicBool,
    /// Set if the engine refused to start (e.g. another copy running).
    fatal: AtomicBool,
    /// Profiles were saved/deleted: re-read the folder for game auto-switch.
    reload_profiles: AtomicBool,
    /// Also look for non-Sony gamepads (Settings > Other controllers).
    include_generic: AtomicBool,
    /// System chords (hold PS + a button) are active.
    chords_enabled: AtomicBool,
    /// The Diagnostics page is open, so the workers record jitter + last report.
    diag_active: AtomicBool,
    /// A chord asked to show/hide the DXO window; the UI polls and clears this.
    window_toggle: AtomicBool,
    /// Ask the manager to scan now instead of waiting for its next tick.
    rescan: AtomicBool,
    enable_virtual: bool,
    pads: Vec<Arc<Pad>>,
    selected: AtomicUsize,
    /// Engine-wide problem (no controller, HidHide missing...).
    global_error: Mutex<Option<String>>,
    /// Devices we restarted after hiding them, so their own reconnect doesn't
    /// trigger another restart (that would loop forever).
    cycled: Mutex<HashMap<String, Instant>>,
    /// A worker is plugging in a virtual pad: a new controller that shows up
    /// before this moment is that virtual pad, not a real one.
    virtual_window: Mutex<Option<Instant>>,
    /// Saved per-controller settings (controllers.toml).
    controllers: Mutex<ControllerStore>,
    /// Profiles from the profiles folder.
    profiles: Mutex<Vec<Profile>>,
    /// The app in front right now (exe name), None = not known yet.
    fg_exe: Mutex<Option<String>>,
    /// A full-screen game or video is in front (touchpad/stick-as-mouse stand down).
    fullscreen: AtomicBool,
    /// How loud the bass in the PC's sound is right now, 0..=255 (audio haptics).
    audio_level: Arc<AtomicU8>,
    /// The on-screen keyboard (milestone 7). One for the whole engine.
    osk: Mutex<Osk>,
    /// Slot of the controller typing on it; OSK_CLOSED while it is closed.
    /// That controller is taken away from the game until the keyboard closes.
    osk_owner: AtomicUsize,
    /// PS + Options opens the keyboard (Settings).
    osk_enabled: AtomicBool,
    /// The keyboard was asked for but not shown (a game in front of a
    /// controller in Native mode); the UI polls this to say why.
    osk_refused: AtomicBool,
    /// Exe names (lower case) of installed games the UI found on this PC.
    known_games: Mutex<HashSet<String>>,
    /// A game is in front: full screen, has a profile, or is a known game.
    game_front: AtomicBool,
    /// Settings: PS + Options opens Windows' own gamepad keyboard instead,
    /// for a controller emulated as an Xbox pad.
    osk_external: AtomicBool,
    /// ...and this asks the UI to open or close it (the UI polls and clears).
    osk_external_toggle: AtomicBool,
    /// The quick menu (radial) and the slot of the controller driving it.
    radial: Mutex<Radial>,
    radial_owner: AtomicUsize,
    /// The system chord table (Chords page) and a counter that goes up when
    /// it changes, so each worker reloads it.
    chord_map: Mutex<Vec<SysBinding>>,
    chord_rev: AtomicU32,
}

/// `Shared::osk_owner` while the keyboard is closed.
const OSK_CLOSED: usize = usize::MAX;

impl Shared {
    /// Is this exe (or whatever is full screen right now) a game? Decides
    /// whether a Native-mode controller may open the keyboard: with no virtual
    /// pad DXO can't keep its presses from the game, so there it stays shut.
    fn is_game(&self, exe: Option<&str>, fullscreen: bool) -> bool {
        if fullscreen {
            return true;
        }
        let Some(exe) = exe else { return false };
        self.profiles.lock().unwrap().iter().any(|p| p.matches(exe))
            || self.controllers.lock().unwrap().knows_game(exe)
            || self.known_games.lock().unwrap().contains(&exe.to_ascii_lowercase())
    }

    fn refresh_game_front(&self) {
        let exe = self.fg_exe.lock().unwrap().clone();
        let game = self.is_game(exe.as_deref(), self.fullscreen.load(Ordering::Acquire));
        self.game_front.store(game, Ordering::Release);
    }

    /// Open the keyboard for the controller in `slot`.
    fn osk_open(&self, slot: usize) {
        self.osk.lock().unwrap().open();
        self.osk_owner.store(slot, Ordering::Release);
    }

    fn osk_close(&self) {
        self.osk_owner.store(OSK_CLOSED, Ordering::Release);
        self.osk.lock().unwrap().close();
    }

    /// Open the quick menu for the controller in `slot`.
    fn radial_open(&self, slot: usize) {
        self.radial.lock().unwrap().open();
        self.radial_owner.store(slot, Ordering::Release);
    }

    fn radial_close(&self) {
        self.radial_owner.store(OSK_CLOSED, Ordering::Release);
        self.radial.lock().unwrap().close();
    }

    /// Pick the profile for one pad from its controller settings and the app in
    /// front. Only swaps it in when the profile actually changes, so live,
    /// unsaved edits survive switching between non-game windows.
    fn apply_rules(&self, pad: &Pad) {
        let Some(id) = pad.controller_id.lock().unwrap().clone() else { return };
        let Some(c) = self.controllers.lock().unwrap().get(&id) else { return };
        let profiles = self.profiles.lock().unwrap().clone();
        let fg = self.fg_exe.lock().unwrap().clone();
        let p = effective_profile(&c, fg.as_deref(), &profiles, &starting_profile(&profiles));
        let mut cur = pad.profile.lock().unwrap();
        if cur.name != p.name {
            log::info!("player {}: profile {} ({})", pad.slot + 1, p.name, fg.as_deref().unwrap_or("desktop"));
            *cur = p;
        } else {
            // same profile: still make sure emulation/hiding follow the controller
            cur.target = c.target;
            cur.hide_physical = c.hide_physical;
        }
    }

    fn apply_rules_all(&self) {
        for pad in self.pads.iter().filter(|p| p.active.load(Ordering::Acquire)) {
            self.apply_rules(pad);
        }
    }

    /// The slot the UI is looking at; falls back to the first connected pad.
    fn current(&self) -> &Arc<Pad> {
        let sel = self.selected.load(Ordering::Acquire).min(MAX_PADS - 1);
        if self.pads[sel].active.load(Ordering::Acquire) {
            return &self.pads[sel];
        }
        self.pads.iter().find(|p| p.active.load(Ordering::Acquire)).unwrap_or(&self.pads[sel])
    }
}

pub struct Engine {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl Engine {
    pub fn start(cfg: EngineConfig) -> Engine {
        let shared = Arc::new(Shared {
            running: AtomicBool::new(true),
            fatal: AtomicBool::new(false),
            reload_profiles: AtomicBool::new(false),
            include_generic: AtomicBool::new(cfg.include_generic),
            chords_enabled: AtomicBool::new(true),
            diag_active: AtomicBool::new(false),
            window_toggle: AtomicBool::new(false),
            rescan: AtomicBool::new(true),
            enable_virtual: cfg.enable_virtual,
            pads: (0..MAX_PADS).map(|i| Arc::new(Pad::new(i))).collect(),
            selected: AtomicUsize::new(0),
            global_error: Mutex::new(None),
            cycled: Mutex::new(HashMap::new()),
            virtual_window: Mutex::new(None),
            // controllers.toml lives beside the profiles folder (%LOCALAPPDATA%\DXO).
            controllers: Mutex::new(ControllerStore::load(
                cfg.profiles_dir.as_ref().and_then(|d| d.parent()).map(|d| d.join("controllers.toml")),
            )),
            profiles: Mutex::new(cfg.profiles_dir.as_deref().map(Profile::load_dir).unwrap_or_default()),
            fg_exe: Mutex::new(None),
            fullscreen: AtomicBool::new(false),
            audio_level: Arc::new(AtomicU8::new(0)),
            osk: Mutex::new(Osk::default()),
            osk_owner: AtomicUsize::new(OSK_CLOSED),
            osk_enabled: AtomicBool::new(true),
            osk_refused: AtomicBool::new(false),
            known_games: Mutex::new(HashSet::new()),
            game_front: AtomicBool::new(false),
            osk_external: AtomicBool::new(false),
            osk_external_toggle: AtomicBool::new(false),
            radial: Mutex::new(Radial::default()),
            radial_owner: AtomicUsize::new(OSK_CLOSED),
            chord_map: Mutex::new(chords::default_map()),
            chord_rev: AtomicU32::new(0),
        });
        let s2 = shared.clone();
        let thread = std::thread::Builder::new()
            .name("dxo-manager".into())
            .spawn(move || {
                // A panic here must not take the host app down: record it and stop the engine.
                let s3 = s2.clone();
                if let Err(p) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| manage(s2, cfg))) {
                    *s3.global_error.lock().unwrap() =
                        Some(format!("Engine crashed: {} (see %LOCALAPPDATA%\\DXO\\engine.log)", panic_text(&p)));
                }
            })
            .expect("spawn engine thread");
        Engine { shared, thread: Some(thread) }
    }

    // ---- the selected controller (what every page edits) ----------------

    /// State of the selected controller. When nothing is connected, carries
    /// the engine-wide error (no controller, HidHide missing...).
    pub fn snapshot(&self) -> Snapshot {
        let pad = self.shared.current();
        let mut s = pad.snapshot.lock().unwrap().clone();
        if !s.connected {
            if let Some(e) = self.shared.global_error.lock().unwrap().clone() {
                s.last_error = Some(e);
            }
        }
        s
    }

    /// Run this profile on the selected controller and make it that controller's
    /// default (what it uses outside games). Emulation/hiding stay the controller's.
    pub fn set_profile(&self, mut p: Profile) {
        let pad = self.shared.current();
        if let Some(id) = pad.controller_id.lock().unwrap().clone() {
            let mut store = self.shared.controllers.lock().unwrap();
            if let Some(mut c) = store.get(&id) {
                p.target = c.target;
                p.hide_physical = c.hide_physical;
                if c.default_profile != p.name {
                    c.default_profile = p.name.clone();
                    store.set(c);
                }
            }
        }
        *pad.profile.lock().unwrap() = p;
    }

    pub fn profile(&self) -> Profile {
        self.shared.current().profile.lock().unwrap().clone()
    }

    /// Change how games see the selected pad. Takes effect immediately: if the
    /// target or hiding changed, its worker reconnects with the new setup.
    pub fn set_virtual(&self, target: VirtualTarget, hide_physical: bool, rumble_passthrough: bool) {
        let pad = self.shared.current();
        // Emulation + hiding belong to the controller: save them there.
        if let Some(id) = pad.controller_id.lock().unwrap().clone() {
            let mut store = self.shared.controllers.lock().unwrap();
            if let Some(mut c) = store.get(&id) {
                c.target = target;
                c.hide_physical = hide_physical;
                store.set(c);
            }
        }
        let mut p = pad.profile.lock().unwrap();
        p.target = target;
        p.hide_physical = hide_physical;
        p.rumble_passthrough = rumble_passthrough;
    }

    /// Saved settings of the controller in `slot` (None if nothing's there).
    pub fn controller(&self, slot: usize) -> Option<ControllerSettings> {
        let id = self.shared.pads.get(slot)?.controller_id.lock().unwrap().clone()?;
        self.shared.controllers.lock().unwrap().get(&id)
    }

    /// Save new settings for the controller in `slot` and apply them now:
    /// emulation/hiding (reconnects that pad if they changed), default profile
    /// and game rules (re-picks its profile for the app in front).
    pub fn set_controller(&self, slot: usize, mut c: ControllerSettings) {
        let Some(pad) = self.shared.pads.get(slot) else { return };
        let Some(id) = pad.controller_id.lock().unwrap().clone() else { return };
        c.id = id;
        self.shared.controllers.lock().unwrap().set(c);
        self.shared.apply_rules(pad);
    }

    /// Drop and rebuild the selected pad's connection: unhide, unplug the
    /// virtual pad, then reconnect and hide again.
    pub fn request_reconnect(&self) {
        self.shared.current().reconnect.store(true, Ordering::Release);
    }

    pub fn diag(&self) -> Diag {
        self.shared.current().diag.lock().unwrap().clone()
    }

    /// Measure the selected pad's gyro drift now (it should lie still for 2 s).
    pub fn calibrate_gyro(&self) {
        self.shared.current().calibrate_gyro.store(true, Ordering::Release);
    }

    /// Override the selected pad's output (lightbar/triggers/rumble); None = use profile.
    pub fn set_output_override(&self, o: Option<OutputState>) {
        *self.shared.current().output_override.lock().unwrap() = o;
    }

    // ---- all controllers -------------------------------------------------

    /// Connected controllers, by slot.
    pub fn pads(&self) -> Vec<PadSummary> {
        let cur = self.shared.current().slot;
        self.shared
            .pads
            .iter()
            .filter(|p| p.active.load(Ordering::Acquire))
            .map(|p| PadSummary {
                slot: p.slot,
                snapshot: p.snapshot.lock().unwrap().clone(),
                profile: p.profile.lock().unwrap().clone(),
                selected: p.slot == cur,
                controller: p
                    .controller_id
                    .lock()
                    .unwrap()
                    .clone()
                    .and_then(|id| self.shared.controllers.lock().unwrap().get(&id)),
            })
            .collect()
    }

    /// Point every per-controller call at this slot.
    pub fn select(&self, slot: usize) {
        if slot < MAX_PADS {
            self.shared.selected.store(slot, Ordering::Release);
        }
    }

    /// The slot calls currently act on.
    pub fn selected(&self) -> usize {
        self.shared.current().slot
    }

    // ---- engine-wide -----------------------------------------------------

    /// Re-read the profiles folder (call after saving or deleting a profile).
    pub fn reload_profiles(&self) {
        self.shared.reload_profiles.store(true, Ordering::Release);
    }

    /// Look for other gamepads too (generic HID / Switch Pro).
    /// Turn system chords (hold PS + button) on or off.
    pub fn set_chords(&self, on: bool) {
        self.shared.chords_enabled.store(on, Ordering::Release);
    }

    /// The Diagnostics page sets this so workers only do per-report jitter/report
    /// bookkeeping while it's on screen.
    pub fn set_diag_active(&self, on: bool) {
        self.shared.diag_active.store(on, Ordering::Release);
    }

    /// Did a chord ask to show/hide the window? Clears the flag.
    pub fn take_window_toggle(&self) -> bool {
        self.shared.window_toggle.swap(false, Ordering::AcqRel)
    }

    pub fn set_include_generic(&self, on: bool) {
        self.shared.include_generic.store(on, Ordering::Release);
        self.shared.rescan.store(true, Ordering::Release);
    }

    // ---- on-screen keyboard -------------------------------------------

    /// What the UI draws: open or not, layer, highlighted key.
    pub fn osk_view(&self) -> OskView {
        self.shared.osk.lock().unwrap().view()
    }

    /// The keys of the layer on show, as JSON (see `Osk::layout_json`).
    pub fn osk_layout_json(&self) -> String {
        self.shared.osk.lock().unwrap().layout_json()
    }

    /// Slot of the controller typing on the keyboard, if it is open.
    pub fn osk_owner(&self) -> Option<usize> {
        let o = self.shared.osk_owner.load(Ordering::Acquire);
        (o != OSK_CLOSED).then_some(o)
    }

    /// Open or close the keyboard from the UI (a text box in DXO, the tray).
    /// The selected controller types on it. Returns false if it could not be
    /// opened because no controller is connected.
    pub fn set_osk_open(&self, open: bool) -> bool {
        if !open {
            self.shared.osk_close();
            return true;
        }
        if self.osk_owner().is_some() {
            return true;
        }
        let pad = self.shared.current();
        if !pad.active.load(Ordering::Acquire) {
            return false;
        }
        self.shared.osk_open(pad.slot);
        true
    }

    /// Let PS + Options open the keyboard (Settings). Off also closes it.
    pub fn set_osk_enabled(&self, on: bool) {
        self.shared.osk_enabled.store(on, Ordering::Release);
        if !on {
            self.shared.osk_close();
        }
    }

    /// Was the keyboard asked for but refused (a game in front, Native mode)?
    /// Clears the flag.
    pub fn take_osk_refused(&self) -> bool {
        self.shared.osk_refused.swap(false, Ordering::AcqRel)
    }

    /// A key clicked with the mouse or tapped on a touch screen.
    pub fn osk_click(&self, row: u8, key: u8) {
        let typed = {
            let mut k = self.shared.osk.lock().unwrap();
            let typed = k.press_at(row, key, Instant::now());
            if !k.is_open() {
                self.shared.osk_owner.store(OSK_CLOSED, Ordering::Release);
            }
            typed
        };
        bindings::send(&osk_events(&typed));
    }

    /// Settings: let PS + Options open Windows' own gamepad keyboard for a
    /// controller emulated as an Xbox pad.
    pub fn set_osk_external(&self, on: bool) {
        self.shared.osk_external.store(on, Ordering::Release);
    }

    /// Should the UI open or close Windows' keyboard now? Clears the flag.
    pub fn take_osk_external_toggle(&self) -> bool {
        self.shared.osk_external_toggle.swap(false, Ordering::AcqRel)
    }

    // ---- quick menu (radial) -------------------------------------------

    pub fn radial_view(&self) -> RadialView {
        self.shared.radial.lock().unwrap().view()
    }

    /// The items as the menu draws them (see `Radial::layout_json`).
    pub fn radial_layout_json(&self) -> String {
        self.shared.radial.lock().unwrap().layout_json()
    }

    /// Slot of the controller driving the quick menu, if it is open.
    pub fn radial_owner(&self) -> Option<usize> {
        let o = self.shared.radial_owner.load(Ordering::Acquire);
        (o != OSK_CLOSED).then_some(o)
    }

    /// The items with their full actions, as JSON, for the editor.
    pub fn radial_items_json(&self) -> String {
        serde_json::to_string(self.shared.radial.lock().unwrap().items()).unwrap_or_default()
    }

    /// Replace the items from JSON (`[{"label":"Mute","action":{"type":"mute"}}]`).
    /// An empty list restores the defaults. False if the JSON is not valid.
    pub fn set_radial_items_json(&self, json: &str) -> bool {
        match serde_json::from_str::<Vec<RadialItem>>(json) {
            Ok(items) => {
                self.shared.radial.lock().unwrap().set_items(items);
                true
            }
            Err(e) => {
                log::warn!("quick menu items: {e}");
                false
            }
        }
    }

    /// Open or close the quick menu from the UI. False if no controller is connected.
    pub fn set_radial_open(&self, open: bool) -> bool {
        if !open {
            self.shared.radial_close();
            return true;
        }
        let pad = self.shared.current();
        if !pad.active.load(Ordering::Acquire) {
            return false;
        }
        self.shared.osk_close();
        self.shared.radial_open(pad.slot);
        true
    }

    // ---- system chords --------------------------------------------------

    /// The chord table as JSON: `[{"button":"share","action":{"type":"power_off"}}]`.
    pub fn chords_json(&self) -> String {
        serde_json::to_string(&*self.shared.chord_map.lock().unwrap()).unwrap_or_default()
    }

    /// Replace the chord table. An empty string restores the
    /// defaults (an empty list means no chords at all). False if not valid.
    pub fn set_chords_json(&self, json: &str) -> bool {
        let map = if json.trim().is_empty() {
            chords::default_map()
        } else {
            match serde_json::from_str::<Vec<SysBinding>>(json) {
                Ok(m) => m,
                Err(e) => {
                    log::warn!("chord table: {e}");
                    return false;
                }
            }
        };
        *self.shared.chord_map.lock().unwrap() = map;
        self.shared.chord_rev.fetch_add(1, Ordering::AcqRel);
        true
    }

    /// Exe names of the games installed on this PC (from the UI's scan).
    pub fn set_known_games(&self, exes: impl IntoIterator<Item = String>) {
        *self.shared.known_games.lock().unwrap() =
            exes.into_iter().map(|e| e.trim().to_ascii_lowercase()).filter(|e| !e.is_empty()).collect();
        self.shared.refresh_game_front();
    }

    /// A full-screen game or video is in front right now.
    pub fn fullscreen(&self) -> bool {
        self.shared.fullscreen.load(Ordering::Acquire)
    }

    /// True if the engine stopped for good (e.g. another copy of DXO is running).
    pub fn has_failed(&self) -> bool {
        self.shared.fatal.load(Ordering::Acquire)
    }

    pub fn stop(&mut self) {
        self.shared.running.store(false, Ordering::Release);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.stop();
    }
}

fn panic_text(p: &Box<dyn std::any::Any + Send>) -> String {
    p.downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| p.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic".into())
}

/// The profile a newly connected pad starts with: the saved "Default" profile
/// if there is one, else the built-in defaults.
fn starting_profile(profiles: &[Profile]) -> Profile {
    profiles.iter().find(|p| p.name.eq_ignore_ascii_case("default")).cloned().unwrap_or_default()
}

// =========================================================================
// Manager
// =========================================================================

fn manage(sh: Arc<Shared>, cfg: EngineConfig) {
    // Two engines on one pad = flickering lightbar, two virtual pads, hide/unhide fights.
    let _lock = match crate::sysdev::SingleInstance::acquire() {
        Ok(l) => l,
        Err(e) => {
            log::error!("{e}");
            *sh.global_error.lock().unwrap() = Some(e.to_string());
            sh.fatal.store(true, Ordering::Release);
            return;
        }
    };
    let mut api = match HidApi::new() {
        Ok(a) => a,
        Err(e) => {
            *sh.global_error.lock().unwrap() = Some(format!("hidapi init: {e}"));
            return;
        }
    };
    let hidhide: Option<Arc<HidHide>> = if cfg.enable_virtual {
        match HidHide::locate() {
            Ok(h) => {
                log::info!("HidHide CLI: {}", h.cli_path().display());
                match h.whitelist_self() {
                    Ok(()) => log::info!("HidHide: DXO whitelisted"),
                    Err(e) => log::warn!("HidHide whitelist failed: {e}"),
                }
                Some(Arc::new(h))
            }
            Err(e) => {
                log::warn!("{e} - controllers will NOT be hidden (double input)");
                *sh.global_error.lock().unwrap() = Some(e.to_string());
                None
            }
        }
    } else {
        None
    };
    let hidhide_problem = sh.global_error.lock().unwrap().clone();

    let mut fg = Watcher::default();
    let mut audio = crate::haptics::AudioListener::default();
    let mut last_fg_check = Instant::now();
    let mut last_scan = Instant::now() - Duration::from_secs(10);
    let mut workers: Vec<Option<JoinHandle<()>>> = (0..MAX_PADS).map(|_| None).collect();
    // DXO's own virtual pads can look exactly like real ones: never open them.
    let mut own_virtual: HashSet<CString> = HashSet::new();
    // Paths seen on the previous scan (to spot what's new).
    let mut prev_paths: HashSet<CString> = HashSet::new();
    // Safety net: a device whose worker keeps dying straight away is left alone for a while.
    let mut started_at: Vec<Option<(CString, Instant)>> = (0..MAX_PADS).map(|_| None).collect();
    let mut quick_exits: HashMap<CString, u32> = HashMap::new();
    let mut cooldown: HashMap<CString, Instant> = HashMap::new();

    while sh.running.load(Ordering::Acquire) {
        std::thread::sleep(Duration::from_millis(50));

        // Reap workers that finished (pad unplugged, reconnect, restart after hiding).
        for (slot, w) in workers.iter_mut().enumerate() {
            if w.as_ref().is_some_and(|h| h.is_finished()) {
                let _ = w.take().unwrap().join();
                if let Some((path, t)) = started_at[slot].take() {
                    if t.elapsed() < Duration::from_secs(3) {
                        let n = quick_exits.entry(path.clone()).or_insert(0);
                        *n += 1;
                        if *n >= 3 {
                            log::warn!("{} keeps dropping straight away; leaving it alone for 30 s", path.to_string_lossy());
                            cooldown.insert(path.clone(), Instant::now() + Duration::from_secs(30));
                            quick_exits.remove(&path);
                        }
                    } else {
                        quick_exits.remove(&path);
                    }
                }
                sh.pads[slot].active.store(false, Ordering::Release);
                *sh.pads[slot].gone_at.lock().unwrap() = Some(Instant::now());
                sh.rescan.store(true, Ordering::Release); // e.g. come straight back after a reconnect
            }
        }

        // ---- profiles + game auto-switch ---------------------------------
        if sh.reload_profiles.swap(false, Ordering::AcqRel) {
            let fresh = cfg.profiles_dir.as_deref().map(Profile::load_dir).unwrap_or_default();
            log::info!("reloaded {} profiles", fresh.len());
            *sh.profiles.lock().unwrap() = fresh;
            fg = Watcher::default(); // re-evaluate the current foreground app
        }
        // The app in front changed: each controller picks its own profile
        // (its rule for that game, else the game's profile, else its default).
        if last_fg_check.elapsed() > Duration::from_millis(500) {
            last_fg_check = Instant::now();
            if let Some(exe) = fg.poll() {
                *sh.fg_exe.lock().unwrap() = Some(exe);
                sh.apply_rules_all();
            }
            sh.fullscreen.store(crate::foreground::fullscreen_app(), Ordering::Release);
            sh.refresh_game_front();

            // Listen to the PC's sound only while some controller's profile wants it.
            let wanted = sh.pads.iter().any(|p| {
                p.active.load(Ordering::Acquire) && p.profile.lock().unwrap().audio_haptics.mode != AudioHapticsMode::Off
            });
            audio.set_running(wanted, &sh.audio_level);
        }

        // ---- scan for controllers (1 Hz, or right away when asked) ----------
        if last_scan.elapsed() < Duration::from_secs(1) && !sh.rescan.swap(false, Ordering::AcqRel) {
            continue;
        }
        last_scan = Instant::now();
        let _ = api.refresh_devices();
        let generic = sh.include_generic.load(Ordering::Acquire);
        let found = device::enumerate(&api, generic);

        // Forget virtual pads that went away; learn ones that just appeared.
        own_virtual.retain(|p| found.iter().any(|f| &f.path == p));
        let window_open = sh.virtual_window.lock().unwrap().is_some_and(|t| Instant::now() < t);
        let owned: HashSet<CString> = sh
            .pads
            .iter()
            .filter(|p| p.active.load(Ordering::Acquire))
            .filter_map(|p| p.path.lock().unwrap().clone())
            .collect();
        if window_open {
            for f in &found {
                if !prev_paths.contains(&f.path) && !owned.contains(&f.path) && own_virtual.insert(f.path.clone()) {
                    log::info!("ignoring DXO's own virtual pad: {} ({})", f.product, f.path.to_string_lossy());
                }
            }
        }
        prev_paths = found.iter().map(|f| f.path.clone()).collect();
        cooldown.retain(|_, until| Instant::now() < *until);

        let any_active = sh.pads.iter().any(|p| p.active.load(Ordering::Acquire));
        let mut open_error: Option<String> = None;
        for f in found {
            if owned.contains(&f.path) || own_virtual.contains(&f.path) || cooldown.contains_key(&f.path) {
                continue;
            }
            let Some(slot) = pick_slot(&sh, &f.path) else {
                log::warn!("already driving {MAX_PADS} controllers; ignoring {}", f.product);
                break;
            };
            let path = f.path.clone();
            let dev = match Device::open(&api, f) {
                Ok(d) => d,
                Err(e) => {
                    log::warn!("open failed: {e}");
                    open_error = Some(e.to_string());
                    continue;
                }
            };
            let pad = sh.pads[slot].clone();
            // A different pad in this slot: start it fresh.
            let same = pad.path.lock().unwrap().as_ref() == Some(&path);
            if !same {
                *pad.output_override.lock().unwrap() = None;
                *pad.snapshot.lock().unwrap() = Snapshot::default();
            }
            // Who is this? Load (or create) its saved settings, then pick its profile.
            let id = dev.hardware_id();
            sh.controllers.lock().unwrap().get_or_create(&id, &dev.info.product);
            log::info!("controller {id} ({}) -> player {}", dev.info.product, slot + 1);
            *pad.controller_id.lock().unwrap() = Some(id);
            pad.profile.lock().unwrap().name.clear(); // force apply_rules to set it
            sh.apply_rules(&pad);
            *pad.path.lock().unwrap() = Some(path);
            *pad.gone_at.lock().unwrap() = None;
            pad.active.store(true, Ordering::Release);

            let (sh2, hh) = (sh.clone(), hidhide.clone());
            let pad2 = pad.clone();
            let spawned = std::thread::Builder::new().name(format!("dxo-pad-{}", slot + 1)).spawn(move || {
                if let Err(p) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_pad(&sh2, &pad2, dev, hh))) {
                    let mut snap = pad2.snapshot.lock().unwrap();
                    snap.connected = false;
                    snap.last_error = Some(format!("Controller worker crashed: {}", panic_text(&p)));
                }
            });
            match spawned {
                Ok(h) => {
                    workers[slot] = Some(h);
                    started_at[slot] = pad.path.lock().unwrap().clone().map(|p| (p, Instant::now()));
                }
                Err(e) => {
                    log::error!("spawn worker: {e}");
                    pad.active.store(false, Ordering::Release);
                }
            }
        }

        // Engine-wide message when nothing is connected.
        let any_now = sh.pads.iter().any(|p| p.active.load(Ordering::Acquire));
        *sh.global_error.lock().unwrap() = if any_now || any_active {
            hidhide_problem.clone()
        } else {
            Some(open_error.unwrap_or_else(|| Error::NoDevice.to_string()))
        };
    }

    // Shutdown: workers see `running == false`, clean up (unhide, unplug) and exit.
    for w in workers.iter_mut() {
        if let Some(h) = w.take() {
            let _ = h.join();
        }
    }
}

/// Slot for a newly found device: the one it used before (keeps its profile),
/// else an empty one that hasn't held a pad recently, else any free one.
fn pick_slot(sh: &Shared, path: &CString) -> Option<usize> {
    let free = |p: &&Arc<Pad>| !p.active.load(Ordering::Acquire);
    if let Some(p) = sh.pads.iter().filter(free).find(|p| p.path.lock().unwrap().as_ref() == Some(path)) {
        return Some(p.slot);
    }
    let stale = |p: &&Arc<Pad>| {
        p.path.lock().unwrap().is_none() || p.gone_at.lock().unwrap().map_or(true, |t| t.elapsed() > Duration::from_secs(10))
    };
    sh.pads
        .iter()
        .filter(free)
        .find(stale)
        .or_else(|| sh.pads.iter().find(free))
        .map(|p| p.slot)
}

// =========================================================================
// Worker: drives one controller until it's unplugged, reconnected or DXO stops
// =========================================================================

struct Conn {
    dev: Device,
    vpad: Box<dyn VirtualPad>,
    target: VirtualTarget,
    hide_requested: bool,
    hidden_instance: Option<String>,
}

/// DualSense player-indicator pattern per slot (players 1-4).
const PLAYER_LEDS: [u8; MAX_PADS] = [0b00100, 0b01010, 0b10101, 0b11011];

fn run_pad(sh: &Shared, pad: &Pad, dev: Device, hh: Option<Arc<HidHide>>) {
    let conn = match connect(sh, pad, dev, hh.as_deref()) {
        Ok(Some(c)) => c,
        Ok(None) => return, // restarted after hiding; the manager picks it up again
        Err(e) => {
            let mut snap = pad.snapshot.lock().unwrap();
            snap.connected = false;
            snap.last_error = Some(e.to_string());
            return;
        }
    };
    let mut conn = conn;
    {
        let mut snap = pad.snapshot.lock().unwrap();
        snap.connected = true;
        snap.kind = Some(conn.dev.info.kind);
        snap.product = conn.dev.info.product.clone();
        snap.last_error = None;
        snap.pad_hidden = conn.hidden_instance.is_some();
        snap.virtual_active = conn.target != VirtualTarget::None && sh.enable_virtual;
    }
    *pad.diag.lock().unwrap() = Diag::default();
    log::info!("player {}: {} ({:?})", pad.slot + 1, conn.dev.info.product, conn.dev.info.transport);

    let mut calib = Calibrator::default();
    let mut gyro_mouse = GyroMouse::default();
    let mut runner = Runner::default();
    let mut pointer = Pointer::default();
    let mut flick = FlickStick::default();
    let mut flick_on = false;
    let mut chords = Chords::default();
    chords.set_map(&sh.chord_map.lock().unwrap());
    let mut chord_rev = sh.chord_rev.load(Ordering::Acquire);
    // On-screen keyboard / quick menu: this pad is typing / waiting for the buttons that
    // closed it to be let go (so "Circle to close" isn't also Circle in the game).
    let mut typing_was = false;
    let mut osk_drain = false;
    // No virtual pad: games read the real controller, DXO can't hide presses from them.
    let native = conn.target == VirtualTarget::None || !sh.enable_virtual;
    let connected_at = Instant::now();
    let mut last_raw = PadState::default();
    // Efficiency: only push to ViGEm when the game-visible state changes, and
    // refresh the UI snapshot at ~250 Hz instead of every report.
    let mut last_vpad: Option<PadState> = None;
    let mut last_snap: Option<Instant> = None;
    // This controller's saved settings (lightbar, idle timeout), re-read every second.
    let ctl_id = pad.controller_id.lock().unwrap().clone();
    let read_ctl = || ctl_id.as_deref().and_then(|id| sh.controllers.lock().unwrap().get(id)).unwrap_or_default();
    let mut ctl = read_ctl();
    let mut ctl_read_at = Instant::now();
    // Start from this controller's saved gyro drift; save it again when it's re-measured.
    if let Some(bias) = ctl.gyro_bias {
        calib.seed(bias);
    }
    let mut bias_rev = calib.revision;
    let mut bias_saved_at: Option<Instant> = None;
    let mut last_activity = Instant::now();
    let mut activity_ref = PadState::default();
    let bt_mac = ctl_id.as_deref().and_then(|id| id.strip_prefix("sony-")).map(str::to_string);
    let mut cemu: Option<CemuHookServer> = None;
    let mut cemu_cfg: Option<(bool, u16)> = None;
    let mut cemu_failed = false;
    let mut game_rumble = virtual_pad::Rumble::default();
    let mut last_output: Option<OutputState> = None;
    let mut last_output_sent = Instant::now();
    let mut bt_seq = 0u8;
    let mut last_report_at: Option<Instant> = None;
    let mut rate_count = 0u32;
    let mut rate_t = Instant::now();

    let reason: String = loop {
        if !sh.running.load(Ordering::Acquire) {
            break "DXO is closing".into();
        }
        let profile = pad.profile.lock().unwrap().clone();
        if ctl_read_at.elapsed() >= Duration::from_secs(1) {
            ctl = read_ctl();
            ctl_read_at = Instant::now();
        }

        // ---- idle disconnect (Bluetooth only) --------------------------------
        if ctl.idle_disconnect_min > 0 && conn.dev.info.transport == Transport::Bluetooth {
            if last_activity.elapsed() >= Duration::from_secs(ctl.idle_disconnect_min as u64 * 60) {
                if let Some(mac) = &bt_mac {
                    match crate::bluetooth::disconnect(mac) {
                        Ok(()) => break format!("idle for {} min, turned off", ctl.idle_disconnect_min),
                        Err(e) => log::warn!("idle disconnect: {e}"),
                    }
                }
                last_activity = Instant::now(); // don't retry every frame
            }
        }

        // CemuHook serves one controller: player 1's.
        let want = (pad.slot == 0 && profile.gyro.cemuhook_server, profile.gyro.cemuhook_port);
        if cemu_cfg != Some(want) {
            cemu_cfg = Some(want);
            cemu = None; // close the old socket first, or re-binding the same port fails
            cemu_failed = false;
            if want.0 {
                match CemuHookServer::bind(want.1) {
                    Ok(c) => cemu = Some(c),
                    Err(e) => {
                        log::warn!("CemuHook port {}: {e}", want.1);
                        cemu_failed = true;
                    }
                }
            }
        }
        if pad.calibrate_gyro.swap(false, Ordering::AcqRel) {
            calib.request();
        }

        // ---- rebuild the connection when the virtual setup changed ----------
        if pad.reconnect.swap(false, Ordering::AcqRel) {
            if let Some(id) = &conn.hidden_instance {
                sh.cycled.lock().unwrap().remove(id); // "Fix double input": restart it again
            }
            break "reconnect requested".into();
        }
        if profile.target != conn.target || profile.hide_physical != conn.hide_requested {
            break format!("setup changed (target {:?}, hide {})", profile.target, profile.hide_physical);
        }

        // ---- input ----------------------------------------------------------
        match conn.dev.poll(4) {
            Ok(Some(raw)) => {
                let now = Instant::now();
                // Diagnostics (jitter + last report) is only shown on the Diagnostics
                // page; skip the per-report copy and lock unless that page is open.
                if sh.diag_active.load(Ordering::Acquire) {
                    let mut d = pad.diag.lock().unwrap();
                    if let Some(prev) = last_report_at {
                        if d.intervals_us.len() >= Diag::KEEP {
                            d.intervals_us.pop_front();
                        }
                        d.intervals_us.push_back(now.duration_since(prev).as_micros().min(u32::MAX as u128) as u32);
                    }
                    d.last_report.clear();
                    d.last_report.extend_from_slice(conn.dev.last_report());
                    d.reports += 1;
                }
                last_report_at = Some(now);

                if is_activity(&activity_ref, &raw) {
                    activity_ref = raw;
                    last_activity = now;
                }
                last_raw = raw;

                // ---- on-screen keyboard / quick menu: this pad is busy with DXO ----
                let mut typing = sh.osk_owner.load(Ordering::Acquire) == pad.slot;
                let mut in_menu = sh.radial_owner.load(Ordering::Acquire) == pad.slot;
                if (typing || in_menu) && native && sh.game_front.load(Ordering::Acquire) {
                    // a game came to the front: hand the pad back
                    if typing {
                        sh.osk_close();
                    }
                    if in_menu {
                        sh.radial_close();
                    }
                    typing = false;
                    in_menu = false;
                }
                let busy = typing || in_menu;
                if busy != typing_was {
                    typing_was = busy;
                    if busy {
                        // Let go of every key and mouse button the profile was holding.
                        bindings::send(&runner.release_all());
                        bindings::send(&pointer.release());
                        flick.reset();
                    } else {
                        osk_drain = true;
                    }
                }
                if osk_drain && !busy && pad_at_rest(&raw) {
                    osk_drain = false;
                }
                let captured = busy || osk_drain;
                let mut fired = ChordFired::default();

                let mut mapped = if captured { neutral(&raw) } else { mapping::apply(&profile, &raw) };
                if typing {
                    let typed = {
                        let mut k = sh.osk.lock().unwrap();
                        let typed = k.step(&raw, now);
                        if !k.is_open() {
                            // closed from the pad (Circle or the close key)
                            let _ = sh.osk_owner.compare_exchange(pad.slot, OSK_CLOSED, Ordering::AcqRel, Ordering::Acquire);
                        }
                        typed
                    };
                    bindings::send(&osk_events(&typed));
                }
                if in_menu {
                    let picked = {
                        let mut m = sh.radial.lock().unwrap();
                        let picked = m.step(&raw, now);
                        if !m.is_open() {
                            let _ = sh.radial_owner.compare_exchange(pad.slot, OSK_CLOSED, Ordering::AcqRel, Ordering::Acquire);
                        }
                        picked
                    };
                    if let Some(action) = picked {
                        chords::perform(&action, &mut fired);
                    }
                }
                if !captured {
                    bindings::send(&runner.process(Layers::of(&profile), &raw, &mut mapped, now));
                    for req in runner.take_launches() {
                        bindings::launch(&req);
                    }
                    // Flick stick takes the right stick before anything else can use it.
                    if profile.flick.enabled {
                        flick_on = true;
                        let dx = flick.step(&profile.flick, &mut mapped, now);
                        if dx != 0 {
                            crate::gyro::move_mouse(dx, 0);
                        }
                    } else if flick_on {
                        flick_on = false;
                        flick.reset();
                    }
                    // Touchpad/stick as mouse are desktop helpers: with a full-screen
                    // game in front they step aside and the game gets them back.
                    let mouse_on = !profile.mouse.is_off()
                        && !(profile.mouse.suspend_in_fullscreen && sh.fullscreen.load(Ordering::Acquire));
                    if mouse_on {
                        let po = pointer.step(&profile.mouse, &raw, &mut mapped, now);
                        bindings::send(&po.events);
                        if po.dx != 0 || po.dy != 0 {
                            crate::gyro::move_mouse(po.dx, po.dy);
                        }
                    } else {
                        bindings::send(&pointer.release());
                    }
                }
                // System chords (hold PS + button). Suppresses the combo from the
                // game, so it runs last, just before the pad state goes out.
                if sh.chords_enabled.load(Ordering::Acquire) {
                    let rev = sh.chord_rev.load(Ordering::Acquire);
                    if rev != chord_rev {
                        chord_rev = rev;
                        chords.set_map(&sh.chord_map.lock().unwrap()); // the Chords page changed it
                    }
                    let f = chords.process(&raw, &mut mapped, now);
                    fired.power_off |= f.power_off;
                    fired.toggle_window |= f.toggle_window;
                    fired.toggle_keyboard |= f.toggle_keyboard;
                    fired.toggle_radial |= f.toggle_radial;
                    fired.launches.extend(f.launches);
                }
                // Carry out what a chord or the quick menu asked for.
                if fired.power_off {
                    if let Some(mac) = &bt_mac {
                        match crate::bluetooth::disconnect(mac) {
                            Ok(()) => log::info!("player {}: powered off via chord", pad.slot + 1),
                            Err(e) => log::warn!("chord power-off: {e}"),
                        }
                    }
                }
                if fired.toggle_window {
                    sh.window_toggle.store(true, Ordering::Release);
                }
                for req in &fired.launches {
                    bindings::launch(req);
                }
                let game_blocks = native && sh.game_front.load(Ordering::Acquire);
                if fired.toggle_keyboard && sh.osk_enabled.load(Ordering::Acquire) {
                    if sh.osk_owner.load(Ordering::Acquire) != OSK_CLOSED {
                        sh.osk_close();
                    } else if game_blocks {
                        sh.osk_refused.store(true, Ordering::Release);
                    } else if sh.osk_external.load(Ordering::Acquire) && sh.enable_virtual && conn.target == VirtualTarget::Xbox360 {
                        // Windows' gamepad keyboard reads the virtual Xbox pad itself.
                        sh.radial_close();
                        sh.osk_external_toggle.store(true, Ordering::Release);
                    } else {
                        sh.radial_close();
                        sh.osk_open(pad.slot);
                    }
                }
                if fired.toggle_radial {
                    if sh.radial_owner.load(Ordering::Acquire) != OSK_CLOSED {
                        sh.radial_close();
                    } else if game_blocks {
                        sh.osk_refused.store(true, Ordering::Release);
                    } else {
                        sh.osk_close();
                        sh.radial_open(pad.slot);
                    }
                }
                if captured {
                    mapped.buttons.0 = 0; // not even the Guide pulse of a quick PS tap
                }
                // Only push to the virtual pad when the game-visible state changed;
                // ViGEm holds the last report, so an identical update is wasted work.
                if last_vpad.as_ref().map_or(true, |p| !vpad_eq(p, &mapped)) {
                    if let Err(e) = conn.vpad.update(&mapped) {
                        pad.snapshot.lock().unwrap().last_error = Some(e.to_string());
                    }
                    last_vpad = Some(mapped);
                }
                calib.feed(raw.gyro, now);
                if calib.revision != bias_rev && bias_saved_at.map_or(true, |t| t.elapsed() >= Duration::from_secs(60)) {
                    // Only ever happens while the pad lies still, so the file write can't cost a frame of play.
                    bias_rev = calib.revision;
                    bias_saved_at = Some(now);
                    save_gyro_bias(sh, ctl_id.as_deref(), calib.cal.gyro_bias);
                }
                if let Some(c) = cemu.as_mut() {
                    c.tick(&raw, &calib.cal);
                }
                if !captured {
                    if let Some((dx, dy)) = gyro_mouse.step(&profile.gyro, &raw, &calib.cal, now) {
                        crate::gyro::move_mouse(dx, dy);
                    }
                }
                rate_count += 1;
                // The UI reads at ~30 Hz; refreshing the shared snapshot at ~250 Hz
                // is ample and cuts lock traffic ~4x at a 1 kHz report rate.
                if last_snap.map_or(true, |t| now.duration_since(t) >= Duration::from_millis(4)) {
                    last_snap = Some(now);
                    let mut snap = pad.snapshot.lock().unwrap();
                    snap.motion = MotionStatus {
                        gyro_dps: calib.cal.gyro_dps(&raw),
                        accel_g: calib.cal.accel_g(&raw),
                        cal_state: calib.state,
                        cemu_listening: cemu.is_some(),
                        cemu_failed,
                        cemu_port: profile.gyro.cemuhook_port,
                        cemu_clients: cemu.as_ref().map_or(0, |c| c.client_count().min(255) as u8),
                        mouse_active: profile.gyro.to_mouse && crate::gyro::mouse_hold_active(&profile.gyro.mouse_hold, &raw),
                    };
                    snap.raw = raw;
                    snap.mapped = mapped;
                    snap.profile_name = profile.name.clone();
                    if rate_t.elapsed() >= Duration::from_secs(1) {
                        snap.reports_per_sec = rate_count;
                        rate_count = 0;
                        rate_t = Instant::now();
                    }
                }
            }
            Ok(None) => {}
            Err(e) => {
                pad.snapshot.lock().unwrap().last_error = Some(e.to_string());
                break format!("device lost: {e}");
            }
        }

        // ---- output ---------------------------------------------------------
        let ui_override = *pad.output_override.lock().unwrap();
        let base_colour = ctl.lightbar_rgb(pad.slot, &profile);
        let mut out = ui_override.unwrap_or(OutputState {
            lightbar: base_colour,
            player_leds: PLAYER_LEDS[pad.slot],
            trigger_left: profile.trigger_left,
            trigger_right: profile.trigger_right,
            ..Default::default()
        });
        // Game rumble persists until the game changes it, so remember it
        // rather than applying it only on the frame it arrives.
        if let Some(r) = conn.vpad.poll_rumble() {
            game_rumble = r;
        }
        if !profile.rumble_passthrough {
            game_rumble = Default::default();
        }
        let (mut big, mut small) = shape_rumble(&profile, game_rumble.large, game_rumble.small);
        if profile.rumble_swap {
            std::mem::swap(&mut big, &mut small);
        }
        // Sound-driven rumble: the heavy motor follows the bass, the light one at half.
        if profile.audio_haptics.mode == AudioHapticsMode::Rumble {
            let level = sh.audio_level.load(Ordering::Relaxed) as f32 * profile.audio_haptics.gain.clamp(0.0, 4.0);
            big = big.max(level.min(255.0) as u8);
            small = small.max((level * 0.5).min(255.0) as u8);
        }
        out.rumble_left = out.rumble_left.max(big);
        out.rumble_right = out.rumble_right.max(small);
        out.audio = profile.controller_audio.then_some(profile.audio_volume.min(100));
        // Light bar effect (not while a page is previewing a colour).
        if ui_override.is_none() {
            let charging = matches!(last_raw.battery_status, BatteryStatus::Charging | BatteryStatus::Full);
            // Moving effects are redrawn 30 times a second, not on every loop.
            let t = if profile.light.animated(charging) { (connected_at.elapsed().as_millis() / 33) as f32 * 0.033 } else { 0.0 };
            out.lightbar = profile.light.render(
                base_colour,
                LightInput { t, battery: last_raw.battery_percent, charging, rumble: out.rumble_left.max(out.rumble_right) },
            );
        }
        // Send on change, plus a 1 s keep-alive (some pads revert otherwise).
        if last_output != Some(out) || last_output_sent.elapsed() > Duration::from_secs(1) {
            bt_seq = (bt_seq + 1) & 0x0F;
            let rep = output::build_report(conn.dev.info.kind, conn.dev.info.transport, &out, bt_seq);
            if !rep.is_empty() {
                let _ = conn.dev.write(&rep);
            }
            last_output = Some(out);
            last_output_sent = Instant::now();
        }
    };

    log::info!("player {}: stopping ({reason})", pad.slot + 1);
    if sh.osk_owner.load(Ordering::Acquire) == pad.slot {
        sh.osk_close(); // the controller typing on the keyboard went away
    }
    if sh.radial_owner.load(Ordering::Acquire) == pad.slot {
        sh.radial_close();
    }
    bindings::send(&runner.release_all()); // never leave a key held down
    bindings::send(&pointer.release());
    if let (Some(h), Some(id)) = (hh.as_deref(), conn.hidden_instance.as_deref()) {
        let _ = h.unhide(id);
    }
    drop(conn); // closes the HID handle, unplugs the virtual pad
    let mut snap = pad.snapshot.lock().unwrap();
    snap.connected = false;
    snap.pad_hidden = false;
    snap.virtual_active = false;
}

/// What the game sees while the controller is typing on the on-screen
/// keyboard: a pad nobody is touching.
fn neutral(raw: &PadState) -> PadState {
    PadState {
        battery_percent: raw.battery_percent,
        battery_status: raw.battery_status,
        transport: raw.transport,
        sequence: raw.sequence,
        sensor_timestamp: raw.sensor_timestamp,
        ..Default::default()
    }
}

/// Every button and trigger let go (PS aside: it is the chord modifier).
fn pad_at_rest(raw: &PadState) -> bool {
    raw.buttons.0 & !crate::state::Buttons::HOME == 0 && raw.l2 < 0.2 && raw.r2 < 0.2
}

/// Keyboard output as key events: characters as text, the rest as key taps.
fn osk_events(typed: &[OskOut]) -> Vec<bindings::InputEvent> {
    use bindings::InputEvent;
    let mut ev = Vec::with_capacity(typed.len() * 2);
    for t in typed {
        match *t {
            OskOut::Char(c) => {
                let mut buf = [0u16; 2];
                for unit in c.encode_utf16(&mut buf).iter().copied() {
                    ev.push(InputEvent::Unicode { unit, down: true });
                    ev.push(InputEvent::Unicode { unit, down: false });
                }
            }
            OskOut::Key(vk) => {
                ev.push(InputEvent::Key { vk, down: true });
                ev.push(InputEvent::Key { vk, down: false });
            }
        }
    }
    ev
}

/// Game rumble as the profile wants it played: shaped (floor / ceiling /
/// response), then scaled by the overall strength. Returns (heavy, light).
fn shape_rumble(p: &Profile, large: u8, small: u8) -> (u8, u8) {
    let k = p.rumble_strength.clamp(0.0, 2.0);
    let one = |v: u8| (p.rumble_shape.apply(v) as f32 * k).round().min(255.0) as u8;
    (one(large), one(small))
}

/// Remember this controller's gyro drift for next time (if it moved enough to matter).
fn save_gyro_bias(sh: &Shared, id: Option<&str>, bias: [f32; 3]) {
    let Some(id) = id else { return };
    let mut store = sh.controllers.lock().unwrap();
    let Some(mut c) = store.get(id) else { return };
    // 4 raw units = a quarter of a degree per second.
    let moved = c.gyro_bias.map_or(true, |old| (0..3).any(|i| (old[i] - bias[i]).abs() > 4.0));
    if moved {
        c.gyro_bias = Some(bias);
        store.set(c);
    }
}

/// Do two states look the same to the virtual pad? Ignores sequence numbers,
/// timestamps and motion, which change every report but never reach ViGEm.
fn vpad_eq(a: &PadState, b: &PadState) -> bool {
    a.buttons == b.buttons
        && a.lx == b.lx
        && a.ly == b.ly
        && a.rx == b.rx
        && a.ry == b.ry
        && a.l2 == b.l2
        && a.r2 == b.r2
}

/// Did the player touch the controller since `prev`? Small stick drift and
/// sensor noise don't count, so a pad left on the desk still times out.
fn is_activity(prev: &PadState, now: &PadState) -> bool {
    let moved = |a: f32, b: f32| (a - b).abs() > 0.2;
    now.buttons != prev.buttons
        || moved(now.lx, prev.lx)
        || moved(now.ly, prev.ly)
        || moved(now.rx, prev.rx)
        || moved(now.ry, prev.ry)
        || moved(now.l2, prev.l2)
        || moved(now.r2, prev.r2)
        || now.touch[0].active != prev.touch[0].active
}

/// Plug in the virtual pad and hide the real one. `Ok(None)` = the device was
/// just restarted so other apps lose it; the manager reopens it in a moment.
fn connect(sh: &Shared, pad: &Pad, dev: Device, hh: Option<&HidHide>) -> Result<Option<Conn>> {
    let profile = pad.profile.lock().unwrap().clone();

    // 1. Hide the real pad first...
    let mut hidden_instance = None;
    if sh.enable_virtual && profile.hide_physical {
        let path = dev.info.path.to_string_lossy().into_owned();
        match (hh, hidhide::instance_id_from_path(&path)) {
            (Some(h), Some(id)) => match h.hide(&id) {
                Ok(()) => {
                    log::info!("HidHide: hid {id}");
                    hidden_instance = Some(id);
                }
                Err(e) => log::warn!("HidHide hide failed: {e}"),
            },
            (Some(_), None) => log::warn!("HidHide: could not turn device path into an instance id: {path}"),
            (None, _) => {}
        }
    }

    // 2. ...and if it was only just hidden, restart it so apps that had it open
    // (browsers, Steam, games) lose it. This happens BEFORE any virtual pad
    // exists, so games don't see a virtual controller come and go.
    if let Some(id) = &hidden_instance {
        let mut cycled = sh.cycled.lock().unwrap();
        cycled.retain(|_, t| t.elapsed() < Duration::from_secs(30));
        if !cycled.contains_key(id) {
            cycled.insert(id.clone(), Instant::now());
            drop(cycled);
            drop(dev); // close our handle; the pad stays hidden
            match crate::sysdev::restart_device(id) {
                Ok(()) => log::info!("restarted {id} so other apps re-enumerate it hidden"),
                Err(e) => log::warn!("device restart skipped: {e}"),
            }
            std::thread::sleep(Duration::from_millis(800)); // let it come back
            return Ok(None);
        }
    }

    // 3. Only now plug in the one virtual pad games will use.
    let vpad: Box<dyn VirtualPad> = if sh.enable_virtual && profile.target != VirtualTarget::None {
        // Our virtual pad is about to appear (some show up as a HID gamepad too):
        // tell the manager that anything new in the next few seconds is ours.
        *sh.virtual_window.lock().unwrap() = Some(Instant::now() + Duration::from_secs(5));
        match virtual_pad::create(profile.target) {
            Ok(v) => v,
            Err(e) => {
                if let (Some(h), Some(id)) = (hh, hidden_instance.as_deref()) {
                    let _ = h.unhide(id); // don't leave the real pad hidden with nothing in its place
                }
                return Err(e);
            }
        }
    } else {
        Box::new(NullPad)
    };

    Ok(Some(Conn { dev, vpad, target: profile.target, hide_requested: profile.hide_physical, hidden_instance }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shared() -> Shared {
        Shared {
            running: AtomicBool::new(true),
            fatal: AtomicBool::new(false),
            reload_profiles: AtomicBool::new(false),
            include_generic: AtomicBool::new(false),
            chords_enabled: AtomicBool::new(true),
            diag_active: AtomicBool::new(false),
            window_toggle: AtomicBool::new(false),
            rescan: AtomicBool::new(false),
            enable_virtual: false,
            pads: (0..MAX_PADS).map(|i| Arc::new(Pad::new(i))).collect(),
            selected: AtomicUsize::new(0),
            global_error: Mutex::new(None),
            cycled: Mutex::new(HashMap::new()),
            virtual_window: Mutex::new(None),
            controllers: Mutex::new(ControllerStore::default()),
            profiles: Mutex::new(vec![]),
            fg_exe: Mutex::new(None),
            fullscreen: AtomicBool::new(false),
            audio_level: Arc::new(AtomicU8::new(0)),
            osk: Mutex::new(Osk::default()),
            osk_owner: AtomicUsize::new(OSK_CLOSED),
            osk_enabled: AtomicBool::new(true),
            osk_refused: AtomicBool::new(false),
            known_games: Mutex::new(HashSet::new()),
            game_front: AtomicBool::new(false),
            osk_external: AtomicBool::new(false),
            osk_external_toggle: AtomicBool::new(false),
            radial: Mutex::new(Radial::default()),
            radial_owner: AtomicUsize::new(OSK_CLOSED),
            chord_map: Mutex::new(chords::default_map()),
            chord_rev: AtomicU32::new(0),
        }
    }

    #[test]
    fn keyboard_has_one_owner_and_closes_cleanly() {
        let sh = shared();
        assert_eq!(sh.osk_owner.load(Ordering::Acquire), OSK_CLOSED);
        sh.osk_open(2);
        assert_eq!(sh.osk_owner.load(Ordering::Acquire), 2);
        assert!(sh.osk.lock().unwrap().is_open());
        sh.osk_close();
        assert_eq!(sh.osk_owner.load(Ordering::Acquire), OSK_CLOSED);
        assert!(!sh.osk.lock().unwrap().is_open());
    }

    #[test]
    fn chord_table_and_menu_items_load_from_json() {
        let e = Engine { shared: Arc::new(shared()), thread: None };
        assert!(e.chords_json().contains("toggle_keyboard"));
        assert!(e.set_chords_json(r#"[{"button":"cross","action":{"type":"keys","keys":[17,83]}}]"#));
        assert_eq!(e.shared.chord_rev.load(Ordering::Acquire), 1, "workers are told to reload");
        assert_eq!(e.shared.chord_map.lock().unwrap().len(), 1);
        assert!(!e.set_chords_json("not json"));
        assert_eq!(e.shared.chord_map.lock().unwrap().len(), 1, "a bad table changes nothing");
        assert!(e.set_chords_json(""));
        assert_eq!(*e.shared.chord_map.lock().unwrap(), chords::default_map());

        assert!(e.set_radial_items_json(r#"[{"label":"Mute","action":{"type":"mute"}}]"#));
        assert_eq!(e.radial_view().count, 1);
        assert!(e.radial_items_json().contains(r#""label":"Mute""#));
        assert!(!e.set_radial_items_json("{"));
        // no controller connected: nothing can drive the menu or the keyboard
        assert!(!e.set_radial_open(true));
        assert!(!e.set_osk_open(true));
    }

    #[test]
    fn what_counts_as_a_game() {
        let sh = shared();
        assert!(!sh.is_game(Some("notepad.exe"), false));
        assert!(!sh.is_game(None, false));
        assert!(sh.is_game(Some("notepad.exe"), true), "anything full screen");
        // a profile made for it
        let mut p = Profile::default();
        p.match_process = vec!["eldenring.exe".into()];
        sh.profiles.lock().unwrap().push(p);
        assert!(sh.is_game(Some("EldenRing.exe"), false));
        // a controller's own rule for it
        let mut c = ControllerSettings { id: "sony-aa".into(), ..Default::default() };
        c.game_profiles.insert("hades.exe".into(), "Default".into());
        sh.controllers.lock().unwrap().set(c);
        assert!(sh.is_game(Some("Hades.exe"), false));
        // found by the UI's scan of installed games
        sh.known_games.lock().unwrap().insert("celeste.exe".into());
        assert!(sh.is_game(Some("Celeste.exe"), false));
        sh.refresh_game_front();
        assert!(!sh.game_front.load(Ordering::Acquire), "nothing in front yet");
        *sh.fg_exe.lock().unwrap() = Some("Celeste.exe".into());
        sh.refresh_game_front();
        assert!(sh.game_front.load(Ordering::Acquire));
    }

    #[test]
    fn a_typing_pad_looks_untouched_to_the_game() {
        let mut raw = PadState::default();
        raw.buttons.set(crate::state::Buttons::SOUTH, true);
        raw.lx = 1.0;
        raw.r2 = 0.8;
        raw.battery_percent = Some(55);
        let n = neutral(&raw);
        assert_eq!(n.buttons.0, 0);
        assert_eq!((n.lx, n.r2), (0.0, 0.0));
        assert_eq!(n.battery_percent, Some(55));
        assert!(!pad_at_rest(&raw));
        // PS alone still counts as "at rest": it is the chord modifier
        let mut ps = PadState::default();
        ps.buttons.set(crate::state::Buttons::HOME, true);
        assert!(pad_at_rest(&ps));
    }

    #[test]
    fn keyboard_output_becomes_text_and_key_taps() {
        use bindings::InputEvent;
        let ev = osk_events(&[OskOut::Char('a'), OskOut::Key(0x0D), OskOut::Char('\u{1F600}')]);
        assert_eq!(ev[0], InputEvent::Unicode { unit: 'a' as u16, down: true });
        assert_eq!(ev[1], InputEvent::Unicode { unit: 'a' as u16, down: false });
        assert_eq!(ev[2], InputEvent::Key { vk: 0x0D, down: true });
        assert_eq!(ev[3], InputEvent::Key { vk: 0x0D, down: false });
        assert_eq!(ev.len(), 8, "a character outside the basic plane is two units");
    }

    #[test]
    fn returning_pad_gets_its_old_slot() {
        let sh = shared();
        let a = CString::new("pad-a").unwrap();
        let b = CString::new("pad-b").unwrap();
        *sh.pads[0].path.lock().unwrap() = Some(a.clone());
        *sh.pads[0].gone_at.lock().unwrap() = Some(Instant::now());
        sh.pads[1].active.store(true, Ordering::Release);
        *sh.pads[1].path.lock().unwrap() = Some(b);
        assert_eq!(pick_slot(&sh, &a), Some(0), "same pad back in slot 0");
        let c = CString::new("pad-c").unwrap();
        assert_eq!(pick_slot(&sh, &c), Some(2), "new pad skips the recently used slot 0");
    }

    #[test]
    fn full_house_has_no_slot() {
        let sh = shared();
        for p in &sh.pads {
            p.active.store(true, Ordering::Release);
        }
        assert_eq!(pick_slot(&sh, &CString::new("x").unwrap()), None);
    }

    #[test]
    fn selection_falls_back_to_a_connected_pad() {
        let sh = shared();
        sh.selected.store(0, Ordering::Release);
        sh.pads[2].active.store(true, Ordering::Release);
        assert_eq!(sh.current().slot, 2);
        sh.pads[0].active.store(true, Ordering::Release);
        assert_eq!(sh.current().slot, 0);
    }

    #[test]
    fn each_controller_follows_its_own_rules() {
        let sh = shared();
        let mk = |name: &str, games: &[&str]| {
            let mut p = Profile::default();
            p.name = name.into();
            p.match_process = games.iter().map(|g| g.to_string()).collect();
            p
        };
        *sh.profiles.lock().unwrap() = vec![mk("Default", &[]), mk("Souls", &["eldenring.exe"]), mk("Casual", &[])];
        {
            let mut st = sh.controllers.lock().unwrap();
            let mut a = st.get_or_create("sony-aaaaaaaaaaaa", "PS4");
            a.target = VirtualTarget::DualShock4;
            st.set(a);
            let mut b = st.get_or_create("0079-0006-port-x", "USB Joystick");
            b.default_profile = "Casual".into();
            b.game_profiles.insert("eldenring.exe".into(), "Casual".into());
            st.set(b);
        }
        for (slot, id) in [(0, "sony-aaaaaaaaaaaa"), (1, "0079-0006-port-x")] {
            sh.pads[slot].active.store(true, Ordering::Release);
            *sh.pads[slot].controller_id.lock().unwrap() = Some(id.into());
        }
        let name = |slot: usize| sh.pads[slot].profile.lock().unwrap().name.clone();

        sh.apply_rules_all(); // desktop: each pad's default
        assert_eq!((name(0), name(1)), ("Default".to_string(), "Casual".to_string()));
        assert_eq!(sh.pads[0].profile.lock().unwrap().target, VirtualTarget::DualShock4);

        *sh.fg_exe.lock().unwrap() = Some("eldenring.exe".into());
        sh.apply_rules_all(); // pad 1 has its own rule for this game
        assert_eq!((name(0), name(1)), ("Souls".to_string(), "Casual".to_string()));
        assert_eq!(sh.pads[0].profile.lock().unwrap().target, VirtualTarget::DualShock4, "games never change emulation");

        *sh.fg_exe.lock().unwrap() = Some("chrome.exe".into());
        sh.apply_rules_all(); // left the game: back to defaults
        assert_eq!(name(0), "Default");
    }

    #[test]
    fn rumble_is_shaped_then_scaled() {
        let mut p = Profile::default();
        assert_eq!(shape_rumble(&p, 100, 0), (100, 0), "defaults change nothing");
        p.rumble_strength = 0.5;
        assert_eq!(shape_rumble(&p, 200, 100), (100, 50));
        p.rumble_strength = 1.0;
        p.rumble_shape.floor = 0.25;
        p.rumble_shape.ceiling = 0.75;
        let (big, small) = shape_rumble(&p, 255, 1);
        assert_eq!(big, 191, "capped at the ceiling");
        assert!((64..=66).contains(&small), "weak rumble lifted to the floor, got {small}");
        assert_eq!(shape_rumble(&p, 0, 0), (0, 0), "silence stays silent");
        p.rumble_strength = 2.0;
        assert_eq!(shape_rumble(&p, 255, 0).0, 255, "never overflows");
    }

    #[test]
    fn gyro_bias_is_saved_per_controller() {
        let sh = shared();
        sh.controllers.lock().unwrap().get_or_create("sony-aaaaaaaaaaaa", "PS4");
        save_gyro_bias(&sh, Some("sony-aaaaaaaaaaaa"), [10.0, -5.0, 2.0]);
        let get = || sh.controllers.lock().unwrap().get("sony-aaaaaaaaaaaa").unwrap().gyro_bias;
        assert_eq!(get(), Some([10.0, -5.0, 2.0]));
        save_gyro_bias(&sh, Some("sony-aaaaaaaaaaaa"), [11.0, -5.5, 2.0]);
        assert_eq!(get(), Some([10.0, -5.0, 2.0]), "tiny changes aren't worth a disk write");
        save_gyro_bias(&sh, Some("sony-aaaaaaaaaaaa"), [20.0, -5.0, 2.0]);
        assert_eq!(get(), Some([20.0, -5.0, 2.0]));
        save_gyro_bias(&sh, Some("unknown"), [1.0; 3]); // no such controller: nothing happens
        save_gyro_bias(&sh, None, [1.0; 3]);
    }

    #[test]
    fn starting_profile_prefers_saved_default() {
        let mut d = Profile::default();
        d.name = "default".into();
        d.trigger_deadzone = 0.3;
        let mut g = Profile::default();
        g.name = "Elden Ring".into();
        assert_eq!(starting_profile(&[g.clone(), d]).trigger_deadzone, 0.3);
        assert_eq!(starting_profile(&[g]).name, Profile::default().name);
    }
}
