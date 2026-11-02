//! M5: motion. Calibration + a CemuHook (DSU) UDP server so Cemu, Yuzu-forks,
//! Dolphin, etc. get native motion controls.
//!
//! DSU protocol summary (all little-endian):
//!   header (16 bytes): magic "DSUS" (server) / "DSUC" (client), u16 version
//!   1001, u16 payload length (excluding header), u32 CRC32 (computed with
//!   this field zeroed), u32 server id; then u32 message type.
//!   Message types: 0x100000 version, 0x100001 port info, 0x100002 pad data.
//! Clients subscribe with a 0x100002 request and must re-send it every few
//! seconds; we stream pad data to every subscriber that pinged in the last 5 s.

use crate::mapping::button_from_name;
use crate::profile::GyroConfig;
use crate::state::PadState;
use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

pub const PROTOCOL_VERSION: u16 = 1001;
pub const MSG_VERSION: u32 = 0x100000;
pub const MSG_PORT_INFO: u32 = 0x100001;
pub const MSG_PAD_DATA: u32 = 0x100002;

/// DualSense/DS4 raw units -> physical units. Scales are the nominal datasheet
/// values (both pads: 16 raw per deg/s, 8192 raw per g); the bias (drift at
/// rest) is measured live by `Calibrator`, which matters far more for aiming.
#[derive(Debug, Clone, Copy)]
pub struct Calibration {
    pub gyro_bias: [f32; 3],
    /// raw -> degrees/second
    pub gyro_scale: f32,
    /// raw -> g
    pub accel_scale: f32,
}

impl Default for Calibration {
    fn default() -> Self {
        Self { gyro_bias: [0.0; 3], gyro_scale: 1.0 / 16.0, accel_scale: 1.0 / 8192.0 }
    }
}

impl Calibration {
    pub fn gyro_dps(&self, s: &PadState) -> [f32; 3] {
        std::array::from_fn(|i| (s.gyro[i] as f32 - self.gyro_bias[i]) * self.gyro_scale)
    }
    pub fn accel_g(&self, s: &PadState) -> [f32; 3] {
        std::array::from_fn(|i| s.accel[i] as f32 * self.accel_scale)
    }
}

// ---- calibration ---------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CalState {
    /// No still moment seen yet: readings include the sensor's drift.
    #[default]
    Uncalibrated,
    /// Manual calibration running (controller should be lying still).
    Calibrating,
    Calibrated,
    /// Manual calibration saw movement; nothing was changed.
    Failed,
}

/// Learns the gyro bias (what it reads while not moving). Runs all the time:
/// every second the pad lies perfectly still refines the bias, the way Steam
/// Input does. `request()` forces a careful 2-second measurement.
#[derive(Debug, Default)]
pub struct Calibrator {
    pub cal: Calibration,
    pub state: CalState,
    manual: bool,
    win_start: Option<Instant>,
    n: u32,
    sum: [f64; 3],
    sumsq: [f64; 3],
}

impl Calibrator {
    /// Max std-dev (raw units, 16 = 1 deg/s) for "lying still".
    const AUTO_STILL: f64 = 10.0;
    const MANUAL_STILL: f64 = 40.0;
    /// A real bias is small; a bigger steady reading is slow turning, not drift.
    const MAX_BIAS: f64 = 480.0; // 30 deg/s

    pub fn request(&mut self) {
        self.manual = true;
        self.state = CalState::Calibrating;
        self.restart_window();
    }

    /// New controller: its drift is different.
    pub fn reset(&mut self) {
        *self = Calibrator::default();
    }

    fn restart_window(&mut self) {
        self.win_start = None;
        self.n = 0;
        self.sum = [0.0; 3];
        self.sumsq = [0.0; 3];
    }

