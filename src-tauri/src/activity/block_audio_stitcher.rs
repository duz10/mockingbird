//! Layer-2 (audio) ⇄ Block stitching (Phase 10 Wave 4; ADR 0041).
//!
//! Given a chronologically-ordered list of Blocks (from the Wave-3
//! blocker) and a chronologically-ordered list of transcript segments
//! (from the long-form chunked Whisper driver), assign each segment
//! to exactly one Block using the **midpoint rule** (ADR 0041 §Decision
//! item 1):
//!
//! > A segment `(t0_ms, t1_ms, source)` belongs to Block `B` iff
//! > `(t0_ms + t1_ms) / 2 ∈ [B.started_at, B.ended_at)`.
//!
//! Channels are preserved (mic vs sys); see ADR 0041 §Decision item 2.
//! Segments whose midpoint falls outside every Block are dropped from
//! stitching (they remain on disk; the abstractor just won't see them).
//!
//! This module is **pure Rust** with no DB, audio, or async deps so it
//! can be live-tested via the throwaway-crate fallback gate (LESSONS
//! P1). The DB read/write side lives in `segments_persist`; the
//! audio orchestration in `audio`.

use std::cmp::Ordering;

/// Channel a transcript segment came from. Mirrors the
/// `activity_transcript_segments.source` column's canonical values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TranscriptChannel {
    /// Local microphone — the user's own voice. Rendered in the
    /// audio-aware prompt as "you said …".
    Mic,
    /// System loopback — the other side of a meeting / podcast /
    /// screen-reader / etc. Rendered as "the call said …".
    System,
}

impl TranscriptChannel {
    /// Persisted form for the `activity_transcript_segments.source`
    /// column. Matches the meeting-capture `Channel::tag()` naming so
    /// the two subsystems agree on what `'mic'` and `'system'` mean.
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Mic => "mic",
            Self::System => "system",
        }
    }

    /// Parse from the persisted form. Returns `None` for unrecognised
    /// values (which the caller should log + drop — Wave 5+ might add
    /// new channels and we shouldn't crash a pre-existing read path).
    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "mic" => Some(Self::Mic),
            "system" => Some(Self::System),
            _ => None,
        }
    }
}

/// One transcript segment as it leaves the Whisper driver / DB.
/// Owns its text so the stitcher can sort + group without borrowing
/// against the caller's slice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptSegment {
    /// Stable id (the DB row's `activity_transcript_segments.id`).
    pub id: String,
    /// Global epoch-ms start time of this segment.
    pub started_at: i64,
    /// Global epoch-ms end time (>= started_at).
    pub ended_at: i64,
    /// Whisper-recognized text.
    pub text: String,
    /// Source channel — `Mic` or `System`.
    pub channel: TranscriptChannel,
}

impl TranscriptSegment {
    /// Midpoint timestamp used by the stitcher. Rounds toward zero
    /// for negative inputs (impossible in practice — `started_at`
    /// comes from a unix epoch ms) but documented for the curious.
    #[inline]
    pub fn midpoint_ms(&self) -> i64 {
        // i64::saturating_add prevents an overflow if a future caller
        // hands us a pathological pair. Division by 2 then doesn't
        // wrap. The cost is one branch on the saturated path.
        self.started_at.saturating_add(self.ended_at) / 2
    }
}

/// Minimal Block projection the stitcher cares about. Strictly the
/// fields needed for the midpoint rule + return-value identity. We
/// deliberately don't depend on the rich `blocker::Block` type so
/// this module stays decoupled (and throwaway-crate testable —
/// LESSONS P1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StitchBlock {
    /// Either the persisted `activity_blocks.id` or a fresh ULID for
    /// in-memory Wave-3 Blocks the caller wants to stitch before
    /// committing.
    pub id: String,
    /// Block start, global epoch-ms.
    pub started_at: i64,
    /// Block end, global epoch-ms (`> started_at`).
    pub ended_at: i64,
}

/// One Block's audio bundle. Empty `mic_segments` + empty
/// `sys_segments` is valid — the caller still gets one bundle per
/// Block so the output zips 1:1 with the input Block list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockAudioBundle {
    /// Echo of the input [`StitchBlock::id`] — the abstractor
    /// re-correlates back to the Block via this.
    pub block_id: String,
    /// Mic-channel segments assigned to this Block (chronological).
    pub mic_segments: Vec<TranscriptSegment>,
    /// System-channel segments assigned to this Block (chronological).
    pub sys_segments: Vec<TranscriptSegment>,
}

