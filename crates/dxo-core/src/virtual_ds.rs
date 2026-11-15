//! Groundwork for a virtual DualSense (roadmap section 5).
//!
//! The goal: a game sees a real DualSense, so it drives adaptive triggers and
//! haptics natively, and DXO passes those on to the physical pad. ViGEm can't
//! present a DualSense, so the device itself needs a HID-level virtual driver
//! (see docs/virtual-dualsense.md). That driver is a milestone of its own.
//!
//! What lives here is the half that does NOT depend on the driver, and that
//! any backend will need:
//!
//!   game  --(output report 0x02)-->  `parse_output_report`  --> OutputState --> real pad
//!   game  <--(input report 0x01)--   `build_input_report`   <-- PadState    <-- real pad
//!
//! plus the fixed descriptors/feature reports a game asks for when it opens
//! the pad. Everything here is pure and unit-tested against DXO's own
//! DualSense parser and output builder, so the two directions agree.

use crate::output::{OutputState, Rgb, TriggerEffect};
use crate::state::{BatteryStatus, Buttons, PadState, TouchPoint};

pub const VID: u16 = crate::device::SONY_VID;
pub const PID: u16 = crate::device::PID_DUALSENSE;
pub const INPUT_REPORT_ID: u8 = 0x01;
pub const OUTPUT_REPORT_ID: u8 = 0x02;
pub const INPUT_REPORT_LEN: usize = 64;
pub const OUTPUT_REPORT_LEN: usize = 48;

fn stick(v: f32) -> u8 {
    ((v.clamp(-1.0, 1.0) * 127.5) + 127.5).round().clamp(0.0, 255.0) as u8
}

fn trigger(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// D-pad bits -> the 8-way hat a DualSense reports (8 = released).
fn hat(b: Buttons) -> u8 {
    let (u, d, l, r) = (b.has(Buttons::DPAD_UP), b.has(Buttons::DPAD_DOWN), b.has(Buttons::DPAD_LEFT), b.has(Buttons::DPAD_RIGHT));
    match (u, r, d, l) {
        (true, false, false, false) => 0,
        (true, true, false, false) => 1,
        (false, true, false, false) => 2,
        (false, true, true, false) => 3,
        (false, false, true, false) => 4,
        (false, false, true, true) => 5,
        (false, false, false, true) => 6,
        (true, false, false, true) => 7,
        _ => 8,
    }
}

fn put_touch(out: &mut [u8], t: TouchPoint) {
    out[0] = (t.id & 0x7F) | if t.active { 0 } else { 0x80 };
    out[1] = (t.x & 0xFF) as u8;
    out[2] = ((t.x >> 8) & 0x0F) as u8 | (((t.y & 0x0F) as u8) << 4);
    out[3] = (t.y >> 4) as u8;
}

/// The USB input report (id 0x01, 64 bytes) a real DualSense would send for
/// this state. `counter` is the report sequence number games use to spot
/// dropped reports.
pub fn build_input_report(s: &PadState, counter: u8) -> [u8; INPUT_REPORT_LEN] {
    let mut r = [0u8; INPUT_REPORT_LEN];
    r[0] = INPUT_REPORT_ID;
    let p = &mut r[1..];
    p[0] = stick(s.lx);
    p[1] = stick(-s.ly); // device Y grows downward
    p[2] = stick(s.rx);
    p[3] = stick(-s.ry);
    p[4] = trigger(s.l2);
    p[5] = trigger(s.r2);
    p[6] = counter;
    let b = s.buttons;
    let bit = |on: bool, v: u8| if on { v } else { 0 };
    p[7] = hat(b)
        | bit(b.has(Buttons::WEST), 0x10)
        | bit(b.has(Buttons::SOUTH), 0x20)
        | bit(b.has(Buttons::EAST), 0x40)
        | bit(b.has(Buttons::NORTH), 0x80);
    p[8] = bit(b.has(Buttons::L1), 0x01)
        | bit(b.has(Buttons::R1), 0x02)
        | bit(b.has(Buttons::L2_DIGITAL) || s.l2 > 0.0, 0x04)
        | bit(b.has(Buttons::R2_DIGITAL) || s.r2 > 0.0, 0x08)
        | bit(b.has(Buttons::SELECT), 0x10)
        | bit(b.has(Buttons::START), 0x20)
        | bit(b.has(Buttons::L3), 0x40)
        | bit(b.has(Buttons::R3), 0x80);
    p[9] = bit(b.has(Buttons::HOME), 0x01) | bit(b.has(Buttons::TOUCHPAD), 0x02) | bit(b.has(Buttons::MUTE), 0x04);
    for i in 0..3 {
        p[15 + i * 2..17 + i * 2].copy_from_slice(&s.gyro[i].to_le_bytes());
        p[21 + i * 2..23 + i * 2].copy_from_slice(&s.accel[i].to_le_bytes());
    }
    p[27..31].copy_from_slice(&s.sensor_timestamp.to_le_bytes());
    put_touch(&mut p[32..36], s.touch[0]);
    put_touch(&mut p[36..40], s.touch[1]);
    // Battery: low nibble = level 0..10, high nibble = 0 discharging, 1 charging, 2 full.
    let level = (s.battery_percent.unwrap_or(100).min(100) / 10).min(10);
    let state = match s.battery_status {
        BatteryStatus::Charging => 0x1,
        BatteryStatus::Full => 0x2,
        _ => 0x0,
    };
    p[52] = level | (state << 4);
    p[53] = 0x08; // "USB data connected", as a wired pad reports
    r
}

/// What a game asked the pad to do, and which parts of it the game actually
/// set (a DualSense output report carries "valid" flags; anything not flagged
/// must be left as it was).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct GameOutput {
    pub rumble: Option<(u8, u8)>, // (heavy/left, light/right)
    pub trigger_right: Option<TriggerEffect>,
    pub trigger_left: Option<TriggerEffect>,
    pub lightbar: Option<Rgb>,
    pub player_leds: Option<u8>,
    pub mic_led: Option<bool>,
}

