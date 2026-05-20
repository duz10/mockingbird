//! Per-channel rolling chunker — 30 s windows with 2 s leading overlap.
//!
//! Pure-state, deterministic. The only side-effect is writing chunk
//! WAVs to a caller-supplied directory. No timers, no clocks — the
//! chunker advances strictly on `feed(...)` calls.
//!
//! Each chunk WAV is stamped with a `crc32fast` checksum over the
//! sample payload bytes (i16 → little-endian → CRC32; NOT including
//! the WAV header) so [`super::long_form_stt`] can verify integrity
//! before submitting to Whisper.
//!
//! ## Sample math (defaults from ADR 0029)
//!
//! - `chunk_samples = 30s · 16 kHz = 480_000`
//! - `overlap_samples = 2s · 16 kHz = 32_000`
//! - Chunk 0 covers global samples `[0, chunk_samples)`.
//! - Chunk N (N ≥ 1) starts at `N · (chunk_samples - overlap_samples)`
//!   and covers `chunk_samples` total — the first `overlap_samples` of
//!   which are the trailing samples of chunk N-1 (the "leading
//!   overlap"). This is why the chunker keeps an `overlap_tail`
//!   buffer between rolls.
//! - `finalize()` flushes whatever remains in the pending buffer as a
//!   final (possibly short) chunk with the overlap prefix prepended.

use std::path::PathBuf;

use crate::error::{AppError, AppResult};

/// Where on disk a chunk WAV was written, with the sample range it
/// covers. Returned from [`MeetingChunker::feed`] (one per chunk that
/// rolled during the feed call; usually 0 or 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkWritten {
    pub path: PathBuf,
    /// First sample index (inclusive) in the channel's global stream.
    pub first_sample: u64,
    /// Last sample index (exclusive).
    pub last_sample: u64,
    /// CRC32 over the i16-as-LE-bytes sample payload (not the WAV
    /// header). `long_form_stt` recomputes and compares on read.
    pub crc32: u32,
}

/// Configuration for the chunker. Defaults match ADR 0029.
#[derive(Debug, Clone)]
pub struct ChunkerConfig {
    /// Samples per chunk window (default = 30 s × 16 kHz = 480_000).
    pub chunk_samples: u32,
    /// Leading-overlap samples (default = 2 s × 16 kHz = 32_000).
    pub overlap_samples: u32,
    /// Sample rate (always 16 kHz today; carried on config so a future
    /// per-meeting override can be added without a struct churn).
    pub sample_rate: u32,
}

impl Default for ChunkerConfig {
    fn default() -> Self {
        Self {
            chunk_samples: 30 * 16_000,
            overlap_samples: 2 * 16_000,
            sample_rate: 16_000,
        }
    }
}

/// Per-channel rolling chunker. One instance per active channel
/// (mic + sys when source = Both).
#[derive(Debug)]
pub struct MeetingChunker {
    config: ChunkerConfig,
    meeting_uuid: String,
    channel_tag: &'static str,
    chunk_dir: PathBuf,

    /// Samples fed but not yet committed to a chunk.
    pending: Vec<i16>,
    /// Last `overlap_samples` samples of the most recent emitted
    /// chunk. Prepended to the next chunk on roll.
    overlap_tail: Vec<i16>,
    /// Index of the NEXT chunk to be written (also the count of
    /// chunks already written).
    next_seq: u32,
    /// Global-stream sample index of the FIRST sample of the next
    /// chunk to be emitted (including its overlap prefix).
    global_first_sample: u64,
}

impl MeetingChunker {
    /// `channel_tag` is `"mic"` or `"sys"` — used in the chunk filename
    /// (`<uuid>_<channel>_<seq>.wav`).
    pub fn new(
        meeting_uuid: String,
        channel_tag: &'static str,
        chunk_dir: PathBuf,
        config: ChunkerConfig,
    ) -> Self {
        Self {
            config,
            meeting_uuid,
            channel_tag,
            chunk_dir,
            pending: Vec::new(),
            overlap_tail: Vec::new(),
            next_seq: 0,
            global_first_sample: 0,
        }
    }

