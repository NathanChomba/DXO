//! M6: generic / knock-off gamepads via the HID Report Descriptor.
//!
//! A deliberately small descriptor parser: enough to find Generic Desktop
//! axes (page 0x01), the hat switch, and Button page (0x09) bits inside the
//! input report. No external crate, so the behaviour is fully under our
//! control and unit-testable.
//!
//! Known gaps (fine for a first pass):
//!   * only the first input report id that contains axes/buttons is mapped
//!   * nested collections are flattened
//!   * Usage Min/Max ranges expand only for Button page usages
//!   * the axis -> stick assignment is a heuristic; profiles should be able
//!     to override it once real pads have been tested

use crate::error::{Error, Result};
use crate::state::{Buttons, PadState};

const PAGE_DESKTOP: u16 = 0x01;
const PAGE_SIMULATION: u16 = 0x02;
const PAGE_BUTTON: u16 = 0x09;

const U_X: u16 = 0x30;
const U_Y: u16 = 0x31;
const U_Z: u16 = 0x32;
const U_RX: u16 = 0x33;
const U_RY: u16 = 0x34;
const U_RZ: u16 = 0x35;
const U_HAT: u16 = 0x39;
const U_ACCEL: u16 = 0xC4; // Simulation page
const U_BRAKE: u16 = 0xC5;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Field {
    pub bit_offset: usize, // from the start of the payload (after report id)
    pub bit_size: usize,
    pub logical_min: i32,
    pub logical_max: i32,
}

impl Field {
    fn read_raw(&self, payload: &[u8]) -> Option<i32> {
        let mut v: u32 = 0;
        for i in 0..self.bit_size.min(32) {
            let bit = self.bit_offset + i;
            let byte = *payload.get(bit / 8)?;
            if byte >> (bit % 8) & 1 == 1 {
                v |= 1 << i;
            }
        }
        // Sign-extend when the logical range is signed.
        if self.logical_min < 0 && self.bit_size < 32 && v >> (self.bit_size - 1) & 1 == 1 {
            v |= !0u32 << self.bit_size;
        }
        Some(v as i32)
    }

    /// 0.0..=1.0
    fn read_unit(&self, payload: &[u8]) -> Option<f32> {
        let raw = self.read_raw(payload)?;
        let span = (self.logical_max - self.logical_min).max(1) as f32;
        Some(((raw - self.logical_min) as f32 / span).clamp(0.0, 1.0))
    }

    /// -1.0..=1.0
    fn read_axis(&self, payload: &[u8]) -> Option<f32> {
        self.read_unit(payload).map(|u| u * 2.0 - 1.0)
    }
}

#[derive(Debug, Clone, Default)]
pub struct Blueprint {
    pub report_id: Option<u8>,
    pub x: Option<Field>,
    pub y: Option<Field>,
    pub z: Option<Field>,
    pub rx: Option<Field>,
    pub ry: Option<Field>,
    pub rz: Option<Field>,
    pub accel: Option<Field>,
    pub brake: Option<Field>,
    pub hat: Option<Field>,
    /// Button page usage n (1-based) -> field
    pub buttons: Vec<(u16, Field)>,
    pub payload_bits: usize,
}

/// Default Button-page usage -> internal button. Index 0 = Button 1.
/// Typical cheap-pad (DirectInput) ordering; override per device in profiles.
const DEFAULT_BUTTON_MAP: [u32; 13] = [
    Buttons::NORTH,
    Buttons::EAST,
    Buttons::SOUTH,
    Buttons::WEST,
    Buttons::L1,
    Buttons::R1,
    Buttons::L2_DIGITAL,
    Buttons::R2_DIGITAL,
    Buttons::SELECT,
    Buttons::START,
    Buttons::L3,
    Buttons::R3,
    Buttons::HOME,
];

#[derive(Clone, Default)]
struct Globals {
    usage_page: u16,
    logical_min: i32,
    logical_max: i32,
    report_size: usize,
    report_count: usize,
    report_id: Option<u8>,
}

