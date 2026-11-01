//! M3: hide the physical pad from games using HidHide (Nefarius).
//!
//! Talks to HidHide through its CLI rather than the driver IOCTLs: simpler,
//! and the CLI ships with every HidHide install. Swap for IOCTLs later if the
//! process spawn cost ever matters (it is only called on connect/disconnect).
//!
//! Flow:
//!   1. whitelist our own exe     (`--app-reg`)  so WE can still read the pad
//!   2. add the pad's instance id (`--dev-hide`)
//!   3. turn cloaking on          (`--cloak-on`)
//!   4. on exit: `--dev-unhide`   so the pad isn't left invisible if we crash
//!      (also expose an "unhide everything" button in the UI as a safety net)

use crate::error::{Error, Result};
use std::path::PathBuf;
use std::process::Command;

const DEFAULT_CLI: &str = r"C:\Program Files\Nefarius Software Solutions\HidHide\x64\HidHideCLI.exe";

pub struct HidHide {
    cli: PathBuf,
}

impl HidHide {
    pub fn locate() -> Result<HidHide> {
        let cli = PathBuf::from(DEFAULT_CLI);
        if cli.exists() {
            Ok(HidHide { cli })
        } else {
            Err(Error::HidHide("HidHideCLI.exe not found - is HidHide installed?".into()))
        }
    }

    fn run(&self, args: &[&str]) -> Result<String> {
        let out = Command::new(&self.cli).args(args).output()?;
        if !out.status.success() {
            return Err(Error::HidHide(String::from_utf8_lossy(&out.stderr).into_owned()));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    pub fn whitelist_self(&self) -> Result<()> {
        let exe = std::env::current_exe()?;
        self.run(&["--app-reg", &exe.to_string_lossy()]).map(|_| ())
    }

    /// `instance_id` is the device instance path, e.g.
    /// `HID\VID_054C&PID_0CE6&MI_03\7&...`; get it from the hidapi path
    /// with `instance_id_from_path`. NOTE: HidHide may need the *parent*
    /// USB/BTHENUM instance as well - verify on hardware.
    pub fn hide(&self, instance_id: &str) -> Result<()> {
        self.run(&["--dev-hide", instance_id])?;
        self.run(&["--cloak-on"]).map(|_| ())
    }

    pub fn unhide(&self, instance_id: &str) -> Result<()> {
        self.run(&["--dev-unhide", instance_id]).map(|_| ())
    }
}

/// Convert a Windows hidapi path to a device instance id (best effort).
pub fn instance_id_from_path(path: &str) -> Option<String> {
    let s = path.strip_prefix(r"\\?\")?;
    let s = match s.rfind("#{") {
        Some(i) => &s[..i],
        None => s,
    };
    Some(s.replace('#', "\\").to_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_to_instance() {
        let p = r"\\?\hid#vid_054c&pid_0ce6&mi_03#7&1a2b3c&0&0000#{4d1e55b2-f16f-11cf-88cb-001111000030}";
        assert_eq!(
            instance_id_from_path(p).unwrap(),
            r"HID\VID_054C&PID_0CE6&MI_03\7&1A2B3C&0&0000"
        );
    }
}
