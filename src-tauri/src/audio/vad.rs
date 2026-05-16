#![allow(missing_docs)] // Brief documents the API.

//! Voice Activity Detection — Silero ONNX via the `ort` crate.
//!
//! See ADR 0012 (ort runtime choice) and `docs/phases/phase2-wave3-brief.md`
//! for design notes. Silero v5 expects:
//!   - `input`: f32 [batch, 512]  — 16 kHz mono PCM in `[-1.0, 1.0]`
//!   - `state`: f32 [2, batch, 128] — LSTM h+c stacked
//!   - `sr`:    i64 scalar          — `16000`
//!
//! Outputs the speech probability + updated state. Probabilities ≥ 0.5
//! are flagged as speech.

use std::path::{Path, PathBuf};

#[cfg(not(target_os = "windows"))]
use crate::error::AppError;
#[cfg(target_os = "windows")]
use crate::error::AppError;
use crate::error::AppResult;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VadFrame {
    pub is_speech: bool,
    pub confidence: f32,
}

pub trait VoiceActivityDetector: Send {
    /// Score one frame of size [`frame_samples()`]. Returns the
    /// model's per-frame speech probability + thresholded decision.
    fn process_frame(&mut self, frame: &[i16]) -> AppResult<VadFrame>;

    /// Reset internal state (use between utterances).
    fn reset(&mut self);

    /// Required input frame size in samples. Silero v5 = 512.
    fn frame_samples(&self) -> usize;
}

/// Construct the platform-default VAD impl.
pub fn make_default_vad() -> AppResult<Box<dyn VoiceActivityDetector>> {
    #[cfg(target_os = "windows")]
    {
        Ok(Box::new(SileroVad::new()?))
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err(AppError::Audio(
            "VAD not implemented for this platform (Phase 9)".into(),
        ))
    }
}

#[cfg(target_os = "windows")]
pub const SILERO_FRAME_SAMPLES: usize = 512;
#[cfg(target_os = "windows")]
const SILERO_STATE_LEN: usize = 2 * 128; // (h+c stacked, batch=1, hidden=128)
#[cfg(target_os = "windows")]
const SILERO_SR: i64 = 16_000;
#[cfg(target_os = "windows")]
const SPEECH_THRESHOLD: f32 = 0.5;

#[cfg(target_os = "windows")]
pub struct SileroVad {
    session: ort::session::Session,
    /// LSTM hidden+cell stacked flat as [2 * 1 * 128].
    state: Vec<f32>,
}

#[cfg(target_os = "windows")]
impl SileroVad {
    pub fn new() -> AppResult<Self> {
        let model_path = locate_model().ok_or_else(|| {
            AppError::Audio(
                "silero_vad.onnx not found — set SILERO_VAD_PATH or run \
                 `scripts/download-models.ps1`"
                    .into(),
            )
        })?;
        Self::from_path(&model_path)
    }

    pub fn from_path(path: &Path) -> AppResult<Self> {
        use ort::session::builder::GraphOptimizationLevel;
        let session = ort::session::Session::builder()
            .map_err(|e| AppError::Audio(format!("ort Session::builder: {e}")))?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| AppError::Audio(format!("set optimization level: {e}")))?
            .commit_from_file(path)
            .map_err(|e| AppError::Audio(format!("load model {}: {e}", path.display())))?;

        Ok(Self {
            session,
            state: vec![0.0f32; SILERO_STATE_LEN],
        })
    }
}

#[cfg(target_os = "windows")]
fn locate_model() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("SILERO_VAD_PATH") {
        let pb = PathBuf::from(p);
        if pb.is_file() {
            return Some(pb);
        }
    }
    if let Ok(dir) = crate::stt::models_dir() {
        let candidate = dir.join("silero_vad.onnx");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(target_os = "windows")]
