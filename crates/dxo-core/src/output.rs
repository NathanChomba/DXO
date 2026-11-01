//! M2: output reports - lightbar, player LEDs, rumble, adaptive triggers.
//!
//! DualSense layout follows `struct dualsense_output_report_common` from
//! Linux `hid-playstation.c`. Trigger effect encodings follow the community
//! reverse-engineering (Nielk1's DualSense trigger notes); verify each effect
//! on hardware before relying on it.

use crate::device::Kind;
use crate::state::Transport;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rgb(pub u8, pub u8, pub u8);

/// Adaptive trigger effect (DualSense only).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum TriggerEffect {
    #[default]
    Off,
    /// Constant resistance from `start` (0..=9) to the end, `strength` 1..=8.
    Feedback { start: u8, strength: u8 },
    /// Click like a gun trigger between `start` (2..=7) and `end` (start+1..=8).
    Weapon { start: u8, end: u8, strength: u8 },
    /// Raw 11-byte effect block for experimenting (M5 UDP telemetry).
    Raw([u8; 11]),
}

impl TriggerEffect {
    pub fn encode(self) -> [u8; 11] {
        let mut d = [0u8; 11];
        match self {
            TriggerEffect::Off => d[0] = 0x05,
            TriggerEffect::Feedback { start, strength } => {
                let start = start.min(9);
                let force = (strength.clamp(1, 8) - 1) as u32;
                let mut zones: u16 = 0;
                let mut forces: u32 = 0;
                for i in start..10 {
                    zones |= 1 << i;
                    forces |= force << (3 * i as u32);
                }
                d[0] = 0x21;
                d[1..3].copy_from_slice(&zones.to_le_bytes());
                d[3..7].copy_from_slice(&forces.to_le_bytes());
            }
            TriggerEffect::Weapon { start, end, strength } => {
                let start = start.clamp(2, 7);
                let end = end.clamp(start + 1, 8);
                let zones: u16 = (1 << start) | (1 << end);
                d[0] = 0x25;
                d[1..3].copy_from_slice(&zones.to_le_bytes());
                d[3] = strength.clamp(1, 8) - 1;
            }
            TriggerEffect::Raw(raw) => d = raw,
        }
        d
    }
}

/// Everything the engine wants the pad to show/do. The engine diffs this and
/// only sends a report when it changes (or on a keep-alive timer).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct OutputState {
    pub lightbar: Rgb,
    /// Bitmask of the 5 DualSense player LEDs.
    pub player_leds: u8,
    /// Heavy (left) motor 0..=255
    pub rumble_left: u8,
    /// Light (right) motor 0..=255
    pub rumble_right: u8,
    pub trigger_left: TriggerEffect,
    pub trigger_right: TriggerEffect,
    pub mic_led: bool,
}

// valid_flag0
const F0_COMPAT_VIBRATION: u8 = 1 << 0;
const F0_HAPTICS_SELECT: u8 = 1 << 1;
const F0_RIGHT_TRIGGER: u8 = 1 << 2;
const F0_LEFT_TRIGGER: u8 = 1 << 3;
// valid_flag1
const F1_MIC_LED: u8 = 1 << 0;
const F1_LIGHTBAR: u8 = 1 << 2;
const F1_PLAYER_LEDS: u8 = 1 << 4;

/// 47-byte common block shared by the USB (0x02) and BT (0x31) reports.
fn dualsense_common(o: &OutputState) -> [u8; 47] {
    let mut c = [0u8; 47];
    c[0] = F0_COMPAT_VIBRATION | F0_HAPTICS_SELECT | F0_RIGHT_TRIGGER | F0_LEFT_TRIGGER;
    c[1] = F1_MIC_LED | F1_LIGHTBAR | F1_PLAYER_LEDS;
    c[2] = o.rumble_right;
    c[3] = o.rumble_left;
    c[8] = o.mic_led as u8;
    c[10..21].copy_from_slice(&o.trigger_right.encode());
    c[21..32].copy_from_slice(&o.trigger_left.encode());
    // c[38] valid_flag2, c[41] lightbar_setup, c[42] led_brightness.
    // If the lightbar stays on the boot-time blue, send ONE report at connect
    // with c[38] = 1 << 1 and c[41] = 0x02 (release LED control), as Linux
    // hid-playstation does. Not sent every time to avoid flicker.
    c[43] = o.player_leds & 0x1F;
    c[44] = o.lightbar.0;
    c[45] = o.lightbar.1;
    c[46] = o.lightbar.2;
    c
}

