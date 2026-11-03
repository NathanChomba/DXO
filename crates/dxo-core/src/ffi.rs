//! M4: C ABI for the WinUI 3 front end (see ui/DXO.UI/Native/DxoNative.cs).
//!
//! Rules that keep P/Invoke boring and safe:
//!   * every struct crossing the boundary is #[repr(C)] with fixed-size fields
//!   * no Rust String/Vec crosses; strings go out as caller-provided UTF-8 buffers
//!   * no panics cross the boundary (catch_unwind on every entry point)
//!   * the engine handle is an opaque pointer owned by the caller until dxo_destroy

use crate::engine::{Engine, EngineConfig};
use crate::output::{OutputState, Rgb, TriggerEffect};
use crate::profile::Profile;
use std::ffi::{c_char, CStr};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;

pub const DXO_ABI_VERSION: u32 = 9; // 2: virtual-device controls, 3: profile editor, 4: motion, 5: diagnostics, 6: DS4 target + other pads, 7: multiple controllers, 8: per-controller settings, 9: controller navigation (raw nav)

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct DxoState {
    pub connected: u8,
    /// 0 none, 1 DS4, 2 DualSense, 3 DualSense Edge, 4 generic
    pub kind: u8,
    /// 0 USB, 1 Bluetooth
    pub transport: u8,
    /// 0..=100, 255 = unknown
    pub battery_percent: u8,
    /// 0 unknown, 1 discharging, 2 charging, 3 full
    pub battery_status: u8,
    pub _pad: [u8; 3],
    pub buttons: u32,
    pub reports_per_sec: u32,
    pub lx: f32,
    pub ly: f32,
    pub rx: f32,
    pub ry: f32,
    pub l2: f32,
    pub r2: f32,
    pub gyro: [i16; 3],
    pub accel: [i16; 3],
    pub touch0_active: u8,
    pub touch1_active: u8,
    pub _pad2: [u8; 2],
    pub touch0_x: u16,
    pub touch0_y: u16,
    pub touch1_x: u16,
    pub touch1_y: u16,
}

/// Mirror of `TriggerEffect` for FFI: mode 0 off, 1 feedback, 2 weapon.
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct DxoTrigger {
    pub mode: u8,
    pub start: u8,
    pub end: u8,
    pub strength: u8,
}

impl From<DxoTrigger> for TriggerEffect {
    fn from(t: DxoTrigger) -> Self {
        match t.mode {
            1 => TriggerEffect::Feedback { start: t.start, strength: t.strength },
            2 => TriggerEffect::Weapon { start: t.start, end: t.end, strength: t.strength },
            _ => TriggerEffect::Off,
        }
    }
}

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct DxoOutput {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub player_leds: u8,
    pub rumble_left: u8,
    pub rumble_right: u8,
    pub _pad: [u8; 2],
    pub trigger_left: DxoTrigger,
    pub trigger_right: DxoTrigger,
}

/// %LOCALAPPDATA%\DXO\engine.log - panics with file:line, so crashes inside the
/// UI process are never silent.
fn install_panic_log() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if let Some(dir) = std::env::var_os("LOCALAPPDATA") {
                let dir = PathBuf::from(dir).join("DXO");
                let _ = std::fs::create_dir_all(&dir);
                let bt = std::backtrace::Backtrace::force_capture();
                let line = format!("panic: {info}\n{bt}\n\n");
                use std::io::Write;
                if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("engine.log")) {
                    let _ = f.write_all(line.as_bytes());
                }
            }
            prev(info);
        }));
    });
}

/// Virtual-device setup and live status for the Virtual Device page.
/// target: 0 = none (native), 1 = Xbox 360, 2 = DualShock 4.
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct DxoVirtual {
    pub target: u8,
    pub hide_physical: u8,
    pub rumble_passthrough: u8,
    /// live status, read-only
    pub pad_hidden: u8,
    pub virtual_active: u8,
    pub _pad: [u8; 3],
}

