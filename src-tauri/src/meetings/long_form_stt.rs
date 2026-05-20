//! Chunked Whisper driver — long-form transcription per channel.
//!
//! Consumes [`ChannelChunk`]s from a [`TwinStreamCapture`] receiver in
//! arrival order, calls `SpeechToText::transcribe_segments` (ADR 0030)
//! on each chunk with a rolling per-channel `initial_prompt`, drops
//! segments that fall inside the leading-overlap window of chunks
//! N ≥ 1 (they're duplicates of the prior chunk's tail), and emits
//! the stitched per-channel segment vectors in the **global meeting
//! timeline (ms)**.
//!
//! ## Stitch invariants (binding — see `mc-long-form-stitched-losslessly`)
//!
//! 1. **CRC32 over the i16-LE payload** is recomputed on read and
//!    must match `ChunkWritten::crc32`. Mismatch → `AppError::LongFormStt`.
//! 2. **Per-channel monotonic seq.** A chunk arriving with a `seq` ≤
//!    the channel's last seen `seq` is rejected. The chunker emits
//!    seqs in order by construction; this is a safety net against
//!    re-ordering by the chunk channel under load.
//! 3. **Overlap dedup.** For chunks N ≥ 1, segments with
//!    `t1_ms <= overlap_ms` (default 2000 ms) are dropped — chunk N's
//!    audio starts with the trailing `overlap_samples` of chunk N-1's
//!    payload, so Whisper will redundantly transcribe that region.
//! 4. **Global timeline.** Each kept segment's `(t0_ms, t1_ms)` is
//!    shifted by `chunk.first_sample * 1000 / sample_rate` so the
//!    output is meeting-relative.
//! 5. **Rolling prompt.** Chunks N ≥ 1 get an `initial_prompt`
//!    built from the trailing ~224 tokens of the prior chunk's joined
//!    text on the same channel. Cross-channel prompts never leak.
//!
//! ## Test seam
//!
//! The driver is parameterized over `&mut dyn SpeechToText`, so unit
//! tests can pass a `StubStt` that returns canned segments + records
//! its received `initial_prompt`. No real Whisper required to verify
//! the stitch algorithm — that's the whole point of segregating the
//! glue from the STT impl.

use std::sync::mpsc::Receiver;

use crate::error::{AppError, AppResult};
use crate::meetings::capture::{Channel, ChannelChunk};
use crate::stt::{SpeechToText, TranscribeRequest};

/// Per-Whisper-segment timing — re-exported from `stt::` so the
/// meetings layer can use the friendlier local name `TimedSegment`
/// while the canonical type lives next to `Transcript`.
pub use crate::stt::SttSegment as TimedSegment;

// ---------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------

/// Long-form driver knobs. Defaults match ADR 0029.
#[derive(Debug, Clone)]
pub struct LongFormConfig {
    /// Leading-overlap samples per chunk N ≥ 1. Default 32_000 (2 s
    /// @ 16 kHz). Used both to dedup segments AND to expose `overlap_ms`
    /// to test fixtures.
    pub overlap_samples: u32,
    /// PCM sample rate. Default 16_000.
    pub sample_rate: u32,
    /// Cap on `initial_prompt` length in approximate tokens. Whisper's
    /// internal limit is 224; we never exceed that. Default 224.
    pub max_prompt_tokens: usize,
}

impl Default for LongFormConfig {
    fn default() -> Self {
        Self {
            overlap_samples: 32_000,
            sample_rate: 16_000,
            max_prompt_tokens: 224,
        }
    }
}

impl LongFormConfig {
    /// Overlap window expressed in milliseconds (what Whisper segments
    /// are measured in).
    pub fn overlap_ms(&self) -> u32 {
        // (samples * 1000) / sample_rate — done in u64 to avoid the
        // pathological u32 overflow if a future config bumps overlap
        // to multi-minute. 32_000 * 1000 = 32_000_000 fits in u32 but
        // we promote to u64 for safety + clarity.
        ((self.overlap_samples as u64) * 1000 / self.sample_rate as u64) as u32
    }
}

// ---------------------------------------------------------------------
// Progress + Output
// ---------------------------------------------------------------------

/// Progress event emitted per processed chunk. The runtime fans these
/// out to the overlay window via Tauri's event bus.
#[derive(Debug, Clone)]
pub struct LongFormProgress {
    pub channel: Channel,
    pub chunk_seq: u32,
    pub chunks_done: u32,
    /// `None` until the chunk receiver disconnects (i.e. the
    /// `TwinStreamCapture` has been stopped and the consumer can
    /// count what arrived). The driver itself can't know the total
    /// while chunks are still streaming.
    pub chunks_total: Option<u32>,
}