#[derive(Default)]
struct Locals {
    usages: Vec<(u16, u16)>, // (page, usage)
    usage_min: Option<(u16, u16)>,
    usage_max: Option<u16>,
}

fn data_u32(d: &[u8]) -> u32 {
    d.iter().enumerate().fold(0, |acc, (i, &b)| acc | (b as u32) << (8 * i))
}

fn data_i32(d: &[u8]) -> i32 {
    match d.len() {
        1 => d[0] as i8 as i32,
        2 => i16::from_le_bytes([d[0], d[1]]) as i32,
        4 => i32::from_le_bytes([d[0], d[1], d[2], d[3]]),
        _ => 0,
    }
}

impl Blueprint {
    pub fn from_descriptor(desc: &[u8]) -> Result<Blueprint> {
        let mut g = Globals::default();
        let mut stack: Vec<Globals> = Vec::new();
        let mut l = Locals::default();
        let mut bp = Blueprint::default();
        // bit cursor per report id
        let mut cursors: std::collections::HashMap<Option<u8>, usize> = Default::default();
        let mut chosen_id: Option<Option<u8>> = None;

        let mut i = 0;
        while i < desc.len() {
            let prefix = desc[i];
            if prefix == 0xFE {
                // long item: skip
                let len = *desc.get(i + 1).unwrap_or(&0) as usize;
                i += 3 + len;
                continue;
            }
            let size = match prefix & 0x03 {
                3 => 4,
                n => n as usize,
            };
            let d = desc.get(i + 1..i + 1 + size).ok_or(Error::Profile("truncated descriptor".into()))?;
            let tag = prefix & 0xFC;
            i += 1 + size;

            match tag {
                // ---- Global ----
                0x04 => g.usage_page = data_u32(d) as u16,
                0x14 => g.logical_min = data_i32(d),
                0x24 => {
                    g.logical_max = data_i32(d);
                    // Unsigned max encoded in a short field (e.g. 0xFF as 1 byte)
                    if g.logical_max < 0 && g.logical_min >= 0 {
                        g.logical_max = data_u32(d) as i32;
                    }
                }
                0x74 => g.report_size = data_u32(d) as usize,
                0x94 => g.report_count = data_u32(d) as usize,
                0x84 => g.report_id = Some(data_u32(d) as u8),
                0xA4 => stack.push(g.clone()),
                0xB4 => g = stack.pop().unwrap_or_default(),
                // ---- Local ----
                0x08 => {
                    // 4-byte usage carries its own page in the high word
                    if size == 4 {
                        let v = data_u32(d);
                        l.usages.push(((v >> 16) as u16, v as u16));
                    } else {
                        l.usages.push((g.usage_page, data_u32(d) as u16));
                    }
                }
                0x18 => l.usage_min = Some((g.usage_page, data_u32(d) as u16)),
                0x28 => l.usage_max = Some(data_u32(d) as u16),
                // ---- Main ----
                0x80 => {
                    // Input
                    let flags = data_u32(d);
                    let is_const = flags & 0x01 != 0;
                    let is_variable = flags & 0x02 != 0;
                    let cursor = cursors.entry(g.report_id).or_insert(0);
                    let start = *cursor;
                    *cursor += g.report_size * g.report_count;

                    if !is_const && is_variable {
                        // Expand usage min/max into a usage list
                        let mut usages = std::mem::take(&mut l.usages);
                        if let (Some((page, lo)), Some(hi)) = (l.usage_min, l.usage_max) {
                            usages.extend((lo..=hi).map(|u| (page, u)));
                        }
                        let mut matched = false;
                        for n in 0..g.report_count {
                            // HID rule: the last usage repeats for remaining fields
                            let Some(&(page, usage)) = usages.get(n).or(usages.last()) else { break };
                            let f = Field {
                                bit_offset: start + n * g.report_size,
                                bit_size: g.report_size,
                                logical_min: g.logical_min,
                                logical_max: g.logical_max,
                            };
                            matched |= bp.assign(page, usage, f);
                        }
                        if matched && chosen_id.is_none() {
                            chosen_id = Some(g.report_id);
                        }
                    }
                    l = Locals::default();
                }
                0x90 | 0xB0 | 0xA0 | 0xC0 => l = Locals::default(), // Output/Feature/Collection
                _ => {}
            }
        }

        bp.report_id = chosen_id.flatten();
        bp.payload_bits = cursors.get(&bp.report_id).copied().unwrap_or(0);
        if bp.x.is_none() && bp.buttons.is_empty() {
            return Err(Error::Profile("descriptor has no gamepad axes or buttons".into()));
        }
        Ok(bp)
    }

