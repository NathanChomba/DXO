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

use crate::state::PadState;
use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

pub const PROTOCOL_VERSION: u16 = 1001;
pub const MSG_VERSION: u32 = 0x100000;
pub const MSG_PORT_INFO: u32 = 0x100001;
pub const MSG_PAD_DATA: u32 = 0x100002;

/// DualSense/DS4 raw units -> physical units. Defaults are the nominal
/// datasheet scales; TODO M5: use the per-device calibration from feature
/// report 0x05 (DualSense) / 0x02 (DS4) instead.
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
}