    /// Push more samples. Returns the chunk(s) that rolled as a result
    /// (typically 0 or 1; for a very large `samples` slice could be ≥2).
    pub fn feed(&mut self, samples: &[i16]) -> AppResult<Vec<ChunkWritten>> {
        self.pending.extend_from_slice(samples);
        let mut out = Vec::new();

        loop {
            let needed_new = self.needed_new_samples();
            if self.pending.len() < needed_new {
                break;
            }
            let chunk = self.emit_chunk(needed_new)?;
            out.push(chunk);
        }

        Ok(out)
    }

    /// Flush the trailing partial chunk (if any). Called by the
    /// runtime on `meeting_stop`. Returns the final chunk's metadata
    /// if one was written.
    ///
    /// If `pending` is empty, returns `Ok(None)` — the previous full
    /// chunk already covered everything fed.
    pub fn finalize(&mut self) -> AppResult<Option<ChunkWritten>> {
        if self.pending.is_empty() {
            return Ok(None);
        }
        // Trailing chunk: overlap prefix + everything left in pending.
        // Drain all remaining pending samples regardless of how many.
        let needed_new = self.pending.len();
        let chunk = self.emit_chunk(needed_new)?;
        Ok(Some(chunk))
    }

    // ---------- internals ----------

    /// How many NEW samples (not counting the overlap prefix) we need
    /// in `pending` before we can emit the next chunk.
    fn needed_new_samples(&self) -> usize {
        if self.next_seq == 0 {
            self.config.chunk_samples as usize
        } else {
            (self.config.chunk_samples as usize)
                .saturating_sub(self.config.overlap_samples as usize)
        }
    }

    /// Compose the next chunk's payload (overlap prefix + `take_new`
    /// drained from pending), write the WAV, advance state, return
    /// the ChunkWritten record. Caller has already verified
    /// `pending.len() >= take_new`.
    fn emit_chunk(&mut self, take_new: usize) -> AppResult<ChunkWritten> {
        let overlap_len = self.overlap_tail.len();
        let mut payload: Vec<i16> = Vec::with_capacity(overlap_len + take_new);
        payload.extend_from_slice(&self.overlap_tail);
        payload.extend(self.pending.drain(..take_new));

        let first_sample = self.global_first_sample;
        let last_sample = first_sample + payload.len() as u64;

        let path = self.chunk_path(self.next_seq);
        let crc32 = write_chunk_wav(&path, &payload, self.config.sample_rate)?;

        // Save trailing `overlap_samples` of THIS chunk for the next.
        let ov = self.config.overlap_samples as usize;
        self.overlap_tail = if ov == 0 {
            Vec::new()
        } else if payload.len() <= ov {
            // Pathological tiny chunk: keep the whole payload (every
            // sample would be in the overlap region).
            payload.clone()
        } else {
            payload[payload.len() - ov..].to_vec()
        };

        // Next chunk's first global sample = our last - overlap (so
        // the next chunk's `first_sample` correctly points at where
        // its overlap prefix starts in the global timeline).
        self.global_first_sample = last_sample - self.overlap_tail.len() as u64;
        self.next_seq += 1;

        Ok(ChunkWritten {
            path,
            first_sample,
            last_sample,
            crc32,
        })
    }

    fn chunk_path(&self, seq: u32) -> PathBuf {
        self.chunk_dir.join(format!(
            "{}_{}_{}.wav",
            self.meeting_uuid, self.channel_tag, seq
        ))
    }
}

/// Compute the CRC32 over the i16 sample stream, little-endian. Does
/// NOT include the WAV header — the integrity check is on the audio
/// payload only so that a round-trip through hound's header writer
/// can't introduce a false negative.
fn crc32_of_i16(samples: &[i16]) -> u32 {
    let mut h = crc32fast::Hasher::new();
    for s in samples {
        h.update(&s.to_le_bytes());
    }
    h.finalize()
}

