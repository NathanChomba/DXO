//! The engine thread: ties every milestone together.
//!
//!   physical pad --(M1 parse)--> PadState --(mapping/profile)--> virtual pad (M3)
//!        ^                                 \--> CemuHook server (M5)
//!        \--(M2 output report)-- OutputState <-- profile / UI / game rumble
//!
//! One dedicated thread owns the HID handle. The UI (via FFI, M4) only ever
//! reads `Snapshot` and writes requests through the mutex-guarded fields.

use crate::device::{self, Device, Kind};
use crate::error::Result;
use crate::foreground::Watcher;
use crate::gyro::{Calibration, CemuHookServer};
use crate::hidhide::{self, HidHide};
use crate::mapping;
use crate::output::{self, OutputState, TriggerEffect};
use crate::profile::{Profile, VirtualTarget};
use crate::state::PadState;
use crate::virtual_pad::{self, NullPad, VirtualPad};
use hidapi::HidApi;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

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

struct Shared {
    running: AtomicBool,
    snapshot: Mutex<Snapshot>,
    /// UI-requested overrides layered on top of the profile (lightbar, triggers)
    output_override: Mutex<Option<OutputState>>,
    profile: Mutex<Profile>,
    profile_dirty: AtomicBool,
    /// UI asked for a fresh connect (e.g. "Fix double input").
    reconnect: AtomicBool,
    /// Profiles were saved/deleted: re-read the folder for game auto-switch.
    reload_profiles: AtomicBool,
    /// Set if the engine refused to start (e.g. another copy running).
    fatal: AtomicBool,
}