    fn assign(&mut self, page: u16, usage: u16, f: Field) -> bool {
        let slot = match (page, usage) {
            (PAGE_DESKTOP, U_X) => &mut self.x,
            (PAGE_DESKTOP, U_Y) => &mut self.y,
            (PAGE_DESKTOP, U_Z) => &mut self.z,
            (PAGE_DESKTOP, U_RX) => &mut self.rx,
            (PAGE_DESKTOP, U_RY) => &mut self.ry,
            (PAGE_DESKTOP, U_RZ) => &mut self.rz,
            (PAGE_DESKTOP, U_HAT) => &mut self.hat,
            (PAGE_SIMULATION, U_ACCEL) => &mut self.accel,
            (PAGE_SIMULATION, U_BRAKE) => &mut self.brake,
            (PAGE_BUTTON, n) if n > 0 => {
                self.buttons.push((n, f));
                return true;
            }
            _ => return false,
        };
        if slot.is_none() {
            *slot = Some(f);
        }
        true
    }

    pub fn parse(&self, report: &[u8]) -> Result<PadState> {
        let payload = match self.report_id {
            Some(id) => match report.first() {
                Some(&r) if r == id => &report[1..],
                Some(&r) => return Err(Error::BadReportId(r)),
                None => return Err(Error::ShortReport { got: 0, need: 1 }),
            },
            None => report,
        };
        let need = self.payload_bits.div_ceil(8);
        if payload.len() < need {
            return Err(Error::ShortReport { got: payload.len(), need });
        }

        let axis = |f: &Option<Field>| f.and_then(|f| f.read_axis(payload)).unwrap_or(0.0);
        let unit = |f: &Option<Field>| f.and_then(|f| f.read_unit(payload)).unwrap_or(0.0);

        let mut s = PadState { lx: axis(&self.x), ly: -axis(&self.y), ..Default::default() };

        // Heuristic: most cheap pads put the right stick on Z/Rz and (if
        // present) analog triggers on Rx/Ry. Otherwise right stick = Rx/Ry.
        if self.z.is_some() && self.rz.is_some() {
            s.rx = axis(&self.z);
            s.ry = -axis(&self.rz);
            s.l2 = unit(&self.rx);
            s.r2 = unit(&self.ry);
        } else {
            s.rx = axis(&self.rx);
            s.ry = -axis(&self.ry);
            s.l2 = unit(&self.brake);
            s.r2 = unit(&self.accel);
        }

        if let Some(h) = self.hat {
            // Only 8-way hats are handled; a value outside the logical range
            // is the HID "null state" (released).
            let raw = h.read_raw(payload).unwrap_or(-1);
            let eight_way = h.logical_max - h.logical_min == 7;
            let in_range = (h.logical_min..=h.logical_max).contains(&raw);
            s.buttons.set_hat(if eight_way && in_range { (raw - h.logical_min) as u8 } else { 8 });
        }

        for (n, f) in &self.buttons {
            let pressed = f.read_raw(payload).unwrap_or(0) != 0;
            if let Some(&bit) = DEFAULT_BUTTON_MAP.get(*n as usize - 1) {
                s.buttons.set(bit, pressed);
            }
        }
        if s.l2 > 0.5 {
            s.buttons.set(Buttons::L2_DIGITAL, true);
        }
        // Most cheap pads have on/off triggers (buttons 7/8, no axis): treat a
        // press as a full pull so games and the trigger gauges see it.
        if s.buttons.has(Buttons::L2_DIGITAL) && s.l2 < 0.01 {
            s.l2 = 1.0;
        }
        if s.buttons.has(Buttons::R2_DIGITAL) && s.r2 < 0.01 {
            s.r2 = 1.0;
        }
        if s.r2 > 0.5 {
            s.buttons.set(Buttons::R2_DIGITAL, true);
        }
        Ok(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A typical cheap USB gamepad descriptor: 12 buttons, hat, X/Y/Z/Rz.
    const DESC: &[u8] = &[
        0x05, 0x01, // Usage Page (Generic Desktop)
        0x09, 0x05, // Usage (Game Pad)
        0xA1, 0x01, // Collection (Application)
        0x85, 0x01, //   Report ID 1
        0x05, 0x09, //   Usage Page (Button)
        0x19, 0x01, //   Usage Minimum (1)
        0x29, 0x0C, //   Usage Maximum (12)
        0x15, 0x00, //   Logical Min 0
        0x25, 0x01, //   Logical Max 1
        0x75, 0x01, //   Report Size 1
        0x95, 0x0C, //   Report Count 12
        0x81, 0x02, //   Input (Data,Var,Abs)
        0x95, 0x04, //   Report Count 4 (padding)
        0x81, 0x03, //   Input (Const)
        0x05, 0x01, //   Usage Page (Generic Desktop)
        0x09, 0x39, //   Usage (Hat switch)
        0x15, 0x00, //   Logical Min 0
        0x25, 0x07, //   Logical Max 7
        0x75, 0x04, //   Report Size 4
        0x95, 0x01, //   Report Count 1
        0x81, 0x42, //   Input (Data,Var,Abs,Null)
        0x75, 0x04, 0x95, 0x01, 0x81, 0x03, // 4 bits padding
        0x09, 0x30, 0x09, 0x31, 0x09, 0x32, 0x09, 0x35, // X Y Z Rz
        0x15, 0x00, //   Logical Min 0
        0x26, 0xFF, 0x00, // Logical Max 255
        0x75, 0x08, //   Report Size 8
        0x95, 0x04, //   Report Count 4
        0x81, 0x02, //   Input (Data,Var,Abs)
        0xC0, // End Collection
    ];

    #[test]
    fn builds_blueprint() {
        let bp = Blueprint::from_descriptor(DESC).unwrap();
        assert_eq!(bp.report_id, Some(1));
        assert_eq!(bp.buttons.len(), 12);
        assert_eq!(bp.hat.unwrap().bit_offset, 16);
        assert_eq!(bp.x.unwrap().bit_offset, 24);
        assert_eq!(bp.rz.unwrap().bit_offset, 48);
        assert_eq!(bp.payload_bits, 56);
    }

    #[test]
    fn parses_report() {
        let bp = Blueprint::from_descriptor(DESC).unwrap();
        // id, buttons(1 and 10 pressed), hat=2 (right), X=255, Y=0, Z=128, Rz=128
        let report = [0x01, 0b0000_0001, 0b0000_0010, 0x02, 255, 0, 128, 128];
        let s = bp.parse(&report).unwrap();
        assert!(s.lx > 0.99);
        assert!(s.ly > 0.99);
        assert!(s.buttons.has(Buttons::NORTH)); // Button 1
        assert!(s.buttons.has(Buttons::START)); // Button 10
        assert!(s.buttons.has(Buttons::DPAD_RIGHT));
        // hat null state
        let released = [0x01, 0, 0, 0x08, 128, 128, 128, 128];
        assert!(!bp.parse(&released).unwrap().buttons.has(Buttons::DPAD_UP));
    }
}
