//! Sample-rate + channel conversion glue.
//!
//! cpal hands us audio in the device's *native* format: any sample
//! rate (44.1 / 48 / 96 kHz are common), any channel count (often
//! stereo on a webcam mic), in either `f32` or `i16`. The rest of
//! Mockingbird's pipeline (VAD, STT, ringbuf) wants `16 kHz mono i16`.
//!
//! This module owns the conversion. ADR 0013 pre-approved the design;
//! Phase 2 deferred the impl because the dev mic happened to expose
//! 16 kHz natively. Phase 3 Wave 4.5 finished it after the first
//! actual-user test surfaced the gap.
//!
//! ## Design
//!
//! ```text
//!  cpal callback (any rate, any ch, f32 or i16)
//!         │
//!         ▼
//!   downmix to mono f32 in [-1.0, 1.0]   (pure, branch on ch count)
//!         │
//!         ▼
//!   accumulate in `input_buf` until ≥ chunk_size_in frames
//!         │
//!         ▼
//!   rubato::FftFixedIn::process_into_buffer  (NO allocation —
//!                                             buffers pre-sized in
//!                                             new())
//!         │
//!         ▼
//!   convert mono f32 → i16, clip, push to ring producer
//! ```
//!
//! ## Why FftFixedIn
//!
//! - Fixed input chunk → predictable scheduling in the cpal callback.
//! - Pure FFT (not SINC) → cheap CPU. Voice doesn't need SINC quality;
//!   STT downstream is more tolerant than the ear.
//! - `process_into_buffer` accepts pre-allocated output buffers, so we
//!   pay zero allocations per callback once `new()` returns.
//!
//! ## What lives where
//!
//! - [`downmix_to_mono_f32_i16`] / [`downmix_to_mono_f32`]: pure
//!   helpers, exhaustively unit-tested without any audio runtime.
//! - [`AudioPipeline`]: stateful struct holding the resampler and its
//!   pre-allocated buffers. The cpal callback calls
//!   [`AudioPipeline::process_i16`] or [`AudioPipeline::process_f32`].

use ringbuf::traits::Producer;
use rubato::{FftFixedIn, Resampler};

use crate::error::{AppError, AppResult};

use super::capture::{SampleProducer, TARGET_CHANNELS, TARGET_SAMPLE_RATE};

/// rubato `FftFixedIn` chunk size, in input frames per process call.
///
/// 1024 input frames at 48 kHz = ~21 ms latency added by buffering —
/// well under the human just-noticeable-difference for dictation
/// latency (which is dominated by Whisper anyway). Smaller chunks
/// would reduce latency but increase per-call FFT overhead; 1024 is
/// the sweet spot rubato's docs suggest.
pub const RESAMPLER_CHUNK_FRAMES: usize = 1024;

/// rubato `sub_chunks` parameter — how many FFT sub-passes per chunk.
/// 2 is rubato's recommended default for general-purpose use.
const RESAMPLER_SUB_CHUNKS: usize = 2;

/// Audio conversion pipeline owned by the cpal callback.
///
/// Per-instance state is significant (resampler holds internal FFT
/// twiddle factors + ring buffers; we hold pre-allocated working
/// buffers). NOT cheap to construct — build once per
/// `CpalCapture::start()`, reuse for the lifetime of the stream.
///
/// `Debug` is hand-written because `FftFixedIn` doesn't derive it.
/// Only metadata is dumped — internal buffers are noise.
pub struct AudioPipeline {
    /// `None` when the device already matches the target rate AND
    /// channel count AND sample format — the fast path just copies.
    resampler: Option<FftFixedIn<f32>>,
    /// Device's native channel count (`>= 1`).
    device_channels: u16,
    /// Device's native sample rate (Hz).
    device_rate: u32,

    // ── Pre-allocated working buffers (avoid alloc in cpal callback). ──
    /// Mono f32 staging buffer. Holds samples accumulated across cpal
    /// callbacks until we have enough for one rubato chunk.
    mono_input_pending: Vec<f32>,
    /// Borrowed slice handed to rubato as its single input channel.
    /// Wrapped in a Vec because rubato wants `&[AsRef<[T]>]`.
    resampler_input: Vec<Vec<f32>>,
    /// Output buffer rubato writes into. Sized for the maximum frames
    /// it can produce per call.
    resampler_output: Vec<Vec<f32>>,
}

