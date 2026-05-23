//! Audio-file decode helper for Mockingbird's headless ingest path.
//!
//! Bridge between an on-disk audio file (`.m4a`, `.wav`, `.mp3`,
//! `.ogg`/Vorbis) and the `Vec<i16>` 16 kHz mono PCM the rest of the
//! dictation pipeline consumes (`audio::vad_trim`, `whisper-rs`).
//!
//! ## Used by
//!
//! - **ADR 0046 Iter 1** — the "+ Audio file" desktop import button
//!   (`dictation_import_file` IPC; bead `mb-7vyz`). Any format the
//!   feature flags below open.
//! - **ADR 0046 Iter 3** — the inbox watcher courier flow for iOS
//!   Shortcut audio (always `.m4a`, AAC-LC mono, ~32 kbps).
//!
//! ## Design choices
//!
//! - **`symphonia`** (pure-Rust, MIT/Apache, zero system codec deps).
//!   The intentional cost: AAC decode is meaningfully slower than the
//!   libfdk-aac C codec would be — but for a 3-minute Voice Memo on
//!   Dustin's box that's still ~100 ms; we don't care. This module
//!   runs on the user's "I clicked import" trigger, not in a hot loop.
//! - **`rubato::FftFixedIn`** for the resample step. Same crate the
//!   live-mic pipeline already uses (`audio::resampler`), same FFT
//!   path; voice tolerates FFT resampling fine and we avoid pulling
//!   in a second resampler family. Offline batch + small clip sizes
//!   mean we don't bother with the streaming pre-allocation pattern
//!   `AudioPipeline` uses — we feed the whole clip in one go.
//! - **Mono mix-down via simple per-frame average.** Loudness-preserving
//!   downmix matrices are overkill for voice (Whisper doesn't care
//!   about stereo image), and the average maps cleanly across any
//!   channel count.
//!
//! ## Errors
//!
//! Maps every symphonia failure into [`AppError::Audio`] with a
//! caller-actionable message. We deliberately do NOT add a dedicated
//! `AudioDecode` variant — `AppError::Audio` is already where every
//! audio-layer error converges (capture, VAD, resampler), and the
//! string prefix is enough to distinguish in logs.

use std::fs::File;
use std::path::Path;

use rubato::{FftFixedIn, Resampler};
use symphonia::core::audio::{AudioBufferRef, Signal};
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::sample::Sample;

use crate::error::{AppError, AppResult};

/// Target sample rate for the dictation pipeline (Whisper + Silero VAD).
///
/// Kept as a free const so the constant is meaningful in this file
/// without dragging in the `audio::capture` module just for the value.
/// MUST stay in sync with `audio::capture::TARGET_SAMPLE_RATE` — both
/// modules feed the same VAD + STT downstream.
pub const TARGET_RATE_HZ: u32 = 16_000;

