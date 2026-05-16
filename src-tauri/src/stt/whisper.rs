#![allow(missing_docs)] // Public API doc-commented; private fields self-evident.

//! whisper-rs STT impl with GPU-first/CPU-fallback semantics (ADR 0011).
//!
//! Wave 4 ships **CPU-only** because `whisper-rs` 0.16's bundled
//! whisper.cpp/ggml does not build against CUDA 13.x (the only version
//! available via chocolatey at time of writing). The runtime fallback
//! in [`WhisperStt::new`] is wired in code; flip the `cuda` feature on
//! in `Cargo.toml` once CUDA 12.x is installed side-by-side, and the
//! GPU path activates automatically. See bd issue `mb-ltq`.

use std::path::{Path, PathBuf};
use std::time::Instant;

use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use super::{SpeechToText, TranscribeRequest, Transcript};
use crate::error::{AppError, AppResult};

const MODEL_FILENAME: &str = "whisper-large-v3-turbo-q5_0.bin";
const MODEL_ID: &str = "whisper-large-v3-turbo-q5_0";

#[cfg(target_os = "windows")]
pub struct WhisperStt {
    ctx: WhisperContext,
    /// Whether the loaded `ctx` was successfully initialised with GPU
    /// acceleration. Stays `false` when whisper-rs is compiled without
    /// the `cuda` feature.
    gpu_loaded: bool,
}

#[cfg(target_os = "windows")]
impl WhisperStt {
    /// GPU-first, CPU-fallback constructor.
    ///
    /// Tries `use_gpu = true` first. If whisper-rs/ggml errors during
    /// init (driver missing, OOM, no CUDA feature compiled in), retries
    /// with `use_gpu = false` and logs the downgrade. Returns Err only
    /// if even the CPU init fails (e.g. model file missing).
    pub fn new() -> AppResult<Self> {
        Self::new_with_options(false)
    }

    /// Construct with an explicit GPU/CPU preference. When `force_cpu`
    /// is true the GPU attempt is skipped entirely.
    pub fn new_with_options(force_cpu: bool) -> AppResult<Self> {
        let model = locate_whisper_model()?;
        Self::from_path(&model, force_cpu)
    }

    pub fn from_path(model_path: &Path, force_cpu: bool) -> AppResult<Self> {
        let model_str = model_path
            .to_str()
            .ok_or_else(|| AppError::Stt("non-UTF8 model path".into()))?;

        if !force_cpu {
            let mut params = WhisperContextParameters::default();
            params.use_gpu(true);
            match WhisperContext::new_with_params(model_str, params) {
                Ok(ctx) => {
                    tracing::info!(
                        target: "stt",
                        model = MODEL_ID,
                        path = %model_path.display(),
                        "Whisper loaded with GPU"
                    );
                    return Ok(Self {
                        ctx,
                        gpu_loaded: true,
                    });
                }
                Err(e) => {
                    tracing::warn!(
                        target: "stt",
                        error = %e,
                        "GPU init failed; falling back to CPU"
                    );
                }
            }
        }

        let mut params = WhisperContextParameters::default();
        params.use_gpu(false);
        let ctx = WhisperContext::new_with_params(model_str, params)
            .map_err(|e| AppError::Stt(format!("Whisper CPU init: {e}")))?;
        tracing::info!(
            target: "stt",
            model = MODEL_ID,
            path = %model_path.display(),
            "Whisper loaded with CPU"
        );
        Ok(Self {
            ctx,
            gpu_loaded: false,
        })
    }

    /// Whether the active backend is GPU. Surfaced for the
    /// `cuda-verified` Wave-5 judge + `stt_test --json` output.
    pub fn gpu_loaded(&self) -> bool {
        self.gpu_loaded
    }
}