/// Motion page: live readings + gyro feature status (32 bytes).
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct DxoMotion {
    /// calibrated deg/s: pitch, yaw, roll
    pub gyro_dps: [f32; 3],
    /// g: x, y, z
    pub accel_g: [f32; 3],
    /// 0 uncalibrated, 1 calibrating, 2 calibrated, 3 failed (pad moved)
    pub cal_state: u8,
    pub cemu_listening: u8,
    pub cemu_failed: u8,
    pub cemu_clients: u8,
    pub cemu_port: u16,
    pub mouse_active: u8,
    pub _pad: u8,
}

/// Diagnostics: report-arrival statistics over the last `samples` gaps (32 bytes).
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct DxoTiming {
    pub samples: u32,
    pub avg_ms: f32,
    pub min_ms: f32,
    pub max_ms: f32,
    /// standard deviation of the gaps
    pub jitter_ms: f32,
    /// gaps more than twice the average (dropped / late reports)
    pub late: u32,
    pub reports_total: u32,
    pub report_len: u32,
}

/// One connected controller for the controller bar (192 bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DxoPadInfo {
    /// 0..3 = player 1..4; pass to dxo_select_pad
    pub slot: u8,
    /// 1 = per-controller calls currently act on this pad
    pub selected: u8,
    /// same codes as DxoState.kind / transport / battery_*
    pub kind: u8,
    pub transport: u8,
    pub battery_percent: u8,
    pub battery_status: u8,
    pub pad_hidden: u8,
    pub virtual_active: u8,
    /// 0 none (native), 1 Xbox 360, 2 DualShock 4
    pub target: u8,
    pub _pad: [u8; 3],
    pub reports_per_sec: u32,
    /// UTF-8, NUL-terminated (truncated to fit). The user's name for it, else the product name.
    pub name: [u8; 64],
    pub profile: [u8; 64],
    /// Stable hardware id (settings follow the controller, not the slot)
    pub id: [u8; 48],
}

fn fixed_str<const N: usize>(s: &str) -> [u8; N] {
    let mut out = [0u8; N];
    let mut end = s.len().min(N - 1);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    out[..end].copy_from_slice(&s.as_bytes()[..end]);
    out
}

fn kind_code(k: Option<crate::device::Kind>) -> u8 {
    match k {
        Some(crate::device::Kind::DualShock4) => 1,
        Some(crate::device::Kind::DualSense) => 2,
        Some(crate::device::Kind::DualSenseEdge) => 3,
        Some(crate::device::Kind::Generic) => 4,
        Some(crate::device::Kind::SwitchPro) => 5,
        None => 0,
    }
}

fn target_code(t: crate::profile::VirtualTarget) -> u8 {
    match t {
        crate::profile::VirtualTarget::None => 0,
        crate::profile::VirtualTarget::Xbox360 => 1,
        crate::profile::VirtualTarget::DualShock4 => 2,
    }
}

fn guard<T>(fallback: T, f: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(fallback)
}

unsafe fn engine<'a>(h: *mut Engine) -> Option<&'a Engine> {
    h.as_ref()
}

unsafe fn path_arg(p: *const c_char) -> Option<PathBuf> {
    if p.is_null() {
        return None;
    }
    CStr::from_ptr(p).to_str().ok().map(PathBuf::from)
}

#[no_mangle]
pub extern "C" fn dxo_abi_version() -> u32 {
    DXO_ABI_VERSION
}

/// Start the engine. `profiles_dir` may be null. Returns null on failure.
#[no_mangle]
pub unsafe extern "C" fn dxo_create(profiles_dir: *const c_char, enable_virtual: u8) -> *mut Engine {
    install_panic_log();
    guard(std::ptr::null_mut(), || {
        let cfg = EngineConfig {
            profiles_dir: path_arg(profiles_dir),
            include_generic: false,
            enable_virtual: enable_virtual != 0,
        };
        Box::into_raw(Box::new(Engine::start(cfg)))
    })
}

#[no_mangle]
pub unsafe extern "C" fn dxo_destroy(h: *mut Engine) {
    if !h.is_null() {
        guard((), || drop(Box::from_raw(h)));
    }
}

