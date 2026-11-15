//! Audio-driven haptics.
//!
//! 1. Rumble from sound (any pad, USB or Bluetooth) - BUILT, opt-in:
//!    WASAPI loopback capture of the default output -> low-pass 160 Hz ->
//!    envelope follower -> the rumble motors. The DSP (`BassMeter`) is always
//!    compiled and unit-tested; the capture itself needs the `audio-haptics`
//!    cargo feature (it pulls in the `cpal` crate):
//!        cargo build --release --features dxo-core/audio-haptics
//!    or `scripts\build.ps1 -AudioHaptics`.
//!
//! 2. DualSense voice-coil haptics over USB - not yet:
//!    The pad exposes a 4-channel 48 kHz audio endpoint; channels 3/4 drive
//!    the actuators. Render the low-passed signal into ch 3/4 of that
//!    endpoint. Also: stop Windows auto-switching default playback to the
//!    controller on plug-in. Needs a DualSense on the desk to get right.
//!
//! 3. Bluetooth haptics / speaker / mic - research only (SBC frames inside
//!    large HID output reports).

/// One-pole attack/release envelope follower: audio samples in, 0..=1 out.
pub struct EnvelopeFollower {
    attack: f32,
    release: f32,
    pub(crate) env: f32,
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

/// Second-order low-pass (RBJ cookbook biquad): keeps the bass, drops the rest.
pub struct LowPass {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl LowPass {
    pub fn new(sample_rate: f32, cutoff_hz: f32) -> Self {
        let w = std::f32::consts::TAU * (cutoff_hz / sample_rate).clamp(0.0001, 0.49);
        let (sin, cos) = w.sin_cos();
        let alpha = sin / (2.0 * std::f32::consts::FRAC_1_SQRT_2); // Q = 0.707: no resonant bump
        let a0 = 1.0 + alpha;
        Self {
            b0: (1.0 - cos) / 2.0 / a0,
            b1: (1.0 - cos) / a0,
            b2: (1.0 - cos) / 2.0 / a0,
            a1: -2.0 * cos / a0,
            a2: (1.0 - alpha) / a0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    pub fn process(&mut self, x: f32) -> f32 {
        // transposed direct form II
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

/// Sound in, "how hard should the controller shake" out (0..=255).
/// Bass (explosions, engines, kick drums) drives it; speech and music's
/// upper range mostly don't.
pub struct BassMeter {
    low: LowPass,
    env: EnvelopeFollower,
}

impl BassMeter {
    /// Below this the controller stays still (hiss, quiet menus).
    const GATE: f32 = 0.015;
    /// Full-scale bass = full rumble well before clipping.
    const GAIN: f32 = 4.0;

    pub fn new(sample_rate: f32) -> Self {
        Self { low: LowPass::new(sample_rate, 160.0), env: EnvelopeFollower::new(sample_rate, 4.0, 90.0) }
    }

    /// Feed one block of interleaved samples (`channels` per frame; the front
    /// left/right pair is what's listened to). Returns the level after it.
    pub fn block(&mut self, samples: &[f32], channels: usize) -> u8 {
        let ch = channels.max(1);
        let mut env = self.env.env;
        for frame in samples.chunks_exact(ch) {
            let mono = if ch >= 2 { (frame[0] + frame[1]) * 0.5 } else { frame[0] };
            env = self.env.process(self.low.process(mono));
        }
        EnvelopeFollower::to_motor(env, Self::GATE, Self::GAIN)
    }
}

/// Is this engine built with sound capture (`--features audio-haptics`)?
pub const AVAILABLE: bool = cfg!(feature = "audio-haptics");

/// Listens to whatever the PC is playing (the default output device) and
/// keeps a shared "bass level" up to date for the controller workers.
/// Without the `audio-haptics` build feature it does nothing.
#[derive(Default)]
pub struct AudioListener {
    #[cfg(feature = "audio-haptics")]
    running: Option<capture::Capture>,
}

impl AudioListener {
    /// Start or stop listening. Cheap to call repeatedly.
    pub fn set_running(&mut self, wanted: bool, level: &std::sync::Arc<std::sync::atomic::AtomicU8>) {
        #[cfg(feature = "audio-haptics")]
        {
            if wanted && self.running.is_none() {
                self.running = Some(capture::Capture::start(level.clone()));
            } else if !wanted && self.running.is_some() {
                self.running = None; // dropping it stops the thread
                level.store(0, std::sync::atomic::Ordering::Relaxed);
            }
        }
        #[cfg(not(feature = "audio-haptics"))]
        {
            let _ = (wanted, level);
        }
    }
}

#[cfg(feature = "audio-haptics")]
mod capture {
    //! WASAPI loopback through cpal: opening an INPUT stream on the default
    //! OUTPUT device records what that device is playing.
    use super::BassMeter;
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
    use std::sync::Arc;
    use std::thread::JoinHandle;
    use std::time::Duration;

    pub struct Capture {
        stop: Arc<AtomicBool>,
        thread: Option<JoinHandle<()>>,
    }

    impl Capture {
        pub fn start(level: Arc<AtomicU8>) -> Capture {
            let stop = Arc::new(AtomicBool::new(false));
            let stop2 = stop.clone();
            // The stream isn't `Send`, so it lives and dies on its own thread.
            let thread = std::thread::Builder::new()
                .name("dxo-audio".into())
                .spawn(move || {
                    if let Err(e) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(&level, &stop2))) {
                        log::warn!("audio haptics stopped: {e:?}");
                    }
                    level.store(0, Ordering::Relaxed);
                })
                .ok();
            Capture { stop, thread }
        }
    }

    impl Drop for Capture {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Release);
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }

    fn run(level: &Arc<AtomicU8>, stop: &AtomicBool) {
        let host = cpal::default_host();
        let Some(dev) = host.default_output_device() else {
            log::warn!("audio haptics: no sound output device");
            return;
        };
        let cfg = match dev.default_output_config() {
            Ok(c) => c,
            Err(e) => {
                log::warn!("audio haptics: {e}");
                return;
            }
        };
        let channels = cfg.channels() as usize;
        let mut meter = BassMeter::new(cfg.sample_rate().0 as f32);
        let on_error = |e: cpal::StreamError| log::warn!("audio haptics: {e}");
        // Counts callbacks, so the loop below can tell when sound has stopped arriving.
        let beats = Arc::new(AtomicU32::new(0));
        let (lvl, beat) = (level.clone(), beats.clone());
        let stream = match cfg.sample_format() {
            cpal::SampleFormat::F32 => dev.build_input_stream(
                &cfg.config(),
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    lvl.store(meter.block(data, channels), Ordering::Relaxed);
                    beat.fetch_add(1, Ordering::Relaxed);
                },
                on_error,
                None,
            ),
            cpal::SampleFormat::I16 => {
                let mut scratch: Vec<f32> = Vec::new();
                dev.build_input_stream(
                    &cfg.config(),
                    move |data: &[i16], _: &cpal::InputCallbackInfo| {
                        scratch.clear();
                        scratch.extend(data.iter().map(|&v| v as f32 / 32768.0));
                        lvl.store(meter.block(&scratch, channels), Ordering::Relaxed);
                        beat.fetch_add(1, Ordering::Relaxed);
                    },
                    on_error,
                    None,
                )
            }
            other => {
                log::warn!("audio haptics: unsupported sample format {other:?}");
                return;
            }
        };
        let stream = match stream {
            Ok(s) => s,
            Err(e) => {
                log::warn!("audio haptics: {e}");
                return;
            }
        };
        if let Err(e) = stream.play() {
            log::warn!("audio haptics: {e}");
            return;
        }
        log::info!("audio haptics: listening to {}", dev.name().unwrap_or_default());
        // Loopback delivers nothing at all while the PC is silent, so the last
        // level would otherwise stick: if no sound has arrived for 150 ms, it's zero.
        let (mut seen, mut idle) = (0u32, 0u32);
        while !stop.load(Ordering::Acquire) {
            std::thread::sleep(Duration::from_millis(50));
            let now = beats.load(Ordering::Relaxed);
            idle = if now == seen { idle + 1 } else { 0 };
            seen = now;
            if idle >= 3 {
                level.store(0, Ordering::Relaxed);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32, amp: f32, n: usize) -> Vec<f32> {
        // stereo, 48 kHz
        (0..n).flat_map(|i| {
            let v = amp * (std::f32::consts::TAU * freq * i as f32 / 48_000.0).sin();
            [v, v]
        }).collect()
    }

    #[test]
    fn bass_shakes_and_treble_does_not() {
        let mut m = BassMeter::new(48_000.0);
        let bass = m.block(&sine(60.0, 0.5, 9600), 2);
        assert!(bass > 150, "a loud 60 Hz rumble should drive the motor hard, got {bass}");

        let mut m = BassMeter::new(48_000.0);
        let treble = m.block(&sine(3000.0, 0.5, 9600), 2);
        assert!(treble < 10, "3 kHz at the same volume should barely register, got {treble}");

        let mut m = BassMeter::new(48_000.0);
        assert_eq!(m.block(&sine(60.0, 0.005, 9600), 2), 0, "very quiet bass stays under the gate");
    }

    #[test]
    fn level_falls_back_after_the_sound_stops() {
        let mut m = BassMeter::new(48_000.0);
        assert!(m.block(&sine(80.0, 0.6, 4800), 2) > 100);
        assert_eq!(m.block(&vec![0.0; 2 * 48_000], 2), 0, "a second of silence = still");
    }

    #[test]
    fn surround_and_mono_layouts_are_handled() {
        let mut m = BassMeter::new(48_000.0);
        // 6 channels: only front left/right are read; an odd tail is ignored
        let mut six = vec![0.0f32; 6 * 4800 + 3];
        for (i, f) in six.chunks_exact_mut(6).enumerate() {
            let v = 0.5 * (std::f32::consts::TAU * 50.0 * i as f32 / 48_000.0).sin();
            (f[0], f[1]) = (v, v);
        }
        assert!(m.block(&six, 6) > 100);
        let mut m = BassMeter::new(48_000.0);
        let mono: Vec<f32> = (0..4800).map(|i| 0.5 * (std::f32::consts::TAU * 50.0 * i as f32 / 48_000.0).sin()).collect();
        assert!(m.block(&mono, 1) > 100);
        assert_eq!(BassMeter::new(48_000.0).block(&[], 0), 0);
    }

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
