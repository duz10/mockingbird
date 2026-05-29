# ADR-0048: Knowledge Graph — Phase 0 pipeline validation (sandboxed harness)

- **Status:** Accepted (sealed 2026-05-29 with Wave 6 REPORT landing at `docs/knowledge-graph/REPORT.md`)
- **Date:** 2026-05-28 (Proposed), 2026-05-29 (Accepted)
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
  different model family than the pipeline-under-test.** Pipeline =
  `qwen2.5:3b` ⇒ primary judge = `gemma2:9b`, cross-check =
  `llama3.1:8b-instruct-q4_K_M`. Same-model judging is the failure
  mode spec §6.1 warns about — "the grader just agrees with the
  processor."

  **Wave 3.3 amendment (2026-05-29).** This ADR originally pinned
  primary = `llama3.1:8b`, secondary = `gemma2:9b`. The swap is
  data-driven: Wave 3.2's full JVP run on `runs/run-a-baseline`
  cleared Gate 1 (`llama3.1:8b` at 11/12 = 91.7%) but STOPped on
  Gate 3 cross-judge (4/7 = 57.1% with `gemma2:9b`), with two of the
  three disagreements in the same direction — `llama3.1:8b`
  Equivalent

### G5 — Judge Validation Protocol (JVP)

The spec's §8.3 "judge must be spot-checked" is the right instinct but
the wrong granularity for the load-bearing role the LLM tag-equivalence
judge plays. The judge's verdict feeds the tag-correctness metric, which
is one of the six §8.4 gates; rubber-stamping by the judge would silently
produce a green threshold on a failing pipeline. We codify five
mechanical gates that run BEFORE any judge verdict is used in scoring,
plus a retrospective audit trail.

All five gate outputs are persisted to
`runs/<run-id>/JUDGE_VALIDATION.json` regardless of pass/fail/warn — the
audit trail is permanent.

- **Gate 1 — Calibration set ≥ 90% (STOP if fails).** A hand-authored
  set of 12–15 gold-standard tag-equivalence pairs lives at
  `experimental/kg-validation/judge-calibration/tag-equivalence.json`.
  The judge must achieve ≥ 90% verdict-correct on this set before it is
  used on real data. Distribution: ~7 unambiguous-equivalent pairs,
  ~5 unambiguous-different pairs (borderline cases are excluded from
  the GATED set — they would create unfair gate failures).

  **Wave 3.3 amendment (2026-05-29) — Gate 1 borderline (observational
  companion).** Calibration v3 adds a `borderline` section (5–6 pairs,
  each tagged with a `dimension` slug — tokenization, specificity,
  coreference, domain-overlap, abstraction-level, person-specific).
  Every JVP run scores the judge against the documented_verdict on
  each borderline pair and persists a `gate1_borderline` block in
  `JUDGE_VALIDATION.json` with per-pair + per-dimension match rates.
  This is OBSERVATIONAL ONLY — `gate1_borderline` has no
  `GateOutcome` and CANNOT contribute to a Halt. Its purpose:
  (a) detect regression in fuzzy-case handling across judge swaps
  (the immediate motivator — the Wave 3.2 → 3.3 swap left the
  doctor-appointment tokenization pair as a known unknown for the
  new judge), (b) auditable record of judgment quality on the hard
  cases, (c) post-hoc cross-walk with PCRP themes. Borderline
  handling remains a PCRP-territory concern; the borderline section
  is the cheap mechanical complement.
- **Gate 2 — Per-verdict reasoning audit ≥ 95% (STOP if fails).** Every
  judge response must (a) carry a reasoning string longer than 30
  characters, (b) reference at least one token from BOTH candidate tag
  sets in that reasoning, and (c) place the verdict marker AFTER the
  reasoning, not before. Catches rubber-stamping and verdict-first
  shortcut patterns the chain-of-thought prompt is supposed to prevent.
- **Gate 3 — Cross-judge sample 10%, agreement ≥ 85% (STOP if <85%,
  WARN if 85–95%).** 10% of verdicts are re-judged by a second model
  drawn from a different family than both the primary judge AND the
  pipeline-under-test. Concrete picks (Wave 3.3, see §G4 amendment
  above): pipeline = `qwen2.5:3b`, primary judge = `gemma2:9b`,
  secondary cross-check = `llama3.1:8b-instruct-q4_K_M`. If the
  cross-judge model is not pulled and cannot be substituted, Gate 3
  demotes to WARN-only and the JVP report records the demotion
  explicitly — we do NOT attempt to auto-pull a 4–9 GB model.
- **Gate 4 — Distribution sanity 40–80% equivalent rate (WARN).**
  Outside that range, flag in `REPORT.md` but proceed. A judge that
  marks 99% equivalent or 5% equivalent is almost certainly
  collapsed; an in-band rate is a (weak) sign the judge is actually
  reading both sides.