impl std::fmt::Debug for AudioPipeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioPipeline")
            .field("device_rate", &self.device_rate)
            .field("device_channels", &self.device_channels)
            .field("resampling", &self.resampler.is_some())
            .field("pending_input_frames", &self.mono_input_pending.len())
            .finish()
    }
}

impl AudioPipeline {
    /// Build a pipeline for a stream with the given device-side
    /// `rate` (Hz) + `channels`.
    ///
    /// Returns `Err` only if rubato rejects the rate combination
    /// (e.g. rate == 0). Channel counts ≥ 1 are accepted.
    pub fn new(rate: u32, channels: u16) -> AppResult<Self> {
        if channels == 0 {
            return Err(AppError::Audio(
                "device reports 0 channels — invalid config".into(),
            ));
        }

        let needs_resample = rate != TARGET_SAMPLE_RATE;

        let resampler = if needs_resample {
            Some(
                FftFixedIn::<f32>::new(
                    rate as usize,
                    TARGET_SAMPLE_RATE as usize,
                    RESAMPLER_CHUNK_FRAMES,
                    RESAMPLER_SUB_CHUNKS,
                    1, // always mono — we downmix before resampling
                )
                .map_err(|e| {
                    AppError::Audio(format!(
                        "rubato init {rate} Hz → {TARGET_SAMPLE_RATE} Hz: {e}"
                    ))
                })?,
            )
        } else {
            None
        };

        // Pre-size the resampler-output buffer to the worst-case
        // frames-per-call. For the no-resample fast path, size it
        // for the chunk so we have a stable scratch area.
        let max_out = resampler
            .as_ref()
            .map(|r| r.output_frames_max())
            .unwrap_or(RESAMPLER_CHUNK_FRAMES);

        let resampler_input = vec![Vec::with_capacity(RESAMPLER_CHUNK_FRAMES)];
        let resampler_output = vec![vec![0.0_f32; max_out]];

        // Pre-allocate the mono input buffer generously — 2× chunk so
        // we comfortably absorb cpal callback bursts.
        let mono_input_pending = Vec::with_capacity(RESAMPLER_CHUNK_FRAMES * 2);

        tracing::info!(
            target: "audio",
            device_rate = rate,
            device_channels = channels,
            target_rate = TARGET_SAMPLE_RATE,
            target_channels = TARGET_CHANNELS,
            needs_resample,
            "audio pipeline initialised"
        );

        Ok(Self {
            resampler,
            device_channels: channels,
            device_rate: rate,
            mono_input_pending,
            resampler_input,
            resampler_output,
        })
    }

    /// Device's native sample rate (informational; the *consumer* of
    /// the producer always sees `TARGET_SAMPLE_RATE`).
    #[allow(dead_code)] // Diagnostic-only accessor.
    pub fn device_rate(&self) -> u32 {
        self.device_rate
    }

    /// Push `i16` samples from a cpal callback through the pipeline.
    ///
    /// Returns the number of `i16` mono-16kHz samples actually pushed
    /// to the ring producer (which is `<= input.len() * 16000 / device_rate`).
    pub fn process_i16(&mut self, input: &[i16], producer: &mut SampleProducer) -> usize {
        let dropped_before = self.mono_input_pending.len();
        downmix_to_mono_f32_i16(input, self.device_channels, &mut self.mono_input_pending);
        let _ = dropped_before; // (kept for symmetry with potential future telemetry)
        self.drain_resampler_to(producer)
    }

    /// Push `f32` samples from a cpal callback through the pipeline.
    pub fn process_f32(&mut self, input: &[f32], producer: &mut SampleProducer) -> usize {
        downmix_to_mono_f32(input, self.device_channels, &mut self.mono_input_pending);
        self.drain_resampler_to(producer)
    }

