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

pub const DXO_ABI_VERSION: u32 = 3; // 2: virtual-device controls, 3: profile editor

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
/// target: 0 = none (native), 1 = Xbox 360.
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
            kind: match s.kind {
                Some(crate::device::Kind::DualShock4) => 1,
                Some(crate::device::Kind::DualSense) => 2,
                Some(crate::device::Kind::DualSenseEdge) => 3,
                Some(crate::device::Kind::Generic) => 4,
                None => 0,
            },
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
            target: match p.target {
                crate::profile::VirtualTarget::None => 0,
                _ => 1,
            },
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
        let t = if target == 0 { crate::profile::VirtualTarget::None } else { crate::profile::VirtualTarget::Xbox360 };
        e.set_virtual(t, hide_physical != 0, rumble_passthrough != 0);
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
    }
}
