//! Stitch / integration tests for [`super::LongFormStt`].
//! A `StubStt` returns canned `TranscriptWithSegments` per call and
//! records every `initial_prompt`; chunks are written via the real
//! `MeetingChunker` so the CRC32 contract is exercised end-to-end.
//! Pure-helper tests are in `long_form_stt_pure_tests.rs`.

use super::*;
use crate::meetings::capture::{Channel, ChannelChunk};
use crate::meetings::chunker::{ChunkerConfig, MeetingChunker};
use crate::stt::{SpeechToText, SttSegment, TranscribeRequest, Transcript, TranscriptWithSegments};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------------
// StubStt — scripted SpeechToText for stitch-algorithm testing
// ---------------------------------------------------------------------

/// One scripted response. Per call to `transcribe_segments`, the next
/// `ScriptEntry` is consumed.
#[derive(Debug, Clone)]
struct ScriptEntry {
    segments: Vec<SttSegment>,
    gpu_used: bool,
}

#[derive(Default)]
struct StubInner {
    script: Vec<ScriptEntry>,
    /// Each call records the `initial_prompt` it was passed (None
    /// for chunk 0 on a given channel).
    prompts_seen: Vec<Option<String>>,
    /// Audio lengths in samples (so tests can sanity-check the
    /// driver actually read each chunk's PCM).
    audio_lens: Vec<usize>,
}

struct StubStt {
    inner: Arc<Mutex<StubInner>>,
}

impl StubStt {
    fn with_script(entries: Vec<ScriptEntry>) -> (Self, Arc<Mutex<StubInner>>) {
        let inner = Arc::new(Mutex::new(StubInner {
            script: entries,
            prompts_seen: Vec::new(),
            audio_lens: Vec::new(),
        }));
        (
            Self {
                inner: inner.clone(),
            },
            inner,
        )
    }
}

impl SpeechToText for StubStt {
    fn transcribe(&mut self, _req: TranscribeRequest<'_>) -> AppResult<Transcript> {
        unreachable!("LongFormStt only calls transcribe_segments")
    }

    fn transcribe_segments(
        &mut self,
        req: TranscribeRequest<'_>,
    ) -> AppResult<TranscriptWithSegments> {
        let mut inner = self.inner.lock().unwrap();
        inner
            .prompts_seen
            .push(req.initial_prompt.map(String::from));
        inner.audio_lens.push(req.audio.len());
        let entry = if inner.script.is_empty() {
            ScriptEntry {
                segments: vec![],
                gpu_used: false,
            }
        } else {
            inner.script.remove(0)
        };
        let text = entry
            .segments
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        Ok(TranscriptWithSegments {
            text,
            segments: entry.segments,
            gpu_used: entry.gpu_used,
            latency_ms: 1,
            model_id: "stub".into(),
        })
    }
}

// ---------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------

fn seg(text: &str, t0: u32, t1: u32) -> SttSegment {
    SttSegment {
        text: text.into(),
        t0_ms: t0,
        t1_ms: t1,
    }
}

fn entry(segments: Vec<SttSegment>) -> ScriptEntry {
    ScriptEntry {
        segments,
        gpu_used: false,
    }
}

/// Write chunks for `samples` via a real `MeetingChunker`, then
/// stream them into a `(Sender, Receiver)` pair tagged with `channel`.
/// Returns `(receiver, written_chunks)`.
fn build_channel_with_chunks(
    dir: &std::path::Path,
    uuid: &str,
    channel: Channel,
    config: ChunkerConfig,
    samples: &[i16],
) -> (mpsc::Receiver<ChannelChunk>, Vec<ChannelChunk>) {
    let (tx, rx) = mpsc::channel::<ChannelChunk>();
    let mut chunker =
        MeetingChunker::new(uuid.to_string(), channel.tag(), dir.to_path_buf(), config);
    let rolled = chunker.feed(samples).unwrap();
    let trailing = chunker.finalize().unwrap();
    let mut all: Vec<_> = rolled
        .into_iter()
        .map(|c| ChannelChunk { channel, chunk: c })
        .collect();
    if let Some(t) = trailing {
        all.push(ChannelChunk { channel, chunk: t });
    }
    for c in &all {
        tx.send(ChannelChunk {
            channel: c.channel,
            chunk: c.chunk.clone(),
        })
        .unwrap();
    }
    drop(tx);
    (rx, all)
}

