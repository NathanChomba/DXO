//! M5: which game is in front? Drives automatic profile switching.
//!
//! Polled (every ~500 ms from the engine) rather than hooked: a
//! SetWinEventHook(EVENT_SYSTEM_FOREGROUND) version is a later optimisation.

/// Executable file name of the foreground window's process, e.g. "eldenring.exe".
#[cfg(windows)]
pub fn foreground_exe() -> Option<String> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 {
            return None;
        }
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len);
        CloseHandle(h);
        if ok == 0 {
            return None;
        }
        let full = String::from_utf16_lossy(&buf[..len as usize]);
        full.rsplit(['\\', '/']).next().map(str::to_string)
    }
}

#[cfg(not(windows))]
pub fn foreground_exe() -> Option<String> {
    None
}

/// Remembers the last seen exe so callers only react to changes.
#[derive(Default)]
pub struct Watcher {
    last: Option<String>,
}

impl Watcher {
    /// Returns Some(new_exe) only when the foreground app changed.
    pub fn poll(&mut self) -> Option<String> {
        let now = foreground_exe();
        if now != self.last {
            self.last = now.clone();
            now
        } else {
            None
        }
    }
}
