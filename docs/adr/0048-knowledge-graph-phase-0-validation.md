# ADR-0048: Knowledge Graph — Phase 0 pipeline validation (sandboxed harness)

- **Status:** Proposed
- **Date:** 2026-05-28
- **Deciders:** Dustin (project lead), Bernard / code-puppy (chartering + implementor)
- **Charter for:** ADR-lateral epic — Knowledge Graph Phase 0 prerequisite gate.
  Sealed via ADR Accepted + STATUS update + `REPORT.md` landing. **NO new
  `phase-*-complete` tag** (lateral epic; not a numbered PLAN §10 phase —
  see LESSONS PINNED P5).
- **Source spec:** [`docs/knowledge-graph/spec.md`](../knowledge-graph/spec.md)
  (imported this iteration, immutable; future ADRs cite section numbers).
- **Sibling-precedent:** ADR 0036 (sibling-subsystem charter pattern), ADR 0046
  (lateral epic with multi-iteration plan and isolation discipline).

## Context

The spec at `docs/knowledge-graph/spec.md` describes a new
Knowledge-Graph subsystem that turns rambling voice memos into
structured, three-layer-tagged Markdown entries inside a synced
Obsidian vault (reusing the ADR 0046 transport). Spec §4 elevates a
single question above the v1 build: **can the local 3B-7B models we
already run actually do the segment / classify / extract / normalize
work well enough to be trusted as the user's filing system?** If the
answer is no, the entire v1 surface is dead in the water.

Phase 0 turns that "can they?" question from a guess into a
measurement. It is a **prerequisite gate**, not a feature — no UI, no
production wiring, no schema migrations. A hand-authored corpus with
an independent answer key (spec §6.1), run through a multi-pass
pipeline (§8.1), scored against pre-committed thresholds (§8.4), and
checked for run-to-run stability (§8.5). The output is a single
go/no-go report (§8.6) plus a v1 scope recommendation.

This ADR charters that gate as an isolated, deletable sandbox crate.

## Decision

We build a **standalone sandbox crate at `experimental/kg-validation/`**
implementing the Phase 0 harness, corpus, multi-pass pipeline, scoring,
and report. The sandbox is **not** a `[workspace.members]` entry of the
root Cargo workspace — it has its own `[workspace]` table so deleting
the directory is a no-op for production. It has **no path-dep on the
`mockingbird` crate** and pulls in **zero** of the heavy native deps
(no `whisper-rs`, no `ort`, no CUDA). This means it builds with vanilla
`cargo` and runs live `cargo test` without the Windows wrapper —
sidestepping LESSONS PINNED P2 entirely.

The architectural contract is spec §5 in full. The decisions below
follow from it.

## Charter

Spec §5 (isolation discipline) is **the** architectural contract for
this epic:

- **§5.1 — Sandbox folder.** All Phase 0 work lives under
  `experimental/kg-validation/`. Deleting that folder must leave the
  production app completely untouched.
- **§5.2 — Reuse by reference, never by copy.** When Phase 0 needs an
  existing production capability, it imports / calls the production
  module as-is. It does **not** copy production source into the
  sandbox.
- **§5.3 — Do not modify production files.** Phase 0 may read from,
  import, and call production modules, but writes only inside its own
  sandbox. Genuine need to change a production module ⇒ **stop and
  flag**, do not silently edit.
- **§5.4 — Deliberate duplication is allowed only for features under
  active change**, and only as a clearly-marked new file inside the
  sandbox.

The Phase 0 scope carve-outs below (§G1-G4) document the specific,
narrow places where strict §5.2 import-by-reference is sanctioned to
relax — each is a deliberate engineering call, recorded here so it is
visible rather than hidden.

## Inherited by ADR 0049 (v1 charter, drafted post-gate)

These three answers were locked at this dispatch but are **v1 concerns,
not Phase 0 scope**. Recorded here so the future v1 charter ADR
(provisionally 0049) does not re-litigate them.