impl GameOutput {
    /// Lay the game's wishes over the state DXO would otherwise send.
    pub fn apply_to(&self, o: &mut OutputState) {
        if let Some((l, r)) = self.rumble {
            (o.rumble_left, o.rumble_right) = (l, r);
        }
        if let Some(t) = self.trigger_right {
            o.trigger_right = t;
        }
        if let Some(t) = self.trigger_left {
            o.trigger_left = t;
        }
        if let Some(c) = self.lightbar {
            o.lightbar = c;
        }
        if let Some(l) = self.player_leds {
            o.player_leds = l;
        }
        if let Some(m) = self.mic_led {
            o.mic_led = m;
        }
    }
}

fn effect(block: &[u8]) -> TriggerEffect {
    let mut data = [0u8; 11];
    data.copy_from_slice(&block[..11]);
    // Mode 0x05 with everything else zero is the pad's own "off".
    if data[0] == 0x05 || data.iter().all(|&b| b == 0) {
        TriggerEffect::Off
    } else {
        // Games send every effect the firmware knows; pass the block through
        // untouched rather than re-deriving DXO's named effects from it.
        TriggerEffect::Raw { data }
    }
}

/// Parse a USB output report (id 0x02) written by a game. None if it isn't one.
pub fn parse_output_report(report: &[u8]) -> Option<GameOutput> {
    if report.len() < OUTPUT_REPORT_LEN || report[0] != OUTPUT_REPORT_ID {
        return None;
    }
    let c = &report[1..];
    let (f0, f1) = (c[0], c[1]);
    Some(GameOutput {
        // bit 0 = "compatible" rumble, bit 1 = motors selected instead of haptics
        rumble: (f0 & 0x03 != 0).then_some((c[3], c[2])),
        trigger_right: (f0 & 0x04 != 0).then(|| effect(&c[10..21])),
        trigger_left: (f0 & 0x08 != 0).then(|| effect(&c[21..32])),
        mic_led: (f1 & 0x01 != 0).then_some(c[8] != 0),
        lightbar: (f1 & 0x04 != 0).then_some(Rgb(c[44], c[45], c[46])),
        player_leds: (f1 & 0x10 != 0).then_some(c[43] & 0x1F),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::{dualsense, Kind};
    use crate::output::build_report;
    use crate::state::Transport;

    fn busy_state() -> PadState {
        let mut s = PadState::default();
        (s.lx, s.ly, s.rx, s.ry) = (0.5, -1.0, -0.25, 1.0);
        (s.l2, s.r2) = (0.0, 0.6);
        for b in [Buttons::SOUTH, Buttons::NORTH, Buttons::L1, Buttons::R3, Buttons::START, Buttons::HOME, Buttons::MUTE, Buttons::DPAD_UP, Buttons::DPAD_RIGHT, Buttons::R2_DIGITAL] {
            s.buttons.set(b, true);
        }
        s.gyro = [120, -3000, 16];
        s.accel = [-50, 8192, 400];
        s.sensor_timestamp = 0x0102_0304;
        s.touch[0] = TouchPoint { active: true, id: 5, x: 1919, y: 1079 };
        s.touch[1] = TouchPoint { active: false, id: 0, x: 0, y: 0 };
        s.battery_percent = Some(75);
        s.battery_status = BatteryStatus::Charging;
        s
    }

    /// The report we build must read back, through DXO's own DualSense parser,
    /// as the state we started from. This is the contract a virtual DualSense
    /// has to keep with games.
    #[test]
    fn input_report_round_trips_through_the_real_parser() {
        let s = busy_state();
        let r = build_input_report(&s, 7);
        assert_eq!((r.len(), r[0]), (64, 0x01));
        let back = dualsense::parse_input(&r).unwrap();
        assert_eq!(back.buttons, s.buttons);
        for (a, b) in [(back.lx, s.lx), (back.ly, s.ly), (back.rx, s.rx), (back.ry, s.ry), (back.l2, s.l2), (back.r2, s.r2)] {
            assert!((a - b).abs() < 0.01, "{a} vs {b}");
        }
        assert_eq!(back.gyro, s.gyro);
        assert_eq!(back.accel, s.accel);
        assert_eq!(back.sensor_timestamp, s.sensor_timestamp);
        assert_eq!(back.touch, s.touch);
        assert_eq!(back.battery_status, BatteryStatus::Charging);
        assert_eq!(back.battery_percent, Some(75));
    }

    #[test]
    fn resting_pad_is_centred_and_released() {
        let r = build_input_report(&PadState::default(), 0);
        assert_eq!(&r[1..5], &[128, 128, 128, 128]);
        assert_eq!(r[8] & 0x0F, 8, "hat released");
        let back = dualsense::parse_input(&r).unwrap();
        assert_eq!(back.buttons.0, 0);
        assert!(back.lx.abs() < 0.01 && back.ly.abs() < 0.01);
    }

    /// The other direction: what DXO sends to a real pad, parsed as if a game
    /// had sent it to the virtual one, must come back unchanged.
    #[test]
    fn output_report_round_trips_through_the_real_builder() {
        let o = OutputState {
            lightbar: Rgb(10, 200, 30),
            player_leds: 0b10101,
            rumble_left: 180,
            rumble_right: 40,
            trigger_left: TriggerEffect::Feedback { start: 3, strength: 6 },
            trigger_right: TriggerEffect::Weapon { start: 2, end: 6, strength: 8 },
            mic_led: true,
            audio: None,
        };
        let wire = build_report(Kind::DualSense, Transport::Usb, &o, 0);
        let g = parse_output_report(&wire).unwrap();
        assert_eq!(g.rumble, Some((180, 40)));
        assert_eq!(g.lightbar, Some(Rgb(10, 200, 30)));
        assert_eq!(g.player_leds, Some(0b10101));
        assert_eq!(g.mic_led, Some(true));
        // effects come back as the same 11 bytes on the wire
        assert_eq!(g.trigger_left.unwrap().encode(), o.trigger_left.encode());
        assert_eq!(g.trigger_right.unwrap().encode(), o.trigger_right.encode());

        let mut fresh = OutputState::default();
        g.apply_to(&mut fresh);
        assert_eq!(build_report(Kind::DualSense, Transport::Usb, &fresh, 0), wire, "forwarding is lossless");
    }

    #[test]
    fn only_flagged_fields_are_taken() {
        // A game that only sets rumble must not reset the lightbar or triggers.
        let mut wire = [0u8; 48];
        wire[0] = 0x02;
        wire[1] = 0x01; // valid_flag0: rumble only
        wire[3] = 9; // right (light)
        wire[4] = 200; // left (heavy)
        wire[45] = 255; // lightbar bytes present but NOT flagged
        let g = parse_output_report(&wire).unwrap();
        assert_eq!(g.rumble, Some((200, 9)));
        assert_eq!((g.lightbar, g.trigger_left, g.trigger_right, g.player_leds), (None, None, None, None));
        let mut o = OutputState { lightbar: Rgb(1, 2, 3), trigger_left: TriggerEffect::Feedback { start: 1, strength: 2 }, ..Default::default() };
        g.apply_to(&mut o);
        assert_eq!(o.lightbar, Rgb(1, 2, 3));
        assert_eq!(o.trigger_left, TriggerEffect::Feedback { start: 1, strength: 2 });
        assert_eq!((o.rumble_left, o.rumble_right), (200, 9));
    }

    #[test]
    fn junk_is_rejected() {
        assert_eq!(parse_output_report(&[0x02; 10]), None, "too short");
        assert_eq!(parse_output_report(&[0x31; 48]), None, "not a USB output report");
    }
}
