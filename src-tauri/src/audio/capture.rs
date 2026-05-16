#![allow(missing_docs)] // Scaffold; Wave-2 brief will document.

//! Audio capture impl(s).
//!
//! Wave 1 ships the scaffold; Wave 2 fills in the cpal/WASAPI body
//! per ADR 0013.

use crate::error::AppResult;

#[cfg(target_os = "windows")]
pub struct CpalCapture {
    // Wave 2 — fields TBD per ADR 0013:
    //   - cpal Stream handle
    //   - SPSC producer side of the ringbuf (consumer lives behind drain())
    //   - sample-format runtime config (16 kHz mono i16 target;
    //     `rubato` resampler if the device pins to something else)
    //   - default-device-changed subscriber handle
}

#[cfg(target_os = "windows")]
impl CpalCapture {
    pub fn new() -> AppResult<Self> {
        // Wave 2 will: enumerate the default input, request 16 kHz
        // mono i16 (with rubato resample fallback), wire the cpal
        // callback into the ringbuf producer, subscribe to
        // MMNotificationClient via cpal's Host event stream.
        todo!("Phase 2 Wave 2: cpal Windows capture impl")
    }
}

#[cfg(target_os = "windows")]
impl super::AudioCapture for CpalCapture {
    fn start(&mut self) -> AppResult<()> {
        todo!("Phase 2 Wave 2: start cpal stream")
    }
    fn stop(&mut self) -> AppResult<()> {
        todo!("Phase 2 Wave 2: stop cpal stream")
    }
    fn drain(&mut self, _buf: &mut Vec<i16>) -> AppResult<usize> {
        todo!("Phase 2 Wave 2: drain ringbuf into buf")
    }
    fn sample_rate(&self) -> u32 {
        16_000
    }
    fn channels(&self) -> u16 {
        1
    }
}

// macOS / Linux stubs (Phase 9) — the trait impl is gated by cfg and
// the factory in `mod.rs` returns AppError::Audio for non-Windows hosts.