    pub fn feed(&mut self, gyro: [i16; 3], now: Instant) {
        let start = *self.win_start.get_or_insert(now);
        for i in 0..3 {
            let v = gyro[i] as f64;
            self.sum[i] += v;
            self.sumsq[i] += v * v;
        }
        self.n += 1;
        let window = Duration::from_millis(if self.manual { 2000 } else { 1000 });
        if now.duration_since(start) < window {
            return;
        }
        if self.n >= 50 {
            let n = self.n as f64;
            let mean: [f64; 3] = std::array::from_fn(|i| self.sum[i] / n);
            let std = (0..3).map(|i| (self.sumsq[i] / n - mean[i] * mean[i]).max(0.0).sqrt()).fold(0.0, f64::max);
            let big = mean.iter().any(|m| m.abs() > Self::MAX_BIAS);
            if self.manual {
                if std < Self::MANUAL_STILL && !big {
                    self.cal.gyro_bias = mean.map(|m| m as f32);
                    self.state = CalState::Calibrated;
                } else {
                    self.state = CalState::Failed;
                }
                self.manual = false;
            } else if std < Self::AUTO_STILL && !big {
                // First still moment: take it. Later ones: blend, so one odd
                // window can't yank the bias around.
                self.cal.gyro_bias = if self.state == CalState::Calibrated {
                    std::array::from_fn(|i| self.cal.gyro_bias[i] * 0.7 + mean[i] as f32 * 0.3)
                } else {
                    mean.map(|m| m as f32)
                };
                self.state = CalState::Calibrated;
            }
        }
        self.restart_window();
    }
}

// ---- gyro -> mouse ---------------------------------------------------------

/// Pixels per degree turned at sensitivity 1.0.
pub const PX_PER_DEG: f32 = 10.0;

pub fn mouse_hold_active(hold: &str, s: &PadState) -> bool {
    match hold.trim() {
        "" | "always" => true,
        "touch" => s.touch[0].active,
        name => button_from_name(name).is_some_and(|b| s.buttons.has(b)),
    }
}

/// Turns calibrated angular velocity into whole-pixel mouse moves, carrying
/// the fractions so slow turns still move the cursor.
#[derive(Debug, Default)]
pub struct GyroMouse {
    last: Option<Instant>,
    rem: (f32, f32),
}

impl GyroMouse {
    /// Returns the (dx, dy) to send this report, if any.
    pub fn step(&mut self, cfg: &GyroConfig, s: &PadState, cal: &Calibration, now: Instant) -> Option<(i32, i32)> {
        let dt = self.last.map(|t| now.duration_since(t).as_secs_f32()).unwrap_or(0.0).min(0.05);
        self.last = Some(now);
        if !cfg.to_mouse || !mouse_hold_active(&cfg.mouse_hold, s) {
            self.rem = (0.0, 0.0);
            return None;
        }
        let g = cal.gyro_dps(s);
        // Right-hand rule on Sony pads: turning right = negative yaw,
        // tipping the front up = positive pitch.
        let (yaw, pitch) = (g[1], g[0]);
        let dz = cfg.mouse_deadzone.max(0.0);
        let soft = |v: f32| if v.abs() <= dz { 0.0 } else { v - dz * v.signum() };
        let k = cfg.sensitivity.max(0.0) * PX_PER_DEG * dt;
        let mut dx = -soft(yaw) * k;
        let mut dy = -soft(pitch) * k;
        if cfg.invert_x {
            dx = -dx;
        }
        if cfg.invert_y {
            dy = -dy;
        }
        self.rem.0 += dx;
        self.rem.1 += dy;
        let (ix, iy) = (self.rem.0.trunc(), self.rem.1.trunc());
        self.rem.0 -= ix;
        self.rem.1 -= iy;
        (ix != 0.0 || iy != 0.0).then_some((ix as i32, iy as i32))
    }
}

/// Relative mouse move via SendInput (games reading raw input see it too).
#[cfg(windows)]
pub fn move_mouse(dx: i32, dy: i32) {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_MOVE, MOUSEINPUT,
    };
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT { dx, dy, mouseData: 0, dwFlags: MOUSEEVENTF_MOVE, time: 0, dwExtraInfo: 0 },
        },
    };
    unsafe {
        SendInput(1, &input, std::mem::size_of::<INPUT>() as i32);
    }
}

#[cfg(not(windows))]
pub fn move_mouse(_dx: i32, _dy: i32) {}

// ---- DSU / CemuHook ----------------------------------------------------------

