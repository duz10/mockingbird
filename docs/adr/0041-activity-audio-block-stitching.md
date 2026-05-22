# ADR 0041 — Activity Capture Layer 2: transcript-segment-to-Block stitching

- Status: Accepted (Phase 10 Wave 4, 2026-05-25)
- Charters: bead `mb-g1w2`
- Supersedes: none
- Builds on: ADR 0036 (Activity Capture sibling-subsystem), ADR 0040
  (Activity summarization pipeline + Block-boundary semantics)
- Reuses: ADR 0028 (twin-stream capture), ADR 0029 (long-form chunked
  Whisper), ADR 0030 (whisper-segment exposure)

## Context

Phase 10 Wave 4 adds Layer 2 (audio) to Activity Capture. With
`audio_enabled = true` on a session, the orchestrator runs the
Meeting Capture twin-stream pipeline (mic + system loopback) in
parallel with the existing visual sampler, producing two streams of
time-stamped Whisper transcript segments per session.

Wave 3 (ADR 0040) defined the Block as the rendering + LLM-context
unit. Wave 4 must answer one structural question:

> Given the session's transcript segments and the session's Blocks,
> which segments belong to which Block?

Segments come from Whisper's internal VAD; their boundaries have no
relationship to the visual sampler's Block boundaries. A segment can:

- Sit entirely inside one Block (the common case).
- Straddle a Block boundary (Whisper finishes a sentence after the
  user has app-switched).
- Begin during idle and end during the next active Block (rare, but
  real — the user keeps talking after locking the screen).

We need a deterministic, single-pass, no-double-attribution rule.

## Decision

1. **Midpoint rule.** A transcript segment `(t0_ms, t1_ms, source)`
   belongs to Block `B` iff `(t0_ms + t1_ms) / 2 ∈ [B.started_at,
   B.ended_at)`.
   - The half-open interval guarantees a segment whose midpoint sits
     exactly on a Block boundary lands in the later Block, never both.
   - Segments whose midpoint falls outside every Block (idle gaps,
     pre-first-block, post-last-block) are **dropped from Block
     stitching** but **remain in the DB** as raw `activity_transcript_segments`
     rows. The session-detail view shows them; the abstractor never
     sees them.

