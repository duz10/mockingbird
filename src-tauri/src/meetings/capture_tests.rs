//! Tests for `meetings::capture`. Lives in its own file via
//! `#[path = "capture_tests.rs"] mod tests;` so `capture.rs` itself
//! stays under the 600-line hard limit.
//!
//! The tests fall into two groups:
//!   1. MeetingSource pure-value tests (carried forward from Wave 1).
//!   2. TwinStreamCapture integration tests driven by a synthetic
//!      `StubCapture` (Wave 3). The stub plays pre-loaded sample
//!      batches into the owner-thread polling loop, so we can assert
//!      on ChannelChunks without touching real audio hardware.

use super::*;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------------
// Carry-forward: MeetingSource value tests (Wave 1)
// ---------------------------------------------------------------------

#[test]
fn db_str_round_trip() {
    for s in [
        MeetingSource::Mic,
        MeetingSource::System,
        MeetingSource::Both,
    ] {
        assert_eq!(MeetingSource::from_db_str(s.as_db_str()), Some(s));
    }
}

#[test]
fn from_db_str_rejects_unknown() {
    assert!(MeetingSource::from_db_str("speakers").is_none());
    assert!(MeetingSource::from_db_str("").is_none());
    assert!(MeetingSource::from_db_str("MIC").is_none()); // case-sensitive
}

#[test]
fn needs_flags_correct() {
    assert!(MeetingSource::Mic.needs_mic());
    assert!(!MeetingSource::Mic.needs_system());
    assert!(!MeetingSource::System.needs_mic());
    assert!(MeetingSource::System.needs_system());
    assert!(MeetingSource::Both.needs_mic());
    assert!(MeetingSource::Both.needs_system());
}

#[test]
fn channel_tag_strings_match_chunker() {
    // The chunker filename `<uuid>_<channel>_<seq>.wav` depends on
    // these exact strings; a future rename would silently break the
    // long-form stitch downstream.
    assert_eq!(Channel::Mic.tag(), "mic");
    assert_eq!(Channel::Sys.tag(), "sys");
}

// ---------------------------------------------------------------------
// Synthetic StubCapture for TwinStreamCapture integration tests
// ---------------------------------------------------------------------

/// Test-only AudioCapture. Pre-loaded with a queue of i16 batches;
/// each `drain()` call pops the next batch and appends it to `buf`.
/// Returns `Ok(0)` when the queue is empty.
///
/// `Send` because everything inside is `Send`. The TwinStreamCapture
/// constructs it via a `CaptureBuilder` closure that's run on the
/// owner thread, so neither the closure nor the resulting box ever
/// has to cross a thread boundary anyway — but `Send` keeps the
/// closure shape simple.
struct StubCapture {
    feed: Arc<Mutex<VecDeque<Vec<i16>>>>,
    started: bool,
}

impl StubCapture {
    fn new(feed: Arc<Mutex<VecDeque<Vec<i16>>>>) -> Self {
        Self {
            feed,
            started: false,
        }
    }
}

impl AudioCapture for StubCapture {
    fn start(&mut self) -> AppResult<()> {
        self.started = true;
        Ok(())
    }
    fn stop(&mut self) -> AppResult<()> {
        self.started = false;
        Ok(())
    }
    fn drain(&mut self, buf: &mut Vec<i16>) -> AppResult<usize> {
        let mut q = self.feed.lock().unwrap();
        if let Some(batch) = q.pop_front() {
            let n = batch.len();
            buf.extend(batch);
            Ok(n)
        } else {
            Ok(0)
        }
    }
    fn sample_rate(&self) -> u32 {
        16_000
    }
    fn channels(&self) -> u16 {
        1
    }
}

// Helpers ---------------------------------------------------------------

/// Small chunker config so 1-second test feeds actually roll chunks.
/// Default ChunkerConfig is 30 s / 2 s overlap — way too coarse for
/// unit tests. This shrinks to 1 s / 0.1 s overlap.
fn test_config() -> ChunkerConfig {
    ChunkerConfig {
        chunk_samples: 16_000,  // 1 s @ 16 kHz
        overlap_samples: 1_600, // 0.1 s @ 16 kHz
        sample_rate: 16_000,
    }
}