/// Push pre-built ChannelChunks into a fresh channel and drop the
/// sender. Returns the receiver.
fn channel_from_chunks(chunks: Vec<ChannelChunk>) -> mpsc::Receiver<ChannelChunk> {
    let (tx, rx) = mpsc::channel::<ChannelChunk>();
    for c in chunks {
        tx.send(c).unwrap();
    }
    drop(tx);
    rx
}

/// Tiny chunker config for unit tests (1-s window, 0.1-s overlap).
/// Default chunker config is 30 s / 2 s overlap — too coarse to drive
/// "multiple chunks per test feed" cheaply.
fn tiny_chunker_config() -> ChunkerConfig {
    ChunkerConfig {
        chunk_samples: 16_000,
        overlap_samples: 1_600,
        sample_rate: 16_000,
    }
}

/// Long-form driver config matching `tiny_chunker_config` (so the
/// dedup window lines up with what the chunker actually wrote).
fn tiny_long_form_config() -> LongFormConfig {
    LongFormConfig {
        overlap_samples: 1_600,
        sample_rate: 16_000,
        max_prompt_tokens: 224,
    }
}

// ---------------------------------------------------------------------
// Stitch-algorithm tests (Wave 3)
// ---------------------------------------------------------------------

#[test]
fn single_chunk_passes_through_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    // 0.5 s of audio = 8_000 samples; finalize() emits 1 chunk.
    let (rx, written) = build_channel_with_chunks(
        dir.path(),
        "uuid",
        Channel::Mic,
        tiny_chunker_config(),
        &vec![100i16; 8_000],
    );
    assert_eq!(written.len(), 1, "expected 1 chunk for 0.5 s feed");

    let script = vec![ScriptEntry {
        segments: vec![seg("hello world", 0, 500)],
        gpu_used: true,
    }];
    let (mut stt, inner) = StubStt::with_script(script);

    let driver = LongFormStt::new(&mut stt, rx, |_| {}, tiny_long_form_config());
    let out = driver.run().unwrap();

    assert_eq!(out.mic_segments.len(), 1);
    assert_eq!(out.mic_segments[0].text, "hello world");
    // first_sample is 0, so global == local.
    assert_eq!(out.mic_segments[0].t0_ms, 0);
    assert_eq!(out.mic_segments[0].t1_ms, 500);
    assert_eq!(out.sys_segments.len(), 0);
    // gpu_used must propagate per-channel: mic = true (stub said so),
    // sys = false (no chunks ever processed on sys).
    assert!(out.mic_gpu_used);
    assert!(!out.sys_gpu_used);

    let inner = inner.lock().unwrap();
    assert_eq!(inner.prompts_seen.len(), 1);
    assert_eq!(
        inner.prompts_seen[0], None,
        "chunk 0 must not carry an initial_prompt"
    );
}

