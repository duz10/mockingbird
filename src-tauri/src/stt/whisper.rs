#![allow(missing_docs)] // Scaffold; Wave-4 brief will document.

//! whisper-rs CUDA + CPU fallback impl.
//!
//! Wave 1 ships the scaffold; Wave 4 fills in the body per ADR 0011.
//! **Requires cmake + CUDA Toolkit on PATH at build time** once the
//! `whisper-rs` dep is enabled in `Cargo.toml`.

use super::{SpeechToText, TranscribeRequest, Transcript};
use crate::error::AppResult;

#[cfg(target_os = "windows")]
pub struct WhisperStt {
    // Wave 4 — fields TBD per ADR 0011:
    //   - WhisperContext (whisper-rs handle; carries the loaded model)
    //   - whether the active backend is CUDA or CPU
    //   - model_id string for Transcript.model_used
}

#[cfg(target_os = "windows")]
impl WhisperStt {
    pub fn new() -> AppResult<Self> {
        // Wave 4 will:
        //   1. Locate model via stt::models_dir() (Wave 1 ready)
        //   2. Try WhisperContext::new_with_params with use_gpu=true
        //   3. On CUDA error, tracing::warn! and retry with use_gpu=false
        //   4. Log GPU/CUDA init line for the cuda-verified judge
        todo!("Phase 2 Wave 4: whisper-rs context init + CPU fallback")
    }
}

#[cfg(target_os = "windows")]
impl SpeechToText for WhisperStt {
    fn transcribe(&mut self, _req: TranscribeRequest<'_>) -> AppResult<Transcript> {
        todo!("Phase 2 Wave 4: whisper-rs transcribe + latency timing")
    }
}