/// Copy the latest state into `out`. Returns 1 if a pad is connected.
#[no_mangle]
pub unsafe extern "C" fn dxo_get_state(h: *mut Engine, out: *mut DxoState) -> u8 {
    guard(0, || {
        let (Some(e), Some(out)) = (engine(h), out.as_mut()) else { return 0 };
        let s = e.snapshot();
        let m = s.mapped;
        *out = DxoState {
            connected: s.connected as u8,
            kind: kind_code(s.kind),
            transport: m.transport as u8,
            battery_percent: m.battery_percent.unwrap_or(255),
            battery_status: m.battery_status as u8,
            buttons: m.buttons.0,
            reports_per_sec: s.reports_per_sec,
            lx: m.lx,
            ly: m.ly,
            rx: m.rx,
            ry: m.ry,
            l2: m.l2,
            r2: m.r2,
            gyro: m.gyro,
            accel: m.accel,
            touch0_active: m.touch[0].active as u8,
            touch1_active: m.touch[1].active as u8,
            touch0_x: m.touch[0].x,
            touch0_y: m.touch[0].y,
            touch1_x: m.touch[1].x,
            touch1_y: m.touch[1].y,
            ..Default::default()
        };
        s.connected as u8
    })
}

/// Raw input for driving DXO's own UI (controller navigation). Raw = before
/// profile remaps, so a mapping can never break navigation. ABI 9.
#[repr(C)]
pub struct DxoNav {
    pub buttons: u32,
    pub lx: f32,
    pub ly: f32,
    pub rx: f32,
    pub ry: f32,
    pub connected: u8,
    pub _pad: [u8; 3],
}

/// Latest RAW buttons and sticks of the selected pad, for in-app navigation.
/// Returns 1 if a pad is connected.
#[no_mangle]
pub unsafe extern "C" fn dxo_get_nav(h: *mut Engine, out: *mut DxoNav) -> u8 {
    guard(0, || {
        let (Some(e), Some(out)) = (engine(h), out.as_mut()) else { return 0 };
        let s = e.snapshot();
        let r = s.raw;
        *out = DxoNav {
            buttons: r.buttons.0,
            lx: r.lx,
            ly: r.ly,
            rx: r.rx,
            ry: r.ry,
            connected: s.connected as u8,
            _pad: [0; 3],
        };
        s.connected as u8
    })
}

/// Write the product name (UTF-8, NUL-terminated) into `buf`. Returns bytes
/// needed including NUL; call again with a bigger buffer if it was too small.
#[no_mangle]
pub unsafe extern "C" fn dxo_get_product_name(h: *mut Engine, buf: *mut u8, len: usize) -> usize {
    guard(0, || copy_str(engine(h).map(|e| e.snapshot().product).unwrap_or_default(), buf, len))
}

#[no_mangle]
pub unsafe extern "C" fn dxo_get_last_error(h: *mut Engine, buf: *mut u8, len: usize) -> usize {
    guard(0, || copy_str(engine(h).and_then(|e| e.snapshot().last_error).unwrap_or_default(), buf, len))
}

unsafe fn copy_str(s: String, buf: *mut u8, len: usize) -> usize {
    let bytes = s.as_bytes();
    let need = bytes.len() + 1;
    if !buf.is_null() && len >= need {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, bytes.len());
        *buf.add(bytes.len()) = 0;
    }
    need
}

/// Override lightbar/triggers/rumble from the UI. Pass null to clear.
#[no_mangle]
pub unsafe extern "C" fn dxo_set_output(h: *mut Engine, o: *const DxoOutput) {
    guard((), || {
        let Some(e) = engine(h) else { return };
        e.set_output_override(o.as_ref().map(|o| OutputState {
            lightbar: Rgb(o.r, o.g, o.b),
            player_leds: o.player_leds,
            rumble_left: o.rumble_left,
            rumble_right: o.rumble_right,
            trigger_left: o.trigger_left.into(),
            trigger_right: o.trigger_right.into(),
            mic_led: false,
            audio: None, // engine fills this from the profile
        }));
    })
}

