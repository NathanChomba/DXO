//! M1: device discovery and input parsing.

pub mod ds4;
pub mod dualsense;
pub mod generic; // M6
pub mod switch; // M6, experimental

use crate::error::{Error, Result};
use crate::state::{PadState, Transport};
use hidapi::{BusType, DeviceInfo, HidApi, HidDevice};

pub const SONY_VID: u16 = 0x054C;
pub const PID_DS4_V1: u16 = 0x05C4;
pub const PID_DS4_V2: u16 = 0x09CC;
pub const PID_DUALSENSE: u16 = 0x0CE6;
pub const PID_DUALSENSE_EDGE: u16 = 0x0DF2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    DualShock4,
    DualSense,
    DualSenseEdge,
    SwitchPro,
    Generic,
}

impl Kind {
    pub fn from_ids(vid: u16, pid: u16) -> Option<Kind> {
        match (vid, pid) {
            (SONY_VID, PID_DS4_V1 | PID_DS4_V2) => Some(Kind::DualShock4),
            (SONY_VID, PID_DUALSENSE) => Some(Kind::DualSense),
            (SONY_VID, PID_DUALSENSE_EDGE) => Some(Kind::DualSenseEdge),
            (switch::VID, switch::PID_PRO) => Some(Kind::SwitchPro),
            _ => None,
        }
    }
    pub fn is_dualsense(self) -> bool {
        matches!(self, Kind::DualSense | Kind::DualSenseEdge)
    }
}

/// Generic Desktop page, Joystick / Gamepad usages: anything matching this is
/// a candidate for the M6 generic parser.
fn is_gamepad_usage(info: &DeviceInfo) -> bool {
    info.usage_page() == 0x01 && matches!(info.usage(), 0x04 | 0x05) && !is_xinput_hid(&info.path().to_string_lossy())
}

/// Windows gives every XInput pad (real Xbox controllers AND DXO's own virtual
/// Xbox 360 pads) an extra HID view whose path contains "IG_". Never treat those
/// as generic pads: opening our own virtual pad makes another one, endlessly.
pub fn is_xinput_hid(path: &str) -> bool {
    path.to_ascii_lowercase().contains("&ig_")
}

#[derive(Debug, Clone)]
pub struct Found {
    pub kind: Kind,
    pub vid: u16,
    pub pid: u16,
    pub path: std::ffi::CString,
    pub product: String,
    pub transport: Transport,
}

/// List supported controllers. Sony pads first; generic gamepads only when
/// `include_generic` is set (M6).
pub fn enumerate(api: &HidApi, include_generic: bool) -> Vec<Found> {
    let mut out: Vec<Found> = api
        .device_list()
        .filter_map(|d| {
            let kind = match Kind::from_ids(d.vendor_id(), d.product_id()) {
                Some(k) => k,
                None if include_generic && is_gamepad_usage(d) => Kind::Generic,
                None => return None,
            };
            Some(Found {
                kind,
                vid: d.vendor_id(),
                pid: d.product_id(),
                path: d.path().to_owned(),
                product: d.product_string().unwrap_or("Unknown").to_string(),
                transport: if matches!(d.bus_type(), BusType::Bluetooth) {
                    Transport::Bluetooth
                } else {
                    Transport::Usb
                },
            })
        })
        .collect();
    // hidapi on Windows can list one physical pad several times (one per
    // top-level collection); keep the first entry per path.
    out.dedup_by(|a, b| a.path == b.path);
    out.sort_by_key(|f| f.kind == Kind::Generic);
    out
}

/// An open controller plus the parser that understands its reports.
pub struct Device {
    pub info: Found,
    pub hid: HidDevice,
    parser: Parser,
    buf: Vec<u8>,
    last_len: usize,
    seq: u64,
}

enum Parser {
    DualSense,
    Ds4,
    Switch,
    Generic(generic::Blueprint),
}

impl Device {
    pub fn open(api: &HidApi, info: Found) -> Result<Device> {
        let hid = api.open_path(&info.path)?;
        let parser = match info.kind {
            Kind::DualSense | Kind::DualSenseEdge => {
                if info.transport == Transport::Bluetooth {
                    // Reading feature report 0x05 (calibration) switches the
                    // pad from the reduced 0x01 BT report to the full 0x31.
                    let mut cal = [0u8; 41];
                    cal[0] = 0x05;
                    let _ = hid.get_feature_report(&mut cal);
                }
                Parser::DualSense
            }
            Kind::DualShock4 => {
                // Over BT the DS4 starts in a reduced 0x01 report (sticks,
                // buttons, triggers only). Reading calibration feature report
                // 0x05 switches it to the full 0x11 report (gyro, touch,
                // battery). Harmless over USB, so do it whenever BT is
                // possible; ignore errors (USB pads don't have 0x05).
                let mut cal = [0u8; 41];
                cal[0] = 0x05;
                let _ = hid.get_feature_report(&mut cal);
                Parser::Ds4
            }
            Kind::SwitchPro => {
                switch::init(&hid, info.transport == Transport::Usb);
                Parser::Switch
            }
            Kind::Generic => {
                let mut desc = vec![0u8; 4096];
                let n = hid.get_report_descriptor(&mut desc)?;
                desc.truncate(n);
                Parser::Generic(generic::Blueprint::from_descriptor(&desc)?)
            }
        };
        // Windows reads BT HID reports at the descriptor's maximum input
        // length (547 bytes for a DS4 over BT), so leave plenty of room.
        Ok(Device { info, hid, parser, buf: vec![0u8; 1024], last_len: 0, seq: 0 })
    }

