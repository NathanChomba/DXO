//! Turn a Bluetooth controller off by dropping its connection (idle disconnect).
//! The same IOCTL DS4Windows uses; the pad powers down when the link goes.

/// `mac` is 12 hex digits in display order (a4b1c2d3e4f5).
#[cfg(windows)]
pub fn disconnect(mac: &str) -> Result<(), String> {
    use windows_sys::Win32::Devices::Bluetooth::*;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::IO::DeviceIoControl;
    // CTL_CODE(FILE_DEVICE_BLUETOOTH, 0x03, METHOD_BUFFERED, FILE_ANY_ACCESS), bthioctl.h
    const IOCTL_BTH_DISCONNECT_DEVICE: u32 = 0x41000C;

    let addr = u64::from_str_radix(mac, 16).map_err(|_| format!("bad address {mac}"))?;
    let params = BLUETOOTH_FIND_RADIO_PARAMS { dwSize: std::mem::size_of::<BLUETOOTH_FIND_RADIO_PARAMS>() as u32 };
    let mut radio: HANDLE = std::ptr::null_mut();
    let find = unsafe { BluetoothFindFirstRadio(&params, &mut radio) };
    if find.is_null() {
        return Err("no Bluetooth radio".into());
    }
    let mut done = false;
    loop {
        let mut ret = 0u32;
        let ok = unsafe {
            DeviceIoControl(radio, IOCTL_BTH_DISCONNECT_DEVICE, &addr as *const u64 as _, 8, std::ptr::null_mut(), 0, &mut ret, std::ptr::null_mut())
        };
        unsafe { CloseHandle(radio) };
        if ok != 0 {
            done = true;
            break;
        }
        if unsafe { BluetoothFindNextRadio(find, &mut radio) } == 0 {
            break;
        }
    }
    unsafe { BluetoothFindRadioClose(find) };
    if done {
        Ok(())
    } else {
        Err("the radio refused to disconnect it".into())
    }
}

#[cfg(not(windows))]
pub fn disconnect(_mac: &str) -> Result<(), String> {
    Err("not supported here".into())
}
