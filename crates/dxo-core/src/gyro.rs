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
use crate::profile::{GyroConfig, GyroSpace};
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
    /// Goes up every time the bias is re-measured (so the engine knows when
    /// there's a new value worth saving for this controller).
    pub revision: u32,
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

    /// Start from the bias saved for this controller last time, so gyro aim
    /// is steady from the first second instead of after the first rest.
    /// Auto-calibration keeps refining it from there.
    pub fn seed(&mut self, bias: [f32; 3]) {
        if bias.iter().all(|b| b.is_finite() && b.abs() <= Self::MAX_BIAS as f32) {
            self.cal.gyro_bias = bias;
            self.state = CalState::Calibrated;
        }
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
                    self.revision = self.revision.wrapping_add(1);
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
                self.revision = self.revision.wrapping_add(1);
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

// ---- sensor fusion + smoothing -------------------------------------------------

/// Tracks which way is down, in the controller's own axes.
///
/// The gyro says how the pad just turned (fast, but drifts); the
/// accelerometer says where gravity is (noisy, wrong while the pad is being
/// shaken, but never drifts). Blending the two - the idea at the heart of the
/// Mahony and Madgwick filters - gives a gravity direction that is both
/// steady and quick. Gyro aiming uses it for "player space": a sideways turn
/// is measured about gravity, so it feels the same however the pad is held.
#[derive(Debug, Clone, Copy)]
pub struct GravityTracker {
    /// Unit vector pointing up, in pad axes. Flat on a table = (0, 1, 0).
    pub up: [f32; 3],
    seeded: bool,
}

impl Default for GravityTracker {
    fn default() -> Self {
        Self { up: [0.0, 1.0, 0.0], seeded: false }
    }
}

impl GravityTracker {
    /// How quickly the accelerometer pulls the estimate back, per second.
    const CORRECTION: f32 = 4.0;

    pub fn update(&mut self, gyro_dps: [f32; 3], accel_g: [f32; 3], dt: f32) {
        let len = |v: [f32; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        let a_len = len(accel_g);
        // The first trustworthy reading sets the direction outright.
        if !self.seeded {
            if (0.8..1.2).contains(&a_len) {
                self.up = accel_g.map(|v| v / a_len);
                self.seeded = true;
            }
            return;
        }
        // 1. Gyro: a direction fixed in the room appears to turn the opposite
        //    way to the pad:  up' = up - (w x up) dt
        let w = gyro_dps.map(|d| d.to_radians());
        let u = self.up;
        let cross = [w[1] * u[2] - w[2] * u[1], w[2] * u[0] - w[0] * u[2], w[0] * u[1] - w[1] * u[0]];
        let mut up = [u[0] - cross[0] * dt, u[1] - cross[1] * dt, u[2] - cross[2] * dt];
        // 2. Accelerometer: lean towards measured gravity, but only when the
        //    reading is about 1 g (otherwise the pad is being swung around).
        if (0.85..1.15).contains(&a_len) {
            let k = (Self::CORRECTION * dt).min(1.0);
            for i in 0..3 {
                up[i] += (accel_g[i] / a_len - up[i]) * k;
            }
        }
        let n = len(up);
        if n > 1e-6 {
            self.up = up.map(|v| v / n);
        }
    }

    /// Turn rate about the vertical (deg/s): the "player space" sideways turn.
    pub fn yaw_about_up(&self, gyro_dps: [f32; 3]) -> f32 {
        gyro_dps[0] * self.up[0] + gyro_dps[1] * self.up[1] + gyro_dps[2] * self.up[2]
    }
}

/// One-value Kalman filter for a turn rate. It trusts its own estimate while
/// the readings stay inside the sensor's noise band (so jitter is averaged
/// away) and jumps to the reading the moment it leaves that band (so a real
/// turn is never delayed).
#[derive(Debug, Clone, Copy, Default)]
struct RateFilter {
    x: f32,
    p: f32,
}

impl RateFilter {
    /// `noise` = how much the sensor jitters (deg/s, one standard deviation).
    fn update(&mut self, z: f32, noise: f32) -> f32 {
        const DRIFT: f32 = 1.5; // how much a real turn rate changes between reports, deg/s
        let r = noise * noise;
        self.p += DRIFT * DRIFT;
        let innovation = z - self.x;
        // More than 3 standard deviations out: that's movement, not noise.
        if innovation * innovation > 9.0 * (self.p + r) {
            self.p += innovation * innovation;
        }
        let k = self.p / (self.p + r);
        self.x += k * innovation;
        self.p *= 1.0 - k;
        self.x
    }
}

/// Turns calibrated angular velocity into whole-pixel mouse moves, carrying
/// the fractions so slow turns still move the cursor.
#[derive(Debug, Default)]
pub struct GyroMouse {
    last: Option<Instant>,
    rem: (f32, f32),
    gravity: GravityTracker,
    yaw_f: RateFilter,
    pitch_f: RateFilter,
}

impl GyroMouse {
    /// Returns the (dx, dy) to send this report, if any.
    pub fn step(&mut self, cfg: &GyroConfig, s: &PadState, cal: &Calibration, now: Instant) -> Option<(i32, i32)> {
        let dt = self.last.map(|t| now.duration_since(t).as_secs_f32()).unwrap_or(0.0).min(0.05);
        self.last = Some(now);
        let g = cal.gyro_dps(s);
        // Keep following gravity even while aim is off, so it's right the
        // moment the player squeezes the aim button.
        if cfg.to_mouse && cfg.space == GyroSpace::Player {
            self.gravity.update(g, cal.accel_g(s), dt);
        }
        if !cfg.to_mouse || !mouse_hold_active(&cfg.mouse_hold, s) {
            self.rem = (0.0, 0.0);
            return None;
        }
        // Ratchet: the button is held, so the player is re-centring their
        // hands. Like lifting a mouse off the mat: nothing moves.
        if !cfg.ratchet.trim().is_empty() && mouse_hold_active(&cfg.ratchet, s) {
            return None;
        }
        // Right-hand rule on Sony pads: turning right = negative yaw,
        // tipping the front up = positive pitch.
        let (mut yaw, mut pitch) = (g[1], g[0]);
        if cfg.space == GyroSpace::Player {
            yaw = self.gravity.yaw_about_up(g);
        }
        let smooth = cfg.smoothing.clamp(0.0, 1.0);
        if smooth > 0.0 {
            let noise = 0.3 + 5.0 * smooth;
            yaw = self.yaw_f.update(yaw, noise);
            pitch = self.pitch_f.update(pitch, noise);
        }
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
    fn ratchet_pauses_aim_while_held() {
        let cfg = GyroConfig { to_mouse: true, mouse_deadzone: 0.0, mouse_hold: String::new(), ratchet: "r1".into(), ..Default::default() };
        let mut m = GyroMouse::default();
        let cal = Calibration::default();
        let t0 = Instant::now();
        let mut s = PadState::default();
        s.gyro = [0, -16 * 90, 0];
        m.step(&cfg, &s, &cal, t0);
        assert!(m.step(&cfg, &s, &cal, t0 + Duration::from_millis(20)).is_some(), "aiming normally");
        s.buttons.set(crate::state::Buttons::R1, true);
        for i in 2..20u64 {
            assert_eq!(m.step(&cfg, &s, &cal, t0 + Duration::from_millis(i * 20)), None, "ratchet held: frozen");
        }
        s.buttons.set(crate::state::Buttons::R1, false);
        // released: aim resumes without a jump for the time it was paused
        let (dx, _) = m.step(&cfg, &s, &cal, t0 + Duration::from_millis(400)).unwrap();
        assert!((15..=20).contains(&dx), "one report's worth (90 deg/s * 20 ms * 10 px = 18), got {dx}");
    }

    #[test]
    fn smoothing_quiets_jitter_but_follows_a_flick() {
        // resting pad: +-2 deg/s of sensor noise around zero
        let noisy = |i: usize| [1.9f32, -1.3, 0.4, -2.0, 1.1, -0.2, 1.6, -1.7][i % 8];
        let mut f = RateFilter::default();
        let (mut raw_sq, mut out_sq) = (0.0f32, 0.0f32);
        for i in 0..400 {
            let z = noisy(i);
            let x = f.update(z, 0.3 + 5.0 * 0.6);
            if i >= 50 {
                raw_sq += z * z;
                out_sq += x * x;
            }
        }
        assert!(out_sq < raw_sq * 0.25, "jitter energy cut by 4x or more: {out_sq} vs {raw_sq}");

        // a sudden 300 deg/s flick must come straight through
        let first = f.update(300.0, 0.3 + 5.0 * 0.6);
        assert!(first > 270.0, "no lag on a real turn, got {first}");
        // and a steady turn is reported as it is
        let mut x = 0.0;
        for _ in 0..50 {
            x = f.update(300.0, 3.3);
        }
        assert!((x - 300.0).abs() < 1.0);
    }

    #[test]
    fn smoothing_off_is_bit_identical() {
        let mk = |smoothing| GyroConfig { to_mouse: true, mouse_deadzone: 0.0, mouse_hold: String::new(), smoothing, ..Default::default() };
        let cal = Calibration::default();
        let t0 = Instant::now();
        let mut s = PadState::default();
        let run = |cfg: &GyroConfig, s: &mut PadState| {
            let mut m = GyroMouse::default();
            let mut total = 0;
            for i in 0..=100u64 {
                s.gyro = [0, -16 * 40 + (i as i16 % 5) * 16, 0];
                if let Some((dx, _)) = m.step(cfg, s, &cal, t0 + Duration::from_millis(i * 4)) {
                    total += dx;
                }
            }
            total
        };
        let off = run(&mk(0.0), &mut s);
        let on = run(&mk(1.0), &mut s);
        assert!(off > 100);
        // smoothing must not change how far a steady turn moves the cursor (to within a few px)
        assert!((off - on).abs() <= 4, "off {off}, smoothed {on}");
    }

    #[test]
    fn gravity_tracker_follows_a_tilt() {
        let mut g = GravityTracker::default();
        // flat on the table
        for _ in 0..50 {
            g.update([0.0; 3], [0.0, 1.0, 0.0], 0.004);
        }
        assert!((g.up[1] - 1.0).abs() < 1e-3);
        // now held nose-up at 45 degrees: gravity splits between Y and Z
        let tilt = [0.0, 0.70710677, 0.70710677];
        for _ in 0..500 {
            g.update([0.0; 3], tilt, 0.004);
        }
        assert!((g.up[1] - 0.707).abs() < 0.02 && (g.up[2] - 0.707).abs() < 0.02, "{:?}", g.up);
        // being shaken (3 g) must not move the estimate
        let before = g.up;
        for _ in 0..100 {
            g.update([0.0; 3], [3.0, 0.0, 0.0], 0.004);
        }
        assert_eq!(g.up, before);
    }

    #[test]
    fn player_space_yaw_is_the_same_flat_or_tilted() {
        // Turning about the vertical at 90 deg/s. Held flat, that is pure pad-yaw;
        // tilted 45 degrees nose-up, the same turn shows up half on yaw, half on roll.
        let flat = GravityTracker { up: [0.0, 1.0, 0.0], seeded: true };
        assert!((flat.yaw_about_up([0.0, -90.0, 0.0]) + 90.0).abs() < 1e-3);
        let h = 0.70710677;
        let tilted = GravityTracker { up: [0.0, h, h], seeded: true };
        let world = tilted.yaw_about_up([0.0, -90.0 * h, -90.0 * h]);
        assert!((world + 90.0).abs() < 0.1, "{world}");
        // local space would only have seen 64 deg/s of it
        assert!((-90.0 * h + 63.64f32).abs() < 0.1);
    }

    #[test]
    fn seeded_bias_starts_calibrated() {
        let mut c = Calibrator::default();
        c.seed([12.0, -7.5, 3.0]);
        assert_eq!(c.state, CalState::Calibrated);
        assert_eq!(c.cal.gyro_bias, [12.0, -7.5, 3.0]);
        // garbage from a damaged file is refused
        let mut c = Calibrator::default();
        c.seed([f32::NAN, 0.0, 0.0]);
        assert_eq!(c.state, CalState::Uncalibrated);
        c.seed([9000.0, 0.0, 0.0]);
        assert_eq!(c.state, CalState::Uncalibrated);
        // and a re-measurement bumps the revision so the engine re-saves it
        let t0 = Instant::now();
        let r0 = c.revision;
        for i in 0..300u64 {
            c.feed([20, -12, 5], t0 + Duration::from_millis(i * 4));
        }
        assert!(c.revision > r0);
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
