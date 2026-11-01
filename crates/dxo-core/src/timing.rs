//! M1/M3: precise loop pacing.
//!
//! The main loop is mostly paced by `hid.read_timeout` blocking until the pad
//! sends a report (DualSense USB ~250 Hz by default, BT lower). This ticker is
//! for work that must run on a fixed cadence regardless of input: output
//! keep-alives, gyro integration, macros.
//!
//! On Windows it uses a high-resolution waitable timer
//! (CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, Win10 1803+), which gets ~0.5 ms
//! precision WITHOUT changing the global timer resolution via
//! timeBeginPeriod / NtSetTimerResolution.

use std::time::{Duration, Instant};

pub struct Ticker {
    period: Duration,
    next: Instant,
    #[cfg(windows)]
    timer: win::Timer,
}

impl Ticker {
    pub fn new(period: Duration) -> Self {
        Self {
            period,
            next: Instant::now() + period,
            #[cfg(windows)]
            timer: win::Timer::new(),
        }
    }

    /// Sleep until the next tick. Returns how late we woke (jitter).
    pub fn wait(&mut self) -> Duration {
        let now = Instant::now();
        if self.next > now {
            let d = self.next - now;
            #[cfg(windows)]
            self.timer.sleep(d);
            #[cfg(not(windows))]
            std::thread::sleep(d);
        }
        let woke = Instant::now();
        let late = woke.saturating_duration_since(self.next);
        self.next += self.period;
        // If we fell far behind, resync instead of bursting to catch up.
        if woke > self.next + self.period * 4 {
            self.next = woke + self.period;
        }
        late
    }
}

#[cfg(windows)]
mod win {
    use std::time::Duration;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::Threading::{
        CreateWaitableTimerExW, SetWaitableTimer, WaitForSingleObject, CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
        INFINITE, TIMER_ALL_ACCESS,
    };

    pub struct Timer(HANDLE);
    unsafe impl Send for Timer {}

    impl Timer {
        pub fn new() -> Self {
            let h = unsafe {
                CreateWaitableTimerExW(
                    std::ptr::null(),
                    std::ptr::null(),
                    CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
                    TIMER_ALL_ACCESS,
                )
            };
            Timer(h)
        }

        pub fn sleep(&self, d: Duration) {
            if self.0.is_null() {
                std::thread::sleep(d);
                return;
            }
            // Negative = relative time in 100 ns units.
            let due: i64 = -((d.as_nanos() / 100) as i64).max(1);
            unsafe {
                SetWaitableTimer(self.0, &due, 0, None, std::ptr::null(), 0);
                WaitForSingleObject(self.0, INFINITE);
            }
        }
    }

    impl Drop for Timer {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { CloseHandle(self.0) };
            }
        }
    }
}
