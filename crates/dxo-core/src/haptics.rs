//! M6 (stretch): audio-driven haptics and Bluetooth audio.
//!
//! Plan, in order of difficulty:
//!
//! 1. Rumble-from-audio (any pad, USB or BT)
//!    WASAPI loopback capture of the default render device -> band-pass
//!    20-150 Hz -> envelope follower -> `OutputState::rumble_*`. Cheapest win;
//!    `EnvelopeFollower` below is the DSP core.
//!
//! 2. DualSense voice-coil haptics over USB
//!    The pad exposes a 4-channel 48 kHz audio endpoint; channels 3/4 drive
//!    the actuators. Render the band-passed signal into ch 3/4 of that
//!    endpoint with WASAPI (exclusive or shared 4ch). Also: use
//!    IMMNotificationClient to stop Windows auto-switching default playback
//!    to the controller on plug-in.
//!
//! 3. Bluetooth haptics / speaker / mic
//!    Reverse-engineered territory (SBC frames inside large HID output
//!    reports). Treat as research, not a feature. Read existing open-source
//!    work first before writing code here.
//!
//! Crates to evaluate when starting: `wasapi` (loopback capture),
//! `biquad` (filters), `windows` (IMMDeviceEnumerator / IMMNotificationClient).

/// One-pole attack/release envelope follower: audio samples in, 0..=1 out.
pub struct EnvelopeFollower {
    attack: f32,
    release: f32,
    env: f32,
}

impl EnvelopeFollower {
    pub fn new(sample_rate: f32, attack_ms: f32, release_ms: f32) -> Self {
        let coef = |ms: f32| (-1.0 / (sample_rate * ms / 1000.0)).exp();
        Self { attack: coef(attack_ms), release: coef(release_ms), env: 0.0 }
    }

    pub fn process(&mut self, x: f32) -> f32 {
        let x = x.abs();
        let c = if x > self.env { self.attack } else { self.release };
        self.env = c * self.env + (1.0 - c) * x;
        self.env
    }

    /// Envelope (0..=1) -> motor strength with a noise gate and gain.
    pub fn to_motor(env: f32, gate: f32, gain: f32) -> u8 {
        if env < gate {
            0
        } else {
            ((env - gate) * gain * 255.0).clamp(0.0, 255.0) as u8
        }
    }
}

/// Placeholder so the engine can hold a haptics source today.
pub trait HapticsSource: Send {
    /// (left/heavy, right/light) motor 0..=255
    fn current(&mut self) -> (u8, u8);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_rises_and_falls() {
        let mut e = EnvelopeFollower::new(48_000.0, 5.0, 50.0);
        let mut v = 0.0;
        for _ in 0..4800 {
            v = e.process(1.0);
        }
        assert!(v > 0.9);
        for _ in 0..48_000 {
            v = e.process(0.0);
        }
        assert!(v < 0.01);
    }
}