/// Decode an audio file at `path` into 16 kHz mono `i16` PCM, the
/// canonical pipeline shape.
///
/// Accepts every format the workspace `symphonia` feature set covers
/// (AAC + ISO MP4 → iPhone `.m4a`; WAV; MP3; OGG + Vorbis). Returns
/// an [`AppError::Audio`] with a human-readable message on:
///
/// - File-open errors (forwarded via `?` from `File::open`).
/// - Symphonia probe / decode / metadata errors.
/// - "No audio track" — container parsed but contains zero audio
///   streams (e.g. a video file with the audio stripped).
/// - Resampler construction errors (only triggers on source rate 0,
///   which symphonia would already reject).
pub fn decode_to_pcm16_mono_16k(path: &Path) -> AppResult<Vec<i16>> {
    let file =
        File::open(path).map_err(|e| AppError::Audio(format!("open {}: {}", path.display(), e)))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    // Hint the probe with the file extension — symphonia uses it as a
    // tie-breaker between formats with similar magic bytes (the
    // m4a/mp4 family in particular).
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| AppError::Audio(format!("decode probe failed for {}: {e}", path.display())))?;
    let mut format = probed.format;

    // Pick the first track that actually carries audio. A muxed file
    // (mp4 with audio + video, ogg with multiple logical streams) can
    // have several tracks; we want the first audio one.
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| AppError::Audio(format!("no audio track in {}", path.display())))?;
    let track_id = track.id;
    let codec_params = track.codec_params.clone();

    let mut decoder = symphonia::default::get_codecs()
        .make(&codec_params, &DecoderOptions::default())
        .map_err(|e| AppError::Audio(format!("unsupported codec: {e}")))?;

    let mut mono_f32: Vec<f32> = Vec::new();
    // Source rate falls back to the target rate only if symphonia
    // can't tell us — in practice every supported codec carries a
    // rate, so this guards against future format additions.
    let mut source_rate: u32 = codec_params.sample_rate.unwrap_or(TARGET_RATE_HZ);

    // Decode loop. EOF is signaled as `Error::IoError(UnexpectedEof)`
    // by symphonia 0.5; we treat any IoError as graceful end-of-stream
    // (the test fixtures synthesize files that hit this).
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymphoniaError::IoError(_)) => break,
            Err(SymphoniaError::ResetRequired) => {
                return Err(AppError::Audio(
                    "decode requires reset (unsupported on offline import)".into(),
                ));
            }
            Err(e) => {
                return Err(AppError::Audio(format!("packet read: {e}")));
            }
        };
        if packet.track_id() != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(decoded) => {
                // The first non-empty buffer is the source of truth for
                // the actual sample rate (codec_params may report 0 for
                // some containers).
                let spec_rate = decoded.spec().rate;
                if spec_rate > 0 {
                    source_rate = spec_rate;
                }
                append_as_mono_f32(decoded, &mut mono_f32);
            }
            Err(SymphoniaError::IoError(_)) => break,
            Err(SymphoniaError::DecodeError(_)) => continue, // skip corrupt packet
            Err(e) => {
                return Err(AppError::Audio(format!("decode: {e}")));
            }
        }
    }

    if mono_f32.is_empty() {
        return Err(AppError::Audio(format!(
            "decoded 0 samples from {} — empty or unsupported stream",
            path.display()
        )));
    }

    let resampled = resample_to_16k(&mono_f32, source_rate)?;
    Ok(f32_to_i16_clipped(&resampled))
}

/// Append a decoded symphonia [`AudioBufferRef`] to `dest` as mono
/// `f32` in [-1.0, 1.0].
///
/// `AudioBufferRef` is an enum over every sample format symphonia
/// supports — we handle the planar variants and downmix to mono via
/// a simple per-frame average. Voice doesn't need a loudness-preserving
/// matrix; the average is correct enough and works for any channel
/// count.
fn append_as_mono_f32(decoded: AudioBufferRef<'_>, dest: &mut Vec<f32>) {
    match decoded {
        AudioBufferRef::F32(buf) => append_planar(&buf, dest, |s| s),
        AudioBufferRef::S32(buf) => append_planar(&buf, dest, |s| s as f32 / i32::MAX as f32),
        AudioBufferRef::S16(buf) => append_planar(&buf, dest, |s| s as f32 / i16::MAX as f32),
        AudioBufferRef::U8(buf) => append_planar(&buf, dest, |s| (s as f32 - 128.0) / 128.0),
        AudioBufferRef::U16(buf) => append_planar(&buf, dest, |s| (s as f32 - 32_768.0) / 32_768.0),
        AudioBufferRef::U24(buf) => append_planar(&buf, dest, |s| {
            (s.inner() as f32 - 8_388_608.0) / 8_388_608.0
        }),
        AudioBufferRef::S24(buf) => append_planar(&buf, dest, |s| s.inner() as f32 / 8_388_608.0),
        AudioBufferRef::U32(buf) => append_planar(&buf, dest, |s| {
            (s as f32 - 2_147_483_648.0) / 2_147_483_648.0
        }),
        AudioBufferRef::F64(buf) => append_planar(&buf, dest, |s| s as f32),
        AudioBufferRef::S8(buf) => append_planar(&buf, dest, |s| s as f32 / i8::MAX as f32),
    }
}

