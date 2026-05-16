#![allow(missing_docs)] // Scaffold; Wave-3 brief will document.

//! Voice Activity Detection — Silero ONNX via the `ort` crate.
//!
//! Wave 1 ships the trait scaffold; Wave 3 fills in the ort wiring
//! per ADR 0012.

#[cfg(not(target_os = "windows"))]
use crate::error::AppError;
use crate::error::AppResult;

/// Per-frame VAD output. `confidence` is the model's probability that
/// the 30 ms frame contains speech.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VadFrame {
    pub is_speech: bool,
    pub confidence: f32,
}

/// Stateful VAD that processes 30 ms frames of 16 kHz mono PCM.
pub trait VoiceActivityDetector: Send {
    /// Score one 30 ms frame (480 samples at 16 kHz). Returns the
    /// model's per-frame speech probability + thresholded decision.
    fn process_frame(&mut self, frame: &[i16]) -> AppResult<VadFrame>;

    /// Reset internal state (useful between utterances).
    fn reset(&mut self);
}

/// Construct the default VAD impl. Loads `silero_vad.onnx` from the
/// path returned by `stt::models_dir()`.
pub fn make_default_vad() -> AppResult<Box<dyn VoiceActivityDetector>> {
    #[cfg(target_os = "windows")]
    {
        Ok(Box::new(SileroVad::new()?))
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err(AppError::Audio(
            "VAD not implemented for this platform (Phase 9 macOS/Linux)".into(),
        ))
    }
}

#[cfg(target_os = "windows")]
pub struct SileroVad {
    // Wave 3 — fields TBD per ADR 0012:
    //   - ort Session (or whatever ort v2 calls its inference handle)
    //   - threshold (f32, default 0.5)
    //   - LSTM hidden state for the streaming variant of Silero
}

#[cfg(target_os = "windows")]
impl SileroVad {
    pub fn new() -> AppResult<Self> {
        todo!("Phase 2 Wave 3: load silero_vad.onnx via ort and warm up")
    }
}

#[cfg(target_os = "windows")]
impl VoiceActivityDetector for SileroVad {
    fn process_frame(&mut self, _frame: &[i16]) -> AppResult<VadFrame> {
        todo!("Phase 2 Wave 3: feed frame through Silero, threshold, return VadFrame")
    }
    fn reset(&mut self) {
        // Wave 3: clear LSTM hidden state
    }
}