fn stub_feed(batches: Vec<Vec<i16>>) -> Arc<Mutex<VecDeque<Vec<i16>>>> {
    Arc::new(Mutex::new(VecDeque::from(batches)))
}

fn stub_builder(feed: Arc<Mutex<VecDeque<Vec<i16>>>>) -> CaptureBuilder {
    Box::new(move || Ok(Box::new(StubCapture::new(feed)) as Box<dyn AudioCapture>))
}

/// Poll `try_recv_chunks` until at least `n` chunks have been seen or
/// `timeout` elapses. Returns whatever was collected (may be < n on
/// timeout — callers assert on it).
fn wait_chunks(twin: &mut TwinStreamCapture, n: usize, timeout: Duration) -> Vec<ChannelChunk> {
    let start = std::time::Instant::now();
    let mut out = Vec::new();
    while out.len() < n && start.elapsed() < timeout {
        out.extend(twin.try_recv_chunks());
        if out.len() < n {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    out
}

// ---------------------------------------------------------------------
// TwinStreamCapture integration tests
// ---------------------------------------------------------------------

#[test]
fn no_builders_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let r = TwinStreamCapture::start_with(
        "test".into(),
        dir.path().to_path_buf(),
        test_config(),
        None,
        None,
    );
    assert!(r.is_err(), "must reject all-None builders");
}

#[test]
fn single_channel_mic_only_emits_chunks_in_order() {
    let dir = tempfile::tempdir().unwrap();
    // 1.5 s of audio fed in one batch = 1 full 1-s chunk + 0.5 s
    // residual that finalize() will emit on stop.
    let feed = stub_feed(vec![vec![100i16; 24_000]]);
    let mut twin = TwinStreamCapture::start_with(
        "test".into(),
        dir.path().to_path_buf(),
        test_config(),
        Some(stub_builder(feed)),
        None,
    )
    .unwrap();

    let mid_chunks = wait_chunks(&mut twin, 1, Duration::from_secs(2));
    let trailing = twin.stop().unwrap();
    let all: Vec<_> = mid_chunks.into_iter().chain(trailing).collect();

    assert!(!all.is_empty(), "expected ≥1 chunk, got {}", all.len());
    for c in &all {
        assert_eq!(c.channel, Channel::Mic, "mic-only must not emit Sys");
    }
    // Seqs are zero-indexed and monotonic by construction (chunker
    // contract). Verify by checking the filenames have ascending seqs.
    let seqs: Vec<u32> = all
        .iter()
        .filter_map(|c| {
            c.chunk
                .path
                .file_stem()
                .and_then(|s| s.to_str())
                .and_then(|s| s.rsplit('_').next())
                .and_then(|s| s.parse::<u32>().ok())
        })
        .collect();
    for w in seqs.windows(2) {
        assert!(w[0] < w[1], "seqs must be strictly increasing: {seqs:?}");
    }
}

#[test]
fn both_sources_produce_disjoint_filenames() {
    let dir = tempfile::tempdir().unwrap();
    let mic_feed = stub_feed(vec![vec![100i16; 32_000]]); // 2 s
    let sys_feed = stub_feed(vec![vec![-100i16; 32_000]]);
    let mut twin = TwinStreamCapture::start_with(
        "test".into(),
        dir.path().to_path_buf(),
        test_config(),
        Some(stub_builder(mic_feed)),
        Some(stub_builder(sys_feed)),
    )
    .unwrap();

    // 2 s × 2 channels = 4 chunks roughly (modulo overlap).
    let mid = wait_chunks(&mut twin, 2, Duration::from_secs(3));
    let trailing = twin.stop().unwrap();
    let all: Vec<_> = mid.into_iter().chain(trailing).collect();

    let mic_paths: Vec<_> = all
        .iter()
        .filter(|c| c.channel == Channel::Mic)
        .map(|c| c.chunk.path.clone())
        .collect();
    let sys_paths: Vec<_> = all
        .iter()
        .filter(|c| c.channel == Channel::Sys)
        .map(|c| c.chunk.path.clone())
        .collect();
    assert!(!mic_paths.is_empty(), "no mic chunks observed");
    assert!(!sys_paths.is_empty(), "no sys chunks observed");
    for p in &mic_paths {
        assert!(
            p.to_string_lossy().contains("_mic_"),
            "mic path doesn't contain _mic_: {p:?}"
        );
    }
    for p in &sys_paths {
        assert!(
            p.to_string_lossy().contains("_sys_"),
            "sys path doesn't contain _sys_: {p:?}"
        );
        assert!(!mic_paths.contains(p), "mic+sys filename collision: {p:?}");
    }
}

#[test]
fn stop_returns_trailing_chunks_from_both() {
    let dir = tempfile::tempdir().unwrap();
    // 0.5 s per channel = no full chunk, but finalize() emits a
    // trailing partial chunk per channel.
    let mic_feed = stub_feed(vec![vec![100i16; 8_000]]);
    let sys_feed = stub_feed(vec![vec![-100i16; 8_000]]);
    let mut twin = TwinStreamCapture::start_with(
        "test".into(),
        dir.path().to_path_buf(),
        test_config(),
        Some(stub_builder(mic_feed)),
        Some(stub_builder(sys_feed)),
    )
    .unwrap();
    // Give the owner threads a tick to drain the feed.
    std::thread::sleep(Duration::from_millis(150));

    // try_recv now would only see chunks rolled DURING the loop;
    // for 0.5 s feeds, those should be zero. The trailing chunks
    // surface from stop().
    let trailing = twin.stop().unwrap();
    let mic_count = trailing
        .iter()
        .filter(|c| c.channel == Channel::Mic)
        .count();
    let sys_count = trailing
        .iter()
        .filter(|c| c.channel == Channel::Sys)
        .count();
    assert!(
        mic_count >= 1,
        "expected ≥1 trailing mic chunk, got {mic_count}"
    );
    assert!(
        sys_count >= 1,
        "expected ≥1 trailing sys chunk, got {sys_count}"
    );
}

#[test]
fn drop_without_explicit_stop_finalizes_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let chunk_dir = dir.path().to_path_buf();
    let feed = stub_feed(vec![vec![100i16; 8_000]]);
    {
        let _twin = TwinStreamCapture::start_with(
            "test".into(),
            chunk_dir.clone(),
            test_config(),
            Some(stub_builder(feed)),
            None,
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(150));
        // Drop fires when _twin goes out of scope at end of block.
    }
    // Verify at least one WAV file was written by the trailing
    // finalize() — proves Drop didn't skip the shutdown path.
    let wavs: Vec<_> = std::fs::read_dir(&chunk_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".wav"))
        .collect();
    assert!(!wavs.is_empty(), "Drop must finalize at least one WAV file");
}