/// Generic per-channel append + mono-downmix helper. Symphonia stores
/// every supported buffer as planar (one slice per channel), so we
/// iterate frame-by-frame and average across channels. The `Sample`
/// trait bound is required by `AudioBuffer`'s `frames()` / `chan()`
/// inherent-via-trait methods.
fn append_planar<S: Sample + Copy, F: Fn(S) -> f32>(
    buf: &symphonia::core::audio::AudioBuffer<S>,
    dest: &mut Vec<f32>,
    cvt: F,
) {
    let channels = buf.spec().channels.count().max(1);
    let frames = buf.frames();
    dest.reserve(frames);
    for frame in 0..frames {
        let mut sum = 0.0f32;
        for ch in 0..channels {
            sum += cvt(buf.chan(ch)[frame]);
        }
        dest.push(sum / channels as f32);
    }
}

/// Resample mono f32 from `source_rate` Hz to 16 kHz using
/// `rubato::FftFixedIn`. Returns the input unchanged when source rate
/// already matches the target.
///
/// `FftFixedIn` is the same family `audio::resampler::AudioPipeline`
/// uses for the live mic. Offline batch + small clip sizes mean we
/// feed the whole clip in one or two calls; we don't bother with the
/// pre-allocation pattern the live pipeline needs.
fn resample_to_16k(mono: &[f32], source_rate: u32) -> AppResult<Vec<f32>> {
    if source_rate == TARGET_RATE_HZ {
        return Ok(mono.to_vec());
    }
    // chunk size: rubato handles any input length per process_partial.
    // 1024 mirrors `audio::resampler::RESAMPLER_CHUNK_FRAMES` for
    // consistency with the live-mic path.
    const CHUNK: usize = 1024;
    let mut resampler = FftFixedIn::<f32>::new(
        source_rate as usize,
        TARGET_RATE_HZ as usize,
        CHUNK,
        2,
        1, // mono
    )
    .map_err(|e| AppError::Audio(format!("resampler init {source_rate}->16000: {e}")))?;

    let mut output: Vec<f32> = Vec::with_capacity(
        ((mono.len() as u64 * TARGET_RATE_HZ as u64) / source_rate as u64) as usize + CHUNK,
    );
    let mut cursor = 0usize;
    while cursor + CHUNK <= mono.len() {
        let slice = &mono[cursor..cursor + CHUNK];
        let out = resampler
            .process(&[slice], None)
            .map_err(|e| AppError::Audio(format!("resample: {e}")))?;
        output.extend_from_slice(&out[0]);
        cursor += CHUNK;
    }
    // Flush the tail with process_partial so we don't lose the last
    // sub-chunk of samples (rubato pads internally).
    if cursor < mono.len() {
        let tail = &mono[cursor..];
        let out = resampler
            .process_partial(Some(&[tail]), None)
            .map_err(|e| AppError::Audio(format!("resample tail: {e}")))?;
        output.extend_from_slice(&out[0]);
    }
    // One more empty partial to drain any internal latency.
    if let Ok(out) = resampler.process_partial::<Vec<f32>>(None, None) {
        output.extend_from_slice(&out[0]);
    }
    Ok(output)
}