- **Q1 = A — Separate vault subtree.** Outputs land at
  `<vault>/Knowledge Graph/{Inbox,Entries,History}/`. Two iOS Shortcuts
  (one per inbox) on the mobile side. The KG watcher is a **sibling**
  subsystem to the existing inbox courier; the existing `inbox/`
  module (ADR 0046) is **untouched**.
- **Q2 = positional routing.** Routing is purely positional: a file
  landing in `Knowledge Graph/Inbox/` ⇒ KG path; a file landing in
  `inbox/` ⇒ existing dictation/voice-memo path. **No prefixes, no
  settings, no sidecars.** The folder location is the routing signal.
- **Q3 = A — Markdown files are the source of truth.** The Mockingbird
  DB holds only a shadow FTS index of the KG entries. A reverse-watcher
  reconciles vault edits back into the index; on conflict, **the file
  wins**. (This is a v1 architectural novelty — the existing
  dictation/meeting data has DB as source of truth. Not built in
  Phase 0; flagged for the v1 charter.)

## Phase 0 scope carve-outs

### G1 — Ollama dispatch is a local sandbox helper, not a path-dep on `mockingbird::cleanup::ollama`

The harness needs to call the local Ollama server for each pipeline
pass and for the LLM tag-equivalence judge. The production cleanup
provider (`src-tauri/src/cleanup/ollama.rs`) carries provider-trait
plumbing, settings-store coupling, streaming, prompt-version tracking,
and per-mode wiring that has zero relevance to the harness.

