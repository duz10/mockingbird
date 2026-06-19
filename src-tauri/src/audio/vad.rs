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

// macOS port (.4.7a): the Silero impl is un-gated to `any(windows, macos)` —
// the ort/Silero ONNX path is cross-platform (proven by `mac_ort_vad_smoke`).
// Imports are orphaned only on Linux/other until that backend lands.
#![cfg_attr(
    not(any(target_os = "windows", target_os = "macos")),
    allow(unused_imports)
)]

use std::path::{Path, PathBuf};

// AppError is used on every platform: SileroVad's impl on win+mac, and the
// not-implemented Linux arm of `make_default_vad`.
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
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        Ok(Box::new(SileroVad::new()?))
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        Err(AppError::Audio(
            "VAD not implemented for this platform (Phase 9 Linux)".into(),
        ))
    }
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
pub const SILERO_FRAME_SAMPLES: usize = 512;
#[cfg(any(target_os = "windows", target_os = "macos"))]
const SILERO_STATE_LEN: usize = 2 * 128; // (h+c stacked, batch=1, hidden=128)
#[cfg(any(target_os = "windows", target_os = "macos"))]
const SILERO_SR: i64 = 16_000;
#[cfg(any(target_os = "windows", target_os = "macos"))]
const SPEECH_THRESHOLD: f32 = 0.5;
/// Silero v5 at 16 kHz requires 64 samples of context (last 64 of
/// the previous frame) prepended to each new 512-sample frame.
/// Total model input is therefore 576 samples per inference. Without
/// this, the model's STFT windowing is incoherent and it produces
/// essentially constant output regardless of input content. The
/// official Python reference (`silero-vad/src/silero_vad/utils_vad.py`)
/// makes this implicit — only readable by tracing the `_context`
/// buffer through `__call__`. Wave 4.8 finding.
#[cfg(any(target_os = "windows", target_os = "macos"))]
const SILERO_CONTEXT_SAMPLES: usize = 64;
#[cfg(any(target_os = "windows", target_os = "macos"))]
const SILERO_INPUT_SAMPLES: usize = SILERO_CONTEXT_SAMPLES + SILERO_FRAME_SAMPLES;

#[cfg(any(target_os = "windows", target_os = "macos"))]
pub struct SileroVad {
    session: ort::session::Session,
    /// LSTM hidden+cell stacked flat as [2 * 1 * 128].
    state: Vec<f32>,
    /// Last 64 audio samples (f32 in `[-1, 1]`) from the previous
    /// frame, prepended to the next frame before inference. Required
    /// by Silero v5 — see [`SILERO_CONTEXT_SAMPLES`] for the why.
    context: Vec<f32>,
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
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
            context: vec![0.0f32; SILERO_CONTEXT_SAMPLES],
        })
    }
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
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

#[cfg(any(target_os = "windows", target_os = "macos"))]
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
        let new_audio_f32: Vec<f32> = frame.iter().map(|&s| s as f32 / 32768.0).collect();

        // Silero v5: prepend 64-sample context (last 64 of previous
        // frame, init zeros) to the new 512 samples. Total input is
        // 576 samples. The model uses the context to make the
        // internal STFT window contiguous across frame boundaries.
        let mut audio_f32: Vec<f32> = Vec::with_capacity(SILERO_INPUT_SAMPLES);
        audio_f32.extend_from_slice(&self.context);
        audio_f32.extend_from_slice(&new_audio_f32);
        debug_assert_eq!(audio_f32.len(), SILERO_INPUT_SAMPLES);

        // Update context buffer for the NEXT call: the last 64
        // samples of THIS frame's new audio. We do this before
        // moving `audio_f32` into the tensor.
        let new_context_start = SILERO_FRAME_SAMPLES - SILERO_CONTEXT_SAMPLES;
        self.context.clear();
        self.context
            .extend_from_slice(&new_audio_f32[new_context_start..]);

        // Build input tensor with the combined context+new shape.
        // ort 2.0.0-rc.10: `Tensor::from_array((shape, Vec<T>))`.
        let input_t = ort::value::Tensor::from_array(([1_usize, SILERO_INPUT_SAMPLES], audio_f32))
            .map_err(|e| AppError::Audio(format!("build input tensor: {e}")))?;

        let state_t =
            ort::value::Tensor::from_array(([2_usize, 1_usize, 128_usize], self.state.clone()))
                .map_err(|e| AppError::Audio(format!("build state tensor: {e}")))?;

        // Silero v5: pass sr as a 1-d tensor of length 1 — matches
        // the `silero-rs` reference impl. The model declares the
        // input as scalar shape `[]`, but ort + ONNX Runtime accept
        // `[1]` (single-element 1-d) transparently. We tried both
        // `()` and `[0_usize; 0]` for true 0-d — both produced WORSE
        // confidence, suggesting some ort path silently miscompiles
        // them. Stick with `[1]`.
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
        self.context.clear();
        self.context.resize(SILERO_CONTEXT_SAMPLES, 0.0);
    }

    fn frame_samples(&self) -> usize {
        SILERO_FRAME_SAMPLES
    }
}

