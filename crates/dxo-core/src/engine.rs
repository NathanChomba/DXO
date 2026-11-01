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
        });
        let s2 = shared.clone();
        let thread = std::thread::Builder::new()
            .name("dxo-engine".into())
            .spawn(move || run(s2, cfg))
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
    game_rumble: virtual_pad::Rumble,
    hidden_instance: Option<String>,
    last_output: Option<OutputState>,
    last_output_sent: Instant,
    bt_seq: u8,
}

fn run(sh: Arc<Shared>, cfg: EngineConfig) {
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

    let profiles = cfg.profiles_dir.as_deref().map(Profile::load_dir).unwrap_or_default();
    let mut fg = Watcher::default();
    let mut last_fg_check = Instant::now();
    let mut cemu: Option<CemuHookServer> = None;
    let cal = Calibration::default();

    let mut live: Option<Live> = None;
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
                    let mut snap = sh.snapshot.lock().unwrap();
                    snap.connected = true;
                    snap.kind = Some(l.dev.info.kind);
                    snap.product = l.dev.info.product.clone();
                    snap.last_error = None;
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