#[cfg(target_os = "windows")]
impl SpeechToText for WhisperStt {
    fn transcribe(&mut self, req: TranscribeRequest<'_>) -> AppResult<Transcript> {
        let started = Instant::now();

        // whisper-rs takes f32 audio in [-1.0, 1.0].
        let audio_f32: Vec<f32> = req.audio.iter().map(|&s| s as f32 / 32768.0).collect();

        // If the caller asked for CPU but we loaded GPU, log the
        // mismatch — we honor whatever is loaded (per-call backend
        // switching is a Phase 5 concern; reloading whisper costs ~10s).
        if req.force_cpu && self.gpu_loaded {
            tracing::warn!(
                target: "stt",
                "TranscribeRequest.force_cpu=true but context is GPU-loaded; \
                 using GPU. Construct WhisperStt::new_with_options(true) to \
                 force CPU at load time."
            );
        }

        let mut state = self
            .ctx
            .create_state()
            .map_err(|e| AppError::Stt(format!("create_state: {e}")))?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_print_progress(false);
        params.set_print_special(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        // Wave 4 hard-codes English; Phase 6 will wire language settings.
        params.set_language(Some("en"));
        if let Some(p) = req.initial_prompt {
            params.set_initial_prompt(p);
        }

        state
            .full(params, &audio_f32)
            .map_err(|e| AppError::Stt(format!("whisper full: {e}")))?;

        // whisper-rs 0.16: `full_n_segments` returns i32 directly,
        // `get_segment(i)` returns Option<Segment>, and the segment
        // exposes the text via `to_str_lossy()` (UTF-8-safe).
        let n = state.full_n_segments();

        let mut text = String::new();
        for i in 0..n {
            let seg = state
                .get_segment(i)
                .ok_or_else(|| AppError::Stt(format!("missing segment {i}")))?;
            let chunk = seg
                .to_str_lossy()
                .map_err(|e| AppError::Stt(format!("segment text {i}: {e}")))?;
            text.push_str(&chunk);
        }

        Ok(Transcript {
            text: text.trim().to_string(),
            gpu_used: self.gpu_loaded,
            latency_ms: started.elapsed().as_millis() as u64,
            model_id: MODEL_ID.to_string(),
        })
    }
}

#[cfg(target_os = "windows")]
fn locate_whisper_model() -> AppResult<PathBuf> {
    // Honor an explicit override env first (used by stt_test --model-path).
    if let Ok(p) = std::env::var("WHISPER_MODEL_PATH") {
        let pb = PathBuf::from(p);
        if pb.is_file() {
            return Ok(pb);
        }
    }
    let dir = crate::stt::models_dir()?;
    let candidate = dir.join(MODEL_FILENAME);
    if !candidate.is_file() {
        return Err(AppError::Stt(format!(
            "Whisper model not found at {} - download from \
             https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin",
            candidate.display()
        )));
    }
    Ok(candidate)
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    fn model_available() -> bool {
        locate_whisper_model().is_ok()
    }

    #[test]
    fn force_cpu_constructs_without_gpu_attempt() {
        if !model_available() {
            eprintln!("SKIP: whisper model not on disk");
            return;
        }
        let stt = WhisperStt::new_with_options(true).expect("CPU construct");
        assert!(!stt.gpu_loaded(), "force_cpu must yield CPU-loaded context");
    }

    #[test]
    fn transcribe_silent_audio_returns_short_text() {
        if !model_available() {
            return;
        }
        let mut stt = WhisperStt::new_with_options(true).unwrap();
        let silent = vec![0i16; 16_000]; // 1 s
        let req = TranscribeRequest {
            audio: &silent,
            initial_prompt: None,
            force_cpu: true,
        };
        let tx = stt.transcribe(req).unwrap();
        // Whisper may emit special tokens like "[BLANK_AUDIO]" for
        // pure silence; assert it didn't fabricate a long sentence.
        assert!(
            tx.text.len() < 50,
            "silent audio produced unexpected text: {:?}",
            tx.text
        );
        assert_eq!(tx.model_id, MODEL_ID);
        assert!(!tx.gpu_used);
    }

    #[test]
    fn transcribe_writes_nonzero_latency() {
        if !model_available() {
            return;
        }
        let mut stt = WhisperStt::new_with_options(true).unwrap();
        let req = TranscribeRequest {
            audio: &vec![0i16; 16_000],
            initial_prompt: None,
            force_cpu: true,
        };
        let tx = stt.transcribe(req).unwrap();
        assert!(tx.latency_ms > 0, "latency_ms is zero — clock issue?");
    }

    #[test]
    fn transcribe_accepts_initial_prompt() {
        if !model_available() {
            return;
        }
        let mut stt = WhisperStt::new_with_options(true).unwrap();
        let req = TranscribeRequest {
            audio: &vec![0i16; 16_000],
            initial_prompt: Some("Hello world."),
            force_cpu: true,
        };
        let tx = stt.transcribe(req).unwrap();
        // Pure silence + a prompt shouldn't fabricate. Just verify it didn't error.
        assert_eq!(tx.model_id, MODEL_ID);
    }
}
