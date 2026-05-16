#![allow(missing_docs)] // Trait + factory + helper; method-level docs are the API.

//! Speech-to-text — Whisper via whisper-rs.
//!
//! Wave 1 ships the trait + the `models_dir()` resolver (binding for
//! Wave 3 VAD too). Wave 4 fills in whisper-rs CUDA + CPU fallback
//! per ADR 0011.

pub mod prompt_builder;
pub mod whisper;

use std::path::PathBuf;

#[cfg(not(target_os = "windows"))]
use crate::error::AppError;
#[cfg(target_os = "windows")]
use crate::error::AppError;
use crate::error::AppResult;

/// One STT pass output.
#[derive(Debug, Clone)]
pub struct Transcript {
    pub text: String,
    /// Whether GPU (CUDA) was used. Logged + asserted by the
    /// Wave-5 `cuda-verified` judge.
    pub gpu_used: bool,
    /// End-to-end latency for this transcribe call.
    pub latency_ms: u64,
    /// Which Whisper model produced this. Recorded in
    /// `transcripts.model_used`.
    pub model_id: String,
}

/// Transcription request.
#[derive(Debug, Clone)]
pub struct TranscribeRequest<'a> {
    /// 16 kHz mono i16 PCM, already VAD-trimmed.
    pub audio: &'a [i16],
    /// Optional 224-token `initial_prompt` from
    /// [`prompt_builder::build_prompt`]. Whisper's prompt cap.
    pub initial_prompt: Option<&'a str>,
    /// Force CPU even if CUDA is available (CLI / test path).
    pub force_cpu: bool,
}

pub trait SpeechToText: Send {
    fn transcribe(&mut self, req: TranscribeRequest<'_>) -> AppResult<Transcript>;
}

/// Construct the platform-default STT impl.
pub fn make_default_stt() -> AppResult<Box<dyn SpeechToText>> {
    #[cfg(target_os = "windows")]
    {
        Ok(Box::new(whisper::WhisperStt::new()?))
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err(AppError::Stt(
            "STT not implemented for this platform (Phase 9 macOS/Linux)".into(),
        ))
    }
}

/// Resolve the directory containing on-disk ML model files.
///
/// Resolution order (per ADR 0014):
///   1. `MODEL_PATH` env var (absolute path; dev override)
///   2. `<exe_dir>/models/` (portable install)
///   3. `%LOCALAPPDATA%\Mockingbird\models\` (release default; Windows)
///
/// Returns `Err(AppError::Stt)` if none resolve. Caller is responsible
/// for verifying the directory actually contains the model it wants —
/// this function only locates the directory.
pub fn models_dir() -> AppResult<PathBuf> {
    if let Ok(p) = std::env::var("MODEL_PATH") {
        return Ok(PathBuf::from(p));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let candidate = parent.join("models");
            if candidate.is_dir() {
                return Ok(candidate);
            }
        }
    }
    #[cfg(target_os = "windows")]
    if let Ok(localappdata) = std::env::var("LOCALAPPDATA") {
        let path = PathBuf::from(localappdata)
            .join("Mockingbird")
            .join("models");
        return Ok(path);
    }
    Err(AppError::Stt(
        "could not resolve models directory (set MODEL_PATH or run on Windows)".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn models_dir_honors_model_path_env() {
        let prev = std::env::var("MODEL_PATH").ok();
        // SAFETY: tests run single-threaded for env mutation via
        // `cargo test -- --test-threads=1` if you have other env-touching
        // tests. Phase 1 didn't have any; this is currently safe.
        std::env::set_var("MODEL_PATH", "C:\\custom\\models");
        let result = models_dir().unwrap();
        assert_eq!(result, PathBuf::from("C:\\custom\\models"));
        // Restore prior state.
        match prev {
            Some(v) => std::env::set_var("MODEL_PATH", v),
            None => std::env::remove_var("MODEL_PATH"),
        }
    }
}