#[no_mangle]
pub unsafe extern "C" fn dxo_get_virtual(h: *mut Engine, out: *mut DxoVirtual) -> u8 {
    guard(0, || {
        let (Some(e), Some(out)) = (engine(h), out.as_mut()) else { return 0 };
        let p = e.profile();
        let s = e.snapshot();
        *out = DxoVirtual {
            target: target_code(p.target),
            hide_physical: p.hide_physical as u8,
            rumble_passthrough: p.rumble_passthrough as u8,
            pad_hidden: s.pad_hidden as u8,
            virtual_active: s.virtual_active as u8,
            _pad: [0; 3],
        };
        1
    })
}

#[no_mangle]
pub unsafe extern "C" fn dxo_set_virtual(h: *mut Engine, target: u8, hide_physical: u8, rumble_passthrough: u8) {
    guard((), || {
        let Some(e) = engine(h) else { return };
        let t = match target {
            0 => crate::profile::VirtualTarget::None,
            2 => crate::profile::VirtualTarget::DualShock4,
            _ => crate::profile::VirtualTarget::Xbox360,
        };
        e.set_virtual(t, hide_physical != 0, rumble_passthrough != 0);
    })
}

/// Also use non-Sony gamepads (Switch Pro, generic HID). Takes effect at the next connect.
#[no_mangle]
pub unsafe extern "C" fn dxo_set_include_generic(h: *mut Engine, on: u8) {
    guard((), || {
        if let Some(e) = engine(h) {
            e.set_include_generic(on != 0);
        }
    })
}

/// Unhide, unplug the virtual pad, reconnect and hide again.
#[no_mangle]
pub unsafe extern "C" fn dxo_reconnect(h: *mut Engine) {
    guard((), || {
        if let Some(e) = engine(h) {
            e.request_reconnect();
        }
    })
}

// ---- multiple controllers (ABI 7) -----------------------------------------
// Every other per-controller call acts on the SELECTED controller.

/// Fill `out` with up to `max` connected controllers. Returns how many are
/// connected (may be more than `max`).
#[no_mangle]
pub unsafe extern "C" fn dxo_list_pads(h: *mut Engine, out: *mut DxoPadInfo, max: usize) -> usize {
    guard(0, || {
        let Some(e) = engine(h) else { return 0 };
        let pads = e.pads();
        if !out.is_null() {
            for (i, p) in pads.iter().take(max).enumerate() {
                let m = p.snapshot.mapped;
                *out.add(i) = DxoPadInfo {
                    slot: p.slot as u8,
                    selected: p.selected as u8,
                    kind: kind_code(p.snapshot.kind),
                    transport: m.transport as u8,
                    battery_percent: m.battery_percent.unwrap_or(255),
                    battery_status: m.battery_status as u8,
                    pad_hidden: p.snapshot.pad_hidden as u8,
                    virtual_active: p.snapshot.virtual_active as u8,
                    target: target_code(p.profile.target),
                    _pad: [0; 3],
                    reports_per_sec: p.snapshot.reports_per_sec,
                    name: fixed_str(
                        p.controller.as_ref().map(|c| c.name.as_str()).filter(|n| !n.is_empty()).unwrap_or(&p.snapshot.product),
                    ),
                    profile: fixed_str(&p.profile.name),
                    id: fixed_str(p.controller.as_ref().map_or("", |c| c.id.as_str())),
                };
            }
        }
        pads.len()
    })
}

/// Saved settings of the controller in `slot` as JSON (name, default_profile,
/// target, hide_physical, game_profiles). Returns bytes needed incl. NUL; 0 = none.
#[no_mangle]
pub unsafe extern "C" fn dxo_get_controller_json(h: *mut Engine, slot: u8, buf: *mut u8, len: usize) -> usize {
    guard(0, || {
        let Some(c) = engine(h).and_then(|e| e.controller(slot as usize)) else { return 0 };
        copy_str(serde_json::to_string(&c).unwrap_or_default(), buf, len)
    })
}

/// Save settings for the controller in `slot` and apply them now. Returns 1 on success.
#[no_mangle]
pub unsafe extern "C" fn dxo_set_controller_json(h: *mut Engine, slot: u8, json: *const c_char) -> u8 {
    guard(0, || {
        let (Some(e), Some(j)) = (engine(h), str_arg(json)) else { return 0 };
        match serde_json::from_str::<crate::controllers::ControllerSettings>(j) {
            Ok(c) => {
                e.set_controller(slot as usize, c);
                1
            }
            Err(err) => {
                log::warn!("bad controller json: {err}");
                0
            }
        }
    })
}

