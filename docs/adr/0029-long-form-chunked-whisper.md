# ADR-0029: Long-form chunked Whisper inference (closes standing P1 mb-2bi)

- **Status:** Accepted
- **Date:** 2026-05-20
- **Deciders:** Dustin (project lead), code-puppy (implementor)
- **Phase MC companion to:** ADR 0026 (sibling-subsystem charter)
- **Closes:** standing P1 `mb-2bi` ("Audio streaming + chunked Whisper
  inference — proper long-form fix")

## Context

The dictation pipeline sizes its ring buffer for 300 s and runs Whisper
**once** over the whole sealed buffer at stop. This works because
dictation is hold-to-talk and the typical utterance is ≤ 30 s. A
300 s ceiling was a Phase 2.x interim fix to keep users productive
after a long-form dictation incident; `mb-2bi` was filed at the time as
a P1 standing issue ("proper fix: stream PCM into Whisper during
recording, chunked / stitched").

Phase MC's meeting capture pushes that buffer into territory the
single-pass design cannot tolerate:

- Default ceiling 4 h (14 400 s); hard ceiling 6 h.
- Whisper-rs's `state.full()` call processes its entire input
  synchronously. On a CUDA-accelerated large-v3-turbo model, 4 h of
  16 kHz mono is ≈ 1 h of pure inference time even on a 4090; on CPU
  it's not finishing in any human timeframe.
- A single 4 h PCM buffer at 16 kHz × 2 bytes ≈ 460 MB live RAM per
  channel, ballooning to ~920 MB for two-channel meetings. Even if
  inference were instant, the resident-set blow-up is unacceptable.
- A mid-meeting Whisper failure on a 4 h buffer loses 4 h of audio.

The forcing question: do we **stream Whisper during the meeting** (per-chunk
inference live), or do we **buffer chunks to disk during the meeting and
batch-transcribe at stop**?

Live streaming offers a "transcript as you go" UX but introduces a
non-trivial real-time test matrix, makes the formatter and merge step
race-prone, and burns GPU during the meeting (competing with other apps
the user might run while a meeting runs in the background). Batch-at-stop
is simpler, predictable, fully unit-testable, matches the "no LLM in
the critical path" sibling rule (the formatter is the critical pass;
Whisper is itself a deterministic-enough STT step), and aligns with the
WisprFlow-parity goal — Wispr Flow doesn't show live meeting transcripts
either; it shows them on stop.

## Decision

**During recording, the meetings thread drains each ringbuf every 30 s
into fixed 30-second PCM chunks with a 2-second leading overlap; each
chunk is written to a temp WAV under
`<appdata>\Mockingbird\meeting_audio\<uuid>\<chunk_index>.wav`. At stop,
each channel's chunk pile is fed sequentially to
`WhisperStt::transcribe_segments` (per ADR 0030), with a rolling
`initial_prompt` carrying the prior chunk's tail text as context;
overlap-window segments are deduplicated by `t0 < overlap_boundary`.
Stitching is loss-less by construction.**

Concretely:

1. **Chunk size 30 s, leading overlap 2 s.** Chunk N covers
   `[28*(N) , 28*(N)+30)` seconds for N ≥ 1; chunk 0 covers `[0, 30)`.
   So chunk 1 starts at 28 s (2 s of overlap with chunk 0), chunk 2
   starts at 58 s (2 s of overlap with chunk 1), etc. The 2 s figure
   is empirical guidance from the Whisper community for preserving
   word-boundary context across chunk seams; it's also tunable via a
   future setting if real-world stitches drop words.
2. **Chunk WAVs on disk, not in RAM.** Each chunk lands as a standalone
   16-bit PCM WAV via `hound`. The ringbuf only ever holds ~32 s of
   PCM live (one chunk + the next chunk's overlap region). Disk
   footprint per channel: ~3.6 MB per chunk × N chunks. A 4 h meeting
   produces ~480 chunks (~1.7 GB per channel). Configurable retention
   per `MeetingAudioRetentionDays` (defaults to inheriting
   `AudioRetentionDays`, currently 30 days).
3. **Optional CRC32 on each chunk write** via `crc32fast`. Cheap
   insurance against power-loss-mid-write; a corrupt chunk is detected
   at stop and skipped (session marked `partial`).
4. **Batch transcription at stop, one chunk at a time, in order.**
   `WhisperStt::transcribe_segments` (ADR 0030) returns
   `Vec<TimedSegment { text, t0_ms, t1_ms }>` per chunk. The driver:
   - Maintains a `prev_tail_prompt: String` (initially empty).
   - For chunk K, calls `transcribe_segments(chunk_K_pcm, prev_tail_prompt)`
     where `prev_tail_prompt` is fed as Whisper's `initial_prompt`
     parameter (≈ last 64 chars of the prior chunk's joined segment
     text — picks up names, technical terms, mid-sentence context).
   - Drops any returned segment with `t0_ms < overlap_boundary_ms`
     (the prior chunk already emitted that segment).
   - Concatenates the surviving segments into the channel's
     `Vec<TimedSegment>`.
   - Updates `prev_tail_prompt` from the last surviving segment.
5. **Failure on chunk K is non-fatal.** The driver marks the session
   `status='partial'`, emits the formatter pass over chunks `0..K-1`,
   and surfaces a "transcription incomplete after N minutes — retry?"
   affordance in the transcript view. The audio is preserved; the user
   can re-run the long-form pass against the saved chunks.
6. **Full audio retention.** After successful batch-transcribe, the
   chunk WAVs are concatenated into a canonical `meeting.wav` (saved to
   the session's `audio_blob_path`), and the chunk-WAV directory is
   removed. On `partial` sessions, the chunks are retained alongside
   the (incomplete) `meeting.wav`-so-far so retry is possible.

## Consequences

### Positive

- **Long-form is finally tractable.** A 4 h meeting transcribes in
  proportional time to the GPU's inference rate, not to "the entire
  buffer at once." On a 4090 + large-v3-turbo, ≈ 30 min of inference
  for a 4 h meeting (empirical from public Whisper benchmarks).
- **RAM stays bounded.** ≤ 32 s of PCM per channel live, regardless of
  meeting length. The 690 MB paranoid ceiling from ADR 0028 stays a
  documentary upper bound, not a real footprint.
- **Mid-meeting failure isn't catastrophic.** A crash, OOM, or Whisper
  abort costs at most one chunk's worth of work. The on-disk chunks
  outlive the process and can be retranscribed.
- **`mb-2bi` closes.** The standing P1 has been "audio streaming +
  chunked Whisper inference (proper long-form fix)" since 2026-05-17;
  this ADR delivers the spec and Wave 3 ships the implementation.
- **The dictation pipeline is unaffected.** Dictation continues to use
  the single-pass `transcribe` method on a 300 s ringbuf — its existing
  semantics, its existing 383 tests. The chunked path is opt-in via
  the new module.

### Negative

- **Stitch artifacts at chunk boundaries are possible.** Whisper's
  segment timestamps drift by ±50 ms typically; the overlap dedup uses
  a half-open boundary which can occasionally drop or duplicate one
  word at the seam. The 2 s overlap + rolling `initial_prompt` minimize
  this; the `mc-long-form-stitched-losslessly` judge (Wave 6) asserts
  the stitched transcript matches the single-pass transcript on a 90 s
  synthetic fixture within edit-distance 0.5 %. If real-world stitches
  drop words post-seal, the fix is bumping overlap to 3 s + LESSONS
  entry — no architectural change.
- **Disk I/O during recording.** ~3.6 MB / 30 s / channel ≈ 0.12 MB/s
  per channel of sequential writes. Imperceptible on any SSD, modest on
  a 5400 RPM HDD. The chunk writer batches with `hound`'s default
  buffering.
- **Inference happens at stop, not live.** Users see a "transcribing
  N of M" progress chip after they press Stop; for a long meeting this
  is real wall-clock wait time. Acceptable for v1 (matches WisprFlow);
  live transcription is explicitly out of scope.
- **GPU contention at stop.** If the user starts another Whisper-using
  workflow (a dictation, another meeting) before the batch transcribe
  finishes, the second workflow blocks on the same `WhisperStt`
  instance. The runtime serializes via the existing `WhisperStt` lazy
  init; no new lock is added.

### Neutral

- **`crc32fast` is a tiny new dep** (already commonly transitively
  pulled by other tooling) and easy to remove if Wave 1 implementation
  decides hand-written CRC is preferable. The plan's Cargo-deps section
  flags both options.

## Alternatives considered

- **Streaming live transcription** (per-chunk inference *during* the
  meeting). Rejected for v1. Adds a real-time test matrix, makes the
  formatter race-prone, and burns GPU concurrently with the user's
  work. The plan defers it explicitly to a possible "Phase MC.2" —
  feasible (chunks are already 30 s, formatter is pure) but not
  blocking the WisprFlow-parity goal.

- **Variable-length chunks driven by VAD silence boundaries.**
  Rejected. Adds a per-channel VAD pass that has to keep up with
  capture, complicates the chunker's pure-state-machine testability,
  and ties chunk boundaries to acoustic content (so the test fixture
  set explodes). Fixed 30 s windows are deterministic, easy to test,
  and the 2 s overlap covers context across whatever boundary they
  land on.

- **Single-pass against a memory-mapped WAV at stop.** Rejected. The
  inference time argument stands (Whisper full-pass on hours of audio
  is not finishing in reasonable wall-clock even with mmap saving the
  RAM cost) and the failure mode is still all-or-nothing.

- **Chunk size 60 s with no overlap.** Rejected. Longer chunks reduce
  the number of stitches and the `initial_prompt` overhead but increase
  the cost of a single chunk failure (lose 60 s instead of 30 s of work)
  and make stitch artifacts harder to recover from. 30 s is the
  Whisper-community-empirical sweet spot.

## Cross-references

- **PLAN:** `docs/phases/phase-meeting-capture.md` §MC.2 (chunker +
  long-form STT pipeline), Risk #2 (stitching artifacts), Risk #3
  (memory blow-up).
- **ADR 0026:** charter — this work lives entirely under `meetings/`
  and reuses `stt::SpeechToText` (extended per ADR 0030) without
  modifying dictation's call site.
- **ADR 0030:** `transcribe_segments` additive method — this ADR's
  driver is its first consumer.
- **ADR 0013:** cpal ringbuf design — the producer side of the chunker.
- **Issue closed:** `mb-2bi`. Closure is scheduled for Wave 6 alongside
  the `phase-mc-complete` seal tag.
- **bd issues:** this ADR is `mb-qjb`. Implementation work is scheduled
  for Wave 2 (chunker, pure) and Wave 3 (long-form STT driver).

---

_The `adr-format` judge validates this structure exists in every numbered
ADR. Keep section headings stable._