    /// Block up to `timeout_ms` for one input report. `Ok(None)` on timeout.
    pub fn poll(&mut self, timeout_ms: i32) -> Result<Option<PadState>> {
        let n = self.hid.read_timeout(&mut self.buf, timeout_ms)?;
        if n == 0 {
            return Ok(None);
        }
        self.last_len = n;
        let report = &self.buf[..n];
        // Trust the report id over hidapi's bus type: some Windows BT stacks
        // report an unknown bus. 0x11 (DS4) / 0x31 (DualSense) only exist on BT.
        if matches!(
            (&self.parser, report[0]),
            (Parser::Ds4, ds4::BT_INPUT_ID) | (Parser::DualSense, dualsense::BT_INPUT_ID)
        ) {
            self.info.transport = Transport::Bluetooth;
        }
        let mut st = match &self.parser {
            Parser::DualSense => dualsense::parse_input(report)?,
            Parser::Ds4 => ds4::parse_input(report)?,
            Parser::Switch => match switch::parse_input(report) {
                Some(s) => s,
                None => return Ok(None), // handshake ack / subcommand reply
            },
            Parser::Generic(bp) => bp.parse(report)?,
        };
        self.seq += 1;
        st.sequence = self.seq;
        st.transport = self.info.transport;
        Ok(Some(st))
    }

    /// A stable id for THIS physical controller, so its settings follow it
    /// across ports and between USB and Bluetooth where possible:
    ///   * PlayStation pads: their Bluetooth address ("sony-a4b1c2d3e4f5"),
    ///     from the serial string (Bluetooth) or a feature report (USB);
    ///   * others: model + serial, or model + USB port when there's no serial
    ///     (two identical cheap pads are told apart by the port they're in).
    pub fn hardware_id(&self) -> String {
        let i = &self.info;
        let sony = matches!(i.kind, Kind::DualShock4 | Kind::DualSense | Kind::DualSenseEdge);
        let serial = self.hid.get_serial_number_string().ok().flatten().unwrap_or_default();
        if sony {
            if let Some(mac) = mac_from_text(&serial) {
                return format!("sony-{mac}");
            }
            // USB: DS4 report 0x12 / DualSense report 0x09 hold the address at bytes 1..7, reversed.
            let (id, len) = if i.kind == Kind::DualShock4 { (0x12u8, 16) } else { (0x09u8, 20) };
            let mut buf = vec![0u8; len];
            buf[0] = id;
            if let Ok(n) = self.hid.get_feature_report(&mut buf) {
                if n >= 7 && buf[1..7].iter().any(|&b| b != 0) {
                    let mac: String = buf[1..7].iter().rev().map(|b| format!("{b:02x}")).collect();
                    return format!("sony-{mac}");
                }
            }
        }
        let model = format!("{:04x}-{:04x}", i.vid, i.pid);
        let serial: String = serial.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
        if !serial.is_empty() {
            return format!("{model}-{}", serial.to_ascii_lowercase());
        }
        // No serial: the device instance part of the path names the port.
        let path = i.path.to_string_lossy().to_ascii_lowercase();
        let port = path.split('#').nth(2).unwrap_or(&path).replace('&', "-");
        format!("{model}-port-{port}")
    }

    /// Bytes of the most recent input report (Diagnostics page).
    pub fn last_report(&self) -> &[u8] {
        &self.buf[..self.last_len]
    }

    pub fn write(&self, report: &[u8]) -> Result<()> {
        self.hid.write(report)?;
        Ok(())
    }
}

/// "A4:B1:C2:D3:E4:F5" / "a4b1c2d3e4f5" -> "a4b1c2d3e4f5"; None if not an address.
fn mac_from_text(s: &str) -> Option<String> {
    let hex: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect::<String>().to_ascii_lowercase();
    (hex.len() == 12 && hex != "000000000000").then_some(hex)
}

/// Open the first supported controller.
pub fn open_first(api: &HidApi, include_generic: bool) -> Result<Device> {
    open_first_except(api, include_generic, &Default::default())
}

/// Open the first supported controller whose path isn't in `skip` (DXO's own
/// virtual DualShock 4 looks exactly like a real one).
pub fn open_first_except(
    api: &HidApi,
    include_generic: bool,
    skip: &std::collections::HashSet<std::ffi::CString>,
) -> Result<Device> {
    let found = enumerate(api, include_generic)
        .into_iter()
        .find(|f| !skip.contains(&f.path))
        .ok_or(Error::NoDevice)?;
    Device::open(api, found)
}

#[cfg(test)]
mod xinput_tests {
    #[test]
    fn xinput_hid_paths_are_recognised() {
        assert!(super::is_xinput_hid(r"\\?\HID#VID_045E&PID_028E&IG_00#3&2f6a9d28&0&0000#{4d1e55b2-f16f-11cf-88cb-001111000030}"));
        assert!(!super::is_xinput_hid(r"\\?\HID#VID_0079&PID_0006#7&1fe6ccf2&0&0000#{4d1e55b2-f16f-11cf-88cb-001111000030}"));
    }
}

#[cfg(test)]
mod id_tests {
    #[test]
    fn mac_parsing() {
        assert_eq!(super::mac_from_text("A4:B1:C2:D3:E4:F5").as_deref(), Some("a4b1c2d3e4f5"));
        assert_eq!(super::mac_from_text(""), None);
        assert_eq!(super::mac_from_text("00:00:00:00:00:00"), None);
    }
}
