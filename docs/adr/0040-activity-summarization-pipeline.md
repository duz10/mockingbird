# ADR 0040 — Activity-Capture Summarization Pipeline (Phase 10 Wave 3)

**Status:** Accepted
**Date:** 2026-05-25
**Charter:** [ADR 0036](0036-activity-capture-sibling-subsystem.md), [Phase 10 doc](../phases/phase10.md) § Wave 3.
**Supersedes:** none.
**Related:** ADR 0021 (sync cleanup provider), ADR 0029 (long-form chunked Whisper), ADR 0036 (sibling-subsystem boundary).

## Context

Phase 10 Waves 1B + 2 ship the capture layer: foreground polling, UIA
deep snapshots, idle tracking, multi-monitor attribution. Every
heartbeat lands as an immutable row in `activity_events`. Wave 3 is
the layer that turns the firehose into something a human can actually
read after their session.

The phase doc names four pure pipeline stages — merge/segment, block,
abstract (LLM), assemble — plus a Block-CRUD surface that lets the
user fix anything the heuristics got wrong. The shape is locked; the
*decisions* delegated to this wave are:

1. **Block-boundary heuristic.** When does one Block end and the next begin?
2. **Pipeline call shape.** Direct `OllamaProvider` invocation vs. reuse `meetings::llm_pass`.
3. **No-payload handling.** What does the abstractor do when the snapshot was a game window / locked screen / opaque Electron canvas?
4. **Block rename storage.** The migration 012 schema has no `label` column on `activity_blocks`; rename has nowhere to write.
5. **FTS surface.** Migration 013 was earmarked for Wave 3; what does it cover?

This ADR records those five calls inline rather than ship them as
silent code.

## Decision

### 1. Block-boundary heuristic (five rules, OR-combined)

A new Block starts when ANY of the following hold while iterating the
normalized event stream:

1. **App switch.** The new event's `app_name` differs from the
   previous Block's primary app. (`"chrome.exe"` → `"code.exe"` →
   new Block.)
2. **Title delta within the same app, large enough to be a context
   change.** Same `app_name` but window title's normalized
   Levenshtein distance > 0.4 (i.e. titles share less than 60% of
   their characters). Tuned to keep "Inbox (12) — Gmail" → "Inbox
   (13) — Gmail" in the same Block but break "Inbox — Gmail" →
   "Compose: Re: contract — Gmail".
3. **Idle gap ≥ 60 s.** An `idle_start` → `idle_end` pair whose
   duration is ≥ 60 s ends the previous Block. The post-idle event
   starts a new one. The idle span itself is annotated on the
   previous Block's end_ts; it is not its own Block (one Block per
   *thing the user was doing*, not per heartbeat).
4. **Monitor change.** Same app, same title-ish, but the snapshot
   moved to a different `monitor.name`. This is signal — the user
   physically dragged a window. Per Wave 2 brief § Multi-monitor.
5. **Time cap: 30 minutes.** Within an otherwise homogeneous run
   (same app, same title, no idle), force a break every 30 minutes.
   Avoids the "I had VS Code open for 8 hours" degenerate Block.

**Floor:** any candidate Block shorter than **5 seconds** is folded
into the previous Block (or dropped if it's the very first one).
Stops Alt-Tab-induced noise from generating a 200ms Block.

### 2. Pipeline call shape: direct `OllamaProvider`, NOT `meetings::llm_pass`

The phase doc § Wave 3 deliverables explicitly state:

> Constructs `OllamaProvider::new()` and drives via `CleanupRequest<'_>`.
> NO `CleanupProvider` trait extension (ADR 0036 §Decision item 4).

The dictation IPC `dictation_run_llm_pass` reuses `meetings::llm_pass`
because dictation memos and meetings share enough that the per-pass
engine is generic. Activity-Block abstraction is structurally
different: each call gets ONE small context (one Block, ≤ 32 KB of
UIA payload), and there's no caching surface ("re-run all the passes
for a meeting" doesn't map to Activity — re-running the summary
re-runs every Block).

So `abstractor.rs` builds a one-shot `CleanupRequest` per Block and
calls `OllamaProvider::cleanup` directly. ~50 LoC, no cross-subsystem
import. The Wave-6 judge `ac-no-llm-in-critical-path.md` will grep
that `OllamaProvider` only appears in `abstractor.rs` + tests.

### 3. No-payload Block handling: deterministic template, no LLM call

Wave 2 brief § App-quality matrix established that `status.kind = "no_payload"`
is a real category — game windows, in-app DRM canvases, the lock
screen. Calling the LLM on these is waste: there's no semantic input
beyond `(app_name, window_title)`, and the model will hallucinate
detail to fill the gap.

When a Block's source events are all `no_payload` (or there's just
nothing for the LLM to chew on — e.g. only one `app_switch` event),
the abstractor short-circuits to a deterministic template:

```
Spent {duration} in {app}: {title}
```

No Ollama call. `prompt_version_sha` is set to a sentinel
`"template_no_payload_v1"` so the provenance column still distinguishes
LLM-abstracted Blocks from template-rendered Blocks.

This honors Wave 6 invariant judge `ac-summary-degrades-gracefully.md`:
the session still gets a renderable summary even with Ollama down,
because Blocks that *would* have used the LLM also fall back to the
same template when the provider call errors out.

### 4. Block label storage: new `activity_blocks.label` column (migration 013)

Migration 012 ships `activity_blocks` with `primary_app`,
`generated_abstract`, `source_event_ids`, etc., but **no** `label`
column. Rename therefore has nowhere to go.

Three options considered:

| Option | Verdict |
|---|---|
| Re-use `primary_app` as the label | Conflates "what app was used" with "what the user called this Block". Breaks the provenance contract; provenance is total (Principle 2). |
| Re-use `generated_abstract` as the label | Conflates the LLM's 1-sentence summary with a 3-word user label. The UI then can't show both. |
| **Add `label` column via migration 013** | Cheap. Additive. Honors Principle 2. |

Migration 013 adds `activity_blocks.label TEXT` (nullable). Rename
sets it; the UI prefers `label` when present, falls back to the
primary-app + time-range default rendering otherwise.

### 5. FTS5 surface in migration 013

Migration 013 adds **one** FTS5 virtual table:

- `activity_blocks_fts` — contentless shadow over
  `activity_blocks.generated_abstract` + `activity_blocks.label`,
  with `INSERT` / `DELETE` triggers per the meeting-FTS pattern in
  migration 011.

`activity_events` is **NOT** added to FTS5. Two reasons:

1. The events table is the raw firehose; the user-facing search
   surface is the Block summary, not individual snapshots.
2. Indexing `snapshot_json` would dump UIA fragments into FTS5,
   which is exactly the kind of secondary "shadow-copy of raw
   activity" we promised not to keep. Block abstracts have already
   been pruned + summarized; they're appropriate to index.

If a user need surfaces for "find me the exact moment I tabbed away
from VS Code", that's a Wave 5 polish call, not a v1 invariant.

## Consequences

**Positive:**

- Each pipeline stage is small enough to be tested in isolation
  (throwaway-crate recipe; LESSONS P2). `segmenter`, `blocker`,
  `assembler` are pure Rust.
- The abstractor call shape is the simplest provider invocation in
  the codebase (~50 LoC); easy to reason about, easy to mock for
  the Wave-6 graceful-degradation judge.
- Block rename + Block summary live in their own columns; provenance
  intact.
- Search works on Day 1 of Wave 3 (FTS5 against the Block abstracts
  the abstractor just wrote).

**Negative:**

- Block-boundary heuristics are tuned empirically — the constants
  (60s idle, 0.4 Levenshtein, 30m time cap, 5s floor) WILL need
  adjustment after Dustin runs ≥ 1 hour of real sessions through
  them. They're `pub const` in `blocker.rs` for one-line tuning.
- No-payload template short-circuits the LLM for in-game / locked
  sessions; if Dustin actually wanted a heroic LLM attempt on opaque
  windows (he doesn't — Wave 2 was explicit), that's a config knob
  we'd add later.
- Migration 013 means schema_version 12 → 13; one more file to keep
  in mind for future migrations. Standard cost.

**Neutral:**

- Wave 4 (audio) will need a *parallel* prompt file
  (`abstract_block.audio_aware.md`) when transcript segments
  overlap a Block. Wave 3 lands a stub for that file so the
  abstractor's prompt-set SHA is stable across the Wave-3 → Wave-4
  boundary (changing prompts AFTER abstracts are written would
  break the `prompt_set_sha` provenance contract).

## Out of scope (explicit punts)

- **No-LLM-skip for very short Blocks.** If a Block is ≥ 5 s
  (the floor) but only contains 1-2 events, we still LLM-abstract
  it — small Blocks are still informative and the call is cheap.
- **Audio-aware prompt.** File exists, content fleshed in Wave 4.
- **Wave 1A deferral #2** (dictation-runtime direct signal path).
  Not in scope for Wave 3 — it's cross-subsystem (touches
  `dictation`, which is sealed past `phase-3-complete`), and Wave 3's
  blast radius is `activity/*`. Tracked as bead `mb-fzeo` (P3) for
  Dustin to schedule post-Phase 10.

## References

- Phase 10 doc § Wave 3, §Cross-wave invariants.
- ADR 0036 §Decision items 4 (no `CleanupProvider` extension) + 6
  (raw events immutable).
- LESSONS P2 (cargo test --release blocked; throwaway-crate recipe).
- Wave 2 brief shaping notes #2 (`visibleTextFragments` is pre-truncated),
  #4 (`no_payload` exists), #5 (idle ≥ 60 s is a natural Block boundary).