fn header(buf: &mut Vec<u8>, server_id: u32, msg_type: u32) {
    buf.extend_from_slice(b"DSUS");
    buf.extend_from_slice(&PROTOCOL_VERSION.to_le_bytes());
    buf.extend_from_slice(&0u16.to_le_bytes()); // length, patched in finish()
    buf.extend_from_slice(&0u32.to_le_bytes()); // crc, patched in finish()
    buf.extend_from_slice(&server_id.to_le_bytes());
    buf.extend_from_slice(&msg_type.to_le_bytes());
}

fn finish(buf: &mut [u8]) {
    let len = (buf.len() - 16) as u16;
    buf[6..8].copy_from_slice(&len.to_le_bytes());
    buf[8..12].fill(0);
    let crc = crc32fast::hash(buf);
    buf[8..12].copy_from_slice(&crc.to_le_bytes());
}

/// 11-byte shared "controller info" block used by port-info and pad-data.
fn controller_info(buf: &mut Vec<u8>, slot: u8, connected: bool, battery: Option<u8>) {
    buf.push(slot);
    buf.push(if connected { 2 } else { 0 }); // state: 2 = connected
    buf.push(2); // model: full gyro
    buf.push(1); // connection: USB (cosmetic)
    buf.extend_from_slice(&[0u8; 6]); // MAC
    buf.push(match battery {
        Some(p) if p >= 95 => 0x05,
        Some(p) if p >= 70 => 0x04,
        Some(p) if p >= 40 => 0x03,
        Some(p) if p >= 10 => 0x02,
        Some(_) => 0x01,
        None => 0x00,
    });
}

pub fn build_pad_data(server_id: u32, packet_no: u32, s: &PadState, cal: &Calibration, ts_us: u64) -> Vec<u8> {
    let mut b = Vec::with_capacity(100);
    header(&mut b, server_id, MSG_PAD_DATA);
    controller_info(&mut b, 0, true, s.battery_percent);
    b.push(1); // connected
    b.extend_from_slice(&packet_no.to_le_bytes());
    // TODO M5: real button bytes (DS4-style bitfields), sticks, analog buttons,
    // touch. Emulators mostly only need motion, so these are zeroed for now.
    b.extend_from_slice(&[0u8; 2]); // buttons
    b.push(0); // home
    b.push(0); // touch button
    b.extend_from_slice(&[128u8; 4]); // LX LY RX RY
    b.extend_from_slice(&[0u8; 12]); // analog d-pad/face/shoulders/triggers
    b.extend_from_slice(&[0u8; 12]); // 2 x touch (active, id, x u16, y u16)
    b.extend_from_slice(&ts_us.to_le_bytes());
    let a = cal.accel_g(s);
    let g = cal.gyro_dps(s);
    // DSU axis convention differs from Sony's; signs/order need verifying
    // in Cemu's motion test. Start with: accel X, Y, Z; gyro pitch, yaw, roll.
    for v in [a[0], a[1], a[2], g[0], g[1], g[2]] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    finish(&mut b);
    b
}

/// Blocking-free DSU server; call `tick` from the engine loop.
pub struct CemuHookServer {
    sock: UdpSocket,
    server_id: u32,
    packet_no: u32,
    clients: Vec<(SocketAddr, Instant)>,
    started: Instant,
}

impl CemuHookServer {
    /// Emulators that asked for motion in the last 5 seconds.
    pub fn client_count(&self) -> usize {
        self.clients.iter().filter(|(_, t)| t.elapsed() < Duration::from_secs(5)).count()
    }

    pub fn bind(port: u16) -> std::io::Result<Self> {
        let sock = UdpSocket::bind(("127.0.0.1", port))?;
        sock.set_nonblocking(true)?;
        Ok(Self { sock, server_id: 0x5046_4731, packet_no: 0, clients: vec![], started: Instant::now() })
    }

