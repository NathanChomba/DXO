//! M3: the virtual controller games see.
//!
//! Backend: ViGEmBus via the `vigem-client` crate (Windows only). ViGEmBus is
//! retired upstream but still installs and works; the `VirtualPad` trait keeps
//! the door open for a HIDMaestro (virtual DualSense) backend later.

use crate::error::Result;
use crate::state::{Buttons, PadState};

/// Rumble requested by the game, to forward to the physical pad.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Rumble {
    pub large: u8,
    pub small: u8,
}

pub trait VirtualPad: Send {
    fn update(&mut self, s: &PadState) -> Result<()>;
    /// Latest rumble from the game, if it changed. TODO M3: wire up ViGEm
    /// notifications (vigem-client `unstable_xtarget_notification` feature).
    fn poll_rumble(&mut self) -> Option<Rumble> {
        None
    }
}

/// XInput wire layout, kept platform-independent so it can be unit tested.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct XInputReport {
    pub buttons: u16,
    pub left_trigger: u8,
    pub right_trigger: u8,
    pub thumb_lx: i16,
    pub thumb_ly: i16,
    pub thumb_rx: i16,
    pub thumb_ry: i16,
}

pub mod xb {
    pub const UP: u16 = 0x0001;
    pub const DOWN: u16 = 0x0002;
    pub const LEFT: u16 = 0x0004;
    pub const RIGHT: u16 = 0x0008;
    pub const START: u16 = 0x0010;
    pub const BACK: u16 = 0x0020;
    pub const LTHUMB: u16 = 0x0040;
    pub const RTHUMB: u16 = 0x0080;
    pub const LB: u16 = 0x0100;
    pub const RB: u16 = 0x0200;
    pub const GUIDE: u16 = 0x0400;
    pub const A: u16 = 0x1000;
    pub const B: u16 = 0x2000;
    pub const X: u16 = 0x4000;
    pub const Y: u16 = 0x8000;
}

#[inline]
fn axis_i16(v: f32) -> i16 {
    (v.clamp(-1.0, 1.0) * 32767.0).round() as i16
}

pub fn to_xinput(s: &PadState) -> XInputReport {
    const MAP: [(u32, u16); 15] = [
        (Buttons::DPAD_UP, xb::UP),
        (Buttons::DPAD_DOWN, xb::DOWN),
        (Buttons::DPAD_LEFT, xb::LEFT),
        (Buttons::DPAD_RIGHT, xb::RIGHT),
        (Buttons::START, xb::START),
        (Buttons::SELECT, xb::BACK),
        (Buttons::L3, xb::LTHUMB),
        (Buttons::R3, xb::RTHUMB),
        (Buttons::L1, xb::LB),
        (Buttons::R1, xb::RB),
        (Buttons::HOME, xb::GUIDE),
        (Buttons::SOUTH, xb::A),
        (Buttons::EAST, xb::B),
        (Buttons::WEST, xb::X),
        (Buttons::NORTH, xb::Y),
    ];
    let mut buttons = 0u16;
    for (ours, theirs) in MAP {
        if s.buttons.has(ours) {
            buttons |= theirs;
        }
    }
    // Touchpad click -> Back is the common DS4Windows default.
    if s.buttons.has(Buttons::TOUCHPAD) {
        buttons |= xb::BACK;
    }
    XInputReport {
        buttons,
        left_trigger: (s.l2.clamp(0.0, 1.0) * 255.0).round() as u8,
        right_trigger: (s.r2.clamp(0.0, 1.0) * 255.0).round() as u8,
        thumb_lx: axis_i16(s.lx),
        thumb_ly: axis_i16(s.ly), // XInput: +Y = up, same as PadState
        thumb_rx: axis_i16(s.rx),
        thumb_ry: axis_i16(s.ry),
    }
}

/// No-op pad: lets the engine run on non-Windows hosts and in tests.
pub struct NullPad;
impl VirtualPad for NullPad {
    fn update(&mut self, _s: &PadState) -> Result<()> {
        Ok(())
    }
}

#[cfg(windows)]
pub mod vigem {
    use super::*;
    use crate::error::Error;
    use vigem_client::{Client, TargetId, XButtons, XGamepad, Xbox360Wired};

    pub struct ViGEmXbox {
        target: Xbox360Wired<Client>,
        last: XInputReport,
    }

    impl ViGEmXbox {
        pub fn new() -> Result<Self> {
            let client = Client::connect().map_err(|e| Error::Virtual(format!("ViGEmBus not installed? {e:?}")))?;
            let mut target = Xbox360Wired::new(client, TargetId::XBOX360_WIRED);
            target.plugin().map_err(|e| Error::Virtual(format!("plugin: {e:?}")))?;
            target.wait_ready().map_err(|e| Error::Virtual(format!("wait_ready: {e:?}")))?;
            Ok(Self { target, last: XInputReport { buttons: 1, ..Default::default() } })
        }
    }

    impl VirtualPad for ViGEmXbox {
        fn update(&mut self, s: &PadState) -> Result<()> {
            let r = to_xinput(s);
            if r == self.last {
                return Ok(()); // don't spam the bus with identical reports
            }
            self.last = r;
            let g = XGamepad {
                buttons: XButtons { raw: r.buttons },
                left_trigger: r.left_trigger,
                right_trigger: r.right_trigger,
                thumb_lx: r.thumb_lx,
                thumb_ly: r.thumb_ly,
                thumb_rx: r.thumb_rx,
                thumb_ry: r.thumb_ry,
            };
            self.target.update(&g).map_err(|e| Error::Virtual(format!("update: {e:?}")))
        }
    }
}

/// Create the best available virtual pad for this platform.
pub fn create_default() -> Result<Box<dyn VirtualPad>> {
    #[cfg(windows)]
    {
        Ok(Box::new(vigem::ViGEmXbox::new()?))
    }
    #[cfg(not(windows))]
    {
        log::warn!("virtual pads are Windows-only; using NullPad");
        Ok(Box::new(NullPad))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_face_and_axes() {
        let mut s = PadState { lx: 1.0, ly: -1.0, r2: 1.0, ..Default::default() };
        s.buttons.set(Buttons::SOUTH, true);
        s.buttons.set(Buttons::HOME, true);
        let x = to_xinput(&s);
        assert_eq!(x.buttons, xb::A | xb::GUIDE);
        assert_eq!(x.thumb_lx, 32767);
        assert_eq!(x.thumb_ly, -32767);
        assert_eq!(x.right_trigger, 255);
    }
}
