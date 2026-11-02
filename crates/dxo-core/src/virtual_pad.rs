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
    /// Latest rumble from the game, if it changed since the last call.
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

/// DualShock 4 wire layout (ViGEm DS4_REPORT), platform-independent for tests.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Ds4Wire {
    pub lx: u8,
    pub ly: u8,
    pub rx: u8,
    pub ry: u8,
    /// bits 0-3 d-pad hat (8 = centred), 4 square, 5 cross, 6 circle, 7 triangle,
    /// 8 L1, 9 R1, 10 L2, 11 R2, 12 share, 13 options, 14 L3, 15 R3
    pub buttons: u16,
    /// bit 0 PS, bit 1 touchpad click
    pub special: u8,
    pub l2: u8,
    pub r2: u8,
}

#[inline]
fn axis_u8(v: f32) -> u8 {
    ((v.clamp(-1.0, 1.0) + 1.0) * 127.5).round() as u8
}

pub fn to_ds4(s: &PadState) -> Ds4Wire {
    let has = |b| s.buttons.has(b);
    let (u, d, l, r) = (has(Buttons::DPAD_UP), has(Buttons::DPAD_DOWN), has(Buttons::DPAD_LEFT), has(Buttons::DPAD_RIGHT));
    let hat: u16 = match (u, r, d, l) {
        (true, false, false, false) => 0,
        (true, true, false, false) => 1,
        (false, true, false, false) => 2,
        (false, true, true, false) => 3,
        (false, false, true, false) => 4,
        (false, false, true, true) => 5,
        (false, false, false, true) => 6,
        (true, false, false, true) => 7,
        _ => 8,
    };
    const MAP: [(u32, u16); 12] = [
        (Buttons::WEST, 1 << 4),
        (Buttons::SOUTH, 1 << 5),
        (Buttons::EAST, 1 << 6),
        (Buttons::NORTH, 1 << 7),
        (Buttons::L1, 1 << 8),
        (Buttons::R1, 1 << 9),
        (Buttons::L2_DIGITAL, 1 << 10),
        (Buttons::R2_DIGITAL, 1 << 11),
        (Buttons::SELECT, 1 << 12),
        (Buttons::START, 1 << 13),
        (Buttons::L3, 1 << 14),
        (Buttons::R3, 1 << 15),
    ];
    let mut buttons = hat;
    for (ours, theirs) in MAP {
        if has(ours) {
            buttons |= theirs;
        }
    }
    // Triggers count as pressed past a light touch even if the digital bit lagged.
    if s.l2 > 0.1 {
        buttons |= 1 << 10;
    }
    if s.r2 > 0.1 {
        buttons |= 1 << 11;
    }
    Ds4Wire {
        lx: axis_u8(s.lx),
        ly: axis_u8(-s.ly), // DS4: 0 = up
        rx: axis_u8(s.rx),
        ry: axis_u8(-s.ry),
        buttons,
        special: has(Buttons::HOME) as u8 | ((has(Buttons::TOUCHPAD) as u8) << 1),
        l2: (s.l2.clamp(0.0, 1.0) * 255.0).round() as u8,
        r2: (s.r2.clamp(0.0, 1.0) * 255.0).round() as u8,
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
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;
    use vigem_client::{Client, TargetId, XButtons, XGamepad, Xbox360Wired};

    /// Latest rumble from the game, packed so the notification thread and the
    /// engine thread can share it without a lock:
    ///   bits 0-7 small motor, bits 8-15 large motor, bit 16 = "changed".
    const CHANGED: u32 = 1 << 16;

    pub struct ViGEmXbox {
        // Field order matters: `target` drops first, which unplugs the pad and
        // makes the notification thread exit on its own.
        target: Xbox360Wired<Client>,
        last: XInputReport,
        rumble: Arc<AtomicU32>,
    }

    impl ViGEmXbox {
        pub fn new() -> Result<Self> {
            let client = Client::connect().map_err(|e| Error::Virtual(format!("ViGEmBus not installed? {e:?}")))?;
            let mut target = Xbox360Wired::new(client, TargetId::XBOX360_WIRED);
            target.plugin().map_err(|e| Error::Virtual(format!("plugin: {e:?}")))?;
            target.wait_ready().map_err(|e| Error::Virtual(format!("wait_ready: {e:?}")))?;

            // Games send rumble to the virtual pad; ViGEm hands it to us here.
            let rumble = Arc::new(AtomicU32::new(0));
            match target.request_notification() {
                Ok(req) => {
                    let r2 = rumble.clone();
                    // Detached on purpose: it ends when the target is unplugged.
                    let _ = req.spawn_thread(move |_, n| {
                        let packed = (n.small_motor as u32) | ((n.large_motor as u32) << 8) | CHANGED;
                        r2.store(packed, Ordering::Release);
                    });
                }
                Err(e) => log::warn!("rumble notifications unavailable: {e:?}"),
            }

            Ok(Self { target, last: XInputReport { buttons: 1, ..Default::default() }, rumble })
        }
    }

    impl VirtualPad for ViGEmXbox {
        fn poll_rumble(&mut self) -> Option<Rumble> {
            let v = self.rumble.fetch_and(!CHANGED, Ordering::AcqRel);
            (v & CHANGED != 0).then(|| Rumble { large: (v >> 8) as u8, small: v as u8 })
        }

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

#[cfg(windows)]
pub mod vigem_ds4 {
    use super::*;
    use crate::error::Error;
    use vigem_client::{Client, DS4Report, DualShock4Wired, TargetId};

    /// Virtual DualShock 4: games show PlayStation button prompts.
    /// vigem-client can't receive rumble for DS4 targets yet, so no passthrough.
    pub struct ViGEmDs4 {
        target: DualShock4Wired<Client>,
        last: Ds4Wire,
    }

    impl ViGEmDs4 {
        pub fn new() -> Result<Self> {
            let client = Client::connect().map_err(|e| Error::Virtual(format!("ViGEmBus not installed? {e:?}")))?;
            let mut target = DualShock4Wired::new(client, TargetId::DUALSHOCK4_WIRED);
            target.plugin().map_err(|e| Error::Virtual(format!("plugin: {e:?}")))?;
            target.wait_ready().map_err(|e| Error::Virtual(format!("wait_ready: {e:?}")))?;
            Ok(Self { target, last: Ds4Wire { buttons: 0xFFFF, ..Default::default() } })
        }
    }

    impl VirtualPad for ViGEmDs4 {
        fn update(&mut self, s: &PadState) -> Result<()> {
            let w = to_ds4(s);
            if w == self.last {
                return Ok(());
            }
            self.last = w;
            let r = DS4Report {
                thumb_lx: w.lx,
                thumb_ly: w.ly,
                thumb_rx: w.rx,
                thumb_ry: w.ry,
                buttons: w.buttons,
                special: w.special,
                trigger_l: w.l2,
                trigger_r: w.r2,
            };
            self.target.update(&r).map_err(|e| Error::Virtual(format!("update: {e:?}")))
        }
    }
}

/// Create the virtual pad for a profile's target.
pub fn create(target: crate::profile::VirtualTarget) -> Result<Box<dyn VirtualPad>> {
    use crate::profile::VirtualTarget;
    match target {
        VirtualTarget::None => Ok(Box::new(NullPad)),
        VirtualTarget::Xbox360 => create_default(),
        VirtualTarget::DualShock4 => {
            #[cfg(windows)]
            {
                Ok(Box::new(vigem_ds4::ViGEmDs4::new()?))
            }
            #[cfg(not(windows))]
            {
                Ok(Box::new(NullPad))
            }
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

    #[test]
    fn ds4_wire_layout() {
        let mut s = PadState { ly: 1.0, r2: 1.0, ..Default::default() };
        s.buttons.set(Buttons::SOUTH, true);
        s.buttons.set(Buttons::DPAD_UP, true);
        s.buttons.set(Buttons::DPAD_RIGHT, true);
        s.buttons.set(Buttons::HOME, true);
        let w = to_ds4(&s);
        assert_eq!(w.buttons & 0x0F, 1, "up-right hat");
        assert!(w.buttons & (1 << 5) != 0, "cross");
        assert!(w.buttons & (1 << 11) != 0, "R2 from analog");
        assert_eq!(w.ly, 0, "stick up = 0 on DS4");
        assert_eq!(w.lx, 128);
        assert_eq!(w.special, 1);
        assert_eq!(to_ds4(&PadState::default()).buttons, 8, "centred hat");
    }
}