/// Quantize mono f32 [-1.0, 1.0] to i16 with clipping at the
/// representable range. Saturation matches what `audio::resampler`
/// uses for the live path.
fn f32_to_i16_clipped(mono: &[f32]) -> Vec<i16> {
    let mut out = Vec::with_capacity(mono.len());
    for &s in mono {
        let scaled = s * i16::MAX as f32;
        // i16 range is [-32768, 32767]; clamp BEFORE the as-cast or
        // we hit "as-conversion-saturating but on the wrong side".
        let clamped = scaled.clamp(i16::MIN as f32, i16::MAX as f32);
        out.push(clamped as i16);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use hound::{SampleFormat, WavSpec, WavWriter};
    use std::f32::consts::PI;
    use tempfile::tempdir;

    /// Synthesize a sine-wave WAV in a temp dir + return its path. The
    /// caller drops the `TempDir` to clean up.
    fn write_sine_wav(
        dir: &Path,
        name: &str,
        freq_hz: f32,
        secs: f32,
        sample_rate: u32,
        channels: u16,
    ) -> std::path::PathBuf {
        let path = dir.join(name);
        let spec = WavSpec {
            channels,
            sample_rate,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let mut writer = WavWriter::create(&path, spec).expect("open wav writer");
        let total_frames = (secs * sample_rate as f32) as u32;
        for n in 0..total_frames {
            let t = n as f32 / sample_rate as f32;
            let s = (2.0 * PI * freq_hz * t).sin();
            let sample = (s * (i16::MAX as f32 * 0.5)) as i16;
            for _ in 0..channels {
                writer.write_sample(sample).expect("write sample");
            }
        }
        writer.finalize().expect("finalize wav");
        path
    }

    #[test]
    fn decodes_16k_mono_wav_unchanged_shape() {
        let dir = tempdir().unwrap();
        let path = write_sine_wav(dir.path(), "sine_16k.wav", 440.0, 1.0, 16_000, 1);
        let pcm = decode_to_pcm16_mono_16k(&path).expect("decode");
        // 1 second at 16 kHz mono → resampler short-circuits (source
        // rate == target rate) so we get exact sample count.
        let drift = (pcm.len() as i64 - 16_000).abs();
        assert!(
            drift < 256,
            "expected ~16000 samples from 1s of 16k mono, got {} (drift {})",
            pcm.len(),
            drift
        );
        // Sanity: it's not all silence.
        let max_abs = pcm.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0);
        assert!(
            max_abs > 1_000,
            "decoded sine should have meaningful peak; got {max_abs}"
        );
    }

    #[test]
    fn resamples_44100_stereo_to_16k_mono() {
        let dir = tempdir().unwrap();
        let path = write_sine_wav(dir.path(), "sine_44k_stereo.wav", 1_000.0, 1.0, 44_100, 2);
        let pcm = decode_to_pcm16_mono_16k(&path).expect("decode");
        // 1 second of source → ~16000 target samples. FFT resamplers
        // pad output to chunk boundaries, so we allow ~5% drift; STT
        // downstream is tolerant of tiny leading/trailing silence pads.
        let drift = (pcm.len() as i64 - 16_000).abs();
        assert!(
            drift < 1_024,
            "expected ~16000 samples after resample, got {} (drift {})",
            pcm.len(),
            drift
        );
        let max_abs = pcm.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0);
        assert!(
            max_abs > 1_000,
            "resampled sine should still be audible; got peak {max_abs}"
        );
    }

    #[test]
    fn missing_file_returns_audio_error() {
        let path = Path::new("C:\\definitely-not-a-real-mockingbird-fixture.wav");
        let err = decode_to_pcm16_mono_16k(path).expect_err("expected AppError");
        // Path-open failures land via the `?` on `File::open`, which
        // maps to `AppError::Audio(...)` through our explicit `map_err`.
        assert!(
            matches!(err, AppError::Audio(_)),
            "expected AppError::Audio for missing file, got {err:?}"
        );
    }

    #[test]
    fn non_audio_file_returns_audio_error() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("not_audio.wav");
        std::fs::write(&path, b"this is plain text, not a wav file at all").unwrap();
        let err = decode_to_pcm16_mono_16k(&path).expect_err("expected AppError");
        assert!(
            matches!(err, AppError::Audio(_)),
            "expected AppError::Audio for non-audio input, got {err:?}"
        );
    }
}