/// Driver output. Per-channel stitched segments in the global
/// meeting timeline (ms).
#[derive(Debug, Clone, Default)]
pub struct LongFormOutput {
    pub mic_segments: Vec<TimedSegment>,
    pub sys_segments: Vec<TimedSegment>,
    /// `true` iff at least one chunk on the mic channel reported
    /// `gpu_used = true`. The `cuda-verified` judge family asserts
    /// this matches the build configuration.
    pub mic_gpu_used: bool,
    pub sys_gpu_used: bool,
}

// ---------------------------------------------------------------------
// Driver
// ---------------------------------------------------------------------

type ProgressCb<'a> = Box<dyn FnMut(LongFormProgress) + Send + 'a>;

/// Long-form chunked Whisper driver. Construct via [`new`], then call
/// [`run`] (which blocks until the chunk receiver disconnects).
///
/// [`new`]: Self::new
/// [`run`]: Self::run
pub struct LongFormStt<'a> {
    stt: &'a mut dyn SpeechToText,
    chunk_rx: Receiver<ChannelChunk>,
    on_progress: ProgressCb<'a>,
    config: LongFormConfig,
}

impl<'a> LongFormStt<'a> {
    pub fn new(
        stt: &'a mut dyn SpeechToText,
        chunk_rx: Receiver<ChannelChunk>,
        on_progress: impl FnMut(LongFormProgress) + Send + 'a,
        config: LongFormConfig,
    ) -> Self {
        Self {
            stt,
            chunk_rx,
            on_progress: Box::new(on_progress),
            config,
        }
    }

    /// Drain the chunk receiver until disconnected, processing each
    /// chunk in arrival order. Returns the stitched `LongFormOutput`.
    pub fn run(mut self) -> AppResult<LongFormOutput> {
        let mut state = State::default();
        while let Ok(chunk) = self.chunk_rx.recv() {
            self.process_chunk(chunk, &mut state)?;
        }
        // Defensive sort by t0_ms — chunks arrive in seq order per
        // channel, and the chunker emits seqs in order, so this is a
        // no-op on the happy path. But a future re-ordering channel
        // shouldn't be able to produce out-of-order segment output.
        state.mic.segments.sort_by_key(|s| s.t0_ms);
        state.sys.segments.sort_by_key(|s| s.t0_ms);
        Ok(state.into_output())
    }

    fn process_chunk(&mut self, chunk: ChannelChunk, state: &mut State) -> AppResult<()> {
        let seq = parse_seq_from_path(&chunk.chunk.path)?;
        let channel = chunk.channel;
        let chan = state.for_channel_mut(channel);

        // Defensive: reject out-of-order seqs within a channel.
        if let Some(prev) = chan.last_seq {
            if seq <= prev {
                return Err(AppError::LongFormStt(format!(
                    "non-monotonic seq for {channel:?}: last={prev}, got={seq}"
                )));
            }
        }
        let is_first_chunk_on_channel = chan.last_seq.is_none();

        // Read + verify the WAV payload.
        let samples = read_wav_i16(&chunk.chunk.path)?;
        let crc = crc32_of_i16(&samples);
        if crc != chunk.chunk.crc32 {
            return Err(AppError::LongFormStt(format!(
                "CRC mismatch for {channel:?} chunk seq {seq}: got {crc:08x}, \
                 expected {:08x}",
                chunk.chunk.crc32
            )));
        }

        // Build rolling initial_prompt from this channel's prior
        // chunk's text. Cross-channel never leaks (per-channel state).
        let initial_prompt = if is_first_chunk_on_channel {
            None
        } else {
            tail_tokens(&chan.last_text, self.config.max_prompt_tokens)
        };

        let req = TranscribeRequest {
            audio: &samples,
            initial_prompt: initial_prompt.as_deref(),
            force_cpu: false,
        };
        let result = self.stt.transcribe_segments(req)?;

        chan.gpu_used |= result.gpu_used;

        // Drop overlap-window segments (chunks N ≥ 1) and translate
        // to global timeline.
        let overlap_ms = self.config.overlap_ms();
        let global_offset_ms =
            chunk_global_offset_ms(chunk.chunk.first_sample, self.config.sample_rate);

        for seg in result.segments {
            if !is_first_chunk_on_channel && seg.t1_ms <= overlap_ms {
                // Duplicate of prior chunk's trailing region.
                continue;
            }
            chan.segments.push(TimedSegment {
                text: seg.text,
                t0_ms: global_offset_ms.saturating_add(seg.t0_ms),
                t1_ms: global_offset_ms.saturating_add(seg.t1_ms),
            });
        }

        // Update per-channel rolling state.
        chan.last_seq = Some(seq);
        chan.last_text = result.text;
        chan.chunks_done = chan.chunks_done.saturating_add(1);

        // Emit progress.
        let progress = LongFormProgress {
            channel,
            chunk_seq: seq,
            chunks_done: chan.chunks_done,
            chunks_total: None, // we don't know it until receiver closes
        };
        (self.on_progress)(progress);

        Ok(())
    }
}