/// Make per-controller calls act on this slot (0..3).
#[no_mangle]
pub unsafe extern "C" fn dxo_select_pad(h: *mut Engine, slot: u8) {
    guard((), || {
        if let Some(e) = engine(h) {
            e.select(slot as usize);
        }
    })
}

/// The slot per-controller calls act on right now.
#[no_mangle]
pub unsafe extern "C" fn dxo_get_selected_pad(h: *mut Engine) -> u8 {
    guard(0, || engine(h).map_or(0, |e| e.selected() as u8))
}

// ---- motion (ABI 4) -------------------------------------------------------

#[no_mangle]
pub unsafe extern "C" fn dxo_get_motion(h: *mut Engine, out: *mut DxoMotion) -> u8 {
    guard(0, || {
        let (Some(e), Some(out)) = (engine(h), out.as_mut()) else { return 0 };
        let m = e.snapshot().motion;
        *out = DxoMotion {
            gyro_dps: m.gyro_dps,
            accel_g: m.accel_g,
            cal_state: match m.cal_state {
                crate::gyro::CalState::Uncalibrated => 0,
                crate::gyro::CalState::Calibrating => 1,
                crate::gyro::CalState::Calibrated => 2,
                crate::gyro::CalState::Failed => 3,
            },
            cemu_listening: m.cemu_listening as u8,
            cemu_failed: m.cemu_failed as u8,
            cemu_clients: m.cemu_clients,
            cemu_port: m.cemu_port,
            mouse_active: m.mouse_active as u8,
            _pad: 0,
        };
        1
    })
}

/// Measure gyro drift over the next 2 s (controller lying still).
#[no_mangle]
pub unsafe extern "C" fn dxo_calibrate_gyro(h: *mut Engine) {
    guard((), || {
        if let Some(e) = engine(h) {
            e.calibrate_gyro();
        }
    })
}

// ---- diagnostics (ABI 5) --------------------------------------------------

/// Fill `out` with timing stats and copy up to `max` recent gaps (ms, oldest
/// first) into `samples`. Returns how many gaps were copied.
#[no_mangle]
pub unsafe extern "C" fn dxo_get_timing(h: *mut Engine, out: *mut DxoTiming, samples: *mut f32, max: usize) -> usize {
    guard(0, || {
        let (Some(e), Some(out)) = (engine(h), out.as_mut()) else { return 0 };
        let d = e.diag();
        let ms: Vec<f32> = d.intervals_us.iter().map(|&u| u as f32 / 1000.0).collect();
        let n = ms.len();
        let avg = if n > 0 { ms.iter().sum::<f32>() / n as f32 } else { 0.0 };
        let var = if n > 0 { ms.iter().map(|v| (v - avg) * (v - avg)).sum::<f32>() / n as f32 } else { 0.0 };
        *out = DxoTiming {
            samples: n as u32,
            avg_ms: avg,
            min_ms: if n > 0 { ms.iter().copied().fold(f32::INFINITY, f32::min) } else { 0.0 },
            max_ms: ms.iter().copied().fold(0.0, f32::max),
            jitter_ms: var.sqrt(),
            late: ms.iter().filter(|&&v| avg > 0.0 && v > avg * 2.0).count() as u32,
            reports_total: d.reports.min(u32::MAX as u64) as u32,
            report_len: d.last_report.len() as u32,
        };
        if samples.is_null() || max == 0 {
            return 0;
        }
        let take = n.min(max);
        std::ptr::copy_nonoverlapping(ms[n - take..].as_ptr(), samples, take);
        take
    })
}

/// Copy the latest raw input report into `buf`. Returns its full length.
#[no_mangle]
pub unsafe extern "C" fn dxo_get_last_report(h: *mut Engine, buf: *mut u8, len: usize) -> usize {
    guard(0, || {
        let Some(e) = engine(h) else { return 0 };
        let r = e.diag().last_report;
        if !buf.is_null() {
            std::ptr::copy_nonoverlapping(r.as_ptr(), buf, r.len().min(len));
        }
        r.len()
    })
}

