//! Chunked Whisper driver — long-form transcription per channel.
//!
//! Walks the chunk WAVs produced by [`super::chunker::MeetingChunker`]
//! in order, calls `SpeechToText::transcribe_segments` (added in
//! Wave 2 per ADR 0030) on each chunk with a rolling `initial_prompt`
//! built from the last ~64 chars of the previous chunk's last segment,
//! and drops segments whose `t0` falls inside the 2 s overlap window
//! (already emitted by the previous chunk).
//!
//! Wave 1 scaffold — types + `todo!()` stubs.

use crate::error::AppResult;

/// One transcribed segment from Whisper. Mirrors `whisper.cpp`'s
/// per-segment output shape. Added to the existing `Transcript` shape
/// in `stt::` as an additive sidecar in Wave 2 (ADR 0030).
#[derive(Debug, Clone, PartialEq)]
pub struct TimedSegment {
    pub text: String,
    /// Start time in milliseconds, relative to the chunk's first sample.
    pub t0_ms: u32,
    /// End time in milliseconds, relative to the chunk's first sample.
    pub t1_ms: u32,
}

/// Progress event emitted during long-form transcription. The runtime
/// fans these out to the overlay window via Tauri's event bus.
#[derive(Debug, Clone, PartialEq)]
pub struct LongFormProgress {
    pub channel: &'static str, // "mic" | "system"
    pub chunks_done: u32,
    pub chunks_total: u32,
}

/// Per-channel long-form pass result. Segments are time-shifted into
/// the meeting's global timeline (i.e. each segment's `t0_ms` is
/// relative to the meeting start, not the chunk start) by the driver
/// before being returned.
#[derive(Debug, Clone, Default)]
pub struct LongFormResult {
    pub channel: &'static str,
    pub segments: Vec<TimedSegment>,
    pub stt_latency_ms: u64,
}

/// Drive a long-form pass over a chunk-WAV directory for one channel.
///
/// Wave 1: `todo!()` — Wave 3 ships the implementation + the
/// integration test against a 90 s synthetic fixture (3 chunks)
/// verifying stitch is loss-less and overlap dedup is correct (the
/// `mc-long-form-stitched-losslessly` judge feeds off this test).
pub fn run_long_form<P, F>(
    _channel: &'static str,
    _chunk_dir: P,
    _on_progress: F,
) -> AppResult<LongFormResult>
where
    P: AsRef<std::path::Path>,
    F: FnMut(LongFormProgress),
{
    todo!("Wave 3: implement chunked Whisper driver per ADR 0029 + ADR 0030")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Smoke: types construct without surprise. Wave 3 ships the
    /// long-form integration tests.
    #[test]
    fn timed_segment_constructs() {
        let s = TimedSegment {
            text: "hello".into(),
            t0_ms: 0,
            t1_ms: 500,
        };
        assert!(s.t1_ms > s.t0_ms);
    }

    #[test]
    fn progress_struct_constructs() {
        let p = LongFormProgress {
            channel: "mic",
            chunks_done: 4,
            chunks_total: 12,
        };
        assert!(p.chunks_done < p.chunks_total);
    }
}
