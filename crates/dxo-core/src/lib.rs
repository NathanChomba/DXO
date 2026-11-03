//! DXO core engine.
//!
//! Milestone map (see ROADMAP.md):
//!   M1  device::{dualsense, ds4}   read + parse USB input reports
//!   M2  output                     lightbar, rumble, adaptive triggers
//!   M3  virtual_pad, hidhide       ViGEm Xbox 360 pad + hide the physical pad
//!   M4  ffi                        C ABI consumed by the WinUI 3 front end
//!   M5  gyro, profile, foreground  CemuHook DSU server, per-game profiles
//!   M6  device::generic, haptics   generic-pad descriptor parser, audio haptics, BT

pub mod device;
pub mod engine;
pub mod error;
pub mod ffi;
pub mod foreground;
pub mod bindings;
pub mod controllers;
pub mod gyro;
pub mod haptics;
pub mod hidhide;
pub mod mapping;
pub mod output;
pub mod profile;
pub mod state;
pub mod sysdev;
pub mod timing;
pub mod virtual_pad;

pub use engine::Engine;
pub use error::{Error, Result};
pub use state::{Buttons, PadState};

/// Our own DLL entry point. Required on the windows-gnu toolchain: without it,
/// MinGW's linker resolves the CRT's `DllMain` reference against winapi's XInput
/// import library (pulled in by vigem-client), so loading dxo_core.dll ran
/// XInput's DllMain and the host app died with 0xc0000409 in XINPUT1_4.dll.
/// Does nothing; Rust needs no per-process setup here.
#[cfg(all(windows, target_env = "gnu"))]
#[no_mangle]
#[allow(non_snake_case)]
pub extern "system" fn DllMain(
    _module: *mut core::ffi::c_void,
    _reason: u32,
    _reserved: *mut core::ffi::c_void,
) -> i32 {
    1 // TRUE: load succeeded
}