// ---------------------------------------------------------------------
// Internal state
// ---------------------------------------------------------------------

#[derive(Default)]
struct State {
    mic: ChannelState,
    sys: ChannelState,
}

impl State {
    fn for_channel_mut(&mut self, ch: Channel) -> &mut ChannelState {
        match ch {
            Channel::Mic => &mut self.mic,
            Channel::Sys => &mut self.sys,
        }
    }

    fn into_output(self) -> LongFormOutput {
        LongFormOutput {
            mic_segments: self.mic.segments,
            sys_segments: self.sys.segments,
            mic_gpu_used: self.mic.gpu_used,
            sys_gpu_used: self.sys.gpu_used,
        }
    }
}

#[derive(Default)]
struct ChannelState {
    segments: Vec<TimedSegment>,
    /// Most recent chunk seq seen on this channel; `None` until the
    /// first chunk lands.
    last_seq: Option<u32>,
    /// Joined text of the most recent chunk on this channel. Source
    /// for the next chunk's `initial_prompt`.
    last_text: String,
    chunks_done: u32,
    gpu_used: bool,
}

// ---------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------

/// Parse the trailing seq number from a chunk filename of the form
/// `<uuid>_<channel>_<seq>.wav`. The chunker writes this format
/// invariantly; the regex would be overkill.
fn parse_seq_from_path(path: &std::path::Path) -> AppResult<u32> {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| AppError::LongFormStt(format!("chunk path has no file stem: {path:?}")))?;
    let seq_str = stem.rsplit('_').next().ok_or_else(|| {
        AppError::LongFormStt(format!("chunk filename missing seq suffix: {stem}"))
    })?;
    seq_str
        .parse::<u32>()
        .map_err(|e| AppError::LongFormStt(format!("chunk seq {seq_str} not a u32: {e}")))
}

/// Convert a chunk's `first_sample` (u64) to a global timestamp (ms).
/// Promotes to u64 mid-arithmetic so a 4-hour meeting (~230M samples)
/// can't overflow.
fn chunk_global_offset_ms(first_sample: u64, sample_rate: u32) -> u32 {
    (first_sample * 1000 / sample_rate as u64) as u32
}

/// CRC32 over the i16 sample stream, little-endian. Mirrors
/// [`crate::meetings::chunker`]'s internal helper of the same shape
/// so a byte-for-byte CRC equality is asserted across the write/read
/// boundary.
fn crc32_of_i16(samples: &[i16]) -> u32 {
    let mut h = crc32fast::Hasher::new();
    for s in samples {
        h.update(&s.to_le_bytes());
    }
    h.finalize()
}

/// Read a mono 16-bit PCM WAV into a `Vec<i16>`. Anything else
/// (different bit depth, multichannel, non-16k sample rate) is an
/// `AppError::LongFormStt` — the chunker writes exactly one shape
/// and a deviation means corruption upstream.
fn read_wav_i16(path: &std::path::Path) -> AppResult<Vec<i16>> {
    let reader = hound::WavReader::open(path)
        .map_err(|e| AppError::LongFormStt(format!("wav open {path:?}: {e}")))?;
    let spec = reader.spec();
    if spec.channels != 1 || spec.bits_per_sample != 16 {
        return Err(AppError::LongFormStt(format!(
            "unexpected wav spec for {path:?}: channels={}, bits={}",
            spec.channels, spec.bits_per_sample
        )));
    }
    let mut samples = Vec::with_capacity(reader.duration() as usize);
    for s in reader.into_samples::<i16>() {
        samples.push(s.map_err(|e| AppError::LongFormStt(format!("wav read {path:?}: {e}")))?);
    }
    Ok(samples)
}

/// Token-approximate tail of `text`, capped at `max_tokens`. Uses
/// the same word-based approximation as [`crate::stt::prompt_builder`]
/// (~1.3 tokens per word). Returns `None` if `text` is empty after
/// trimming — Whisper rejects an empty `initial_prompt`.
fn tail_tokens(text: &str, max_tokens: usize) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Words-budget = max_tokens / 1.3, conservatively floored.
    // (Whisper's tokenizer is sub-word, so words *underestimate*
    // token count for natural English; we floor the word budget to
    // stay safely under the 224-token cap.)
    let word_budget = ((max_tokens as f64) / 1.3).floor() as usize;
    let words: Vec<&str> = trimmed.split_whitespace().collect();
    let prompt = if words.len() <= word_budget {
        trimmed.to_string()
    } else {
        words[words.len() - word_budget..].join(" ")
    };
    Some(prompt)
}

// =====================================================================
// Tests
// =====================================================================

#[cfg(test)]
#[path = "long_form_stt_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "long_form_stt_pure_tests.rs"]
mod pure_tests;
