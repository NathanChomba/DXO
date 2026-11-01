//! Device-independent controller state. Every parser (DualSense, DS4, generic)
//! normalises into `PadState`; everything downstream (mapping, virtual pad,
//! UI) only ever sees this type.

/// Button bitfield. Bit positions are internal and stable; they are NOT the
/// Xbox or Sony wire layout (see `virtual_pad` for that translation).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Buttons(pub u32);

impl Buttons {
    pub const SOUTH: u32 = 1 << 0; // Cross / A
    pub const EAST: u32 = 1 << 1; // Circle / B
    pub const WEST: u32 = 1 << 2; // Square / X
    pub const NORTH: u32 = 1 << 3; // Triangle / Y
    pub const L1: u32 = 1 << 4;
    pub const R1: u32 = 1 << 5;
    pub const L3: u32 = 1 << 6;
    pub const R3: u32 = 1 << 7;
    pub const SELECT: u32 = 1 << 8; // Create / Share / Back
    pub const START: u32 = 1 << 9; // Options / Start
    pub const HOME: u32 = 1 << 10; // PS / Guide
    pub const TOUCHPAD: u32 = 1 << 11;
    pub const MUTE: u32 = 1 << 12;
    pub const DPAD_UP: u32 = 1 << 13;
    pub const DPAD_DOWN: u32 = 1 << 14;
    pub const DPAD_LEFT: u32 = 1 << 15;
    pub const DPAD_RIGHT: u32 = 1 << 16;
    pub const L2_DIGITAL: u32 = 1 << 17;
    pub const R2_DIGITAL: u32 = 1 << 18;
    // Edge back paddles / Fn (M6)
    pub const PADDLE_L: u32 = 1 << 19;
    pub const PADDLE_R: u32 = 1 << 20;

    #[inline]
    pub fn set(&mut self, bit: u32, on: bool) {
        if on {
            self.0 |= bit
        } else {
            self.0 &= !bit
        }
    }
    #[inline]
    pub fn has(self, bit: u32) -> bool {
        self.0 & bit != 0
    }

    /// Decode an 8-way hat (0=N, clockwise, 8+ = released) into D-pad bits.
    pub fn set_hat(&mut self, hat: u8) {
        let (u, r, d, l) = match hat {
            0 => (true, false, false, false),
            1 => (true, true, false, false),
            2 => (false, true, false, false),
            3 => (false, true, true, false),
            4 => (false, false, true, false),
            5 => (false, false, true, true),
            6 => (false, false, false, true),
            7 => (true, false, false, true),
            _ => (false, false, false, false),
        };
        self.set(Self::DPAD_UP, u);
        self.set(Self::DPAD_RIGHT, r);
        self.set(Self::DPAD_DOWN, d);
        self.set(Self::DPAD_LEFT, l);
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TouchPoint {
    pub active: bool,
    pub id: u8,
    pub x: u16, // 0..=1919 on DualSense
    pub y: u16, // 0..=1079 on DualSense
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum BatteryStatus {
    #[default]
    Unknown = 0,
    Discharging = 1,
    Charging = 2,
    Full = 3,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum Transport {
    #[default]
    Usb = 0,
    Bluetooth = 1,
}

/// Normalised state. Sticks are -1.0..=1.0 with +Y = up.
/// Triggers are 0.0..=1.0.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PadState {
    pub lx: f32,
    pub ly: f32,
    pub rx: f32,
    pub ry: f32,
    pub l2: f32,
    pub r2: f32,
    pub buttons: Buttons,
    /// Raw gyro (pitch, yaw, roll) in device units. Calibrate in `gyro` (M5).
    pub gyro: [i16; 3],
    /// Raw accelerometer (x, y, z) in device units.
    pub accel: [i16; 3],
    pub sensor_timestamp: u32,
    pub touch: [TouchPoint; 2],
    /// 0..=100, or None if the device does not report it.
    pub battery_percent: Option<u8>,
    pub battery_status: BatteryStatus,
    pub transport: Transport,
    pub sequence: u64,
}

/// Map an unsigned 8-bit stick value (0..=255, centre 128) to -1.0..=1.0.
#[inline]
pub fn stick_u8(v: u8) -> f32 {
    ((v as f32 - 127.5) / 127.5).clamp(-1.0, 1.0)
}

/// Map an 8-bit trigger value to 0.0..=1.0.
#[inline]
pub fn trigger_u8(v: u8) -> f32 {
    v as f32 / 255.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hat_decode() {
        let mut b = Buttons::default();
        b.set_hat(1);
        assert!(b.has(Buttons::DPAD_UP) && b.has(Buttons::DPAD_RIGHT));
        b.set_hat(8);
        assert_eq!(b.0 & (Buttons::DPAD_UP | Buttons::DPAD_RIGHT), 0);
    }

    #[test]
    fn stick_range() {
        assert!(stick_u8(0) <= -0.99);
        assert!(stick_u8(255) >= 0.99);
        assert!(stick_u8(128).abs() < 0.01);
    }
}