    /// Consume as many full chunks as `mono_input_pending` holds,
    /// resample each, push i16 to the producer. Leftover (< chunk)
    /// samples stay in `mono_input_pending` for the next callback.
    fn drain_resampler_to(&mut self, producer: &mut SampleProducer) -> usize {
        let mut total_pushed = 0;

        match &mut self.resampler {
            // Fast path: device is already 16 kHz mono. Just push i16.
            None => {
                for sample in self.mono_input_pending.drain(..) {
                    let s = f32_to_i16_clamped(sample);
                    if producer.try_push(s).is_ok() {
                        total_pushed += 1;
                    } else {
                        // Ring full — drop the rest of this callback.
                        // tracing::warn would spam, so we only log on
                        // the boundary; capture.rs's existing
                        // "ring overflow" log line near the producer
                        // is sufficient.
                        break;
                    }
                }
            }

            // Resample path: process in chunk-sized blocks.
            Some(resampler) => {
                while self.mono_input_pending.len() >= RESAMPLER_CHUNK_FRAMES {
                    // Move one chunk into the resampler's input Vec.
                    // We use clear + extend rather than splice/drain
                    // so the inner Vec keeps its allocated capacity.
                    let chunk_iter = self.mono_input_pending.drain(..RESAMPLER_CHUNK_FRAMES);
                    let in_buf = &mut self.resampler_input[0];
                    in_buf.clear();
                    in_buf.extend(chunk_iter);

                    let result = resampler.process_into_buffer(
                        &self.resampler_input,
                        &mut self.resampler_output,
                        None,
                    );
                    let frames_out = match result {
                        Ok((_in_used, out_frames)) => out_frames,
                        Err(e) => {
                            tracing::warn!(
                                target: "audio",
                                error = %e,
                                "rubato process_into_buffer failed; dropping chunk"
                            );
                            continue;
                        }
                    };

                    for &sample in &self.resampler_output[0][..frames_out] {
                        let s = f32_to_i16_clamped(sample);
                        if producer.try_push(s).is_ok() {
                            total_pushed += 1;
                        } else {
                            break;
                        }
                    }
                }
            }
        }

        total_pushed
    }
}

/// Clamp + scale a normalized f32 sample to `i16`.
///
/// Voice STT models tolerate slight saturation, so we clamp rather
/// than soft-knee. Out-of-range inputs (>|1.0|) get pinned to the
/// extremes.
#[inline]
fn f32_to_i16_clamped(s: f32) -> i16 {
    // i16::MAX is 32_767; multiplying by 32_767 keeps both ends
    // symmetric. (Using 32_768 would let positive saturate before
    // negative.)
    let scaled = (s.clamp(-1.0, 1.0) * 32_767.0) as i32;
    scaled.clamp(i16::MIN as i32, i16::MAX as i32) as i16
}

/// Downmix interleaved `i16` samples to mono `f32` in `[-1.0, 1.0]`.
///
/// Pure. Appends to `out` (does not clear). Mono input is a 1:1
/// scale; stereo+ averages all channels. Inputs whose length is not
/// a multiple of `device_channels` have the tail truncated — cpal
/// always hands us full frames, so this is a defensive guard.
pub fn downmix_to_mono_f32_i16(input: &[i16], device_channels: u16, out: &mut Vec<f32>) {
    if device_channels == 0 {
        return;
    }
    let ch = device_channels as usize;
    if ch == 1 {
        out.extend(input.iter().map(|&s| s as f32 / 32_768.0));
        return;
    }
    // Multi-channel: average over each frame.
    let inv_scale = 1.0_f32 / (32_768.0 * ch as f32);
    for frame in input.chunks_exact(ch) {
        let sum: i32 = frame.iter().map(|&s| s as i32).sum();
        out.push(sum as f32 * inv_scale);
    }
}

