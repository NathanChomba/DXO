//! M6: Nintendo Switch Pro Controller (057E:2009). EXPERIMENTAL - written from
//! the community protocol notes (dekuNukem/Nintendo_Switch_Reverse_Engineering),
//! not yet tested on hardware.
//!
//! The pad starts in a simple HID mode (report 0x3F, no gyro). We switch it to
//! the full 0x30 report:
//!   * USB: handshake 0x80 0x02, then 0x80 0x04 ("USB only, no BT timeout")
//!   * both: subcommand 0x03 (input mode) = 0x30, subcommand 0x40 (IMU) = 1
//!
//! 0x30 layout (byte index in the report, incl. the id):
//!   2      battery level (high nibble) + charging bit 0x10
//!   3      right buttons: Y 01 X 02 B 04 A 08 SR 10 SL 20 R 40 ZR 80
//!   4      shared: - 01 + 02 RStick 04 LStick 08 Home 10 Capture 20
//!   5      left: Down 01 Up 02 Right 04 Left 08 SR 10 SL 20 L 40 ZL 80
//!   6..9   left stick, two 12-bit values
//!   9..12  right stick
//!   13..   IMU: accel x,y,z then gyro x,y,z (i16 LE), 3 samples of 12 bytes

use crate::state::{stick_u8, Buttons, PadState};
use hidapi::HidDevice;

pub const VID: u16 = 0x057E;
pub const PID_PRO: u16 = 0x2009;

/// Neutral rumble data that has to precede every subcommand.
const RUMBLE_NEUTRAL: [u8; 8] = [0x00, 0x01, 0x40, 0x40, 0x00, 0x01, 0x40, 0x40];