#[test]
fn two_chunks_overlap_dedup_drops_overlap_segments() {
    let dir = tempfile::tempdir().unwrap();
    // 1.5 s of audio with 1-s chunks + 0.1-s overlap:
    //   chunk 0: samples [0, 16_000), first_sample = 0
    //   chunk 1: samples [14_400, 16_000 + residual), first_sample = 14_400
    //   (chunker carries 1_600-sample overlap tail forward)
    let (rx, written) = build_channel_with_chunks(
        dir.path(),
        "uuid",
        Channel::Sys,
        tiny_chunker_config(),
        &vec![100i16; 24_000],
    );
    assert!(
        written.len() >= 2,
        "expected ≥2 chunks, got {}",
        written.len()
    );

    // Chunk 0 emits one segment spanning the whole window.
    // Chunk 1 emits TWO segments: one inside the 100 ms overlap
    // (must be dropped) and one after (must be kept and shifted to
    // global timeline = chunk1.first_sample + local).
    let script = vec![
        entry(vec![seg("chunk-zero", 0, 1000)]),
        entry(vec![
            seg("dup-in-overlap", 0, 50), // t1 <= 100 ms → drop
            seg("survivor", 100, 600),    // t1 > 100 ms → keep
        ]),
    ];
    let (mut stt, _inner) = StubStt::with_script(script);

    let driver = LongFormStt::new(&mut stt, rx, |_| {}, tiny_long_form_config());
    let out = driver.run().unwrap();

    let texts: Vec<&str> = out.sys_segments.iter().map(|s| s.text.as_str()).collect();
    assert!(
        texts.contains(&"chunk-zero"),
        "missing chunk 0 segment: {texts:?}"
    );
    assert!(
        texts.contains(&"survivor"),
        "missing post-overlap segment: {texts:?}"
    );
    assert!(
        !texts.contains(&"dup-in-overlap"),
        "overlap-window segment was not dropped: {texts:?}"
    );

    // Survivor's global timeline: chunk 1 starts at sample 14_400 →
    // 900 ms; local t0=100 ms → global t0 = 1000 ms; local t1=600 ms
    // → global t1 = 1500 ms.
    let survivor = out
        .sys_segments
        .iter()
        .find(|s| s.text == "survivor")
        .unwrap();
    assert_eq!(survivor.t0_ms, 1000);
    assert_eq!(survivor.t1_ms, 1500);
}

#[test]
fn crc32_mismatch_returns_long_form_stt_error() {
    let dir = tempfile::tempdir().unwrap();
    let (_, written) = build_channel_with_chunks(
        dir.path(),
        "uuid",
        Channel::Mic,
        tiny_chunker_config(),
        &vec![1i16; 8_000],
    );
    assert_eq!(written.len(), 1);
    // Corrupt the WAV on disk: overwrite the last data byte. The
    // chunker computed CRC over the original samples; rereading will
    // produce a different CRC → AppError::LongFormStt.
    let path = &written[0].chunk.path;
    let mut bytes = std::fs::read(path).unwrap();
    let last = bytes.len() - 1;
    bytes[last] = bytes[last].wrapping_add(1);
    std::fs::write(path, bytes).unwrap();

    let rx = channel_from_chunks(written);
    let script = vec![entry(vec![seg("never reached", 0, 100)])];
    let (mut stt, _) = StubStt::with_script(script);

    let driver = LongFormStt::new(&mut stt, rx, |_| {}, tiny_long_form_config());
    let err = driver.run().unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("CRC mismatch"),
        "expected CRC error, got: {msg}"
    );
}

#[test]
fn initial_prompt_is_tail_of_prior_chunk_text() {
    let dir = tempfile::tempdir().unwrap();
    // 2.5 s of audio → ≥3 chunks at tiny config.
    let (rx, written) = build_channel_with_chunks(
        dir.path(),
        "uuid",
        Channel::Mic,
        tiny_chunker_config(),
        &vec![100i16; 40_000],
    );
    assert!(
        written.len() >= 3,
        "expected ≥3 chunks, got {}",
        written.len()
    );

    // Each chunk emits a unique segment so we can verify the prompt
    // is the trailing text of the immediately prior chunk.
    let script: Vec<_> = (0..written.len())
        .map(|i| entry(vec![seg(&format!("chunk-{i}-words"), 200, 800)]))
        .collect();
    let (mut stt, inner) = StubStt::with_script(script);

    let driver = LongFormStt::new(&mut stt, rx, |_| {}, tiny_long_form_config());
    let _ = driver.run().unwrap();

    let inner = inner.lock().unwrap();
    assert_eq!(inner.prompts_seen[0], None, "chunk 0: no prompt");
    for i in 1..inner.prompts_seen.len() {
        let prompt = inner.prompts_seen[i]
            .as_ref()
            .unwrap_or_else(|| panic!("chunk {i} missing initial_prompt"));
        let expected_prior = format!("chunk-{}-words", i - 1);
        assert!(
            prompt.contains(&expected_prior),
            "chunk {i} prompt should contain prior text {expected_prior:?}, got {prompt:?}"
        );
    }
}