/// Downmix interleaved `f32` samples to mono `f32`.
///
/// Pure. cpal already hands `f32` in `[-1.0, 1.0]`, so no scaling
/// — only channel averaging.
pub fn downmix_to_mono_f32(input: &[f32], device_channels: u16, out: &mut Vec<f32>) {
    if device_channels == 0 {
        return;
    }
    let ch = device_channels as usize;
    if ch == 1 {
        out.extend_from_slice(input);
        return;
    }
    let inv_ch = 1.0_f32 / ch as f32;
    for frame in input.chunks_exact(ch) {
        let sum: f32 = frame.iter().sum();
        out.push(sum * inv_ch);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Pure helpers (no audio runtime needed). ──

    #[test]
    fn downmix_i16_mono_scales_to_unit_range() {
        let mut out = Vec::new();
        downmix_to_mono_f32_i16(&[0, 16_384, -16_384, i16::MAX, i16::MIN], 1, &mut out);
        assert_eq!(out.len(), 5);
        assert!((out[0] - 0.0).abs() < 1e-6);
        assert!((out[1] - 0.5).abs() < 1e-3);
        assert!((out[2] - -0.5).abs() < 1e-3);
        // i16::MAX = 32767, 32767/32768 ≈ 0.99997
        assert!((out[3] - 0.99997).abs() < 1e-4);
        // i16::MIN = -32768, -32768/32768 = -1.0 exactly
        assert!((out[4] - -1.0).abs() < 1e-6);
    }

    #[test]
    fn downmix_i16_stereo_averages_channels() {
        let mut out = Vec::new();
        // 3 frames of stereo: [L0,R0, L1,R1, L2,R2]
        downmix_to_mono_f32_i16(&[100, 200, -100, -200, 1000, -1000], 2, &mut out);
        assert_eq!(out.len(), 3);
        // (100+200)/(2 * 32768) ≈ 0.00458
        assert!((out[0] - 150.0 / 32_768.0).abs() < 1e-6);
        assert!((out[1] - -150.0 / 32_768.0).abs() < 1e-6);
        assert!((out[2] - 0.0).abs() < 1e-6);
    }

    #[test]
    fn downmix_i16_truncates_incomplete_frames() {
        let mut out = Vec::new();
        // 2.5 stereo frames — the dangling `9999` should be dropped.
        downmix_to_mono_f32_i16(&[100, 100, 200, 200, 9999], 2, &mut out);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn downmix_i16_zero_channels_is_noop() {
        let mut out = Vec::new();
        downmix_to_mono_f32_i16(&[1, 2, 3], 0, &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn downmix_f32_mono_is_passthrough() {
        let mut out = Vec::new();
        downmix_to_mono_f32(&[0.1, -0.2, 0.5], 1, &mut out);
        assert_eq!(out, vec![0.1, -0.2, 0.5]);
    }

    #[test]
    fn downmix_f32_stereo_averages() {
        let mut out = Vec::new();
        downmix_to_mono_f32(&[0.4, 0.6, -0.5, 0.5, 1.0, -1.0], 2, &mut out);
        assert_eq!(out.len(), 3);
        assert!((out[0] - 0.5).abs() < 1e-6);
        assert!((out[1] - 0.0).abs() < 1e-6);
        assert!((out[2] - 0.0).abs() < 1e-6);
    }

    #[test]
    fn downmix_f32_seven_one_audio_averages_all_eight_channels() {
        let mut out = Vec::new();
        // One frame of "7.1": eight channels all at 0.25 — average is 0.25.
        let input: Vec<f32> = vec![0.25_f32; 8];
        downmix_to_mono_f32(&input, 8, &mut out);
        assert_eq!(out, vec![0.25]);
    }

    #[test]
    fn f32_to_i16_clamps_above_one_to_i16_max() {
        assert_eq!(f32_to_i16_clamped(2.0), i16::MAX);
        assert_eq!(f32_to_i16_clamped(1.0), i16::MAX);
    }

    #[test]
    fn f32_to_i16_clamps_below_minus_one_to_i16_min() {
        assert_eq!(f32_to_i16_clamped(-2.0), i16::MIN + 1);
        // -1.0 * 32_767 = -32_767, which is i16::MIN + 1; that's the
        // intended symmetric behavior (see comment in helper).
        assert_eq!(f32_to_i16_clamped(-1.0), i16::MIN + 1);
    }

    #[test]
    fn f32_to_i16_zero_round_trips_to_zero() {
        assert_eq!(f32_to_i16_clamped(0.0), 0);
    }

    // ── AudioPipeline shape (no producer-side push, just init). ──

    #[test]
    fn pipeline_fast_path_when_native_matches_target() {
        let p = AudioPipeline::new(TARGET_SAMPLE_RATE, 1).unwrap();
        assert!(
            p.resampler.is_none(),
            "16 kHz mono input should skip rubato init"
        );
        assert_eq!(p.device_rate(), TARGET_SAMPLE_RATE);
    }

    #[test]
    fn pipeline_resamples_when_rate_differs() {
        let p = AudioPipeline::new(44_100, 2).unwrap();
        assert!(p.resampler.is_some(), "44.1 kHz must build a resampler");
        // Output buffer must be pre-sized for the worst case.
        assert!(!p.resampler_output[0].is_empty());
    }

    #[test]
    fn pipeline_rejects_zero_channels() {
        let err = AudioPipeline::new(48_000, 0).unwrap_err();
        assert!(format!("{err}").contains("0 channels"));
    }

    #[test]
    fn pipeline_handles_48khz_stereo() {
        // The other extremely common Windows default.
        let p = AudioPipeline::new(48_000, 2).unwrap();
        assert!(p.resampler.is_some());
    }
}
