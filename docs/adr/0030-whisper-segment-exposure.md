# ADR-0030: Whisper segment exposure via an additive `transcribe_segments` STT-trait method

- **Status:** Accepted
- **Date:** 2026-05-20
- **Deciders:** Dustin (project lead), code-puppy (implementor)
- **Phase MC companion to:** ADR 0026 (sibling-subsystem charter)

## Context

Today's `SpeechToText` trait returns:

```rust
pub trait SpeechToText {
    fn transcribe(&self, req: TranscribeRequest<'_>) -> AppResult<Transcript>;
}

pub struct Transcript {
    pub text: String,
    pub gpu_used: bool,
    pub latency_ms: u64,
    pub model_id: String,
}
```

Dictation uses this exactly — it wants one cleaned line, no segments. The
383 dictation tests pin this contract.

Phase MC's deterministic formatter (PLAN §MC.3) needs **per-segment
timestamps** to do its job:

- Paragraph breaks fire when `segment[i+1].t0_ms - segment[i].t1_ms ≥
  paragraph_gap_ms` (default 2000 ms). No segment timestamps → no
  paragraph breaks → wall of text.
- Long-form chunk stitching (ADR 0029) drops overlap-window segments
  by `t0_ms < overlap_boundary_ms`. No segment timestamps → no dedup
  → duplicated text at every seam.
- Two-channel merging (ADR 0028) interleaves by `(channel,
  segment_start_ms)`. No segment timestamps → no chronological merge.

Whisper itself emits segment information; whisper-rs exposes it via
`state.full_n_segments()`, `state.full_get_segment_t0(i)`,
`state.full_get_segment_t1(i)`, `state.full_get_segment_text(i)`. The
existing `transcribe` implementation builds its `text` field by joining
the segments and discarding the timestamps. Phase MC needs the
timestamps preserved.

**The forcing question**: change `transcribe`'s return type to include
segments (and migrate dictation to ignore them), or add a sibling method
`transcribe_segments`?

