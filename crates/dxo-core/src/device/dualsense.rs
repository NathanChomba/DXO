//! M1: DualSense / DualSense Edge input parsing.
//!
//! Layout follows Linux `hid-playstation.c` (`struct dualsense_input_report`).
//! USB: report 0x01, payload at byte 1, 64 bytes total.
//! BT : report 0x31, payload at byte 2, 78 bytes total (after the 0x05
//!      feature-report read in `Device::open`).

use crate::error::{Error, Result};
use crate::state::*;

pub const USB_INPUT_ID: u8 = 0x01;
pub const BT_INPUT_ID: u8 = 0x31;

// Offsets inside the common payload (NOT counting the report-id prefix).
mod off {
    pub const LX: usize = 0;
    pub const LY: usize = 1;
    pub const RX: usize = 2;
    pub const RY: usize = 3;
    pub const L2: usize = 4;
    pub const R2: usize = 5;
    pub const BTN0: usize = 7; // hat (low nibble) + face buttons
    pub const BTN1: usize = 8; // shoulders, create, options, sticks
    pub const BTN2: usize = 9; // PS, touchpad, mute (+ Edge extras)
    pub const GYRO: usize = 15; // 3 x i16 LE
    pub const ACCEL: usize = 21; // 3 x i16 LE
    pub const SENSOR_TS: usize = 27; // u32 LE
    pub const TOUCH0: usize = 32; // 4 bytes
    pub const TOUCH1: usize = 36; // 4 bytes
    pub const STATUS: usize = 52; // low nibble level 0..10, high nibble charge state
    pub const MIN_LEN: usize = 53;
}

#[inline]
fn i16le(b: &[u8], o: usize) -> i16 {
    i16::from_le_bytes([b[o], b[o + 1]])
}

fn touch(b: &[u8], o: usize) -> TouchPoint {
    // byte0: bit7 = NOT touching, bits0-6 = contact id
    // bytes1-3: 12-bit X, 12-bit Y
    TouchPoint {
        active: b[o] & 0x80 == 0,
        id: b[o] & 0x7F,
        x: (b[o + 1] as u16) | (((b[o + 2] & 0x0F) as u16) << 8),
        y: ((b[o + 2] >> 4) as u16) | ((b[o + 3] as u16) << 4),
    }
}

/// Parse a raw report (including the report-id byte).
pub fn parse_input(report: &[u8]) -> Result<PadState> {
    let p = match report.first() {
        Some(&USB_INPUT_ID) if report.len() >= 1 + off::MIN_LEN => &report[1..],
        Some(&BT_INPUT_ID) if report.len() >= 2 + off::MIN_LEN => &report[2..],
        Some(&USB_INPUT_ID) | Some(&BT_INPUT_ID) => {
            return Err(Error::ShortReport { got: report.len(), need: 2 + off::MIN_LEN })
        }
        Some(&id) => return Err(Error::BadReportId(id)),
        None => return Err(Error::ShortReport { got: 0, need: 1 }),
    };

    let mut s = PadState {
        lx: stick_u8(p[off::LX]),
        ly: -stick_u8(p[off::LY]), // device Y grows downward
        rx: stick_u8(p[off::RX]),
        ry: -stick_u8(p[off::RY]),
        l2: trigger_u8(p[off::L2]),
        r2: trigger_u8(p[off::R2]),
        ..Default::default()
    };

    let (b0, b1, b2) = (p[off::BTN0], p[off::BTN1], p[off::BTN2]);
    let b = &mut s.buttons;
    b.set_hat(b0 & 0x0F);
    b.set(Buttons::WEST, b0 & 0x10 != 0);
    b.set(Buttons::SOUTH, b0 & 0x20 != 0);
    b.set(Buttons::EAST, b0 & 0x40 != 0);
    b.set(Buttons::NORTH, b0 & 0x80 != 0);
    b.set(Buttons::L1, b1 & 0x01 != 0);
    b.set(Buttons::R1, b1 & 0x02 != 0);
    b.set(Buttons::L2_DIGITAL, b1 & 0x04 != 0);
    b.set(Buttons::R2_DIGITAL, b1 & 0x08 != 0);
    b.set(Buttons::SELECT, b1 & 0x10 != 0);
    b.set(Buttons::START, b1 & 0x20 != 0);
    b.set(Buttons::L3, b1 & 0x40 != 0);
    b.set(Buttons::R3, b1 & 0x80 != 0);
    b.set(Buttons::HOME, b2 & 0x01 != 0);
    b.set(Buttons::TOUCHPAD, b2 & 0x02 != 0);
    b.set(Buttons::MUTE, b2 & 0x04 != 0);
    // DualSense Edge back paddles (verify on hardware - M6)
    b.set(Buttons::PADDLE_L, b2 & 0x40 != 0);
    b.set(Buttons::PADDLE_R, b2 & 0x80 != 0);

    s.gyro = [i16le(p, off::GYRO), i16le(p, off::GYRO + 2), i16le(p, off::GYRO + 4)];
    s.accel = [i16le(p, off::ACCEL), i16le(p, off::ACCEL + 2), i16le(p, off::ACCEL + 4)];
    s.sensor_timestamp = u32::from_le_bytes(p[off::SENSOR_TS..off::SENSOR_TS + 4].try_into().unwrap());
    s.touch = [touch(p, off::TOUCH0), touch(p, off::TOUCH1)];

    let status = p[off::STATUS];
    let level = status & 0x0F;
    s.battery_status = match status >> 4 {
        0x0 => BatteryStatus::Discharging,
        0x1 => BatteryStatus::Charging,
        0x2 => BatteryStatus::Full,
        _ => BatteryStatus::Unknown,
    };
    s.battery_percent = Some(if s.battery_status == BatteryStatus::Full {
        100
    } else {
        (level.min(10) * 10 + 5).min(100)
    });
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usb_report() -> [u8; 64] {
        let mut r = [0u8; 64];
        r[0] = USB_INPUT_ID;
        r[1] = 128; // LX centre
        r[2] = 0; // LY full up
        r[3] = 255; // RX full right
        r[4] = 128;
        r[6] = 255; // R2 full
        r[8] = 0x08 | 0x20; // hat released, Cross
        r[9] = 0x02; // R1
        r[10] = 0x01; // PS
        r[33] = 0x80; // touch0 inactive
        r[37] = 0x80; // touch1 inactive
        r[53] = 0x15; // charging, level 5
        r
    }

    #[test]
    fn parses_usb() {
        let s = parse_input(&usb_report()).unwrap();
        assert!(s.lx.abs() < 0.01);
        assert!(s.ly > 0.99, "up is positive");
        assert!(s.rx > 0.99);
        assert_eq!(s.r2, 1.0);
        assert!(s.buttons.has(Buttons::SOUTH));
        assert!(s.buttons.has(Buttons::R1));
        assert!(s.buttons.has(Buttons::HOME));
        assert!(!s.buttons.has(Buttons::DPAD_UP));
        assert_eq!(s.battery_status, BatteryStatus::Charging);
        assert_eq!(s.battery_percent, Some(55));
        assert!(!s.touch[0].active);
    }

    #[test]
    fn parses_bt_with_offset() {
        let usb = usb_report();
        let mut bt = [0u8; 78];
        bt[0] = BT_INPUT_ID;
        bt[2..2 + 63].copy_from_slice(&usb[1..64]);
        assert_eq!(parse_input(&bt).unwrap(), parse_input(&usb).unwrap());
    }

    #[test]
    fn rejects_short() {
        assert!(parse_input(&[USB_INPUT_ID, 0, 0]).is_err());
    }
}