#[test]
fn try_recv_chunks_is_nonblocking_when_empty() {
    let dir = tempfile::tempdir().unwrap();
    let feed = stub_feed(vec![]); // no batches
    let mut twin = TwinStreamCapture::start_with(
        "test".into(),
        dir.path().to_path_buf(),
        test_config(),
        Some(stub_builder(feed)),
        None,
    )
    .unwrap();
    // Call before any owner-loop tick has had a chance to drain.
    // Must return immediately with an empty vec (not block).
    let t0 = std::time::Instant::now();
    let chunks = twin.try_recv_chunks();
    assert!(t0.elapsed() < Duration::from_millis(10));
    assert!(chunks.is_empty());
    let _ = twin.stop();
}

#[test]
fn double_stop_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let feed = stub_feed(vec![vec![0i16; 4_000]]);
    let mut twin = TwinStreamCapture::start_with(
        "test".into(),
        dir.path().to_path_buf(),
        test_config(),
        Some(stub_builder(feed)),
        None,
    )
    .unwrap();
    let _ = twin.stop().unwrap();
    let second = twin.stop().unwrap();
    assert!(second.is_empty(), "second stop must not double-emit chunks");
}

#[test]
fn clock_alignment_first_sample_is_zero_for_both() {
    let dir = tempfile::tempdir().unwrap();
    let mic_feed = stub_feed(vec![vec![100i16; 32_000]]);
    let sys_feed = stub_feed(vec![vec![-100i16; 32_000]]);
    let mut twin = TwinStreamCapture::start_with(
        "test".into(),
        dir.path().to_path_buf(),
        test_config(),
        Some(stub_builder(mic_feed)),
        Some(stub_builder(sys_feed)),
    )
    .unwrap();
    let mid = wait_chunks(&mut twin, 2, Duration::from_secs(3));
    let trailing = twin.stop().unwrap();
    let all: Vec<_> = mid.into_iter().chain(trailing).collect();

    // The FIRST chunk on each channel must start at global sample 0
    // (the chunker's per-channel timeline is zero-indexed).
    let mic_first = all
        .iter()
        .find(|c| c.channel == Channel::Mic)
        .expect("mic chunk");
    let sys_first = all
        .iter()
        .find(|c| c.channel == Channel::Sys)
        .expect("sys chunk");
    assert_eq!(mic_first.chunk.first_sample, 0);
    assert_eq!(sys_first.chunk.first_sample, 0);
}