Changing `transcribe`'s return type would touch every call site in
dictation, force a re-baseline of dictation's 383 tests (even if just
in the test fixtures that pattern-match on `Transcript`), and make the
contract for a feature dictation doesn't need pay marginal allocation
cost (the segment `Vec` is small but it's non-zero). It would also
violate the Phase MC binding list — `stt/mod.rs` and `stt/whisper.rs`
are reachable from inside dictation's hot path and the binding list
seals all of dictation's surface.

Adding a sibling method is purely additive: dictation continues to
call `transcribe`, meeting calls `transcribe_segments`, the existing
383 tests don't change.

## Decision

**Add a sibling method `transcribe_segments` to the `SpeechToText`
trait. The existing `transcribe` method is left untouched. Dictation
continues to call `transcribe`; meeting calls `transcribe_segments`.
The `WhisperStt` impl gains a second method that walks
whisper-rs's segment API and packages a `TranscriptWithSegments`.**

Concretely:

```rust
pub trait SpeechToText {
    /// Existing: dictation's one-pass joined-text return.
    /// Sealed for Phase MC — no signature changes.
    fn transcribe(&self, req: TranscribeRequest<'_>) -> AppResult<Transcript>;

    /// New (Phase MC): same input, returns per-segment timestamps
    /// alongside the joined text. Used by `meetings/long_form_stt.rs`
    /// (one call per 30 s chunk). Default impl returns
    /// `AppError::LongFormStt("transcribe_segments unimplemented")`
    /// so non-Whisper STT impls (if any) aren't forced to implement it.
    fn transcribe_segments(
        &self,
        req: TranscribeRequest<'_>,
    ) -> AppResult<TranscriptWithSegments> {
        let _ = req;
        Err(AppError::LongFormStt(
            "transcribe_segments not implemented for this STT backend".into(),
        ))
    }
}

pub struct TranscriptWithSegments {
    pub text: String,                 // joined; identical-up-to-whitespace to transcribe().text
    pub segments: Vec<TimedSegment>,
    pub gpu_used: bool,
    pub latency_ms: u64,
    pub model_id: String,
}

pub struct TimedSegment {
    pub text: String,                 // single segment's text
    pub t0_ms: u64,                   // segment start, milliseconds from PCM origin
    pub t1_ms: u64,                   // segment end
}
```

Rules:

1. **`transcribe`'s signature is sealed.** No edits to the existing
   trait method, no edits to dictation's call sites. The
   `mc-dictation-untouched` judge enforces this.
2. **`transcribe_segments` has a default impl** that returns
   `AppError::LongFormStt`. Non-Whisper impls (if any future provider
   lands) aren't forced to implement segments until they actually need
   to.
3. **`WhisperStt::transcribe_segments`** walks
   `state.full_n_segments()` / `full_get_segment_t0` /
   `full_get_segment_t1` / `full_get_segment_text` and emits
   `Vec<TimedSegment>`. The joined `text` is the same algorithm
   `transcribe` uses today (segments joined by whitespace), so a per-
   chunk equivalence test holds: `transcribe(x).text` equals
   `transcribe_segments(x).text` modulo whitespace normalization.
4. **`TranscribeRequest<'_>` is reused unchanged.** Both methods take
   the same input shape; the meeting driver supplies `initial_prompt`
   via the existing field on `TranscribeRequest` (it's already there
   for dictation's prompt-context use).
5. **No dictation migration in Phase MC.** A future epic could
   re-implement `transcribe` as `transcribe_segments(...).map(|t|
   t.flatten())` to remove the duplication, but **not in Phase MC** —
   it would touch the dictation orchestrator and re-baseline its tests,
   which violates the binding list.

## Consequences

### Positive

- **Dictation surface stays sealed byte-for-byte.** All 383 dictation
  tests pass unchanged. The static `mc-dictation-untouched` judge
  passes trivially for the STT surface.
- **The trait's two methods carry honest, narrow contracts.**
  `transcribe` does what dictation has always asked it to do.
  `transcribe_segments` is the long-form / meeting method. Future
  consumers pick the one that matches their need.
- **Default impl protects future STT providers.** If anyone adds a
  non-Whisper STT (e.g. cloud Whisper, Vosk, NeMo), they don't have
  to implement segments to ship. They only do it if they want long-form.
- **Implementation is small and isolated.** `WhisperStt::transcribe_segments`
  is ≈ 30 LoC; the existing `transcribe` impl moves nothing. 4 new
  unit tests against existing fixture WAVs cover it (segment count > 0,
  t0 < t1 monotonic, joined text matches `transcribe().text` up to
  whitespace, error case for malformed input).

### Negative

- **Some duplication between `transcribe` and `transcribe_segments`.**
  Both methods load the model state, both call `state.full(...)`. The
  duplication is ≈ 15 lines in `WhisperStt`. A shared private helper
  could DRY it, and Wave 2 will likely do exactly that (private to
  `stt/whisper.rs`, no public API change). The point is the *trait*
  has two methods; the *impl* can share whatever it likes internally.
- **`TranscriptWithSegments.text` can drift from `Transcript.text`** if
  the joining algorithm changes for one but not the other. Mitigation:
  a unit test asserts they remain whitespace-equivalent for a fixture
  WAV. Drift surfaces immediately, not in production.
- **One more public type to maintain** (`TranscriptWithSegments`,
  `TimedSegment`). Both are dumb data structs; no behavior to evolve.

### Neutral

- **Whisper's segment timing precision** is what it is (±50 ms typical
  per the whisper-rs docs). The formatter's 2 s paragraph-gap default
  is well above that noise floor; the long-form chunk overlap (2 s)
  similarly absorbs it. No new timing accuracy guarantee is implied
  by exposing the segments.

## Alternatives considered

- **Change `transcribe`'s return type to include `segments:
  Vec<TimedSegment>` and migrate dictation to ignore the field.**
  Rejected. Violates the Phase MC binding list (would touch dictation's
  call sites). Pays marginal allocation cost on every dictation call
  for data dictation doesn't use. Re-baselines 383 tests for no
  user-visible benefit.

- **Expose Whisper segments through a separate non-trait helper
  function** (e.g. `pub fn whisper_segments(req) -> AppResult<Vec<TimedSegment>>`).
  Rejected. Bypasses the trait abstraction, hardcoding Whisper for the
  meeting pipeline. If a non-Whisper STT lands later, the meeting
  pipeline would have to be rewired. Keeping segments on the trait
  means swapping providers is still a one-line change in `meetings/`.

- **Stream segments via a callback or iterator instead of returning
  `Vec<TimedSegment>`.** Rejected for v1. Adds streaming semantics
  the meeting batch path doesn't need (it processes one chunk at a
  time, end-to-end, before moving on). A streaming variant could be
  added later if live-during-meeting transcription ever ships.

- **Reuse `Transcript` and pack segments into the existing struct as
  an `Option<Vec<TimedSegment>>`** flag. Rejected. Makes
  `Transcript`'s contract polymorphic — callers have to handle
  `Some` / `None` and the semantic difference between "no segments
  requested" and "segments unavailable for this provider." The two-
  method approach has clear, narrow types per call site.

## Cross-references

- **PLAN:** `docs/phases/phase-meeting-capture.md` Pre-flight ADR 0030
  description, §MC.3 (formatter consumes segments), Wave 2 task table
  (`stt::SpeechToText::transcribe_segments` lands here).
- **ADR 0026:** charter — establishes the binding-list rule that this
  ADR's additive-only approach honors.
- **ADR 0028:** twin cpal stream capture — produces the per-channel
  chunks this method transcribes.
- **ADR 0029:** long-form chunked Whisper — the primary consumer of
  this method.
- **bd issues:** this ADR is `mb-gh4`. Implementation lands in Wave 2
  (4 unit tests against existing fixture WAVs covering segment-count
  positivity, t0/t1 monotonicity, joined-text equivalence to
  `transcribe`, error path).

---

_The `adr-format` judge validates this structure exists in every numbered
ADR. Keep section headings stable._