    /// Handle incoming requests, then push the current state to subscribers.
    pub fn tick(&mut self, s: &PadState, cal: &Calibration) {
        let mut buf = [0u8; 128];
        while let Ok((n, from)) = self.sock.recv_from(&mut buf) {
            if n < 20 || &buf[..4] != b"DSUC" {
                continue;
            }
            let msg = u32::from_le_bytes(buf[16..20].try_into().unwrap());
            match msg {
                MSG_VERSION => {
                    let mut r = vec![];
                    header(&mut r, self.server_id, MSG_VERSION);
                    r.extend_from_slice(&PROTOCOL_VERSION.to_le_bytes());
                    finish(&mut r);
                    let _ = self.sock.send_to(&r, from);
                }
                MSG_PORT_INFO => {
                    // Request lists slots; we only ever expose slot 0.
                    let mut r = vec![];
                    header(&mut r, self.server_id, MSG_PORT_INFO);
                    controller_info(&mut r, 0, true, s.battery_percent);
                    r.push(0);
                    finish(&mut r);
                    let _ = self.sock.send_to(&r, from);
                }
                MSG_PAD_DATA => {
                    let now = Instant::now();
                    match self.clients.iter_mut().find(|(a, _)| *a == from) {
                        Some(c) => c.1 = now,
                        None => self.clients.push((from, now)),
                    }
                }
                _ => {}
            }
        }

        self.clients.retain(|(_, t)| t.elapsed() < Duration::from_secs(5));
        if self.clients.is_empty() {
            return;
        }
        self.packet_no = self.packet_no.wrapping_add(1);
        let ts = self.started.elapsed().as_micros() as u64;
        let pkt = build_pad_data(self.server_id, self.packet_no, s, cal, ts);
        for (addr, _) in &self.clients {
            let _ = self.sock.send_to(&pkt, addr);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pad_data_crc_valid() {
        let p = build_pad_data(1, 1, &PadState::default(), &Calibration::default(), 0);
        let mut copy = p.clone();
        copy[8..12].fill(0);
        assert_eq!(crc32fast::hash(&copy), u32::from_le_bytes(p[8..12].try_into().unwrap()));
        assert_eq!(u16::from_le_bytes([p[6], p[7]]) as usize, p.len() - 16);
        assert_eq!(p.len(), 100); // standard DSU pad-data size
    }

    #[test]
    fn auto_calibration_learns_bias_when_still() {
        let mut c = Calibrator::default();
        let t0 = Instant::now();
        for i in 0..300u64 {
            let wobble = (i % 3) as i16 - 1; // sensor noise
            c.feed([20 + wobble, -12, 5], t0 + Duration::from_millis(i * 4));
        }
        assert_eq!(c.state, CalState::Calibrated);
        assert!((c.cal.gyro_bias[0] - 20.0).abs() < 1.0);
        assert!((c.cal.gyro_bias[1] + 12.0).abs() < 1.0);
    }

    #[test]
    fn moving_pad_does_not_calibrate() {
        let mut c = Calibrator::default();
        let t0 = Instant::now();
        for i in 0..300u64 {
            let v = ((i as f32 / 10.0).sin() * 2000.0) as i16;
            c.feed([v, 0, 0], t0 + Duration::from_millis(i * 4));
        }
        assert_eq!(c.state, CalState::Uncalibrated);
        c.request();
        for i in 300..900u64 {
            let v = ((i as f32 / 10.0).sin() * 2000.0) as i16;
            c.feed([v, 0, 0], t0 + Duration::from_millis(i * 4));
        }
        assert_eq!(c.state, CalState::Failed);
    }

    #[test]
    fn gyro_mouse_turn_right_moves_right() {
        let cfg = GyroConfig { to_mouse: true, mouse_deadzone: 0.0, mouse_hold: String::new(), ..Default::default() };
        let mut m = GyroMouse::default();
        let mut s = PadState::default();
        s.gyro = [0, -16 * 90, 0]; // turning right at 90 deg/s
        let cal = Calibration::default();
        let t0 = Instant::now();
        let mut x = 0;
        for i in 0..=100u64 {
            if let Some((dx, dy)) = m.step(&cfg, &s, &cal, t0 + Duration::from_millis(i * 4)) {
                x += dx;
                assert_eq!(dy, 0);
            }
        }
        // 0.4 s * 90 deg/s * 10 px/deg = 360 px
        assert!((x - 360).abs() <= 2, "x = {x}");
    }

    #[test]
    fn gyro_mouse_respects_hold_button() {
        let cfg = GyroConfig { to_mouse: true, mouse_hold: "l2".into(), ..Default::default() };
        let mut s = PadState::default();
        assert!(!mouse_hold_active(&cfg.mouse_hold, &s));
        s.buttons.set(crate::state::Buttons::L2_DIGITAL, true);
        assert!(mouse_hold_active(&cfg.mouse_hold, &s));
    }
}