#[test]
fn progress_events_emitted_per_chunk() {
    let dir = tempfile::tempdir().unwrap();
    let (rx, written) = build_channel_with_chunks(
        dir.path(),
        "uuid",
        Channel::Mic,
        tiny_chunker_config(),
        &vec![100i16; 24_000],
    );
    let expected = written.len() as u32;
    let script: Vec<_> = (0..written.len())
        .map(|i| entry(vec![seg(&format!("s{i}"), 0, 500)]))
        .collect();
    let (mut stt, _) = StubStt::with_script(script);

    let events: Arc<Mutex<Vec<LongFormProgress>>> = Arc::new(Mutex::new(Vec::new()));
    let events_clone = events.clone();
    let driver = LongFormStt::new(
        &mut stt,
        rx,
        move |p| events_clone.lock().unwrap().push(p),
        tiny_long_form_config(),
    );
    let _ = driver.run().unwrap();
    let events = events.lock().unwrap();
    assert_eq!(events.len(), expected as usize);
    // chunks_done counter must be monotonic + reach `expected`.
    for (i, p) in events.iter().enumerate() {
        assert_eq!(p.chunks_done, (i + 1) as u32);
        assert_eq!(p.channel, Channel::Mic);
        assert!(
            p.chunks_total.is_none(),
            "total is unknown until receiver closes"
        );
    }
}

#[test]
fn two_channels_yield_independent_segment_streams() {
    let dir = tempfile::tempdir().unwrap();
    // Build chunks separately for mic and sys.
    let (_, mic_chunks) = build_channel_with_chunks(
        dir.path(),
        "uuid-mic",
        Channel::Mic,
        tiny_chunker_config(),
        &vec![100i16; 16_000],
    );
    let (_, sys_chunks) = build_channel_with_chunks(
        dir.path(),
        "uuid-sys",
        Channel::Sys,
        tiny_chunker_config(),
        &vec![-100i16; 16_000],
    );
    // Interleave mic + sys into one combined stream — order matters
    // less than that the stitch keeps them on separate vectors.
    let mut interleaved = Vec::new();
    let mut mi = mic_chunks.into_iter();
    let mut si = sys_chunks.into_iter();
    loop {
        match (mi.next(), si.next()) {
            (Some(m), Some(s)) => {
                interleaved.push(m);
                interleaved.push(s);
            }
            (Some(m), None) => interleaved.push(m),
            (None, Some(s)) => interleaved.push(s),
            (None, None) => break,
        }
    }
    let rx = channel_from_chunks(interleaved);

    // Stub script alternates mic/sys responses. Easier: just one
    // canned per call, marked by channel-distinct text.
    let script = vec![
        entry(vec![seg("mic-0", 0, 300)]),
        entry(vec![seg("sys-0", 0, 300)]),
        entry(vec![seg("mic-1", 200, 500)]),
        entry(vec![seg("sys-1", 200, 500)]),
    ];
    let (mut stt, _) = StubStt::with_script(script);

    let driver = LongFormStt::new(&mut stt, rx, |_| {}, tiny_long_form_config());
    let out = driver.run().unwrap();

    // Mic channel must contain only mic-* texts; sys only sys-*.
    for s in &out.mic_segments {
        assert!(
            s.text.starts_with("mic-"),
            "mic channel has cross-channel segment: {:?}",
            s.text
        );
    }
    for s in &out.sys_segments {
        assert!(
            s.text.starts_with("sys-"),
            "sys channel has cross-channel segment: {:?}",
            s.text
        );
    }
    assert!(!out.mic_segments.is_empty());
    assert!(!out.sys_segments.is_empty());
}

#[test]
fn chunks_arrive_out_of_order_within_channel_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let (_, mut chunks) = build_channel_with_chunks(
        dir.path(),
        "uuid",
        Channel::Mic,
        tiny_chunker_config(),
        &vec![100i16; 32_000],
    );
    assert!(chunks.len() >= 3, "need ≥3 chunks for the reorder test");
    // Reorder to 0, 2, 1 (forward then backward).
    let c0 = chunks.remove(0);
    let c1 = chunks.remove(0);
    let c2 = chunks.remove(0);
    let reordered = vec![c0, c2, c1];
    let rx = channel_from_chunks(reordered);

    let script = vec![
        entry(vec![seg("a", 0, 100)]),
        entry(vec![seg("b", 0, 100)]),
        entry(vec![seg("c", 0, 100)]),
    ];
    let (mut stt, _) = StubStt::with_script(script);

    let driver = LongFormStt::new(&mut stt, rx, |_| {}, tiny_long_form_config());
    let err = driver.run().unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("non-monotonic seq"),
        "expected non-monotonic error, got: {msg}"
    );
}