#[test]
fn system_only_mode_skips_mic_thread() {
    let dir = tempfile::tempdir().unwrap();
    let sys_feed = stub_feed(vec![vec![0i16; 4_000]]);
    let twin = TwinStreamCapture::start_with(
        "test".into(),
        dir.path().to_path_buf(),
        test_config(),
        None,
        Some(stub_builder(sys_feed)),
    )
    .unwrap();
    // Internal field — readable inside the same module (we're in
    // `super`'s tests submodule via the `#[path]` pull-in).
    assert!(twin.mic_thread.is_none(), "mic thread must not spawn");
    assert!(twin.sys_thread.is_some(), "sys thread must spawn");
    // Cleanup via Drop.
}

#[test]
fn mic_only_mode_skips_sys_thread() {
    let dir = tempfile::tempdir().unwrap();
    let mic_feed = stub_feed(vec![vec![0i16; 4_000]]);
    let twin = TwinStreamCapture::start_with(
        "test".into(),
        dir.path().to_path_buf(),
        test_config(),
        Some(stub_builder(mic_feed)),
        None,
    )
    .unwrap();
    assert!(twin.mic_thread.is_some(), "mic thread must spawn");
    assert!(twin.sys_thread.is_none(), "sys thread must not spawn");
}

#[test]
fn channel_chunks_carry_correct_channel_tag() {
    // Regression: a refactor that swaps the mic vs sys channel
    // labels would still produce filenames with the right tag (the
    // chunker bakes it in) but the runtime would route the wrong
    // bytes to the wrong transcript row.
    let dir = tempfile::tempdir().unwrap();
    let mic_feed = stub_feed(vec![vec![100i16; 24_000]]);
    let sys_feed = stub_feed(vec![vec![-100i16; 24_000]]);
    let mut twin = TwinStreamCapture::start_with(
        "test".into(),
        dir.path().to_path_buf(),
        test_config(),
        Some(stub_builder(mic_feed)),
        Some(stub_builder(sys_feed)),
    )
    .unwrap();
    let mid = wait_chunks(&mut twin, 2, Duration::from_secs(3));
    let trailing = twin.stop().unwrap();
    let all: Vec<_> = mid.into_iter().chain(trailing).collect();
    for c in &all {
        let path_str = c.chunk.path.to_string_lossy();
        match c.channel {
            Channel::Mic => assert!(path_str.contains("_mic_"), "mic chan path: {path_str}"),
            Channel::Sys => assert!(path_str.contains("_sys_"), "sys chan path: {path_str}"),
        }
    }
}