- **Gate 5 — Determinism re-run of first 5 verdicts (WARN).** Re-issue
  the first 5 judge calls with the same seed and assert byte-identical
  output. If they differ, document the judge-stability concern; do
  not block scoring on it (small local models drift even with seeds,
  and the §8.5 two-run pipeline stability check is the primary
  determinism gate).

Overall verdict semantics: any STOP-class failure on Gates 1/2/3 halts
scoring (the tag metric would be invalid). WARN-class outcomes proceed
but land in `REPORT.md` prominently. Gate sequence is fixed; later gates
do not run when an earlier one STOP-fails (no point re-judging the same
verdicts with a second model if the calibration set proves the primary
judge is broken).

### G6 — Persona Cross-Reference Pass (PCRP)

JVP is quantitative; PCRP is the structured qualitative audit that
complements it. It runs AFTER scoring and writes
`runs/<run-id>/PERSONA_REVIEW.md`. The intent is to catch the failure
modes that the metric blindspots — "pipeline produced an entry that
scored as correct on every metric but is, in fact, the wrong reading of
what the user said" — exist precisely because the answer key is one
interpretation of an ambiguous utterance, not the only one.

PCRP samples ~12–15 dictations, weighted toward multi-item rambler /
ambiguous-category / no-date cases, plus a few that scored well
quantitatively (the confirmation-bias guardrail below).

**Discipline rules — these are the load-bearing anti-rubber-stamp
safeguards. Without them PCRP devolves into the LLM-judges-LLM-output
failure mode spec §6.1 warns against:**

- **Persona-first reading order.** Re-read the persona's voice notes
  from `CORPUS_NOTES.md` BEFORE looking at the persona's pipeline
  outputs. The audit anchors on the user model, not the model output.
- **Structured failure-mode prompts.** Not "how does this look?" but
  "look for these specific failure shapes: hallucinated dates, weird
  tags, miscategorized obvious cases, titles misrepresenting intent,
  over-formalization of casual speech."
- **Bias toward finding problems.** Default assumption: at
  `qwen2.5:3b` scale, problems exist. If the reviewer finds none,
  look harder — especially at multi-item and ambiguous-category cases.
- **Evidence required.** Every claim cites a specific dictation ID +
  a quoted output line. No abstract observations.
- **Confirmation-bias guardrail.** The review must include at least 3
  cases where quantitative scores PASSED. If qualitative says "bad"
  on metric-good cases, that is a metric-blindspot finding worth
  surfacing. If qualitative says "great" unanimously on metric-bad
  cases, that is a code-puppy rubber-stamp signal — go back and look
  harder.

PCRP output contains a per-persona summary + top-3 trust-eroding
failures + top-3 trust-building wins + cross-cutting observations.

**Cadence:** PCRP runs twice — after Wave 3 baseline scoring AND after
Wave 5 final iteration. The two runs are comparable; degradation
between them on the Wave-5 corpus is itself a finding.

