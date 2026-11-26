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

/// Install locations seen across HidHide versions / installers.
const CANDIDATES: &[&str] = &[
    r"C:\Program Files\Nefarius Software Solutions\HidHide\x64\HidHideCLI.exe",
    r"C:\Program Files\Nefarius Software Solutions\HidHide\HidHideCLI.exe",
    r"C:\Program Files\Nefarius Software Solutions e.U\HidHide\x64\HidHideCLI.exe",
    r"C:\Program Files\HidHide\x64\HidHideCLI.exe",
];

pub struct HidHide {
    cli: PathBuf,
}

impl HidHide {
    pub fn locate() -> Result<HidHide> {
        // DXO_HIDHIDE_CLI lets you point at a non-standard install.
        if let Ok(p) = std::env::var("DXO_HIDHIDE_CLI") {
            let cli = PathBuf::from(p);
            if cli.exists() {
                return Ok(HidHide { cli });
            }
        }
        for c in CANDIDATES {
            let cli = PathBuf::from(c);
            if cli.exists() {
                return Ok(HidHide { cli });
            }
        }
        Err(Error::HidHide(format!(
            "HidHideCLI.exe not found (looked in {} places). Is HidHide installed? \
             Set DXO_HIDHIDE_CLI to its full path if it lives elsewhere.",
            CANDIDATES.len()
        )))
    }

    pub fn cli_path(&self) -> &std::path::Path {
        &self.cli
    }

    fn run(&self, args: &[&str]) -> Result<String> {
        let mut cmd = Command::new(&self.cli);
        cmd.args(args);
        #[cfg(windows)]
        {
            // DXO's window app has no console, so without this every call pops up
            // a terminal window (several per controller).
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let out = cmd.output()?;
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        if !out.status.success() {
            // HidHide changes need admin rights; say so instead of failing silently.
            let hint = if cfg!(windows) && (stderr.contains("ccess") || stdout.contains("ccess")) {
                " (run DXO from an Administrator terminal)"
            } else {
                ""
            };
            return Err(Error::HidHide(format!(
                "`HidHideCLI {}` failed with {}: {}{}{}",
                args.join(" "),
                out.status,
                stderr.trim(),
                stdout.trim(),
                hint
            )));
        }
        Ok(stdout)
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

/// Which controllers DXO has hidden right now, kept on disk
/// (`%LOCALAPPDATA%\\DXO\\hidden.txt`, one instance id per line).
///
/// DXO unhides a controller when it lets go of it. If DXO crashes or is
/// killed first, the controller would stay invisible to every game. So the
/// list is written as controllers are hidden, and the next start (or the
/// uninstaller) unhides whatever is still on it.
#[derive(Debug, Default)]
pub struct HiddenLog {
    path: Option<PathBuf>,
    ids: std::sync::Mutex<std::collections::BTreeSet<String>>,
}

impl HiddenLog {
    /// The log in DXO's data folder (None where there is no LOCALAPPDATA).
    pub fn in_app_data() -> HiddenLog {
        let path = std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("DXO").join("hidden.txt"));
        HiddenLog::at(path)
    }

    pub fn at(path: Option<PathBuf>) -> HiddenLog {
        HiddenLog { path, ids: Default::default() }
    }

    /// Ids left by a run that didn't shut down cleanly. Clears the list.
    pub fn take_leftovers(&self) -> Vec<String> {
        let Some(p) = &self.path else { return vec![] };
        let text = std::fs::read_to_string(p).unwrap_or_default();
        let ids: Vec<String> = text.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect();
        self.ids.lock().unwrap_or_else(|e| e.into_inner()).clear();
        self.save(&Default::default());
        ids
    }

    pub fn add(&self, id: &str) {
        let mut ids = self.ids.lock().unwrap_or_else(|e| e.into_inner());
        if ids.insert(id.to_string()) {
            self.save(&ids);
        }
    }

    pub fn remove(&self, id: &str) {
        let mut ids = self.ids.lock().unwrap_or_else(|e| e.into_inner());
        if ids.remove(id) {
            self.save(&ids);
        }
    }

    /// Write to a temporary file, then rename: a crash mid-write never leaves
    /// a half-written list.
    fn save(&self, ids: &std::collections::BTreeSet<String>) {
        let Some(p) = &self.path else { return };
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let text: String = ids.iter().map(|id| format!("{id}\n")).collect();
        let tmp = p.with_extension("tmp");
        let ok = std::fs::write(&tmp, text).and_then(|_| std::fs::rename(&tmp, p));
        if let Err(e) = ok {
            log::warn!("{}: {e}", p.display());
        }
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

    #[test]
    fn hidden_log_survives_a_crash() {
        let dir = std::env::temp_dir().join(format!("dxo-hidden-{}", std::process::id()));
        let path = dir.join("hidden.txt");
        let _ = std::fs::remove_dir_all(&dir);
        {
            let log = HiddenLog::at(Some(path.clone()));
            log.add(r"HID\VID_054C&PID_0CE6\1");
            log.add(r"HID\VID_054C&PID_05C4\2");
            log.remove(r"HID\VID_054C&PID_0CE6\1"); // let go of cleanly
            // ...then the process "crashes": nothing else runs
        }
        let next = HiddenLog::at(Some(path.clone()));
        assert_eq!(next.take_leftovers(), vec![r"HID\VID_054C&PID_05C4\2".to_string()]);
        assert!(next.take_leftovers().is_empty(), "cleared once read");
        assert!(HiddenLog::at(None).take_leftovers().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }
}