pub struct Engine {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl Engine {
    pub fn start(cfg: EngineConfig) -> Engine {
        let shared = Arc::new(Shared {
            running: AtomicBool::new(true),
            snapshot: Mutex::new(Snapshot::default()),
            output_override: Mutex::new(None),
            profile: Mutex::new(Profile::default()),
            profile_dirty: AtomicBool::new(true),
            reconnect: AtomicBool::new(false),
            reload_profiles: AtomicBool::new(false),
            fatal: AtomicBool::new(false),
        });
        let s2 = shared.clone();
        let thread = std::thread::Builder::new()
            .name("dxo-engine".into())
            .spawn(move || {
                // A panic here must not take the host app down: record it and stop the engine.
                let s3 = s2.clone();
                if let Err(p) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(s2, cfg))) {
                    let msg = p
                        .downcast_ref::<&str>()
                        .map(|s| s.to_string())
                        .or_else(|| p.downcast_ref::<String>().cloned())
                        .unwrap_or_else(|| "unknown panic".into());
                    if let Ok(mut snap) = s3.snapshot.lock() {
                        snap.connected = false;
                        snap.last_error = Some(format!("Engine crashed: {msg} (see %LOCALAPPDATA%\\DXO\\engine.log)"));
                    }
                }
            })
            .expect("spawn engine thread");
        Engine { shared, thread: Some(thread) }
    }

    pub fn snapshot(&self) -> Snapshot {
        self.shared.snapshot.lock().unwrap().clone()
    }

    pub fn set_profile(&self, p: Profile) {
        *self.shared.profile.lock().unwrap() = p;
        self.shared.profile_dirty.store(true, Ordering::Release);
    }

    pub fn profile(&self) -> Profile {
        self.shared.profile.lock().unwrap().clone()
    }

    /// Change how games see the pad. Takes effect immediately: if the target or
    /// hiding changed, the engine reconnects with the new setup.
    pub fn set_virtual(&self, target: VirtualTarget, hide_physical: bool, rumble_passthrough: bool) {
        let mut p = self.shared.profile.lock().unwrap();
        p.target = target;
        p.hide_physical = hide_physical;
        p.rumble_passthrough = rumble_passthrough;
        drop(p);
        self.shared.profile_dirty.store(true, Ordering::Release);
    }

    /// Drop and rebuild the connection: unhide, unplug the virtual pad, then
    /// reconnect and hide again. Fixes apps that still see the real pad.
    pub fn request_reconnect(&self) {
        self.shared.reconnect.store(true, Ordering::Release);
    }

    /// Re-read the profiles folder (call after saving or deleting a profile).
    pub fn reload_profiles(&self) {
        self.shared.reload_profiles.store(true, Ordering::Release);
    }

    /// True if the engine stopped for good (e.g. another copy of DXO is running).
    pub fn has_failed(&self) -> bool {
        self.shared.fatal.load(Ordering::Acquire)
    }

    /// Override output (lightbar/triggers/rumble) from the UI; None = use profile.
    pub fn set_output_override(&self, o: Option<OutputState>) {
        *self.shared.output_override.lock().unwrap() = o;
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

struct Live {
    dev: Device,
    vpad: Box<dyn VirtualPad>,
    /// The setup this connection was built with; a profile change to either
    /// means rebuilding the connection.
    target: VirtualTarget,
    hide_requested: bool,
    game_rumble: virtual_pad::Rumble,
    hidden_instance: Option<String>,
    last_output: Option<OutputState>,
    last_output_sent: Instant,
    bt_seq: u8,
}

fn run(sh: Arc<Shared>, cfg: EngineConfig) {
    // Two engines on one pad = flickering lightbar, two virtual pads, hide/unhide fights.
    let _lock = match crate::sysdev::SingleInstance::acquire() {
        Ok(l) => l,
        Err(e) => {
            log::error!("{e}");
            sh.snapshot.lock().unwrap().last_error = Some(e.to_string());
            sh.fatal.store(true, Ordering::Release);
            return;
        }
    };
    let mut api = match HidApi::new() {
        Ok(a) => a,
        Err(e) => {
            sh.snapshot.lock().unwrap().last_error = Some(format!("hidapi init: {e}"));
            return;
        }
    };
    let hidhide = if cfg.enable_virtual {
        match HidHide::locate() {
            Ok(h) => {
                log::info!("HidHide CLI: {}", h.cli_path().display());
                match h.whitelist_self() {
                    Ok(()) => log::info!("HidHide: DXO whitelisted"),
                    Err(e) => log::warn!("HidHide whitelist failed: {e}"),
                }
                Some(h)
            }
            Err(e) => {
                log::warn!("{e} - the physical controller will NOT be hidden (double input)");
                sh.snapshot.lock().unwrap().last_error = Some(e.to_string());
                None
            }
        }
    } else {
        None
    };

    let mut profiles = cfg.profiles_dir.as_deref().map(Profile::load_dir).unwrap_or_default();
    let mut fg = Watcher::default();
    let mut last_fg_check = Instant::now();
    let mut cemu: Option<CemuHookServer> = None;
    let cal = Calibration::default();

    let mut live: Option<Live> = None;
    // Device we already restarted after hiding it, so its own reconnect
    // doesn't trigger another restart (that would loop forever).
    let mut cycled: Option<(String, Instant)> = None;
    let mut last_scan = Instant::now() - Duration::from_secs(10);
    let mut rate_count = 0u32;
    let mut rate_t = Instant::now();

    while sh.running.load(Ordering::Acquire) {
        // ---- connect (re-scan once a second while disconnected) -------------
        if live.is_none() {
            if last_scan.elapsed() < Duration::from_secs(1) {
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }
            last_scan = Instant::now();
            let _ = api.refresh_devices();
            match connect(&api, &cfg, &sh, hidhide.as_ref()) {
                Ok(l) => {
                    // Freshly hidden? Restart the device so apps that had it open
                    // (browsers, Steam, games) lose it. Our own handle closes first.
                    if let Some(id) = l.hidden_instance.clone() {
                        let recent = matches!(&cycled, Some((c, t)) if *c == id && t.elapsed() < Duration::from_secs(30));
                        if !recent {
                            cycled = Some((id.clone(), Instant::now()));
                            drop(l); // close HID handle + unplug virtual pad, but stay hidden
                            match crate::sysdev::restart_device(&id) {
                                Ok(()) => log::info!("restarted {id} so other apps re-enumerate it hidden"),
                                Err(e) => log::warn!("device restart skipped: {e}"),
                            }
                            last_scan = Instant::now() - Duration::from_secs(10);
                            std::thread::sleep(Duration::from_millis(800)); // let it come back
                            continue;
                        }
                    }
                    let mut snap = sh.snapshot.lock().unwrap();
                    snap.connected = true;
                    snap.kind = Some(l.dev.info.kind);
                    snap.product = l.dev.info.product.clone();
                    snap.last_error = None;
                    snap.pad_hidden = l.hidden_instance.is_some();
                    snap.virtual_active = l.target != VirtualTarget::None && cfg.enable_virtual;
                    live = Some(l);
                }
                Err(e) => {
                    sh.snapshot.lock().unwrap().last_error = Some(e.to_string());
                    continue;
                }
            }
        }
        let l = live.as_mut().unwrap();

        // ---- profile switching (M5) ------------------------------------------
        if sh.reload_profiles.swap(false, Ordering::AcqRel) {
            profiles = cfg.profiles_dir.as_deref().map(Profile::load_dir).unwrap_or_default();
            fg = Watcher::default(); // re-evaluate the current foreground app
            log::info!("reloaded {} profiles", profiles.len());
        }
        if last_fg_check.elapsed() > Duration::from_millis(500) {
            last_fg_check = Instant::now();
            if let Some(exe) = fg.poll() {
                if let Some(p) = profiles.iter().find(|p| p.matches(&exe)) {
                    log::info!("foreground {exe} -> profile {}", p.name);
                    *sh.profile.lock().unwrap() = p.clone();
                    sh.profile_dirty.store(true, Ordering::Release);
                }
            }
        }
        let profile = sh.profile.lock().unwrap().clone();
        if sh.profile_dirty.swap(false, Ordering::AcqRel) {
            cemu = if profile.gyro.cemuhook_server {
                CemuHookServer::bind(profile.gyro.cemuhook_port).ok()
            } else {
                None
            };
        }

        // ---- rebuild the connection when the virtual setup changed ------------
        let setup_changed = profile.target != l.target || profile.hide_physical != l.hide_requested;
        let asked = sh.reconnect.swap(false, Ordering::AcqRel);
        if asked {
            cycled = None; // "Fix double input": restart the device again on reconnect
        }
        if setup_changed || asked {
            log::info!("reconnecting (target {:?}, hide {})", profile.target, profile.hide_physical);
            disconnect(live.take().unwrap(), hidhide.as_ref());
            {
                let mut snap = sh.snapshot.lock().unwrap();
                snap.pad_hidden = false;
                snap.virtual_active = false;
            }
            last_scan = Instant::now() - Duration::from_secs(10); // reconnect right away
            std::thread::sleep(Duration::from_millis(300)); // let Windows settle the unplug
            continue;
        }

        // ---- input (M1) -------------------------------------------------------
        match l.dev.poll(4) {
            Ok(Some(raw)) => {
                let mapped = mapping::apply(&profile, &raw);
                if let Err(e) = l.vpad.update(&mapped) {
                    sh.snapshot.lock().unwrap().last_error = Some(e.to_string());
                }
                if let Some(c) = cemu.as_mut() {
                    c.tick(&raw, &cal);
                }
                rate_count += 1;
                let mut snap = sh.snapshot.lock().unwrap();
                snap.raw = raw;
                snap.mapped = mapped;
                snap.profile_name = profile.name.clone();
                if rate_t.elapsed() >= Duration::from_secs(1) {
                    snap.reports_per_sec = rate_count;
                    rate_count = 0;
                    rate_t = Instant::now();
                }
            }
            Ok(None) => {}
            Err(e) => {
                log::warn!("device lost: {e}");
                disconnect(live.take().unwrap(), hidhide.as_ref());
                let mut snap = sh.snapshot.lock().unwrap();
                snap.connected = false;
                snap.pad_hidden = false;
                snap.virtual_active = false;
                snap.last_error = Some(e.to_string());
                continue;
            }
        }

        // ---- output (M2) ------------------------------------------------------
        let mut out = sh.output_override.lock().unwrap().unwrap_or(OutputState {
            lightbar: profile.lightbar_rgb(),
            player_leds: profile.player_leds,
            trigger_left: TriggerEffect::Off,
            trigger_right: TriggerEffect::Off,
            ..Default::default()
        });
        // Game rumble persists until the game changes it, so remember it
        // rather than applying it only on the frame it arrives.
        if let Some(r) = l.vpad.poll_rumble() {
            l.game_rumble = r;
        }
        if !profile.rumble_passthrough {
            l.game_rumble = Default::default();
        }
        out.rumble_left = out.rumble_left.max(l.game_rumble.large);
        out.rumble_right = out.rumble_right.max(l.game_rumble.small);
        // Send on change, plus a 1 s keep-alive (some pads revert otherwise).
        if l.last_output != Some(out) || l.last_output_sent.elapsed() > Duration::from_secs(1) {
            l.bt_seq = (l.bt_seq + 1) & 0x0F;
            let rep = output::build_report(l.dev.info.kind, l.dev.info.transport, &out, l.bt_seq);
            if !rep.is_empty() {
                let _ = l.dev.write(&rep);
            }
            l.last_output = Some(out);
            l.last_output_sent = Instant::now();
        }
    }

    if let Some(l) = live.take() {
        disconnect(l, hidhide.as_ref());
    }
}

fn connect(api: &HidApi, cfg: &EngineConfig, sh: &Shared, hh: Option<&HidHide>) -> Result<Live> {
    let dev = device::open_first(api, cfg.include_generic)?;
    let profile = sh.profile.lock().unwrap().clone();

    let vpad: Box<dyn VirtualPad> = if cfg.enable_virtual && profile.target != VirtualTarget::None {
        virtual_pad::create_default()?
    } else {
        Box::new(NullPad)
    };

    let mut hidden_instance = None;
    if cfg.enable_virtual && profile.hide_physical {
        let path = dev.info.path.to_string_lossy().into_owned();
        match (hh, hidhide::instance_id_from_path(&path)) {
            (Some(h), Some(id)) => match h.hide(&id) {
                Ok(()) => {
                    log::info!("HidHide: hid {id}");
                    log::info!("HidHide: apps that already had the controller open (browsers, Steam) keep seeing it until they restart or the pad reconnects");
                    hidden_instance = Some(id);
                }
                Err(e) => log::warn!("HidHide hide failed: {e}"),
            },
            (Some(_), None) => log::warn!("HidHide: could not turn device path into an instance id: {path}"),
            (None, _) => {}
        }
    }

    Ok(Live {
        dev,
        vpad,
        target: profile.target,
        hide_requested: profile.hide_physical,
        hidden_instance,
        last_output: None,
        last_output_sent: Instant::now(),
        bt_seq: 0,
        game_rumble: Default::default(),
    })
}

fn disconnect(l: Live, hh: Option<&HidHide>) {
    if let (Some(h), Some(id)) = (hh, l.hidden_instance.as_deref()) {
        let _ = h.unhide(id);
    }
    // vpad dropped here -> ViGEm target unplugged
}
