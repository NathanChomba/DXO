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
pub mod gyro;
pub mod haptics;
pub mod hidhide;
pub mod mapping;
pub mod output;
pub mod profile;
pub mod state;
pub mod timing;
pub mod virtual_pad;

pub use engine::Engine;
pub use error::{Error, Result};
pub use state::{Buttons, PadState};