2. **Per-channel preservation.** Each Block's stitched audio context
   is a pair `(mic_excerpts, sys_excerpts)`, NOT a single merged
   timeline. The audio-aware prompt uses the channel split as
   first-person vs. third-person framing ("you said X" vs. "the
   call/system said Y"). Merging would lose that framing and confuse
   the LLM.

3. **No cross-Block segment splitting.** A segment that the midpoint
   rule assigns to Block B but whose `[t0, t1]` overlaps Block C
   stays whole in B. Splitting Whisper output mid-phrase produces
   garbled context for both Blocks; whole-segment assignment biased
   by the midpoint is the simpler-and-better trade-off.

4. **Distinct prompt fingerprint family.** Audio-aware abstracted
   Blocks carry `prompt_version_sha = "abstract_v2_audio-<crc8hex>"`
   (the CRC covers the audio-aware prompt body). Without-audio
   Blocks keep `"abstract_v1-<crc8hex>"`. This lets future re-runs
   distinguish the two regimes from a DB query alone (Principle 2:
   provenance is total).

5. **`user_edited = 1` is sacred across audio re-runs.** When a
   session is re-summarized after audio capture (or after the user
   toggles audio off and re-runs), Blocks with `user_edited = 1`
   are NOT regenerated — their label + generated_abstract pass through
   untouched. This mirrors the Wave 3 invariant in
   `export::abstract_blocks_respecting_user_edits` and extends it to
   the audio-aware path.

6. **Audio-only-without-visual-context fallback.** If a Block has
   audio segments but no visual snapshots with `has_real_payload`
   (e.g. the user locked the screen while the call continued), the
   abstractor still uses the audio-aware template, but with
   `visibleTextFragments: []` and an explicit `screenContext:
   "locked"` hint in the user block. We do NOT mint a third template
   file (`abstract_block.audio_only.md`) — the audio-aware prompt
   handles this case cleanly enough, and one fewer file is one less
   thing for the prompt-set fingerprint to drift on.

## Consequences

- **Pure-Rust stitcher.** The `block_audio_stitcher` module is pure
  Rust with no DB or audio dependencies. Throwaway-crate testable
  per LESSONS P1 fallback gate.
- **Re-run cost.** Audio-aware Block abstraction is ~2-3× the prompt
  body size of Wave 3, so re-runs are slower. Acceptable: the abstractor
  is already off the hot path (manual user action).
- **Storage growth.** `activity_transcript_segments` adds ~1 row per
  Whisper segment (typically 1-5 s long). A 4-hour session with both
  channels active produces ~5k-10k rows. Index on `(session_id,
  started_at)` is already present (migration 012); this is fine.
- **Channel-tag in the abstractor.** The audio-aware prompt sees
  per-channel excerpts. The prompt fingerprint covers the prompt
  body, not the per-Block channel split — that's data, not config.

## Alternatives considered

- **Overlap-area rule** ("assign to the Block holding the larger
  share of the segment's duration"). Rejected: requires a per-Block
  loop with comparison vs. the midpoint rule's O(log n) binary search,
  and only changes the result for segments straddling boundaries —
  a small minority that the midpoint rule already handles correctly.
- **Segment splitting at Block boundaries.** Rejected: cuts Whisper
  output mid-phrase. The audio-aware prompt would receive incoherent
  fragments. Whisper's segment boundaries are content-aligned (VAD);
  preserving them is the higher-quality choice even at the cost of
  some "this audio is technically slightly before/after the visual
  Block" approximation.
- **Single merged audio timeline (no channel split).** Rejected:
  loses the first-person/third-person framing that makes the audio-aware
  summary useful. The user wants to read "You said X, then they said Y"
  not "Someone said X, then someone said Y."
- **Third template file for audio-only.** Rejected per Decision item
  6 above. One fewer file in the prompt set, one fewer SHA contribution,
  one fewer thing to keep in sync.

## Implementation notes

- `block_audio_stitcher::stitch(blocks, segments) -> Vec<BlockAudioBundle>`
  returns one bundle per Block (in Block order). `BlockAudioBundle`
  carries `(block_id, mic_segments, sys_segments)`. Empty bundles for
  Blocks with no audio overlap are still emitted so the caller can
  zip 1:1 with the Block list.
- The midpoint binary search uses `slice::binary_search_by_key` on a
  pre-sorted Block list (the blocker emits chronologically by
  construction; assert this in debug).
- Channel routing: `activity_transcript_segments.source` is `'mic'` or
  `'system'`. The stitcher dispatches on that string; unknown values
  are dropped with a `tracing::warn!`.

### Coordinate system

Whisper segment timestamps surface from
`meetings::long_form_stt::LongFormOutput.{mic,sys}_segments` as **u32
milliseconds relative to the start of capture** (`first_sample == 0`),
NOT epoch milliseconds. Activity Blocks live in epoch ms (`now_ms()`
at boundary detection). To make the midpoint rule's comparison valid,
Wave 4 normalizes to epoch at *insert time* in
`activity/runtime.rs::persist_audio_results`: a one-shot
`SELECT started_at FROM activity_sessions WHERE id = ?` provides the
offset that's added to every segment's `t0_ms`/`t1_ms` before the
bulk insert. After this shift the stitcher is coordinate-system-pure
(everything is epoch ms) and the midpoint binary search is correct.
This also makes `activity_transcript_segments.{started_at, ended_at}`
directly comparable to `activity_blocks.{started_at, ended_at}` in
future cross-table queries (e.g. "transcript for the past hour")
without a JOIN on session-start.

## Notes on what this ADR explicitly does NOT cover

- Wave 5 encryption (ADR 0038 reserved). The transcript segment
  storage is plaintext in Wave 4 by design; encryption is a session-
  level concern that wraps the whole storage layer in Wave 5.
- Wave 7 OCR / screen-content Layer 3 (ADR 0039 reserved).
- The audio retention policy (per-session vs. global). Today the
  segments live as long as the session row; cleanup wires in Wave 5.