/// Write a mono 16-bit PCM WAV via `hound` and return the
/// payload CRC32.
fn write_chunk_wav(path: &std::path::Path, samples: &[i16], sample_rate: u32) -> AppResult<u32> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec)
        .map_err(|e| AppError::MeetingCapture(format!("chunk wav create: {e}")))?;
    for s in samples {
        writer
            .write_sample(*s)
            .map_err(|e| AppError::MeetingCapture(format!("chunk wav write: {e}")))?;
    }
    writer
        .finalize()
        .map_err(|e| AppError::MeetingCapture(format!("chunk wav finalize: {e}")))?;
    Ok(crc32_of_i16(samples))
}

// =====================================================================
// Tests
// =====================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    // ---------- helpers ----------

    fn make_chunker(dir: &TempDir) -> MeetingChunker {
        MeetingChunker::new(
            "test-uuid".into(),
            "mic",
            dir.path().to_path_buf(),
            ChunkerConfig::default(),
        )
    }

    fn make_chunker_with(
        dir: &TempDir,
        channel: &'static str,
        config: ChunkerConfig,
    ) -> MeetingChunker {
        MeetingChunker::new(
            "test-uuid".into(),
            channel,
            dir.path().to_path_buf(),
            config,
        )
    }

    fn zeros(n: usize) -> Vec<i16> {
        vec![0i16; n]
    }

    fn ramp(n: usize) -> Vec<i16> {
        (0..n).map(|i| (i % 32_768) as i16).collect()
    }

    // ---------- basic rolling behaviour ----------

    #[test]
    fn default_config_matches_adr_0029() {
        let c = ChunkerConfig::default();
        assert_eq!(c.chunk_samples, 30 * 16_000);
        assert_eq!(c.overlap_samples, 2 * 16_000);
        assert_eq!(c.sample_rate, 16_000);
    }

    #[test]
    fn feed_under_chunk_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = make_chunker(&dir);
        let out = c.feed(&zeros(479_999)).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn feed_exactly_one_chunk_rolls_one() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = make_chunker(&dir);
        let out = c.feed(&zeros(480_000)).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].first_sample, 0);
        assert_eq!(out[0].last_sample, 480_000);
        assert!(out[0].path.exists(), "chunk file should exist on disk");
    }

    #[test]
    fn feed_two_chunks_in_one_call() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = make_chunker(&dir);
        // 2 * chunk_samples - overlap = exactly enough for 2 chunks
        // sharing the 32 000-sample overlap.
        let out = c.feed(&zeros(2 * 480_000 - 32_000)).unwrap();
        assert_eq!(out.len(), 2);
        assert!(out[0].path.to_string_lossy().contains("_0.wav"));
        assert!(out[1].path.to_string_lossy().contains("_1.wav"));
    }

    #[test]
    fn second_chunk_includes_overlap_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = make_chunker(&dir);
        let first = c.feed(&zeros(480_000)).unwrap();
        assert_eq!(first.len(), 1);
        let second = c.feed(&ramp(480_000 - 32_000)).unwrap();
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].first_sample, 480_000 - 32_000);
        assert_eq!(second[0].last_sample, 2 * 480_000 - 32_000);
    }

    // ---------- finalize() behaviour ----------

    #[test]
    fn finalize_with_empty_pending_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = make_chunker(&dir);
        let out = c.finalize().unwrap();
        assert!(out.is_none());
    }

    #[test]
    fn finalize_with_residual_writes_trailing_chunk() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = make_chunker(&dir);
        let _ = c.feed(&zeros(100_000)).unwrap();
        let out = c.finalize().unwrap().expect("trailing chunk");
        assert_eq!(out.first_sample, 0);
        assert_eq!(out.last_sample, 100_000);
        assert!(out.path.exists());
    }

    #[test]
    fn finalize_after_full_chunk_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = make_chunker(&dir);
        let _ = c.feed(&zeros(480_000)).unwrap();
        let out = c.finalize().unwrap();
        assert!(out.is_none(), "pending should be empty after a clean roll");
    }

    // ---------- CRC + filename + WAV round-trip ----------

    #[test]
    fn crc32_matches_hand_computation() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = make_chunker(&dir);
        let r = ramp(480_000);
        let out = c.feed(&r).unwrap();
        let expected = crc32_of_i16(&r);
        assert_eq!(out[0].crc32, expected);
    }

    #[test]
    fn filenames_use_uuid_and_channel_tag() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = make_chunker(&dir);
        let out = c.feed(&zeros(480_000)).unwrap();
        let expected: PathBuf = dir.path().join("test-uuid_mic_0.wav");
        assert_eq!(out[0].path, expected);
    }

    #[test]
    fn sequential_seqs_zero_indexed() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = make_chunker(&dir);
        // 3 * chunk_samples should yield at least 2 rolls (more with
        // overlap shrinking needed_new from chunk_samples to
        // chunk_samples-overlap for chunks 1+).
        let out = c.feed(&zeros(3 * 480_000)).unwrap();
        assert!(out.len() >= 2, "expected ≥2 chunks, got {}", out.len());
        assert!(out[0].path.to_string_lossy().contains("_0.wav"));
        assert!(out[1].path.to_string_lossy().contains("_1.wav"));
    }

    #[test]
    fn mic_and_sys_chunkers_separate_files() {
        let dir = tempfile::tempdir().unwrap();
        let mut mic = make_chunker_with(&dir, "mic", ChunkerConfig::default());
        let mut sys = make_chunker_with(&dir, "sys", ChunkerConfig::default());
        let m = mic.feed(&zeros(480_000)).unwrap();
        let s = sys.feed(&zeros(480_000)).unwrap();
        assert_ne!(m[0].path, s[0].path);
        assert!(m[0].path.to_string_lossy().contains("_mic_0.wav"));
        assert!(s[0].path.to_string_lossy().contains("_sys_0.wav"));
    }

    #[test]
    fn overlap_zero_works() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = ChunkerConfig {
            overlap_samples: 0,
            ..ChunkerConfig::default()
        };
        let mut c = make_chunker_with(&dir, "mic", cfg);
        let out = c.feed(&zeros(2 * 480_000)).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].last_sample, out[1].first_sample);
    }

    #[test]
    fn very_small_chunk_size() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = ChunkerConfig {
            chunk_samples: 100,
            overlap_samples: 10,
            sample_rate: 16_000,
        };
        let mut c = make_chunker_with(&dir, "mic", cfg);
        let out = c.feed(&ramp(250)).unwrap();
        // chunk 0 takes 100; chunk 1 takes 90 (overlap covers 10);
        // residual = 250 - 100 - 90 = 60 in pending.
        assert_eq!(out.len(), 2);
        let trailing = c.finalize().unwrap();
        assert!(trailing.is_some(), "residual should flush on finalize");
    }

    #[test]
    fn wav_round_trip_via_hound() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = make_chunker(&dir);
        let r = ramp(480_000);
        let out = c.feed(&r).unwrap();
        let reader = hound::WavReader::open(&out[0].path).unwrap();
        let spec = reader.spec();
        assert_eq!(spec.channels, 1);
        assert_eq!(spec.sample_rate, 16_000);
        assert_eq!(spec.bits_per_sample, 16);
        let samples: Vec<i16> = reader
            .into_samples::<i16>()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(samples.len(), 480_000);
        // First few samples should match the input ramp.
        for (i, &s) in samples.iter().take(8).enumerate() {
            assert_eq!(s, (i % 32_768) as i16);
        }
    }
}