**Load-bearing on go/no-go.** If PCRP's final-run (post-Wave-5) report
lists ≥ 5 trust-eroding failures AND the quantitative scores do not
EXCEED all spec §8.4 thresholds by more than 5 points (i.e. the
pipeline is barely passing on the numbers and PCRP finds the user
wouldn't trust it in practice), Wave 6's `REPORT.md` must default to
NO-GO, with the conjunction of those two conditions documented as the
reasoning.

### G7 — Tag-collapse metric: deterministic measurement (Option E, 2026-05-29)

**Supersedes spec §8.3 (LLM tag-equivalence judge) for the tag-collapse
metric only.** The Judge Validation Protocol architecture in §G5 and the
`src/scoring/judge_validation.rs` module are preserved for any future
LLM-judged metric, but **JVP is not invoked for tag scoring** under §G7.

#### Why

Wave 3.2 and Wave 3.3 empirically falsified the LLM-judged approach on
this corpus:

- **Wave 3.2** (`llama3.1:8b` primary, `gemma2:9b` cross-check) — Gate 3
  STOP at 4/7 (57.1%, threshold ≥ 85%). Tag-collapse reported 81.8%
  (45/55) but the verdict-level disagreement on three specific personas
  meant the headline was not defensible.
- **Wave 3.3** (`gemma2:9b` primary, `llama3.1:8b` cross-check — judge
  swap per §G4) — Gate 3 STOP at 5/9 (55.6%). Same three personas; the
  **disagreement direction inverted** with the swap. Same pipeline data
  re-scored produced a tag-collapse number of 38.2% (21/55) — a **43-point
  judge-dependent gap** with no defensible "correct" number in between.

The two consecutive Gate 3 STOPs with directionally-inverted disagreement
are not a prompt-tuning problem (rejected: Wave 3.3 ran with calibration
v3 borderline-pair telemetry) and not a judge-selection problem
(empirically falsified by the swap). Per AGENTS.md Principle 6 ("if
something is hard to verify, that's the bug"), the metric design — not
the judge — is the bug.

#### What (replacement metric)

For each pipeline entry vs. its answer-key entry:

1. **Canonicalize** the actual `topic_tags` via a versioned synonym map
   at `experimental/kg-validation/judge-calibration/synonym-map.json`:
   each tag is replaced by its canonical form if mapped, else passes
   through unchanged. Result is a `BTreeSet<String>` (set semantics,
   order-independent).
2. **Canonicalize** every `acceptable_topic_tag_sets[i]` from the answer
   key the same way.
3. Compute the **Jaccard similarity** of `actual_canonical` against each
   `accept_canonical[i]`. Record the MAX.
4. **Pass condition:** the entry is "tag-collapse correct" iff
   `MAX(jaccard) == 1.0` — i.e. one of the acceptable sets matches the
   actual set exactly after canonicalization.
5. Compute the per-run ratio of correct entries / total scoreable
   entries. **Spec §8.4 threshold ≥ 80% is unchanged.**

Jaccard at lower thresholds (0.8, 0.67, 0.5) is **reported
observationally** in `SCORE.json` and `SCORE_SUMMARY.md` but does NOT
gate. Picking exact match (1.0) — rather than a partial-overlap
threshold — is deliberate: the synonym map IS the equivalence engine,
and accepting partial overlap on top of it would double-count forgiveness
and obscure synonym-map gaps. **Misses point directly to synonym-map
candidates for iteration**, which is the desired feedback loop.

A **near-miss report** (top 10 most-frequent `(actual_tag,
answer_key_tag)` pairs that prevented a 1.0 Jaccard, ranked by
frequency) is surfaced in `SCORE_SUMMARY.md`. These are the
empirical Wave-5 prompt-iteration + synonym-map-iteration candidates.

#### Synonym map sourcing & versioning

The map is JSON, schema_version `synonym-map-v1`, top-level fields
`version` + `synonyms[]`. Each entry: `canonical` (string), `variants`
(string[], may be empty), `rationale` (string), `source` (one of
`auto-seed-answer-key` | `bernard-seed` | `diff-driven-codepuppy`).

**Authoring procedure for v1:**

1. **Auto-seed.** Walk every answer-key file; for every distinct tag in
   every `acceptable_topic_tag_sets[*]`, seed an entry with that tag as
   `canonical`, empty `variants`, `source: "auto-seed-answer-key"`.
   This guarantees every answer-key tag is at minimum its own canonical
   form (identity self-map for unknowns).
2. **Hand-augment with Bernard's project-knowledge seed list** (see
   Wave 3 dispatch brief for the full list — household / professional /
   tradesperson / caregiver domain coverage). Source `bernard-seed`.
3. **Diff-driven discovery.** Collect every tag emitted by the pipeline
   in `runs/run-a-baseline/structured/*.json`; for each tag that isn't
   already a canonical or a variant, propose a canonical only when the
   equivalence is **clear and unambiguous**. Source
   `diff-driven-codepuppy`. **Borderline → leave out** — the metric is
   supposed to surface synonym-map gaps; over-collapsing hides them.

**Discipline rules (binding on every authoring pass):**

- Person-name tags NEVER collapse into domain tags (`karen` stays
  distinct from `finance` even when consistently paired).
- Specificity collapse only when the specific tag's extra information
  is genuinely redundant (`auto-maintenance` → `car-repair` is fine;
  `brake-pads` → `maintenance` is not, because `brake-pads` carries
  irreducible specificity).
- Domain overlap is NOT equivalence (`etsy` and `social-media` are
  different channels even though Etsy is social commerce).
- When in doubt: leave out. Adding a variant later is one-line; ripping
  out a wrong collapse retroactively invalidates prior scoring.

#### Operational consequences

- **No LLM calls** during tag scoring. Wall time for the tag metric
  drops from ~40 min (55 entries × ~45s LLM per call × multi-acceptable-set
  fan-out) to milliseconds.
- `--judge-model` / `--cross-judge-model` / `--judge-seed` flags remain
  on `score-run` for the JVP architecture (preserved for future
  LLM-judged metrics), but the tag-collapse code path no longer
  consults them. JVP itself is not invoked in current Phase 0 since
  tag-collapse is the only metric that used it.
- The Wave 4 invariant-judge suite drops the JVP-completeness judge
  (no judge to validate); the threshold judge picks up tag-collapse
  via the same deterministic ratio as every other metric.
- The Wave 5 prompt-iteration loop gains a concrete new input: the
  near-miss report. Each top-N near-miss is either (a) a synonym-map
  gap to close in `synonym-map.json`, or (b) a genuine pipeline
  miscategorization to address in the extract / classify prompt.

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