#[cfg(all(test, any(target_os = "windows", target_os = "macos")))]
mod tests {
    use super::*;

    fn silero_path() -> Option<PathBuf> {
        locate_model()
    }

    #[test]
    fn frame_samples_constant_is_512() {
        assert_eq!(SILERO_FRAME_SAMPLES, 512);
    }

    /// Regression guard: ensures Silero v5 produces meaningfully
    /// different outputs for differently-structured inputs.
    ///
    /// Before Wave 4.8 the [`SILERO_CONTEXT_SAMPLES`] context buffer
    /// wasn't prepended to the per-frame input. The model still
    /// ran, but produced ~constant near-zero output for ANY input
    /// (silence, speech, tone, noise — all scored ≈0.001). The
    /// per-frame confidence had no dynamic range.
    ///
    /// This test feeds three structurally distinct synthetic
    /// signals (silence, white noise, loud sweep) and asserts the
    /// max confidence varies across them by at least an order of
    /// magnitude. A regression that drops the context buffer fails
    /// this immediately — outputs collapse to a flat ~0.001.
    ///
    /// We deliberately do NOT assert exact confidence values: the
    /// model is what it is, and we don't want to lock its specific
    /// numerical behaviour. We only assert it's ALIVE.
    #[test]
    fn silero_output_has_dynamic_range() {
        let Some(path) = silero_path() else {
            eprintln!("SKIP: silero_vad.onnx not on disk");
            return;
        };
        let mut vad = SileroVad::from_path(&path).unwrap();

        let max_for_signal = |vad: &mut SileroVad, mut gen: Box<dyn FnMut(usize) -> i16>| -> f32 {
            vad.reset();
            let mut max = 0.0_f32;
            for _ in 0..30 {
                let frame: Vec<i16> = (0..SILERO_FRAME_SAMPLES).map(&mut *gen).collect();
                let f = vad.process_frame(&frame).unwrap();
                if f.confidence > max {
                    max = f.confidence;
                }
            }
            max
        };

        let silence_max = max_for_signal(&mut vad, Box::new(|_| 0));
        let mut x = 12345_i32;
        let noise_max = max_for_signal(
            &mut vad,
            Box::new(move |_| {
                x = x.wrapping_mul(1103515245).wrapping_add(12345);
                ((x >> 16) as i16).saturating_mul(8)
            }),
        );
        let mut phase = 0.0_f32;
        let sweep_max = max_for_signal(
            &mut vad,
            Box::new(move |i| {
                // Frequency-swept sine: low → mid-band. Loud.
                let f = 200.0 + (i as f32 * 4.0);
                phase += 2.0 * std::f32::consts::PI * f / 16_000.0;
                (phase.sin() * 20_000.0) as i16
            }),
        );

        eprintln!("silence_max={silence_max:.4} noise_max={noise_max:.4} sweep_max={sweep_max:.4}");

        // Sanity: silence should score near-zero.
        assert!(
            silence_max < 0.1,
            "silence should score low; got {silence_max}"
        );
        // The real assertion: the model has dynamic range. Without
        // the context-buffer fix all three would collapse to ~0.001
        // (max diff ~0.003 < 0.05). With the fix, structured signals
        // produce visibly higher confidence than silence.
        let max_dynamic_range = (noise_max - silence_max)
            .abs()
            .max((sweep_max - silence_max).abs());
        assert!(
            max_dynamic_range > 0.05,
            "Silero output appears stuck — dynamic range {max_dynamic_range:.4} is \
             too small (silence={silence_max}, noise={noise_max}, sweep={sweep_max}). \
             Probable cause: SILERO_CONTEXT_SAMPLES context buffer is not being \
             prepended to per-frame input. See `process_frame` impl."
        );
    }