#[test]
fn empty_receiver_returns_empty_output() {
    let (tx, rx) = mpsc::channel::<ChannelChunk>();
    drop(tx); // disconnect immediately
    let (mut stt, _) = StubStt::with_script(Vec::new());
    let driver = LongFormStt::new(&mut stt, rx, |_| {}, tiny_long_form_config());
    let out = driver.run().unwrap();
    assert!(out.mic_segments.is_empty());
    assert!(out.sys_segments.is_empty());
    assert!(!out.mic_gpu_used);
    assert!(!out.sys_gpu_used);
}

#[test]
fn segments_are_sorted_by_global_t0_ms() {
    let dir = tempfile::tempdir().unwrap();
    let (rx, written) = build_channel_with_chunks(
        dir.path(),
        "uuid",
        Channel::Mic,
        tiny_chunker_config(),
        &vec![100i16; 24_000],
    );
    let script: Vec<_> = (0..written.len())
        .map(|_| entry(vec![seg("first", 200, 400), seg("second", 500, 700)]))
        .collect();
    let (mut stt, _) = StubStt::with_script(script);

    let driver = LongFormStt::new(&mut stt, rx, |_| {}, tiny_long_form_config());
    let out = driver.run().unwrap();
    // Global t0_ms strictly non-decreasing.
    for w in out.mic_segments.windows(2) {
        assert!(
            w[0].t0_ms <= w[1].t0_ms,
            "segments not sorted: {} > {}",
            w[0].t0_ms,
            w[1].t0_ms
        );
    }
}

// ---------------------------------------------------------------------
// Lossless 90-s integration test (Phase MC plan §3.3 task #3)
// ---------------------------------------------------------------------

#[test]
fn lossless_synthetic_long_feed_no_gaps_no_dupes() {
    // Plan asks for 90 s @ 30 s chunks. We re-use tiny_chunker_config
    // (1 s / 0.1 s) for ~30 chunks instead — stitch invariants are
    // chunk-size-independent, so this catches the same regressions
    // without writing 4 MB to disk per run.
    let dir = tempfile::tempdir().unwrap();
    let total_samples = 30 * 16_000; // 30 s @ 16 kHz
    let samples: Vec<i16> = (0..total_samples)
        .map(|i| (i % 1000) as i16) // ramp pattern; not interpreted
        .collect();
    let (rx, written) = build_channel_with_chunks(
        dir.path(),
        "uuid",
        Channel::Mic,
        tiny_chunker_config(),
        &samples,
    );
    let n = written.len();
    assert!(n >= 25, "expected ≥25 chunks for 30 s feed, got {n}");

    // Two segments per chunk: [0, 100] (dropped on N≥1 — inside the
    // 100 ms overlap window) and [100, 1000] (kept). The first chunk
    // keeps both; subsequent ones keep only "body".
    let script: Vec<_> = (0..n)
        .map(|_| entry(vec![seg("overlap-dup", 0, 100), seg("body", 100, 1000)]))
        .collect();
    let (mut stt, _) = StubStt::with_script(script);

    let driver = LongFormStt::new(&mut stt, rx, |_| {}, tiny_long_form_config());
    let out = driver.run().unwrap();

    // The first chunk keeps both segments (no overlap to drop).
    // Subsequent chunks keep only "body". So total segments =
    // 2 + (n - 1) = n + 1.
    assert_eq!(
        out.mic_segments.len(),
        n + 1,
        "expected {} kept segments, got {}",
        n + 1,
        out.mic_segments.len()
    );

    // No duplicate `(t0_ms, t1_ms)` pairs allowed.
    let mut seen = std::collections::HashSet::new();
    for s in &out.mic_segments {
        let key = (s.t0_ms, s.t1_ms, s.text.clone());
        assert!(
            seen.insert(key.clone()),
            "duplicate segment in stitched output: {key:?}"
        );
    }
}
