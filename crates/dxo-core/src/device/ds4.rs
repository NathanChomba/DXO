//! M1: DualShock 4 input parsing (v1 0x05C4 and v2 0x09CC).
//!
//! Layout follows Linux `hid-playstation.c`
//! (`struct dualshock4_input_report_common`).
//! USB: report 0x01, payload at byte 1.
//! BT : report 0x11, payload at byte 3 (needs feature report 0x02 read first
//!      to switch out of the reduced report - TODO M6).

use crate::error::{Error, Result};
use crate::state::*;

pub const USB_INPUT_ID: u8 = 0x01;
pub const BT_INPUT_ID: u8 = 0x11;

mod off {
    pub const LX: usize = 0;
    pub const LY: usize = 1;
    pub const RX: usize = 2;
    pub const RY: usize = 3;
    pub const BTN0: usize = 4;
    pub const BTN1: usize = 5;
    pub const BTN2: usize = 6;
    pub const L2: usize = 7;
    pub const R2: usize = 8;
    pub const SENSOR_TS: usize = 9; // u16 LE
    pub const GYRO: usize = 12;
    pub const ACCEL: usize = 18;
    pub const STATUS: usize = 29; // low nibble level, bit4 = cable
    pub const TOUCH0: usize = 34; // after touch-report count + counter
    pub const TOUCH1: usize = 38;
    pub const MIN_LEN: usize = 42;
}

#[inline]
fn i16le(b: &[u8], o: usize) -> i16 {
    i16::from_le_bytes([b[o], b[o + 1]])
}

fn touch(b: &[u8], o: usize) -> TouchPoint {
    TouchPoint {
        active: b[o] & 0x80 == 0,
        id: b[o] & 0x7F,
        x: (b[o + 1] as u16) | (((b[o + 2] & 0x0F) as u16) << 8),
        y: ((b[o + 2] >> 4) as u16) | ((b[o + 3] as u16) << 4),
    }
}

/// Bytes needed for the reduced BT 0x01 report: sticks, buttons, triggers.
const REDUCED_LEN: usize = 1 + off::R2 + 1;

pub fn parse_input(report: &[u8]) -> Result<PadState> {
    let (p, full) = match report.first() {
        Some(&USB_INPUT_ID) if report.len() > off::MIN_LEN => (&report[1..], true),
        // Reduced BT report: same layout, but it stops after the triggers.
        // Seen until feature report 0x05 has been read (see Device::open).
        Some(&USB_INPUT_ID) if report.len() >= REDUCED_LEN => (&report[1..], false),
        Some(&BT_INPUT_ID) if report.len() > 3 + off::MIN_LEN => (&report[3..], true),
        Some(&USB_INPUT_ID) | Some(&BT_INPUT_ID) => {
            return Err(Error::ShortReport { got: report.len(), need: 3 + off::MIN_LEN })
        }
        Some(&id) => return Err(Error::BadReportId(id)),
        None => return Err(Error::ShortReport { got: 0, need: 1 }),
    };

    let mut s = PadState {
        lx: stick_u8(p[off::LX]),
        ly: -stick_u8(p[off::LY]),
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

    if !full {
        return Ok(s); // reduced report: no motion, touch or battery
    }

    s.gyro = [i16le(p, off::GYRO), i16le(p, off::GYRO + 2), i16le(p, off::GYRO + 4)];
    s.accel = [i16le(p, off::ACCEL), i16le(p, off::ACCEL + 2), i16le(p, off::ACCEL + 4)];
    s.sensor_timestamp = u16::from_le_bytes([p[off::SENSOR_TS], p[off::SENSOR_TS + 1]]) as u32;
    s.touch = [touch(p, off::TOUCH0), touch(p, off::TOUCH1)];

    let status = p[off::STATUS];
    let cable = status & 0x10 != 0;
    let level = status & 0x0F;
    // On cable the scale is 0..=11 (11 = full); on battery 0..=10.
    let (pct, st) = if cable {
        if level >= 11 {
            (100, BatteryStatus::Full)
        } else {
            ((level as u16 * 10 + 5).min(100) as u8, BatteryStatus::Charging)
        }
    } else {
        ((level.min(10) as u16 * 10 + 5).min(100) as u8, BatteryStatus::Discharging)
    };
    s.battery_percent = Some(pct);
    s.battery_status = st;
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_usb() {
        let mut r = [0u8; 64];
        r[0] = USB_INPUT_ID;
        r[1] = 255;
        r[2] = 128;
        r[3] = 128;
        r[4] = 128;
        r[5] = 0x08 | 0x80; // hat released, Triangle
        r[6] = 0x20; // Options
        r[8] = 255; // L2
        r[30] = 0x1B; // cable + full
        let s = parse_input(&r).unwrap();
        assert!(s.lx > 0.99);
        assert!(s.buttons.has(Buttons::NORTH));
        assert!(s.buttons.has(Buttons::START));
        assert_eq!(s.l2, 1.0);
        assert_eq!(s.battery_status, BatteryStatus::Full);
    }

    #[test]
    fn parses_bt_full_with_offset() {
        let mut usb = [0u8; 64];
        usb[0] = USB_INPUT_ID;
        usb[1] = 0; // LX full left
        usb[2] = 128;
        usb[3] = 128;
        usb[4] = 128;
        usb[5] = 0x08 | 0x20; // Cross
        usb[13] = 0x34; // gyro x low byte
        usb[30] = 0x07; // on battery, level 7
        let mut bt = [0u8; 78];
        bt[0] = BT_INPUT_ID;
        bt[3..3 + 63].copy_from_slice(&usb[1..64]);
        let s = parse_input(&bt).unwrap();
        assert_eq!(s, parse_input(&usb).unwrap());
        assert!(s.lx < -0.99);
        assert!(s.buttons.has(Buttons::SOUTH));
        assert_eq!(s.gyro[0], 0x34);
        assert_eq!(s.battery_status, BatteryStatus::Discharging);
        assert_eq!(s.battery_percent, Some(75));
    }

    #[test]
    fn parses_bt_reduced() {
        // 10-byte report the DS4 sends over BT before calibration is read
        let r = [USB_INPUT_ID, 128, 128, 255, 128, 0x08 | 0x40, 0x01, 0x00, 0, 200];
        let s = parse_input(&r).unwrap();
        assert!(s.rx > 0.99);
        assert!(s.buttons.has(Buttons::EAST));
        assert!(s.buttons.has(Buttons::L1));
        assert!(s.r2 > 0.78);
        assert_eq!(s.battery_percent, None);
    }
}