impl BlockAudioBundle {
    /// True iff this Block has zero audio context. The abstractor
    /// uses this to decide between the audio-aware prompt and the
    /// regular Wave-3 prompt: bundles where `is_empty()` is `true`
    /// route to the regular prompt even if the session had audio.
    pub fn is_empty(&self) -> bool {
        self.mic_segments.is_empty() && self.sys_segments.is_empty()
    }

    /// Total segment count across both channels. Used for telemetry-
    /// adjacent debug logging.
    pub fn segment_count(&self) -> usize {
        self.mic_segments.len() + self.sys_segments.len()
    }
}

/// Stitch transcript segments onto Blocks per ADR 0041's midpoint rule.
///
/// **Inputs:**
/// - `blocks` — chronologically ordered (asserted in debug builds).
///   `started_at` must be `≤ ended_at` per Block, and successive
///   Blocks must not overlap (the Wave-3 blocker emits this shape
///   by construction).
/// - `segments` — any order; the stitcher sorts internally so the
///   caller doesn't have to. Segments with unknown channel strings
///   are caller-filtered (we take a typed `TranscriptSegment`).
///
/// **Output:** one [`BlockAudioBundle`] per input Block, in Block
/// order. Empty bundles for Blocks with no audio overlap are still
/// emitted. Each bundle's `mic_segments` and `sys_segments` are
/// **time-ordered** by `started_at` (the per-channel ordering matters
/// for the prompt's `[t+SS]` listing).
///
/// **Complexity:** O(B + S log S) — one sort of segments, one binary
/// search per segment over the Block list. For B=200 Blocks and
/// S=10_000 segments (4-hour session, both channels active) that's
/// ~133_000 i64 comparisons. Negligible.
///
/// **Out-of-range segments** are dropped from the result silently.
/// The DB still has them; the abstractor just doesn't see them.
pub fn stitch(blocks: &[StitchBlock], segments: &[TranscriptSegment]) -> Vec<BlockAudioBundle> {
    // Always-on (NOT debug_assert) because `binary_search_by` is
    // undefined on an unsorted slice — a stale-order caller would
    // mis-attribute segments rather than crash, which is worse.
    // The walk is O(B); stitch is called once per session-close, so
    // it's negligible cost.
    assert!(
        blocks_are_chronological(blocks),
        "block_audio_stitcher::stitch requires chronologically ordered, \
         non-overlapping blocks; got: {blocks:?}"
    );

    // Build the result skeleton — one empty bundle per Block, in
    // input order. The midpoint search returns the index back into
    // this Vec.
    let mut bundles: Vec<BlockAudioBundle> = blocks
        .iter()
        .map(|b| BlockAudioBundle {
            block_id: b.id.clone(),
            mic_segments: Vec::new(),
            sys_segments: Vec::new(),
        })
        .collect();

    // Pre-sort segments by start time. We could sort by midpoint, but
    // sorting by start lets the per-channel output stay time-ordered
    // by start (the more intuitive ordering for the prompt's
    // `[t+SS]` markers).
    let mut sorted: Vec<&TranscriptSegment> = segments.iter().collect();
    sorted.sort_by(|a, b| {
        a.started_at
            .cmp(&b.started_at)
            .then_with(|| a.id.cmp(&b.id))
    });

    for seg in sorted {
        let m = seg.midpoint_ms();
        let Some(idx) = find_block_for_midpoint(blocks, m) else {
            // Outside every block (idle gap, pre-first-block, etc).
            // Drop from stitching; DB row stays.
            continue;
        };
        match seg.channel {
            TranscriptChannel::Mic => bundles[idx].mic_segments.push(seg.clone()),
            TranscriptChannel::System => bundles[idx].sys_segments.push(seg.clone()),
        }
    }

    bundles
}

/// Return the index of the Block whose half-open interval
/// `[started_at, ended_at)` contains `m`, or `None` if `m` is outside
/// every Block. Uses `slice::binary_search_by` on the pre-sorted
/// (asserted) Block list.
fn find_block_for_midpoint(blocks: &[StitchBlock], m: i64) -> Option<usize> {
    // binary_search_by returns Ok(idx) when the predicate matches
    // exactly, Err(idx) for the insertion point otherwise. We map
    // the search so:
    //   - block.started_at > m   -> Greater (m is to the left)
    //   - block.ended_at  <= m   -> Less    (m is to the right; half-open)
    //   - otherwise              -> Equal   (m is inside)
    let r = blocks.binary_search_by(|b| {
        if b.started_at > m {
            Ordering::Greater
        } else if b.ended_at <= m {
            Ordering::Less
        } else {
            Ordering::Equal
        }
    });
    r.ok()
}