    /// Manual-only: load the most recent `last_capture.wav` dumped by
    /// the running app and run Silero on it. Reports the full
    /// per-frame confidence distribution.
    ///
    /// Useful when debugging: run the app, dictate a phrase, then:
    ///   `cargo test --release silero_dumped_wav -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn silero_dumped_wav() {
        let appdata = std::env::var("APPDATA").expect("APPDATA");
        let wav_path = std::path::PathBuf::from(appdata)
            .join("com.dustin.mockingbird")
            .join("last_capture.wav");
        let mut reader = hound::WavReader::open(&wav_path)
            .unwrap_or_else(|e| panic!("open {}: {e}", wav_path.display()));
        let spec = reader.spec();
        eprintln!(
            "WAV: rate={} channels={} bits={} format={:?} len={}",
            spec.sample_rate,
            spec.channels,
            spec.bits_per_sample,
            spec.sample_format,
            reader.duration()
        );
        let samples: Vec<i16> = reader.samples::<i16>().filter_map(|s| s.ok()).collect();
        eprintln!("loaded {} samples", samples.len());

        // Sanity-check amplitude.
        let peak = samples
            .iter()
            .map(|s| (*s as i32).unsigned_abs())
            .max()
            .unwrap_or(0);
        let rms = {
            let sum_sq: u64 = samples.iter().map(|s| (*s as i64 * *s as i64) as u64).sum();
            ((sum_sq / samples.len() as u64) as f64).sqrt() as u32
        };
        eprintln!("peak_abs={peak} rms={rms}");

        let Some(path) = silero_path() else {
            panic!("no silero model");
        };
        let mut vad = SileroVad::from_path(&path).unwrap();

        let mut confs = Vec::new();
        for chunk in samples.chunks_exact(SILERO_FRAME_SAMPLES) {
            let f = vad.process_frame(chunk).unwrap();
            confs.push(f.confidence);
        }

        let max = confs.iter().cloned().fold(0.0_f32, f32::max);
        let avg = confs.iter().sum::<f32>() / confs.len() as f32;
        let speech_frames = confs.iter().filter(|c| **c >= SPEECH_THRESHOLD).count();
        eprintln!(
            "\nSPEECH: {} frames | max={:.4} | avg={:.4} | speech_frames={}",
            confs.len(),
            max,
            avg,
            speech_frames
        );

        // Compare against pure silence (all zeros).
        vad.reset();
        let silent_frame = vec![0i16; SILERO_FRAME_SAMPLES];
        let mut silent_confs = Vec::new();
        for _ in 0..20 {
            let f = vad.process_frame(&silent_frame).unwrap();
            silent_confs.push(f.confidence);
        }
        eprintln!(
            "SILENCE: 20 frames | first={:.4} last={:.4} max={:.4}",
            silent_confs[0],
            silent_confs[19],
            silent_confs.iter().cloned().fold(0.0_f32, f32::max)
        );

        // Compare against a strong synthesized 440 Hz tone (not
        // speech, but loud + structured — should NOT score as speech
        // but should differ from silence).
        vad.reset();
        let mut tone_confs = Vec::new();
        let mut phase = 0.0_f32;
        let phase_inc = 2.0 * std::f32::consts::PI * 440.0 / 16_000.0;
        for _ in 0..20 {
            let frame: Vec<i16> = (0..SILERO_FRAME_SAMPLES)
                .map(|_| {
                    let v = (phase.sin() * 16_000.0) as i16;
                    phase += phase_inc;
                    v
                })
                .collect();
            let f = vad.process_frame(&frame).unwrap();
            tone_confs.push(f.confidence);
        }
        eprintln!(
            "TONE (440Hz): 20 frames | first={:.4} last={:.4} max={:.4}",
            tone_confs[0],
            tone_confs[19],
            tone_confs.iter().cloned().fold(0.0_f32, f32::max)
        );

        // Compare against synthesized noise (random-ish PCM).
        vad.reset();
        let mut noise_confs = Vec::new();
        let mut x = 12345_i32;
        for _ in 0..20 {
            let frame: Vec<i16> = (0..SILERO_FRAME_SAMPLES)
                .map(|_| {
                    x = x.wrapping_mul(1103515245).wrapping_add(12345);
                    ((x >> 16) as i16).saturating_mul(2)
                })
                .collect();
            let f = vad.process_frame(&frame).unwrap();
            noise_confs.push(f.confidence);
        }
        eprintln!(
            "NOISE: 20 frames | first={:.4} last={:.4} max={:.4}",
            noise_confs[0],
            noise_confs[19],
            noise_confs.iter().cloned().fold(0.0_f32, f32::max)
        );
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