impl VoiceActivityDetector for SileroVad {
    fn process_frame(&mut self, frame: &[i16]) -> AppResult<VadFrame> {
        if frame.len() != SILERO_FRAME_SAMPLES {
            return Err(AppError::Audio(format!(
                "Silero expects {SILERO_FRAME_SAMPLES} samples, got {}",
                frame.len()
            )));
        }

        // i16 → f32 in [-1.0, 1.0]. Divide by 32768.0 (i16::MIN.abs())
        // for symmetric mapping; the +1 asymmetry of i16::MAX is fine.
        let audio_f32: Vec<f32> = frame.iter().map(|&s| s as f32 / 32768.0).collect();

        // Build input tensors. ort rc.12: ort::value::Tensor::from_array
        // takes (shape, Vec<T>) where shape is `Vec<i64>` or `[usize; N]`.
        let input_t = ort::value::Tensor::from_array(([1_usize, SILERO_FRAME_SAMPLES], audio_f32))
            .map_err(|e| AppError::Audio(format!("build input tensor: {e}")))?;

        let state_t =
            ort::value::Tensor::from_array(([2_usize, 1_usize, 128_usize], self.state.clone()))
                .map_err(|e| AppError::Audio(format!("build state tensor: {e}")))?;

        let sr_t = ort::value::Tensor::from_array(([1_usize], vec![SILERO_SR]))
            .map_err(|e| AppError::Audio(format!("build sr tensor: {e}")))?;

        let outputs = self
            .session
            .run(ort::inputs! {
                "input" => input_t,
                "state" => state_t,
                "sr" => sr_t,
            })
            .map_err(|e| AppError::Audio(format!("ort run: {e}")))?;

        // The model emits `output` (speech probability) and `stateN`
        // (next state). Names verified against the downloaded model
        // 2026-05-16.
        let (_, prob_data) = outputs["output"]
            .try_extract_tensor::<f32>()
            .map_err(|e| AppError::Audio(format!("extract output: {e}")))?;
        if prob_data.is_empty() {
            return Err(AppError::Audio("empty `output` tensor".into()));
        }
        let confidence = prob_data[0];

        let (_, new_state) = outputs["stateN"]
            .try_extract_tensor::<f32>()
            .map_err(|e| AppError::Audio(format!("extract stateN: {e}")))?;
        if new_state.len() != SILERO_STATE_LEN {
            return Err(AppError::Audio(format!(
                "stateN length {} != expected {SILERO_STATE_LEN}",
                new_state.len()
            )));
        }
        self.state.clear();
        self.state.extend_from_slice(new_state);

        Ok(VadFrame {
            is_speech: confidence >= SPEECH_THRESHOLD,
            confidence,
        })
    }

    fn reset(&mut self) {
        self.state.iter_mut().for_each(|x| *x = 0.0);
    }

    fn frame_samples(&self) -> usize {
        SILERO_FRAME_SAMPLES
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    fn silero_path() -> Option<PathBuf> {
        locate_model()
    }

    #[test]
    fn frame_samples_constant_is_512() {
        assert_eq!(SILERO_FRAME_SAMPLES, 512);
    }

    #[test]
    fn process_frame_rejects_wrong_size_input() {
        let Some(path) = silero_path() else {
            eprintln!("SKIP: silero_vad.onnx not on disk");
            return;
        };
        let mut vad = SileroVad::from_path(&path).unwrap();
        let short = vec![0i16; 256];
        let err = vad.process_frame(&short).unwrap_err();
        assert!(err.to_string().contains("Silero expects 512"));
    }

    #[test]
    fn silence_scores_low() {
        let Some(path) = silero_path() else {
            return;
        };
        let mut vad = SileroVad::from_path(&path).unwrap();
        let silent = vec![0i16; SILERO_FRAME_SAMPLES];
        // Run a few frames to let any startup state settle.
        for _ in 0..3 {
            let f = vad.process_frame(&silent).unwrap();
            assert!(
                !f.is_speech,
                "silence flagged as speech with confidence {}",
                f.confidence
            );
            assert!(
                f.confidence < SPEECH_THRESHOLD,
                "silence confidence {} >= threshold",
                f.confidence
            );
        }
    }

    #[test]
    fn reset_zeros_state_and_repeats_output() {
        let Some(path) = silero_path() else {
            return;
        };
        let mut vad = SileroVad::from_path(&path).unwrap();
        let frame = vec![1000i16; SILERO_FRAME_SAMPLES];
        let first = vad.process_frame(&frame).unwrap();
        // Advance state by feeding a few more frames.
        for _ in 0..3 {
            let _ = vad.process_frame(&frame).unwrap();
        }
        vad.reset();
        let after_reset = vad.process_frame(&frame).unwrap();
        // After reset, processing the same frame should produce the same
        // confidence as the first call (LSTM state was zeros both times).
        assert!(
            (first.confidence - after_reset.confidence).abs() < 1e-4,
            "first={} after_reset={}",
            first.confidence,
            after_reset.confidence
        );
    }
}