The sanctioned read of §5.2 here: **a ~50 LoC reqwest-blocking POST to
`/api/generate` is the right primitive for the sandbox**, not a
path-dep on the production module. The production module has
*production state to drift from* (settings keys, prompt versions,
streaming hooks). The sandbox helper is **trivial, stateless, and
deliberately not engaging any of that** — there is nothing to drift.
Duplicating an HTTP POST is not the kind of duplication §5.2 is
guarding against; it is the kind §5.4 explicitly permits ("deliberate
duplication... clearly-marked new file inside the sandbox that never
overwrites the original").

If during Wave 2 we find the harness *would* benefit from any
non-trivial behaviour from the production provider (e.g. its
prompt-versioning machinery), that's a §5.3 stop-and-flag moment, not a
silent path-dep.

### G2 — Corpus is text-only; the transcribe pass is skipped in Phase 0

Spec §8.1 lists 5 passes: transcribe → segment → classify → extract →
normalize. The transcribe pass is **already validated** — Whisper-rs
CUDA at large-v3-turbo Q5_0 is sealed in `phase-2-complete` and
running in production on real user audio every day. Re-validating it
in Phase 0 would add zero signal and would force the sandbox to pull
in `whisper-rs` (which would re-trigger LESSONS PINNED P2 — the test
runner can't launch with whisper-rs / ort / CUDA in the dep graph).

**Phase 0 authors raw dictation as text strings**, skips the transcribe
pass, and validates passes 2-5. Whisper's quality is not the
falsification risk; the structuring layer is.

### G3 — (reserved — no carve-out at this slot)

Numbered to leave room for a future carve-out without renumbering G4.

### G4 — Determinism knobs pinned in advance

Spec §8.5 (two-run stability) is the spec's own determinism guard.
Layered on top of that:

- **Temperature is pinned at `0.2` for every Phase 0 pipeline pass.**
  This matches the standardized cleanup-pipeline temperature from
  migration 019 / ADR 0047. It is the lowest setting that still gives
  the model room to make a choice on ambiguous segmentation; lower than
  this and we are measuring greedy-decode artifacts rather than the
  model's actual judgement.
- **`seed` is set per-pass where Ollama supports it** for the model in
  use, so the two-run stability comparison sees genuine sampling
  variance and not a no-op.
- **The LLM judge for tag semantic equivalence (spec §8.3) uses a
  different model family than the pipeline-under-test.** If the
  pipeline runs on `qwen2.5:3b`, the judge runs on `llama3.1:8b` (or
  similar). Same-model judging is the failure mode spec §6.1 warns
  about — "the grader just agrees with the processor."

## Thresholds (copied verbatim from spec §8.4 — committed BEFORE running)

| Metric | Threshold | Type |
|---|---|---|
| Clean single-item handled correctly | ~100% | Hard floor — if these fail, halt |
| Segmentation correct on multi-item cases | ≥ 85% | Gate |
| Category correct | ≥ 90% | Gate |
| Type correct | ≥ 85% | Gate |
| Invented dates across the no-date set | **0** | **Hard gate** (trust-critical) |
| Tag-variant collapse correct | ≥ 80% | Gate |

The **"0 invented dates"** row is the hard gate. A pipeline that
hallucinates due dates is worse than no pipeline at all — it silently
puts wrong items on the user's calendar. The schema (`Entry.due_iso:
Option<String>`) enforces this at the type level: the model is
required to emit absence, not "" or `"unknown"` or a guess.

## Stability (copied verbatim from spec §8.5)

> Run the entire corpus through the pipeline **twice** and compare the
> two runs *against each other*, not only against the answer key. Small
> local models are non-deterministic; if the same dictation tags or
> splits differently across runs, that instability is itself a finding
> and means prompts need tightening before any accuracy number is
> trustworthy. Cheap to add, highly revealing.

Stability is reported as a per-metric agreement rate across runs A and
B and is a **named section** of the report, not a footnote.

## Sandbox location

`experimental/kg-validation/` — a top-level repo directory chosen so
the §5.1 deletability test ("deleting the folder must leave production
untouched") is mechanically obvious.

The crate has its own `[workspace]` table in `Cargo.toml`, so it is
**not** a member of the root workspace at `/Cargo.toml`. This means:

- Vanilla `cargo` invocations from inside the sandbox work without the
  Windows CUDA wrapper.
- `cargo check / clippy / test` from the repo root **do not** sweep the
  sandbox — production gates are unaffected by sandbox state.
- The dep graph contains zero native libraries (`whisper-rs`, `ort`,
  CUDA), so the test runner actually launches (LESSONS PINNED P2
  sidestepped).

Permitted deps (Wave 0 floor): `serde`, `serde_json`, `serde_yaml`,
`reqwest` (blocking feature), `chrono` (serde feature), `sha2`,
`anyhow`, `thiserror`. Adding anything else requires either a §5.3
flag or a Wave-N update to this ADR.

## Output

`experimental/kg-validation/REPORT.md` per spec §8.6 — a single
human-readable report containing:

1. Per-metric scores vs. the §8.4 thresholds, with pass/fail per row.
2. The §8.5 two-run stability findings.
3. Notable failure examples (raw dictation + expected vs. actual
   structured output).
4. Any signal that the controlled vocabularies (categories / types in
   §7.2) are wrong for the general American population — this is a
   valuable finding per spec §7.2 and must not be silently suppressed.
5. A clear **go / no-go** verdict.
6. A **v1 scope recommendation** — lighter vs. fuller per spec §9 —
   based on what the numbers actually support.

The report is the artifact that converts this ADR from Proposed to
Accepted.

## Seal

This ADR moves Proposed → Accepted when `REPORT.md` lands with a
go/no-go verdict and STATUS.md is updated to reflect the seal.

**No `phase-*-complete` git tag.** This is an ADR-chartered lateral
epic per LESSONS PINNED P5. Reopening (or chartering v1 from a "go"
verdict) is the job of a successor ADR (provisionally 0049), which
inherits the Q1/Q2/Q3 decisions recorded above.

## Beads

The epic is tracked in `bd` with the prefix `Phase 0 KG:` on every
title, type `task`, priority 2. The dependency graph (10 beads, Wave 0
→ Wave 6) is built at charter time; see `bd ready` after Wave 0
closes for the live queue.

## References

- `docs/knowledge-graph/spec.md` — canonical Phase 0 + v1 spec.
- ADR 0036 — sibling-subsystem charter pattern.
- ADR 0046 — vault transport + lateral-epic-with-iterations pattern.
- ADR 0047 — temperature 0.2 standardization (migration 019).
- LESSONS PINNED P2 — `cargo test --release` launch bug avoidance via
  zero-native-deps sandbox crate.
- LESSONS PINNED P5 — lateral epics seal via ADR, not phase tag.
