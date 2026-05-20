# ADR-0026: Meeting Capture is a sibling subsystem, not an extension of dictation

- **Status:** Accepted
- **Date:** 2026-05-20
- **Deciders:** Dustin (project lead), code-puppy (implementor)
- **Charter for:** Phase MC (Meeting Capture lateral feature epic — `mb-pdv`)

## Context

Mockingbird ships a single hot-path today: **dictation**. Hold a key, talk,
release, get clean text in the focused app's caret. Sealed at
`phase-4-complete`: hold-to-talk, ≤300 s, three `modes` rows with
empirically-tuned prompts (ADR 0022/0024), Whisper STT, an optional Ollama
cleanup pass guarded by ADR 0021's sync trait, and clipboard-paste injection
guarded by ADR 0018's save/restore protocol. 383 tests cover it.

A user request landed for a **meeting-recording feature** comparable to
Wispr Flow's meeting mode but local-only. Concrete shape:

- Push-to-start / push-to-stop (not hold-to-talk).
- Hours-long (default 4 h ceiling, hard 6 h).
- Optionally captures **two** audio streams (mic + system loopback).
- No injection — transcripts live in the app's own UI, not someone else's caret.
- No fixed `modes` schema — the LLM pass is opt-in, run *after* the canonical
  transcript exists, against a markdown-file-backed prompt the user picks
  per-run, and its output is **not persisted** (reproducible from the saved
  transcript).
- A new activation gesture (double-tap a configurable key) so it doesn't
  clash with the dictation hotkey.

The dictation pipeline and the meeting pipeline share **three primitives**
and nothing else: audio capture (`audio::AudioCapture`), Whisper STT
(`stt::SpeechToText`), and the Ollama HTTP client (`cleanup::OllamaProvider`).
Every other dimension — activation, ring-buffer sizing, paste/inject vs.
present-in-app, deterministic vs. prompt-driven post-processing, schema,
threading lifetime — diverges.

**The forcing question**: do we extend the dictation modules (state machine,
`modes` table, `CleanupProvider` trait, `recording_window` overlay), or do
we build meeting capture as a parallel subsystem that reuses only the three
primitives?

Extending the dictation modules would introduce a conditional in every
shared module ("is this dictation or meeting?"). The dictation state
machine has 8 states today; adding `MeetingArmed` / `MeetingRecording` /
`MeetingTranscribing` more than doubles the state count and re-litigates
ADR 0024's empirical-tuning baseline. The `modes` table is currently 3 rows
tuned over Phase 4 Waves A–D; adding meeting-mode rows means re-running
that evaluation for a feature that doesn't even *use* the modes-prompt
contract (the meeting LLM pass is post-hoc, not pre-Whisper). The
`CleanupProvider` trait is shaped for one-pass transcript cleanup; meeting
LLM-pass needs multi-prompt + custom-prompt + non-persisted output, which
is a different contract. The `recording_window` overlay is a fixed-size
500 × 60 pip designed for "you're holding a key" feedback; the meeting
overlay needs source selection + Start/Stop controls + duration ticker, a
~400 × 300 footprint.

Every one of those extensions *could* be done, and each would individually
look reasonable in code review. Stacked, they'd turn the dictation surface
into a polymorphic Christmas tree where every refactor pays a two-feature
tax forever.

## Decision

**Meeting Capture is built as a sibling subsystem at `src-tauri/src/meetings/`,
reusing only `audio::AudioCapture`, `stt::SpeechToText`, and
`cleanup::OllamaProvider`. Everything else under `meetings/` is greenfield.**

Concretely:

1. **Dictation modules are sealed for Phase MC.** No edits to
   `hotkey/state.rs`, `hotkey/windows.rs`, `hotkey/driver.rs`, `dictation/`,
   `injection/`, `recording_window.rs`, `cleanup/provider.rs`,
   `cleanup/llm_cleaner.rs`, or migrations `001-010`. This is a binding
   rule, enforced by the `block-cross-module-coupling-meeting-dictation`
   pre-commit hook authored in Wave 1.
2. **No `modes` table additions.** Meeting LLM-pass prompts live as
   markdown files at `src-tauri/src/meetings/prompts/*.md`. The `modes`
   table stays a 3-row contract owned by dictation.
3. **No `CleanupProvider` trait extension.** The meeting LLM pass
   constructs an `OllamaProvider` via its existing arg-less `new()` and
   passes each request through the existing `CleanupRequest<'_>` struct.
   If a future Phase MC.N needs a multi-prompt batch API, it gets its
   own trait (e.g. `MeetingLlmDriver`), not an extension of the dictation
   trait.
4. **No `SpeechToText::transcribe` signature change.** A *new* method
   `transcribe_segments` is added (ADR 0030). Dictation continues calling
   `transcribe`; meeting calls `transcribe_segments`. The 383 dictation
   tests stay green without modification.
5. **Sibling state machine, sibling threads, sibling windows.** The
   meeting hook runs on its own message-pump thread (ADR 0027); the
   meeting overlay is a new Tauri webview window (`meeting_overlay`);
   persistence lives in two new tables (`meeting_sessions` +
   `meeting_transcripts`, migration 011).

## Consequences

### Positive

- **Dictation surface area stays sealed.** The 383 existing tests pass
  byte-identically through the entire Phase MC. The `mc-dictation-untouched`
  judge enforces this statically: `git diff phase-4-complete..HEAD --
  src-tauri/src/hotkey src-tauri/src/dictation src-tauri/src/injection
  src-tauri/src/cleanup/provider.rs src-tauri/src/recording_window.rs`
  must be empty.