/// Build a complete output report for the given pad and transport.
/// `bt_seq` is a 4-bit rolling counter required by Bluetooth reports.
pub fn build_report(kind: Kind, transport: Transport, o: &OutputState, bt_seq: u8) -> Vec<u8> {
    match (kind, transport) {
        (k, Transport::Usb) if k.is_dualsense() => {
            let mut r = vec![0u8; 48];
            r[0] = 0x02;
            r[1..48].copy_from_slice(&dualsense_common(o));
            r
        }
        (k, Transport::Bluetooth) if k.is_dualsense() => {
            // M6: untested on hardware
            let mut r = vec![0u8; 78];
            r[0] = 0x31;
            r[1] = (bt_seq & 0x0F) << 4;
            r[2] = 0x10; // tag
            r[3..50].copy_from_slice(&dualsense_common(o));
            let crc = bt_crc(&r[..74]);
            r[74..78].copy_from_slice(&crc.to_le_bytes());
            r
        }
        (Kind::DualShock4, Transport::Usb) => {
            let mut r = vec![0u8; 32];
            r[0] = 0x05;
            r[1] = 0x07; // rumble | lightbar | flash
            r[4] = o.rumble_right; // light motor
            r[5] = o.rumble_left; // heavy motor
            r[6] = o.lightbar.0;
            r[7] = o.lightbar.1;
            r[8] = o.lightbar.2;
            r
        }
        (Kind::DualShock4, Transport::Bluetooth) => {
            // Layout: Linux hid-playstation `dualshock4_output_report_bt`.
            // Same fields as USB 0x05, shifted by 2, plus a CRC trailer.
            let mut r = vec![0u8; 78];
            r[0] = 0x11;
            r[1] = 0xC0; // hw_control: HID | CRC32
            r[2] = 0x00; // audio_control
            r[3] = 0x07; // valid: rumble | lightbar | flash
            r[6] = o.rumble_right; // light motor
            r[7] = o.rumble_left; // heavy motor
            r[8] = o.lightbar.0;
            r[9] = o.lightbar.1;
            r[10] = o.lightbar.2;
            let crc = bt_crc(&r[..74]);
            r[74..78].copy_from_slice(&crc.to_le_bytes());
            r
        }
        // Generic pads: no output yet (M6)
        _ => Vec::new(),
    }
}

/// Sony BT output CRC: CRC-32 over a 0xA2 prefix byte + the report.
pub fn bt_crc(report: &[u8]) -> u32 {
    let mut h = crc32fast::Hasher::new();
    h.update(&[0xA2]);
    h.update(report);
    h.finalize()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usb_lightbar_lands_at_end() {
        let o = OutputState { lightbar: Rgb(1, 2, 3), ..Default::default() };
        let r = build_report(Kind::DualSense, Transport::Usb, &o, 0);
        assert_eq!(r.len(), 48);
        assert_eq!(&r[45..48], &[1, 2, 3]);
        assert_eq!(r[11], 0x05); // right trigger "off"
    }

    #[test]
    fn feedback_encoding() {
        let d = TriggerEffect::Feedback { start: 8, strength: 8 }.encode();
        assert_eq!(d[0], 0x21);
        assert_eq!(u16::from_le_bytes([d[1], d[2]]), 0b11_0000_0000);
    }

    #[test]
    fn ds4_bt_report() {
        let o = OutputState { lightbar: Rgb(10, 20, 30), rumble_left: 200, ..Default::default() };
        let r = build_report(Kind::DualShock4, Transport::Bluetooth, &o, 0);
        assert_eq!(r.len(), 78);
        assert_eq!((r[0], r[1]), (0x11, 0xC0));
        assert_eq!(r[7], 200);
        assert_eq!(&r[8..11], &[10, 20, 30]);
        assert_eq!(u32::from_le_bytes(r[74..78].try_into().unwrap()), bt_crc(&r[..74]));
    }

    #[test]
    fn bt_report_has_crc() {
        let r = build_report(Kind::DualSense, Transport::Bluetooth, &OutputState::default(), 3);
        assert_eq!(r.len(), 78);
        let crc = u32::from_le_bytes(r[74..78].try_into().unwrap());
        assert_eq!(crc, bt_crc(&r[..74]));
    }
}
