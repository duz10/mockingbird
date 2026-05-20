//! Per-channel rolling chunker — 30 s windows with 2 s leading overlap.
//!
//! Pure-state, deterministic. The only side-effect is writing chunk
//! WAVs to a caller-supplied directory; Wave 2 unit tests inject a
//! `chunk_dir` under `tempfile::tempdir()` and assert on the produced
//! filenames + sample counts. No timers, no clocks — the chunker
//! advances strictly on `feed(...)` calls.
//!
//! Each chunk WAV is stamped with a `crc32fast` checksum (written into
//! the WAV's `LIST INFO` chunk via a custom helper; Wave 2 ships the
//! writer) so `long_form_stt` can verify integrity before submitting
//! to Whisper. Mismatches surface as `AppError::LongFormStt`.
//!
//! Wave 1 scaffold — types + `todo!()` stubs.

use std::path::PathBuf;

use crate::error::AppResult;

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
    /// Samples per chunk window (default = 30 s * 16 kHz = 480_000).
    pub chunk_samples: u32,
    /// Leading-overlap samples (default = 2 s * 16 kHz = 32_000).
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
    #[allow(dead_code)] // Wave 2: used in feed/finalize.
    config: ChunkerConfig,
    #[allow(dead_code)] // Wave 2: used to derive chunk filenames.
    meeting_uuid: String,
    #[allow(dead_code)] // Wave 2: used to derive chunk filenames.
    channel_tag: &'static str,
    #[allow(dead_code)] // Wave 2: caller-supplied chunk directory.
    chunk_dir: PathBuf,
}

impl MeetingChunker {
    /// `channel_tag` is "mic" or "sys" — used in the chunk filename
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
        }
    }

    /// Push more samples. Returns the chunk(s) that rolled as a result
    /// (typically 0 or 1; for a very large `samples` slice could be ≥2).
    ///
    /// Wave 1: `todo!()` — Wave 2 ships the rolling-window logic
    /// (with 2 s leading overlap) and ≥12 unit tests.
    pub fn feed(&mut self, _samples: &[i16]) -> AppResult<Vec<ChunkWritten>> {
        todo!("Wave 2: implement rolling 30s/2s-overlap chunker")
    }

    /// Flush the trailing partial chunk (if any). Called by the
    /// runtime on `meeting_stop`. Returns the final chunk's metadata
    /// if one was written.
    pub fn finalize(&mut self) -> AppResult<Option<ChunkWritten>> {
        todo!("Wave 2: implement trailing-chunk flush")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Smoke: default config is sane. Wave 2 ships the ≥12 boundary/
    /// overlap/finalize tests.
    #[test]
    fn default_config_matches_adr_0029() {
        let c = ChunkerConfig::default();
        assert_eq!(c.chunk_samples, 30 * 16_000);
        assert_eq!(c.overlap_samples, 2 * 16_000);
        assert_eq!(c.sample_rate, 16_000);
    }

    #[test]
    fn new_smoke() {
        let _ = MeetingChunker::new(
            "test-uuid".into(),
            "mic",
            PathBuf::from("."),
            ChunkerConfig::default(),
        );
    }
}