- **The meeting pipeline can evolve at its own pace.** Long-form Whisper,
  two-channel merging, deterministic post-processing, markdown export —
  none of these need to defer to dictation's design constraints.
- **The coupling hook makes architectural drift mechanical to catch.**
  A future developer who reaches across the boundary trips the hook at
  commit time, not at code-review time.
- **Provider/STT/audio primitives stay genuinely shared.** Reusing the
  three primitives validates ADR 0013 (cpal ringbuf design), ADR 0011
  (whisper-rs CUDA build), and ADR 0021 (sync cleanup provider) — three
  abstractions originally designed for dictation now carry a second
  consumer without modification.

### Negative

- **Some real duplication.** Meeting overlay and dictation overlay are
  both Tauri windows with similar `focus: false` / `decorations: false` /
  `alwaysOnTop: true` boilerplate. Activation state machines (dictation's
  tap/hold, meeting's double-tap) are both timed Rust state machines but
  *not* identical (different inputs, different outputs). Future refactor
  could extract a `WindowConventions` helper or a `KeyTimingFsm` trait —
  but **not in Phase MC**. YAGNI: extract the shared pattern only once
  the second consumer exists and the actual shape of the shared abstraction
  is clear from two real call sites, not one + speculation.
- **Two threads doing message pumping** instead of one (ADR 0027 explains
  why this is the right call given the binding list). Cost is sub-microsecond
  per keystroke per pump; benefit is zero modification of the sealed
  dictation driver.
- **Plan-doc discipline tax**: any future feature touching meetings *or*
  dictation has to re-establish whether it's a sibling or an extension of
  one of them. The pattern this ADR establishes (sibling-by-default, with
  the coupling hook enforcing it) makes that conversation cheaper.

### Neutral

- **Code-review burden shifts to the boundary.** The coupling hook catches
  imports crossing the line; what it can't catch is *semantic* coupling
  (e.g. two near-duplicate state machines drifting in subtle ways). Wave 6
  retrospective should note any drift seen across Phase MC and propose
  the right shared abstraction(s) for a later epic.

## Alternatives considered

- **Extend the dictation state machine to handle a "meeting" mode.**
  Rejected. Doubles the state count, introduces "is this dictation or
  meeting?" branches in every transition, re-litigates ADR 0024's
  empirical baseline. The two activation gestures (hold vs. double-tap)
  are also genuinely different problems; they share zero state.

- **Add a fourth `modes` row called `meeting` with `cleanup_kind = 'post-hoc'`.**
  Rejected. The `modes` table's contract is "select a prompt that drives
  the canonical cleanup pass." The meeting LLM pass isn't the canonical
  pass — the deterministic formatter is. And the meeting LLM pass needs
  user-pickable prompts + custom inline prompts per-run, which don't fit
  a row-keyed model. Markdown files under `meetings/prompts/` are a
  better fit and don't burden the dictation evaluation harness.

- **Extend `CleanupProvider` with a `cleanup_with_prompt(prompt, text)` method.**
  Rejected. The existing trait's contract is "given a `CleanupRequest`
  describing the dictation mode, return cleaned text." Adding a free-form
  prompt entry point makes the trait two contracts in one. The existing
  `OllamaProvider` already has the HTTP plumbing we need; we just
  instantiate it directly from `meetings/llm_pass.rs` and build the
  request inline. Trait extension is a future concern (ADR-TBD) once a
  *second* consumer of free-form-prompt LLM also exists.

- **Reuse the `recording_window` Tauri window with conditional content.**
  Rejected. The overlay's size, layout, and lifecycle are dictation-
  shaped (small pip, lives only during a held key, no interactive
  controls). The meeting overlay is the user's *only* control surface
  during a meeting (Start, Stop, source pick) and needs to remain
  interactable. Two windows is the right answer; the `focus: false`
  / `alwaysOnTop: true` boilerplate is small enough that DRY-extracting
  it is YAGNI until window #3.

## Cross-references

- **PLAN:** `docs/phases/phase-meeting-capture.md` — Pre-flight section and
  cross-wave invariant #1 cite this ADR by name.
- **Companion Phase MC ADRs:** 0027 (double-tap activation thread), 0028
  (twin cpal stream capture), 0029 (long-form chunked Whisper, closes
  mb-2bi), 0030 (`transcribe_segments` additive method).
- **Sealed primitives this ADR reuses:** ADR 0013 (cpal ringbuf design),
  ADR 0021 (sync cleanup provider), ADR 0011 (whisper-rs CUDA build).
- **Sealed surfaces this ADR refuses to extend:** ADR 0015 (low-level
  keyboard hook — extended via *parallel install* in 0027, not via the
  existing driver), ADR 0018 (clipboard save/restore protocol — meeting
  copy-to-clipboard is one-shot, no save/restore, per plan §Cargo-deps
  note), ADR 0010 (raw-transcript immutability — meeting transcripts
  add their own tables; existing `transcripts` table is untouched).
- **Lateral-epic pattern precedent:** ADR 0022 (three-mode pipeline,
  charter ADR for the Phase 4 lateral epic), ADR 0023 (Design Language v1
  lateral epic). Phase MC follows the same charter-ADR-then-waves pattern.
- **bd issues:** epic `mb-pdv`; this ADR is `mb-j99`. Phase-MC seal
  criterion: this ADR `Status: Accepted` + 0027-0030 likewise.

---

_The `adr-format` judge validates this structure exists in every numbered
ADR. Keep section headings stable._