// ---- profile editor (ABI 3) ---------------------------------------------
// The UI edits profiles as JSON (System.Text.Json); files on disk stay TOML.
// Rust does every parse/serialise so both sides agree on one schema.

unsafe fn str_arg<'a>(p: *const c_char) -> Option<&'a str> {
    if p.is_null() { None } else { CStr::from_ptr(p).to_str().ok() }
}

/// Active profile as JSON. Returns bytes needed incl. NUL (see dxo_get_product_name).
#[no_mangle]
pub unsafe extern "C" fn dxo_get_active_profile_json(h: *mut Engine, buf: *mut u8, len: usize) -> usize {
    guard(0, || {
        let Some(e) = engine(h) else { return 0 };
        copy_str(serde_json::to_string(&e.profile()).unwrap_or_default(), buf, len)
    })
}

/// Activate a profile given as JSON (does not save it). Returns 1 on success.
#[no_mangle]
pub unsafe extern "C" fn dxo_set_active_profile_json(h: *mut Engine, json: *const c_char) -> u8 {
    guard(0, || {
        let (Some(e), Some(j)) = (engine(h), str_arg(json)) else { return 0 };
        match serde_json::from_str::<Profile>(j) {
            Ok(p) => {
                e.set_profile(p);
                1
            }
            Err(err) => {
                log::warn!("bad profile json: {err}");
                0
            }
        }
    })
}

/// Read a profile TOML file and return it as JSON (0 = unreadable / invalid).
#[no_mangle]
pub unsafe extern "C" fn dxo_profile_file_to_json(path: *const c_char, buf: *mut u8, len: usize) -> usize {
    guard(0, || {
        let Some(p) = path_arg(path) else { return 0 };
        match Profile::load(&p) {
            Ok(pr) => copy_str(serde_json::to_string(&pr).unwrap_or_default(), buf, len),
            Err(err) => {
                log::warn!("{err}");
                0
            }
        }
    })
}

/// Validate a JSON profile and write it to `path` as TOML. Returns 1 on success.
#[no_mangle]
pub unsafe extern "C" fn dxo_profile_json_to_file(json: *const c_char, path: *const c_char) -> u8 {
    guard(0, || {
        let (Some(j), Some(p)) = (str_arg(json), path_arg(path)) else { return 0 };
        match serde_json::from_str::<Profile>(j).map_err(|e| e.to_string()).and_then(|pr| pr.save(&p).map_err(|e| e.to_string())) {
            Ok(()) => 1,
            Err(err) => {
                log::warn!("save profile: {err}");
                0
            }
        }
    })
}

/// Re-read the profiles folder for per-game auto-switching.
#[no_mangle]
pub unsafe extern "C" fn dxo_reload_profiles(h: *mut Engine) {
    guard((), || {
        if let Some(e) = engine(h) {
            e.reload_profiles();
        }
    })
}

/// Load and activate a profile TOML. Returns 1 on success.
#[no_mangle]
pub unsafe extern "C" fn dxo_load_profile(h: *mut Engine, path: *const c_char) -> u8 {
    guard(0, || {
        let (Some(e), Some(p)) = (engine(h), path_arg(path)) else { return 0 };
        match Profile::load(&p) {
            Ok(pr) => {
                e.set_profile(pr);
                1
            }
            Err(err) => {
                log::warn!("{err}");
                0
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_is_stable() {
        // Must match [StructLayout(LayoutKind.Sequential)] in DxoNative.cs
        assert_eq!(std::mem::size_of::<DxoState>(), 64);
        assert_eq!(std::mem::size_of::<DxoOutput>(), 16);
        assert_eq!(std::mem::size_of::<DxoVirtual>(), 8);
        assert_eq!(std::mem::size_of::<DxoMotion>(), 32);
        assert_eq!(std::mem::size_of::<DxoTiming>(), 32);
        assert_eq!(std::mem::size_of::<DxoPadInfo>(), 192);
    }
}