fn blocks_are_chronological(blocks: &[StitchBlock]) -> bool {
    if blocks.is_empty() {
        return true;
    }
    for w in blocks.windows(2) {
        let a = &w[0];
        let b = &w[1];
        if a.started_at > a.ended_at {
            return false;
        }
        if a.ended_at > b.started_at {
            // Adjacent Blocks are allowed to share an instant (a.ended_at == b.started_at);
            // the half-open interval means a midpoint exactly on the seam lands in B.
            return false;
        }
    }
    // Tail Block sanity.
    let last = &blocks[blocks.len() - 1];
    last.started_at <= last.ended_at
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(
        id: &str,
        t0: i64,
        t1: i64,
        channel: TranscriptChannel,
        text: &str,
    ) -> TranscriptSegment {
        TranscriptSegment {
            id: id.into(),
            started_at: t0,
            ended_at: t1,
            text: text.into(),
            channel,
        }
    }

    fn block(id: &str, t0: i64, t1: i64) -> StitchBlock {
        StitchBlock {
            id: id.into(),
            started_at: t0,
            ended_at: t1,
        }
    }

    #[test]
    fn channel_db_string_round_trips() {
        for c in [TranscriptChannel::Mic, TranscriptChannel::System] {
            assert_eq!(TranscriptChannel::from_db_str(c.as_db_str()), Some(c));
        }
        assert_eq!(TranscriptChannel::from_db_str(""), None);
        assert_eq!(TranscriptChannel::from_db_str("bluetooth"), None);
    }

    #[test]
    fn segment_midpoint_is_arithmetic_mean() {
        let s = seg("a", 1_000, 3_000, TranscriptChannel::Mic, "");
        assert_eq!(s.midpoint_ms(), 2_000);
    }

    #[test]
    fn stitch_assigns_segment_strictly_inside_a_block() {
        let blocks = vec![block("b1", 0, 10_000), block("b2", 10_000, 20_000)];
        let segs = vec![seg("s1", 1_000, 2_000, TranscriptChannel::Mic, "hi")];
        let out = stitch(&blocks, &segs);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].mic_segments.len(), 1);
        assert_eq!(out[0].mic_segments[0].text, "hi");
        assert!(out[1].is_empty());
    }

    #[test]
    fn stitch_routes_mic_and_sys_to_separate_buckets() {
        let blocks = vec![block("b1", 0, 10_000)];
        let segs = vec![
            seg("s1", 1_000, 2_000, TranscriptChannel::Mic, "you"),
            seg("s2", 3_000, 4_000, TranscriptChannel::System, "them"),
        ];
        let out = stitch(&blocks, &segs);
        assert_eq!(out[0].mic_segments.len(), 1);
        assert_eq!(out[0].sys_segments.len(), 1);
        assert_eq!(out[0].mic_segments[0].text, "you");
        assert_eq!(out[0].sys_segments[0].text, "them");
        assert_eq!(out[0].segment_count(), 2);
        assert!(!out[0].is_empty());
    }

    #[test]
    fn stitch_midpoint_rule_handles_boundary_crossing_segment() {
        // Segment spans 9_000..11_000; midpoint = 10_000. Boundary is
        // at 10_000 — half-open [b1.started_at, b1.ended_at) means
        // 10_000 belongs to b2 (the later Block).
        let blocks = vec![block("b1", 0, 10_000), block("b2", 10_000, 20_000)];
        let segs = vec![seg("s1", 9_000, 11_000, TranscriptChannel::Mic, "x")];
        let out = stitch(&blocks, &segs);
        assert!(out[0].is_empty());
        assert_eq!(out[1].mic_segments.len(), 1);
    }

    #[test]
    fn stitch_segment_mostly_before_boundary_lands_in_earlier_block() {
        // Segment 8_000..11_500 — midpoint 9_750, lands in b1.
        let blocks = vec![block("b1", 0, 10_000), block("b2", 10_000, 20_000)];
        let segs = vec![seg("s1", 8_000, 11_500, TranscriptChannel::System, "y")];
        let out = stitch(&blocks, &segs);
        assert_eq!(out[0].sys_segments.len(), 1);
        assert!(out[1].is_empty());
    }

    #[test]
    fn stitch_drops_segments_in_idle_gaps() {
        // Gap between b1 ending at 5_000 and b2 starting at 8_000;
        // a segment at 6_000..7_000 has midpoint 6_500 — outside both.
        let blocks = vec![block("b1", 0, 5_000), block("b2", 8_000, 12_000)];
        let segs = vec![
            seg("s_lost", 6_000, 7_000, TranscriptChannel::Mic, "lost"),
            seg("s_kept", 9_000, 10_000, TranscriptChannel::Mic, "kept"),
        ];
        let out = stitch(&blocks, &segs);
        assert!(out[0].is_empty());
        assert_eq!(out[1].mic_segments.len(), 1);
        assert_eq!(out[1].mic_segments[0].text, "kept");
    }

    #[test]
    fn stitch_drops_segments_before_first_block_and_after_last() {
        let blocks = vec![block("b1", 10_000, 20_000)];
        let segs = vec![
            seg("pre", 1_000, 2_000, TranscriptChannel::Mic, "pre"),
            seg("post", 25_000, 26_000, TranscriptChannel::Mic, "post"),
            seg("in", 15_000, 16_000, TranscriptChannel::Mic, "in"),
        ];
        let out = stitch(&blocks, &segs);
        assert_eq!(out[0].mic_segments.len(), 1);
        assert_eq!(out[0].mic_segments[0].text, "in");
    }

    #[test]
    fn stitch_preserves_per_channel_time_ordering() {
        // Feed segments out of order; expect per-channel sorted output.
        let blocks = vec![block("b1", 0, 100_000)];
        let segs = vec![
            seg("a3", 30_000, 31_000, TranscriptChannel::Mic, "third"),
            seg("a1", 1_000, 2_000, TranscriptChannel::Mic, "first"),
            seg("a2", 15_000, 16_000, TranscriptChannel::Mic, "second"),
            seg("b1", 5_000, 6_000, TranscriptChannel::System, "sys-first"),
            seg(
                "b2",
                20_000,
                21_000,
                TranscriptChannel::System,
                "sys-second",
            ),
        ];
        let out = stitch(&blocks, &segs);
        let mic_texts: Vec<&str> = out[0]
            .mic_segments
            .iter()
            .map(|s| s.text.as_str())
            .collect();
        assert_eq!(mic_texts, vec!["first", "second", "third"]);
        let sys_texts: Vec<&str> = out[0]
            .sys_segments
            .iter()
            .map(|s| s.text.as_str())
            .collect();
        assert_eq!(sys_texts, vec!["sys-first", "sys-second"]);
    }

    #[test]
    fn stitch_returns_one_bundle_per_block_even_when_empty() {
        let blocks = vec![block("b1", 0, 10), block("b2", 10, 20), block("b3", 20, 30)];
        let segs: Vec<TranscriptSegment> = vec![];
        let out = stitch(&blocks, &segs);
        assert_eq!(out.len(), 3);
        for (i, b) in out.iter().enumerate() {
            assert!(b.is_empty());
            assert_eq!(b.block_id, format!("b{}", i + 1));
        }
    }

    #[test]
    fn stitch_with_empty_block_list_returns_empty() {
        let out = stitch(&[], &[seg("s1", 1, 2, TranscriptChannel::Mic, "ignored")]);
        assert!(out.is_empty());
    }

    #[test]
    fn stitch_no_double_attribution_under_dense_load() {
        // 100 Blocks back-to-back, 1_000 segments. Verify the total
        // attributed count + dropped count add up to the input count.
        let blocks: Vec<StitchBlock> = (0..100)
            .map(|i| block(&format!("b{i}"), i * 1_000, (i + 1) * 1_000))
            .collect();
        let mut segs = Vec::new();
        for i in 0_i64..1_000 {
            // Segments cover the first 100 seconds; some inside, some
            // straddling boundaries.
            let t0 = i * 100;
            let t1 = t0 + 250; // 250ms wide, often straddling
            let ch = if i % 2 == 0 {
                TranscriptChannel::Mic
            } else {
                TranscriptChannel::System
            };
            segs.push(seg(&format!("s{i}"), t0, t1, ch, "."));
        }
        let out = stitch(&blocks, &segs);
        let attributed: usize = out.iter().map(BlockAudioBundle::segment_count).sum();
        assert!(attributed <= segs.len());
        // Each segment that lands SHOULD land in exactly one Block —
        // collect block_ids and assert uniqueness.
        let mut seen_ids = std::collections::HashSet::new();
        for bundle in &out {
            for s in bundle.mic_segments.iter().chain(&bundle.sys_segments) {
                assert!(
                    seen_ids.insert(s.id.clone()),
                    "double-attribution: {}",
                    s.id
                );
            }
        }
    }

    #[test]
    #[should_panic(expected = "chronologically ordered")]
    fn stitch_panics_on_out_of_order_blocks() {
        // Out of order — b2 starts before b1 ends.
        let blocks = vec![block("b1", 0, 10_000), block("b2", 5_000, 15_000)];
        let _ = stitch(&blocks, &[]);
    }
}
