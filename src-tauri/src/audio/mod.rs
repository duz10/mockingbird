#![allow(missing_docs)] // Trait + factory; method-level docs are the API.

//! Audio capture + VAD.
//!
//! Cross-platform via the [`AudioCapture`] and
//! [`vad::VoiceActivityDetector`] traits. Windows impls land in Wave 2
//! (capture) and Wave 3 (VAD). macOS / Linux impls are `todo!()` stubs
//! per PLAN line 185 — the trait shape locks the contract so Phase 9
//! is "fill the stubs", not "rewrite the layer".

pub mod capture;
pub mod vad;
pub mod vad_trim;

pub use vad_trim::{vad_trim as trim_speech, TrimConfig};

#[cfg(not(target_os = "windows"))]
use crate::error::AppError;
use crate::error::AppResult;

/// 16 kHz mono i16 audio capture from the system default input device.
///
/// See ADR 0013 for the design rationale (frame size, ring-buffer
/// sizing, default-device-changed handling).
///
/// **Not `Send` by design.** cpal's `Stream` is `!Send` on Windows
/// (WASAPI handles are thread-bound). Phase 5 will own the recording
/// thread; until then, construct on whichever thread will drive the
/// capture lifecycle.
pub trait AudioCapture {
    /// Begin capturing from the current default input device.
    fn start(&mut self) -> AppResult<()>;

    /// Stop capturing. Idempotent.
    fn stop(&mut self) -> AppResult<()>;

    /// Drain pending samples into `buf` (appended). Returns the number
    /// of samples copied. Non-blocking; returns 0 if the buffer is empty.
    fn drain(&mut self, buf: &mut Vec<i16>) -> AppResult<usize>;

    /// Sample rate of the captured stream. Phase 2 always returns 16_000.
    fn sample_rate(&self) -> u32;

    /// Channel count of the captured stream. Phase 2 always returns 1.
    fn channels(&self) -> u16;
}

/// Construct the platform-default capture impl.
pub fn make_default_capture() -> AppResult<Box<dyn AudioCapture>> {
    #[cfg(target_os = "windows")]
    {
        Ok(Box::new(capture::CpalCapture::new()?))
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err(AppError::Audio(
            "audio capture not implemented for this platform (Phase 9 macOS/Linux)".into(),
        ))
    }
}
