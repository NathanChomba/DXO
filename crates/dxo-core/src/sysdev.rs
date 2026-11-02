//! Windows device helpers the engine needs around HidHide:
//!
//! * `restart_device` - disable + re-enable a device node, so every app that
//!   already had the controller open (browsers, Steam, games) loses it and
//!   re-enumerates. HidHide only cloaks a device from apps that open it AFTER
//!   hiding starts; this is the software version of "turn the pad off and on".
//! * `SingleInstance` - a named mutex so two copies of DXO (the app and the
//!   CLI, or two apps) can't fight over one controller.

use crate::error::{Error, Result};

#[cfg(windows)]
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Disable then re-enable the device with this instance id
/// (e.g. `HID\VID_054C&PID_05C4\...`). Needs an elevated process.
#[cfg(windows)]
pub fn restart_device(instance_id: &str) -> Result<()> {
    use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
        CM_Disable_DevNode, CM_Enable_DevNode, CM_Locate_DevNodeW, CM_DISABLE_UI_NOT_OK, CM_LOCATE_DEVNODE_NORMAL,
        CR_SUCCESS,
    };
    let id = wide(instance_id);
    let mut node: u32 = 0;
    unsafe {
        let r = CM_Locate_DevNodeW(&mut node, id.as_ptr(), CM_LOCATE_DEVNODE_NORMAL);
        if r != CR_SUCCESS {
            return Err(Error::HidHide(format!("locate devnode {instance_id}: CONFIGRET {r}")));
        }
        let r = CM_Disable_DevNode(node, CM_DISABLE_UI_NOT_OK);
        if r != CR_SUCCESS {
            // 0x17 CR_REMOVE_VETOED: something still has it open; 0x33 CR_ACCESS_DENIED: not admin
            return Err(Error::HidHide(format!("disable {instance_id}: CONFIGRET {r} (needs admin)")));
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
        let r = CM_Enable_DevNode(node, 0);
        if r != CR_SUCCESS {
            return Err(Error::HidHide(format!(
                "re-enable {instance_id}: CONFIGRET {r} - the controller may stay disabled; re-pair it or enable it in Device Manager"
            )));
        }
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn restart_device(_instance_id: &str) -> Result<()> {
    Err(Error::Unsupported("device restart is Windows-only"))
}

/// Held for the engine's lifetime; dropping it releases the lock.
pub struct SingleInstance {
    #[cfg(windows)]
    handle: windows_sys::Win32::Foundation::HANDLE,
}

unsafe impl Send for SingleInstance {}

impl SingleInstance {
    /// Ok(lock) if this is the only DXO engine in the user's session.
    #[cfg(windows)]
    pub fn acquire() -> Result<SingleInstance> {
        use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS};
        use windows_sys::Win32::System::Threading::CreateMutexW;
        let name = wide("Local\\DXO.Engine.SingleInstance");
        unsafe {
            let h = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
            if h.is_null() {
                return Err(Error::Virtual("could not create the single-instance lock".into()));
            }
            if GetLastError() == ERROR_ALREADY_EXISTS {
                CloseHandle(h);
                return Err(Error::AlreadyRunning(
                    "Another copy of DXO is already running (the app or the CLI). Close it first.".into(),
                ));
            }
            Ok(SingleInstance { handle: h })
        }
    }

    #[cfg(not(windows))]
    pub fn acquire() -> Result<SingleInstance> {
        Ok(SingleInstance {})
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        #[cfg(windows)]
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.handle);
        }
    }
}