/// Put the pad in full-report mode. Errors are ignored: a pad that is already
/// set up, or a BT link that drops the handshake, still works in some form.
pub fn init(hid: &HidDevice, usb: bool) {
    if usb {
        let _ = hid.write(&pad64(&[0x80, 0x02])); // handshake
        std::thread::sleep(std::time::Duration::from_millis(20));
        let _ = hid.write(&pad64(&[0x80, 0x04])); // HID only, never time out
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let mut counter = 0u8;
    for (cmd, arg) in [(0x03u8, 0x30u8), (0x40, 0x01)] {
        let mut r = vec![0x01, counter & 0x0F];
        r.extend_from_slice(&RUMBLE_NEUTRAL);
        r.push(cmd);
        r.push(arg);
        // USB wants 64-byte writes, BT 49; try the larger first.
        if hid.write(&pad64(&r)).is_err() {
            let mut bt = r.clone();
            bt.resize(49, 0);
            let _ = hid.write(&bt);
        }
        counter = counter.wrapping_add(1);
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
}

fn pad64(b: &[u8]) -> Vec<u8> {
    let mut v = b.to_vec();
    v.resize(64, 0);
    v
}

/// 12-bit stick value (0..4095, centre ~2048) -> -1..=1. Uses a typical range
/// until per-pad calibration (SPI flash) is read.
fn stick12(v: u16) -> f32 {
    ((v as f32 - 2048.0) / 1500.0).clamp(-1.0, 1.0)
}

/// Parse an input report. `None` for reports that carry no input (subcommand
/// replies on USB, handshake acks) - those are not errors.
pub fn parse_input(r: &[u8]) -> Option<PadState> {
    let id = *r.first()?;
    match id {
        0x30 | 0x21 if r.len() >= 12 => Some(parse_full(r, id == 0x30 && r.len() >= 49)),
        0x3F if r.len() >= 12 => Some(parse_simple(r)),
        _ => None,
    }
}

fn parse_full(r: &[u8], has_imu: bool) -> PadState {
    let mut s = PadState::default();
    let (right, shared, left) = (r[3], r[4], r[5]);
    let b = &mut s.buttons;
    // Positions, not labels: Nintendo B is the bottom face button.
    b.set(Buttons::SOUTH, right & 0x04 != 0); // B
    b.set(Buttons::EAST, right & 0x08 != 0); // A
    b.set(Buttons::WEST, right & 0x01 != 0); // Y
    b.set(Buttons::NORTH, right & 0x02 != 0); // X
    b.set(Buttons::R1, right & 0x40 != 0);
    b.set(Buttons::R2_DIGITAL, right & 0x80 != 0);
    b.set(Buttons::SELECT, shared & 0x01 != 0); // -
    b.set(Buttons::START, shared & 0x02 != 0); // +
    b.set(Buttons::R3, shared & 0x04 != 0);
    b.set(Buttons::L3, shared & 0x08 != 0);
    b.set(Buttons::HOME, shared & 0x10 != 0);
    b.set(Buttons::TOUCHPAD, shared & 0x20 != 0); // Capture
    b.set(Buttons::DPAD_DOWN, left & 0x01 != 0);
    b.set(Buttons::DPAD_UP, left & 0x02 != 0);
    b.set(Buttons::DPAD_RIGHT, left & 0x04 != 0);
    b.set(Buttons::DPAD_LEFT, left & 0x08 != 0);
    b.set(Buttons::L1, left & 0x40 != 0);
    b.set(Buttons::L2_DIGITAL, left & 0x80 != 0);
    // ZL / ZR are digital: full press or nothing.
    s.l2 = if left & 0x80 != 0 { 1.0 } else { 0.0 };
    s.r2 = if right & 0x80 != 0 { 1.0 } else { 0.0 };

    let lx = r[6] as u16 | ((r[7] as u16 & 0x0F) << 8);
    let ly = (r[7] as u16 >> 4) | ((r[8] as u16) << 4);
    let rx = r[9] as u16 | ((r[10] as u16 & 0x0F) << 8);
    let ry = (r[10] as u16 >> 4) | ((r[11] as u16) << 4);
    // Nintendo: +Y is up, same as PadState.
    (s.lx, s.ly, s.rx, s.ry) = (stick12(lx), stick12(ly), stick12(rx), stick12(ry));

    let level = (r[2] >> 4) & 0x0E; // 0,2,4,6,8
    s.battery_percent = Some((level as u16 * 100 / 8).min(100) as u8);

    if has_imu {
        let i16le = |o: usize| i16::from_le_bytes([r[o], r[o + 1]]);
        // Accel is ~0.000244 g/LSB (DS4: 0.000122), gyro ~0.07 dps/LSB (DS4: 0.0625):
        // rescale into DS4 units so calibration and gyro aim behave the same.
        let acc = |o: usize| (i16le(o) as i32 * 2).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        s.accel = [acc(13), acc(15), acc(17)];
        s.gyro = [i16le(19), i16le(21), i16le(23)];
    }
    s
}

/// Simple HID mode (before init took effect): buttons + hat + 8-bit sticks.
fn parse_simple(r: &[u8]) -> PadState {
    let mut s = PadState::default();
    let (b1, b2) = (r[1], r[2]);
    let b = &mut s.buttons;
    b.set(Buttons::SOUTH, b1 & 0x01 != 0); // B
    b.set(Buttons::EAST, b1 & 0x02 != 0); // A
    b.set(Buttons::WEST, b1 & 0x04 != 0); // Y
    b.set(Buttons::NORTH, b1 & 0x08 != 0); // X
    b.set(Buttons::L1, b1 & 0x10 != 0);
    b.set(Buttons::R1, b1 & 0x20 != 0);
    b.set(Buttons::L2_DIGITAL, b1 & 0x40 != 0);
    b.set(Buttons::R2_DIGITAL, b1 & 0x80 != 0);
    b.set(Buttons::SELECT, b2 & 0x01 != 0);
    b.set(Buttons::START, b2 & 0x02 != 0);
    b.set(Buttons::L3, b2 & 0x04 != 0);
    b.set(Buttons::R3, b2 & 0x08 != 0);
    b.set(Buttons::HOME, b2 & 0x10 != 0);
    b.set(Buttons::TOUCHPAD, b2 & 0x20 != 0);
    b.set_hat(r[3] & 0x0F);
    s.l2 = if b1 & 0x40 != 0 { 1.0 } else { 0.0 };
    s.r2 = if b1 & 0x80 != 0 { 1.0 } else { 0.0 };
    // 16-bit LE sticks in simple mode; use the high byte (+Y down here).
    if r.len() >= 12 {
        s.lx = stick_u8(r[5]);
        s.ly = -stick_u8(r[7]);
        s.rx = stick_u8(r[9]);
        s.ry = -stick_u8(r[11]);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_report_buttons_and_sticks() {
        let mut r = vec![0u8; 49];
        r[0] = 0x30;
        r[2] = 0x80; // battery level 8
        r[3] = 0x04 | 0x80; // B + ZR
        r[5] = 0x02; // up
        // left stick centred (2048, 2048), right stick full right (4095, 2048)
        let pack = |x: u16, y: u16| [(x & 0xFF) as u8, ((x >> 8) as u8 & 0x0F) | (((y & 0x0F) as u8) << 4), (y >> 4) as u8];
        r[6..9].copy_from_slice(&pack(2048, 2048));
        r[9..12].copy_from_slice(&pack(4095, 2048));
        let s = parse_input(&r).unwrap();
        assert!(s.buttons.has(Buttons::SOUTH));
        assert!(s.buttons.has(Buttons::R2_DIGITAL));
        assert!(s.buttons.has(Buttons::DPAD_UP));
        assert_eq!(s.r2, 1.0);
        assert!(s.lx.abs() < 0.01 && s.ly.abs() < 0.01);
        assert!(s.rx > 0.99);
        assert_eq!(s.battery_percent, Some(100));
    }

    #[test]
    fn non_input_reports_are_skipped() {
        assert!(parse_input(&[0x81, 0x02, 0x00]).is_none());
        assert!(parse_input(&[]).is_none());
    }
}
