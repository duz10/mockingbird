//! macOS dictation-backend un-gating judge
//! (mb-mac-v1.4.7a / judge `mac-p3-dictation-backends-ungated`).
//!
//! Phase 3 `.4.7a` widens the audio-capture, VAD, and STT `#[cfg]`
//! gates from Windows-only to `any(windows, macos)`, so the three
//! `make_default_*` factories construct real backends on macOS. This
//! probe is the in-tree proof of that un-gating:
//!
//!   1. [`crate::audio::make_default_capture`] returns `Ok` (a real
//!      `CpalCapture` over the CoreAudio mic — constructed only, never
//!      `start()`ed, so no Microphone grant is needed here).
//!   2. [`crate::audio::vad::make_default_vad`] returns `Ok` (a real
//!      Silero `ort` session — exercises the macOS `libonnxruntime.dylib`
//!      discovery wired into `ensure_ort_dylib_set`).
//!   3. [`crate::stt::make_default_stt`] returns `Ok`, AND the PRODUCTION
//!      [`crate::stt::whisper::WhisperStt`] it builds transcribes
//!      whisper.cpp's `jfk.wav` to a NON-EMPTY transcript via a
//!      CONFIRMED Metal backend (not a silent CPU fallback).
//!
//! The Metal confirmation reuses [`crate::stt::judges_macos_v1`]'s
//! ggml/whisper log-capture trampoline + classifier (DRY) rather than
//! re-implementing backend detection. Unlike `metal_transcript_probe`
//! (which drives a raw `WhisperContext`), this probe goes through the
//! real `make_default_stt` -> `WhisperStt::new` production path — the
//! one the dictation runtime will spawn in `.4.7c`.
//!
//! Gated on macOS + the `metal` feature (the backend confirmation needs
//! `whisper-rs/raw-api`, which `metal` pulls in); compiles to nothing
//! on every other config so the default cross-platform build stays green.
#![cfg(all(target_os = "macos", feature = "metal"))]

use std::path::Path;

use crate::stt::judges_macos_v1::Backend;
use crate::stt::TranscribeRequest;

/// Outcome of the `.4.7a` dictation-backend un-gate probe.
#[derive(Debug, Clone)]
pub struct UngateReport {
    /// `make_default_capture()` returned `Ok`.
    pub capture_ok: bool,
    /// `make_default_vad()` returned `Ok`.
    pub vad_ok: bool,
    /// `make_default_stt()` returned `Ok`.
    pub stt_ok: bool,
    /// Trimmed transcript of `jfk.wav` through the production WhisperStt.
    pub transcript: String,
    /// Whether the production `Transcript.gpu_used` flag came back true.
    pub gpu_used: bool,
    /// Classified backend from the captured ggml/whisper log.
    pub backend: Backend,
    /// The exact log line that proved the backend, if any.
    pub backend_evidence: Option<String>,
    /// End-to-end transcribe latency (ms).
    pub latency_ms: u64,
    /// Full captured ggml/whisper log (diagnostics).
    pub log: String,
}

/// Construct each dictation backend via its `make_default_*` factory and
/// transcribe `wav_path` through the production STT wrapper.
///
/// `model_path` pins the GGUF whisper model (the probe sets
/// `WHISPER_MODEL_PATH` so the production `WhisperStt::new` locator finds
/// it). Returns `Err` on any hard failure (factory `Err`, model/wav
/// missing, inference error). A successful return still requires the
/// caller (the judge shim) to assert `transcript` is non-empty and
/// `backend == Metal`.
pub fn ungate_backends_probe(model_path: &Path, wav_path: &Path) -> Result<UngateReport, String> {
    // 1. Audio capture factory. Construction only — opening the device
    //    (start()) needs a Microphone grant and is out of scope for the
    //    construct-and-assert-Ok contract.
    let _capture =
        crate::audio::make_default_capture().map_err(|e| format!("make_default_capture: {e}"))?;
    let capture_ok = true;

    // 2. VAD factory — builds a real Silero ort session (dylib discovery).
    let _vad =
        crate::audio::vad::make_default_vad().map_err(|e| format!("make_default_vad: {e}"))?;
    let vad_ok = true;

    // 3. STT factory + production WhisperStt over jfk.wav, with Metal
    //    confirmation. Begin capturing the backend-init log BEFORE the
    //    factory builds the WhisperContext.
    crate::stt::judges_macos_v1::begin_backend_capture();

    if !model_path.is_file() {
        return Err(format!(
            "whisper model not found at {} (run scripts/download-models.sh)",
            model_path.display()
        ));
    }
    // SAFETY: probe runs single-threaded at startup; no other thread is
    // reading WHISPER_MODEL_PATH yet. This routes make_default_stt's
    // production locator at the pinned model.
    std::env::set_var("WHISPER_MODEL_PATH", model_path);

    let mut stt = crate::stt::make_default_stt().map_err(|e| format!("make_default_stt: {e}"))?;
    let stt_ok = true;

    let audio = crate::stt::judges_macos_v1::read_wav_16k_mono_i16(wav_path)?;
    let req = TranscribeRequest {
        audio: &audio,
        initial_prompt: None,
        force_cpu: false,
    };
    let transcript = stt
        .transcribe(req)
        .map_err(|e| format!("WhisperStt::transcribe: {e}"))?;

    let (backend, backend_evidence, log) = crate::stt::judges_macos_v1::classify_captured_backend();

    Ok(UngateReport {
        capture_ok,
        vad_ok,
        stt_ok,
        transcript: transcript.text.trim().to_string(),
        gpu_used: transcript.gpu_used,
        backend,
        backend_evidence,
        latency_ms: transcript.latency_ms,
        log,
    })
}
