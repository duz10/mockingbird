# Phase 0.5 Knowledge-Graph Architectural-Pivot Report

**Authoring iteration:** Wave 0.5.6 (`mb-qogz`), epic `mb-symi`, Phase 0.5 KG.
**Charter:** [ADR 0049](../adr/0049-knowledge-graph-phase-0-5-and-v1-architectural-pivot.md).
**Phase 0 inheritance:** [Phase 0 REPORT.md](REPORT.md) (the assisted-filing
v1 baseline this pivot reopens), [ADR 0048](../adr/0048-kg-phase-0-validation-methodology.md)
(methodology). LESSONS PINNED **P9 / P10 / P11 / P12** carry the load-bearing
findings of this phase.
**Status:** Phase 0.5 SEALED on Wave 0.5.6 landing; ADR 0049 → Accepted.
**Phase 1 commitments:** binding (§6 below).

---

## 1. Executive summary

Phase 0 sealed with a strict NO-GO / defensible GO-WITH-LIMITATIONS verdict
on an assisted-filing UX: trust gates PASS, quality gates (category 67.3%,
entry-type 78.2%, clean-single 6.7%, tag-collapse 9.1%) FAIL by margins no
single-prompt iteration on qwen2.5:3b could close. Phase 0.5 was chartered
to test whether the **architecture itself**, not the model size, was the
cap — via four interventions on the same sandbox (SCHEMA.md portable
contract / embeddings classifier / closed tag vocabulary / entity
extraction).

**The four moves split 2-2 between architectural keepers and architectural
falsifications.** SCHEMA.md as portable contract + per-model-class
calibration profiles graduated to a v1 commitment and restored the 7b hard
gate with simultaneous Pareto-frontier lifts on category (+14.2), entry-type
(+10.7), and clean-single (+26.6) from the Phase 0 baseline. Entity
extraction at qwen2.5:7b mid-confident reached 54.83% / 53.40% strict Jaccard
(bar 50%) with 97.08% stability — empirically validating LESSONS P11's
"tags ≠ entities" diagnosis and clearing the bar for v1 inclusion.
Embeddings nearest-neighbour classification regressed both prototype methods
(-11 to -20pp across category / entry-type / clean-single) and was retired
as a classification mechanism, preserved as a similarity tool for Move 4
disambiguation. Closed-vocab Move 2 took three iterations to land its
wiring fix, lifted ~2pp on the primary metric, and surfaced the deeper
architectural finding that drove the v1 two-field schema commitment.

**The v1 architecture that emerged:** segment → classify → extract →
**extract_entities** → normalize, driven by a single `SCHEMA.md` portable
contract with per-model-class calibration profiles, structured entries
carrying a two-field `tags:` (closed-vocab semantic categories) + `entities:`
(open-class typed references) schema, pinned to `qwen2.5:7b-instruct-q4_K_M`
for entity-aware operation with `qwen2.5:3b` documented as a tags-only
degraded mode. Per-pass model routing (Clark's Nemotron pattern) deferred
to v1.1; closed-vocab Move 2 (the `tags:` half) deferred to v1.1 awaiting
two-field corpus re-labeling. Phase 0.5 sealed clean; Phase 1A (production
wiring) is the next chapter.

---

## 2. The architectural pivot under test

ADR 0049's charter named four moves, each targeting a different Phase 0
gap or mission-enabling capability:

| Move | Lever | Target gap | Outcome shape |
|---|---|---|---|
| **1. SCHEMA.md as portable contract** | Single Markdown contract owning pass prompts + taxonomy + per-pass model selection. Rust modules become contract-consumers. | Architectural foundation — without it, every later move forks the truth. | **KEEP** (with calibration-profile revision per LESSONS P10). |
| **2. Embeddings classifier** | `nomic-embed-text` nearest-neighbour over labeled exemplars replaces LLM `classify` pass. | Phase 0 category 67.3% + entry-type 78.2% gaps. | **FALSIFIED** at 32-pair corpus scale; infrastructure preserved for Move 4 disambiguation (ADR 0049 amendment A1). |
| **3. Closed canonical tag vocabulary + new-tag-request flow** | Extract pass produces tags only from a 228-entry closed vocabulary; out-of-vocab concepts emit `proposed_new_tag` request entries. | Phase 0 tag-collapse 9.1% gap. | **DEFERRED to v1.1** — wiring fix architecturally correct and on `main`, but Phase 0 corpus tags conflate semantic categories with open-class entities. Closes only the semantic-categories half. |
| **4. Entity extraction probe** | LLM-driven typed entity extraction (person / organization / project / place / object) + lexical + embedding disambiguation. New `entity-quality` metric, ≥50% bar for v1 inclusion. | "Connected" half of the mission. Mission-enabling, not gap-closing. | **ACCEPT** at qwen2.5:7b mid-confident; 54.83% / 53.40% Jaccard ≥ 50% bar, 97.08% stability. |

Two were the level-up; two surfaced the constraints. The 2-2 split is the
shape of the v1 architecture.

---

## 3. The Phase 0.5 scorecard journey

The metric evolution across runs, end-to-end:

| Run | Date | Model / config | Hard gate | Category | Entry-type | Clean-single | Tag-collapse (J=1.0) | Entity-quality (J strict) | Seeds | Notes |
|---|---|---|---|---:|---:|---:|---:|---:|---|---|
| Phase 0 sealed (Wave 3.4 / Wave 5 baseline) | 2026-05-29 | qwen2.5:3b small | **0** ✅ | 67.3% | 78.2% | 6.7% | 9.1% | — | 42 + 137 | Phase 0 REPORT.md final scorecard. |
| Wave 0.5.1 raw 7b (HALT) | 2026-05-29 | qwen2.5:7b, Phase 0 prompts unchanged | **4** / **5** ❌ | 81.5% | 88.9% | 33.3% | 11.1% | — | 42 + 137 | Hard-gate breach on naive model swap (LESSONS P10). `runs/run-7b-baseline/` + `run-7b-stability/`. |
| Wave 0.5.1 `iter-1-7b-fix` | 2026-05-29 | qwen2.5:7b mid-confident, calibration profiles | **0** ✅ | 81.5% / 83.3% | 88.9% / 90.7% | 33.3% | 14.8% | — | 42 + 137 | Hard-gate RESTORED via SCHEMA.md model-class calibration. Pareto-clean. `runs/iter-1-7b-fix/` + `iter-1-7b-fix-stab/`. |
| Wave 0.5.2 embed-NN | 2026-05-29 | nomic-embed-text, nearest-neighbour | 0 ✅ | 70.4% (-11.1) | 68.5% (-20.4) | 13.3% (-20.0) | — | — | 42 | Falsified; A1 amendment. `runs/iter-2-embed-classify/`. |
| Wave 0.5.2 embed-centroid | 2026-05-29 | nomic-embed-text, per-label centroid | 0 ✅ | 66.7% (-14.8) | 75.9% (-13.0) | 13.3% (-20.0) | — | — | 42 | Falsified; A1 amendment. `runs/iter-2-embed-centroid/`. |
| Wave 0.5.3 closed-vocab iter 1 | 2026-05-29 | qwen2.5:7b, verbose closed-vocab prompt | 0 ✅ | 81.1% (-0.4) | 90.6% (+1.7) | 33.3% | 5.7% (-9.1) | — | 42 | Over-tagging regression. `runs/run-7b-closed-vocab-seed42/`. |
| Wave 0.5.3 closed-vocab iter 2 | 2026-05-29 | qwen2.5:7b, tight closed-vocab prompt | 0 ✅ | 81.1% (-0.4) | 90.6% (+1.7) | 33.3% | 3.8% (-11.0) | — | 42 | Under-tagging regression. `runs/run-7b-closed-vocab-iter2-seed42/`. |
| Wave 0.5.3 closed-vocab iter 3 | 2026-05-29 | qwen2.5:7b, wiring fix (SynonymMap in-band) | 0 ✅ | 81.1% / 81.1% | 90.6% / 88.7% | 33.3% | 5.7% / 3.8% | — | 42 + 137 | Wiring architecturally correct; residual 9.1pp gap is tag/entity conflation (LESSONS P11). `runs/run-7b-closed-vocab-iter3-seed42/` + `-seed137/`. |
| **Wave 0.5.4 entity probe (ACCEPT)** | 2026-05-29 | qwen2.5:7b mid-confident, `extract_entities` | 0 ✅ | — | — | — | — | **54.83% / 53.40%** ✅ | 42 + 137 | Bar 50%. Stability 97.08%. `runs/run-7b-entities-seed42/` + `-seed137/`. |
| Wave 0.5.5 3b cross-test (METHODOLOGY) | 2026-05-29 | qwen2.5:3b small-conservative, same SCHEMA | 0 ✅ | — | — | — | — | 33.21% / 35.48% (-21.62 / -17.92) | 42 + 137 | 21pp cliff is structural under-extraction, stability 96.85%. LESSONS P12. `runs/run-3b-entities-seed42/` + `-seed137/`. |

Per-seed run artifacts (gitignored) preserved on disk for the audit trail.
Every `runs/run-*` directory contains the parsed structured outputs +
metric JSON + per-iteration summaries that grounded the journey above.

---

## 4. The four load-bearing findings (LESSONS P9–P12)

Each is framed as a v1-implication, not just a Phase 0.5 observation.

### P9 — Strict-no-regression IAP cannot ratchet on small local models *(carried forward from Phase 0)*

**Origin:** Wave 5 Phase 0 (`mb-ojm5`) — 0/5 prompt iterations accepted
under strict no-per-metric-regression IAP, despite four of five having
aggregate-positive deltas, because joint-distribution shift across passes
made some-metric-regression nearly inevitable on any prompt change.

**v1 implication:** the IAP discipline splits by metric cost-of-regression.
**Strict no-regression** stays as the rule for **trust-critical gates** —
invented dates, raw-data immutability, junk handling, clipboard
save/restore, secure-input detection. **Pareto-frontier acceptance** is
the rule for **quality metrics** — category, entry-type, clean-single,
tag-collapse, PCRP. Both disciplines coexist in the same iteration loop;
the metric, not the loop, defines its own acceptance shape. Phase 0.5
operated under this revised IAP from Wave 0.5.1 onward and accepted
Move 1 cleanly on iteration 1 — vindicating the split.

### P10 — Prompts calibrate to a model's natural prior; SCHEMA.md needs per-model-class calibration profiles

**Origin:** Wave 0.5.1 (`mb-4xtd`) — qwen2.5:7b breached the date hard
gate (4 and 5 invented dates at seeds 42 / 137) on the unchanged Phase 0
prompt that ran clean on qwen2.5:3b. Root cause: the prompt's null-bias
was empirically tuned through Phase 0 Wave 5 against the 3b's
cautious-by-default prior; the 7b's confident-by-default prior is not
sufficiently pushed by the same instructions on borderline temporal
anchors (duration phrases, vague-future, past-tense, multi-segment date
bleed).

**v1 implication:** the honest version of "the schema travels across
models" is *the schema travels across models WHEN the schema encodes
model-aware calibrations*. SCHEMA.md ships with:

1. **`## Model-class calibration profiles`** — named profiles
   (`small-conservative`, `mid-confident`) with assignment table
   (`qwen2.5:3b` → small-conservative; `qwen2.5:7b` → mid-confident).
2. **Unknown-model default = `mid-confident`** — safer on the trust-gate
   axis. Over-cautious-prompt-on-confident-model just adds nulls;
   under-cautious-prompt-on-confident-model invents dates.
3. **`### Profile-specific prompt overrides` table** — `(pass, profile) →
   prompt-file` rows layered on top of the per-pass default table. The
   loader resolves `prompt_body(pass, model) = overrides[(pass,
   profile_for(model))] ∥ default[pass]`.
4. **Parity-gate-on-OLD-model-first** — Wave 0.5.1 ran the SCHEMA refactor
   parity at 3b→3b and got byte-identical results, cleanly isolating
   refactor-regression from model-regression on the subsequent 3b→7b swap.
   The parity gate is itself the schema's protection against silent
   prompt-formatting drift.

The work of taming a specific model gets captured in the schema, persists
across the system's lifetime, and survives model swaps.

### P11 — "Tags" and "entities" are different objects; conflating them in one field defeats both closed-vocab AND entity-extraction layers

**Origin:** Wave 0.5.3 (`mb-rzpd` + `mb-e10v`) — closed-vocab Move 2's
wiring fix (synonym-map applied in-band at validate-time) was
architecturally correct and lifted ~2pp, but left a 9.1pp residual gap
vs the open-vocab baseline. Diagnostic: 9 of the top 10 near-misses
(`becca`, `dad`, `costco`, `brake-pad`, `bakery`, `app`, `business`,
`business-tool`, `design`) are **open-class entity references**, NOT
semantic category tags. The Phase 0 corpus answer keys conflated two
distinct object types in a single `tags:` field.

**v1 implication (binding):** the structured entry schema has **two
fields**:

- `tags: [...]` — closed-vocab semantic categories (`work`, `car-repair`,
  `finance`, `health`). Bounded, closed-world, curatable. Closed
  vocabulary works here (v1.1 mechanism).
- `entities: [{name, type}]` — open-class typed references (person names,
  brand names, project names, specific objects). Unbounded long tail.
  Entity extraction works here (v1 mechanism, per P11's empirical
  validation in Wave 0.5.4).

You cannot curate an infinite tail globally. Closed-vocab fails for entity
references by design; entity extraction fails for semantic categories by
design (no taxonomy means no consistency). The two-field schema lets the
right mechanism own the right object type. Phase 1 corpus authoring
splits answer keys into the two-field schema from the start.

### P12 — Cross-class schema portability has a floor for some pass types; per-class calibration profiles do not always close the gap

**Origin:** Wave 0.5.5 (`mb-5r1b`) — same SCHEMA, same `extract_entities`
pass, same labels, same scorer; qwen2.5:7b mid-confident scored
54.83% / 53.40% Jaccard, qwen2.5:3b small-conservative scored 33.21% /
35.48% — a 21-point cliff with 96.85% stability across 3b seeds (the
under-extraction is consistent, not noisy). Per-dictation diagnostic on
the entity-richest fixture (`persona-05-case-03`, 11-entity rich): 7b@42
scored 69%, 3b@42 scored 17% — the 3b dropped Mom, Dad, Lisa, Smiths,
birthday-cake, soccer-cleats, summer-reading-log, school, receipts.

**v1 implication (binding):** schema portability is a **2-D problem**
(pass-type × model-class), not a 1-D one. P10 surfaces one half (prompts
tuned on a small model don't transfer up); P12 surfaces the symmetric
other half (per-class calibration profiles for a complex extraction task
do not always recover the lift the larger-model profile delivers, even
when the schema is shared). Together they bound SCHEMA portability in
both directions.

For v1 this means: **qwen2.5:7b is pinned for entity-aware operation.**
3b is documented as a tags-only degraded mode (classification still
functional with `small-conservative` profile; entity layer disabled).
Per-pass model routing (Clark's Nemotron pattern — cheap 3b for
segment/classify, capable 7b for extract/extract_entities) remains v1.1
capability. The SCHEMA.md slot for per-pass model selection exists today;
v1 just defaults everything to 7b for readability.

---

## 5. ADR 0049 amendments

### A1 (Wave 0.5.2) — Move 1 mechanism revised

**Original:** Local embeddings (`nomic-embed-text`) with nearest-neighbour
classification over a labeled exemplar pool replaces LLM category/type
reasoning.

**Empirical evidence:** Both prototype methods (nearest-neighbour and
per-label centroid) regressed materially vs the 7b LLM + SCHEMA.md
baseline:

| Metric | 7b LLM + SCHEMA | embed-NN | embed-centroid |
|---|---:|---:|---:|
| Category | 81.5% | 70.4% (-11.1) | 66.7% (-14.8) |
| Entry-type | 88.9% | 68.5% (-20.4) | 75.9% (-13.0) |
| Clean single-item | 33.3% | 13.3% (-20.0) | 13.3% (-20.0) |

**Revised commitment:** Move 1's *outcome* (close Gaps 2+3) is achieved by
SCHEMA.md + 7b LLM classification. Embeddings infrastructure preserved
for entity disambiguation in Move 4 (similarity over candidate aliases,
its natural problem shape). Speculative embedding-based classification
for v1 is dropped; future reconsideration only at corpus ≥ 500 entries/user
AND per-class exemplars ≥ 100 AND measured LLM-only ceiling on category/type.

### A2 (Wave 0.5.3 + 0.5.4) — Two-field structured entry schema commitment

**Original (charter):** structured entry has a single `tags:` field;
closed-vocab Move 2 covers all tagging.

**Empirical evidence (Wave 0.5.3):** Phase 0 corpus's `tags:` answer keys
conflate two distinct object types — semantic categories (curatable,
closed-world) and open-class entity references (uncuratable infinite
tail). Top 9 of 10 closed-vocab near-misses (LESSONS P11) were open-class
entities. Closed-vocab cannot recover entities the model correctly
recognises as out-of-vocabulary and omits.

**Empirical evidence (Wave 0.5.4):** entity extraction at qwen2.5:7b
mid-confident reaches 54.83% / 53.40% strict Jaccard at seeds 42 / 137 —
clears the 50% bar for v1 inclusion. 97.08% stability. 9 of the 10
Wave 0.5.3 closed-vocab near-misses (Mrs Chen, Home Depot, brake-pads,
Karen, launch, Costco, etc.) are recovered cleanly as entities, empirically
validating the diagnosis.

**Revised commitment (v1 binding):** structured entry schema has two
fields:

- `tags: [...]` — closed-vocab semantic categories (handled by Move 3 in
  v1.1).
- `entities: [{name, type}]` — open-class typed references with 5-bucket
  taxonomy from SCHEMA.md rev `phase-0.5-wave-4` (handled by `extract_entities`
  pass, v1).

**Closed-vocab Move 2 deferred to v1.1** after the corpus is re-labeled
to the two-field schema. The `synonyms.rs` lift + `tag_validator.rs`
wiring (commit `8fdc7fb`) remains on `main` and is the v1.1 starting
point.

**v1 fallback for `tags:`:** open-vocabulary extraction with synonym-map
canonicalization + new-tag-request log (the Phase 0 architecture, validated
working). The two-field schema lets us ship `entities:` in v1 without
gating on the `tags:`-half closed-vocab work.

### A3 (Wave 0.5.5) — qwen2.5:7b model pin for entity-aware operation

**Original (charter):** qwen2.5:7b default with qwen2.5:3b cross-test
probe; per-pass model routing deferred to v1.1+.

**Empirical evidence:** 3b small-conservative profile drops entity-quality
54.83% → 33.21% (-21.62pp) on the same SCHEMA, pass, scorer, and labels.
The cliff is structural (96.85% stability across 3b seeds = consistent
under-extraction, not stochastic noise). Schema portability is 2-D (pass
× model-class), not 1-D — LESSONS P12.

**Revised commitment (v1 binding):**

- v1 pins to `qwen2.5:7b-instruct-q4_K_M` for entity-aware operation.
- qwen2.5:3b is documented as a **tags-only degraded mode** — classification
  remains functional with the `small-conservative` profile; entity layer
  is disabled.
- **Hardware-floor disclosure required in v1 install docs:** 16GB+ RAM,
  GPU recommended (~5 GB VRAM working set, ~4.7 GB on disk for the 7b-q4
  GGUF) for full entity-aware operation.
- **Per-pass model routing (Clark's Nemotron pattern) confirmed deferred
  to v1.1.** SCHEMA.md's per-pass-model-selection slot exists today; v1
  defaults everything to 7b.

---

## 6. v1 architecture commitments (binding)

The v1 architecture is what survived Phase 0.5's four moves plus the
amendments above. Recorded here as binding for Phase 1A onwards.

| Commitment | Source | Notes |
|---|---|---|
| **Pipeline:** segment → classify → extract → **extract_entities** → normalize | ADR 0049 Moves 1 + 4; Wave 0.5.4 ACCEPT | Five passes, `extract_entities` is the new layer. |
| **Two-field structured entry schema:** `tags: [...]` + `entities: [{name, type}]` | LESSONS P11; ADR 0049 amendment A2 | Tags-half open-vocab in v1 (closed-vocab deferred to v1.1). |
| **SCHEMA.md drives all passes** with per-model-class calibration profiles | ADR 0049 Move 1 + amendment A1; LESSONS P10 | `small-conservative`, `mid-confident`; unknown model defaults `mid-confident`. Per-pass-model-selection slot exists; defaults to 7b. |
| **qwen2.5:7b-instruct-q4_K_M pinned** for v1 full features | ADR 0049 amendment A3; LESSONS P12 | 3b = documented tags-only degraded mode (entity layer disabled). |
| **Embeddings infrastructure preserved** for entity disambiguation | ADR 0049 amendment A1; Wave 0.5.2 falsification | `nomic-embed-text`; similarity over candidate aliases, NOT classification. |
| **Closed-vocab Move 2 deferred to v1.1** | ADR 0049 amendment A2; LESSONS P11 | `synonyms.rs` lift + `tag_validator.rs` wiring remain on `main` as v1.1 starting point. Picks up after two-field corpus re-labeling. |
| **Graph layer is OPT-IN** | ADR 0049 charter | Default off. Activated via Settings → Knowledge Graph. Dictation experience unchanged. Binding mission-cohesion guarantee. |
| **Intake UX preserved** | ADR 0049 charter | Async filing queue with status indicator; live-dictation latency budget ~1 min for graph backlog drain. |
| **Files-as-source-of-truth + vault subtree + positional routing** | ADR 0048 §3 Q1/Q2/Q3 | Inherited unchanged from Phase 0. |
| **Hard gate `invented_dates_count = 0`** | ADR 0048 + ADR 0049 charter | Strict IAP discipline; never relaxes. |
| **IAP split:** strict on trust gates, Pareto-frontier on quality metrics | LESSONS P9; ADR 0049 IAP section | Phase 1 work runs under this discipline by default. |

---

## 7. v1 build plan (Phase 1A–1E reference)

High-level shape only — each wave gets its own brief at kickoff. Recorded
here so the Phase 0.5 SEAL leaves Phase 1 with a known target.

| Wave | Title | Outcome |
|---|---|---|
| **1A** | Schema-driven pipeline graduates to production | The `SCHEMA.md` contract + loader + 5-pass pipeline move from `experimental/kg-validation/` to `src-tauri/src/kg/`. Sandbox-isolation discipline lifts (window opens per ADR 0049 §"Sandbox isolation"). Production cargo gate replaces vanilla sandbox gate. |
| **1B** | SQLite extensions for entity / tag / edge tables | New migration: `entities`, `canonical_tags`, `edges` (entry↔entity, entry↔tag, entity↔entity co-occurrence). Concept pages as computed SQL views, NOT stored entities (files-as-source-of-truth preserved). |
| **1C** | Retrieval UX v1 | Six retrieval axes at v1: chronological + entity + tag + category + free-text search + date range. New UI surface; opt-in graph activation lives here. |
| **1D** | Migration backfill | One-shot job that classifies + tags + entity-extracts pre-Phase-1 entries. User-visible as a one-time progress overlay (reuse ADR 0046 Wave 4 import-progress pattern). |
| **1E** | v1 beta release tag | Cargo + UI gate green; live-fire Win11 smoke; STATUS update; tag `v0.2.0-beta-kg` (lateral epic, no `phase-*-complete` tag — Phase 1 KG is not a numbered PLAN §10 phase). |

---

## 8. v1 ship criteria

Recorded as the explicit acceptance shape Phase 1 will gate against.
Below this bar → Phase 1 does not ship the graph layer; the assisted-filing
v1 UX from Phase 0 REPORT.md §8 remains the v1 surface.

| Criterion | Threshold | Discipline |
|---|---|---|
| Hard gate intact on production traffic | `invented_dates_count = 0` on rolling sample | Strict IAP (P9). Non-negotiable. |
| Entity-quality on rolling sample | ≥ 50% strict Jaccard | Pareto-frontier IAP. Phase 0.5 Wave 0.5.4 result is the calibration anchor. |
| Opt-in graph guarantee | Existing dictation users see ZERO regression with graph off | Binding mission-cohesion. Audit via dictation-untouched judge (Phase MC pattern). |
| Intake latency budget | Entries appear in the graph within ~1 min of dictation completing | Async queue with visible status indicator. Blocking the dictation post-paste is NOT acceptable. |
| Degraded-mode disclosure | 3b users see clear "tags-only mode (entity layer requires qwen2.5:7b)" surface | Settings → Knowledge Graph; install docs. |
| Stability (sampled) | ≥ 80% structural-metric agreement across seeded re-runs | ADR 0048 §8.5 floor carried forward; Phase 0.5 measured ≥ 95% routinely. |

---

## 9. Red flag status

The six original Bernard red flags from the ADR 0049 charter, with
resolution status:

| # | Red flag | Status |
|---|---|---|
| 1 | Hardware floor — 7b default | **CONFIRMED + MITIGATED.** Wave 0.5.5 (LESSONS P12) sealed the floor; opt-in graph + 3b tags-only degraded mode protect existing dictation users. Hardware-floor disclosure required in v1 install docs (A3). |
| 2 | Latency budget | **DEFERRED to Phase 1.** Async-queue-with-status-indicator commitment recorded (§8 above); measurement happens on production traffic in Phase 1B / 1C. |
| 3 | Entity extraction high-variance | **RESOLVED POSITIVELY.** Wave 0.5.4 head-to-head: 54.83% / 53.40% Jaccard, 97.08% stability. The variance hypothesis was wrong at 7b mid-confident scale on this corpus. |
| 4 | SCHEMA.md refactor parity gate non-trivial | **RESOLVED POSITIVELY.** Wave 0.5.1 parity-gate-on-OLD-model-first pattern (LESSONS P10 sub-finding) cleanly isolated refactor-regression from model-regression. Pattern carries forward to Phase 1. |
| 5 | Compounding validation requires longitudinal data | **DEFERRED to v1.1.** Acknowledged limitation. Phase 0 / 0.5 are snapshot validation; real-world drift over months can't be measured pre-v1. v1.1 linter + already-shipped `sessions.edit_free_within_5min` signal cover this. |
| 6 | Mission scope cohesion via opt-in graph | **RESOLVED POSITIVELY.** Binding design commitment to opt-in (charter + §6 above). Product identity remains "voice dictation with great cleanup"; graph is a power-user surface that doesn't change entry-level UX. |

Charter also surfaced two operational red flags during the phase:

| # | Operational red flag | Status |
|---|---|---|
| 7 | 3-consecutive-Pareto-reject HALT condition | **VINDICATED.** Wave 0.5.2 halted after both embed methods failed (HALT escalation `mb-hnb4`); A1 amendment authored instead of pushing iterations. Wave 0.5.3 halted similarly after 2-iter rejection cascade; root-caused architecturally rather than via more prompt tweaks. |
| 8 | Wave 0.5.6 boundary halt | **HONORED.** Phase paused at Wave 0.5.5 SEAL; Dustin reviewed all 0.5.1–0.5.5 evidence on disk and authorized this Wave 0.5.6 REPORT + amendments + ADR acceptance. |

---

## 10. What's deferred and why

### v1.1 (data-driven, post-v1-ship)

- **Async quality linter** — background pass that flags drafts the user is
  likely to edit (low-confidence classifications, ambiguous dates).
  Depends on the `sessions.edit_free_within_5min` signal already shipped
  by ADR 0047 PLUS v1 user-correction signal.
- **SCHEMA.md learning loop** — capture user corrections, surface patterns,
  propose schema edits. Requires v1 user-correction UX to exist first.
- **`synthesize` operation** — Clark's "compress N entries on the same
  topic into a digest" pass. Architecturally a 6th pass; gated on graph
  health.
- **Obsidian vault export of the graph itself** — entities + tags + edges
  projected into the vault as their own files. Beyond entries, which
  already project per ADR 0046.
- **Closed-vocab Move 2 take-2** — picks up after the two-field corpus
  re-labeling. The wiring on `main` (commit `8fdc7fb`) is the starting
  point; per LESSONS P11 it applies cleanly to the `tags:` half of the
  two-field schema without the entity-conflation confound.
- **Per-pass specialist routing (Clark's Nemotron pattern)** — SCHEMA.md
  slot exists from Phase 0.5 Move 1; v1 defaults everything to 7b for
  readability. v1.1 starts assigning smaller specialists per-pass based on
  Phase 0.5 + v1 data.

### v1.2+ (platform-gated)

- **Mobile/desktop role separation** (Clark): mobile is a thin ingestion +
  draft-review surface; desktop owns the full pipeline + retrieval.
  Charter when the mobile platform lands beyond ADR 0046's iOS Shortcut
  — i.e. Phase 9's macOS cross-platform sweep or a future native iOS
  charter.

---

## 11. Phase 0.5 methodology notes

A brief acknowledgment of what worked process-wise — recorded so Phase 1
inherits the practices that paid off, not just the architecture decisions.

1. **Pareto-frontier IAP (P9) prevented endless rejection cycles.** Wave 0.5.1
   accepted iter-1 cleanly on a 4-of-4 trust-gate + 3-of-4 quality-metric
   lift — the strict Phase 0 IAP would have rejected the same iter-1 for
   the (microscopic) tag-collapse co-movement. The split discipline let
   architectural wins land.
2. **Halt-and-surface on architectural findings (Waves 0.5.2 / 0.5.3) saved
   iterations.** Wave 0.5.2 halted both embed methods on a single iteration
   each instead of pushing through 5 iterations of "what if we tune the
   k-NN threshold?" Wave 0.5.3 halted after 2 iterations and root-caused
   the residual as a corpus-schema problem instead of a prompt problem.
   Both saved iteration budget that funded Wave 0.5.4's empirical entity
   validation.
3. **LESSONS PINNED promotions maintained signal through context clearing.**
   P9 → P10 → P11 → P12 each promoted to PINNED at the wave they emerged;
   subsequent waves inherited them via the session-start ritual and didn't
   need to re-discover the finding. P10's calibration-profile pattern
   actively enabled P12's clean cross-class measurement.
4. **Parity-gate-on-OLD-model-first (P10 sub-finding) is reusable.** The
   Wave 0.5.1 sequence (3b → SCHEMA refactor → 3b parity → 7b swap →
   diagnose breach) cleanly isolated refactor-regression from
   model-regression. Phase 1A's sandbox→production migration should
   follow the same shape: parity-gate the sandbox pipeline → migrate
   files → re-run parity on the migrated layout → only then start
   editing.
5. **Decoupled gate-vs-probe in cross-class methodology runs (P12
   sub-finding).** Wave 0.5.5 was a single-shot methodology probe, not
   an IAP loop — the smaller-model run was comparison-only, not
   gate-checked. Treating both as gate-checked would have falsely
   "halted" Wave 0.5.5 even though the wave's purpose was the methodology
   question. The decoupling made the finding cleanly reportable.

---

## 12. References

- **Charter:** [ADR 0049](../adr/0049-knowledge-graph-phase-0-5-and-v1-architectural-pivot.md)
  (Proposed → **Accepted** on this report landing). Amendments A1 / A2 / A3 in §"Amendments".
- **Methodology inheritance:** [ADR 0048](../adr/0048-kg-phase-0-validation-methodology.md)
  (Accepted) — §G1–§G7 carve-outs and §3 Q1/Q2/Q3 v1 decisions carry forward unchanged.
- **Phase 0 evidence:** [Phase 0 REPORT.md](REPORT.md) — the assisted-filing
  v1 baseline this pivot reopens, and the fallback v1 surface if Phase 1A
  ever needs to retreat.
- **LESSONS PINNED:** [P9](../LESSONS.md), [P10](../LESSONS.md),
  [P11](../LESSONS.md), [P12](../LESSONS.md) — load-bearing findings.
  Plus [P5](../LESSONS.md) for the seal-without-tag discipline (lateral
  epic per LESSONS PINNED P5; no `phase-*-complete` tag).
- **Spec:** [`docs/knowledge-graph/spec.md`](spec.md) (immutable Wave 0 import).
- **Sandbox:** `experimental/kg-validation/` (standalone crate; sandbox-isolation
  discipline carries forward until Phase 1A opens the production-file window).
- **Key run directories** (gitignored, on disk for audit trail):
  - Wave 0.5.1: `runs/parity-baseline-3b/`, `parity-new-3b/`, `run-7b-baseline/`, `run-7b-stability/`, `iter-1-7b-fix/`, `iter-1-7b-fix-stab/`
  - Wave 0.5.2: `runs/iter-2-embed-classify/`, `iter-2-embed-centroid/`
  - Wave 0.5.3: `runs/run-7b-closed-vocab-seed{42,137}/`, `run-7b-closed-vocab-iter2-seed{42,137}/`, `run-7b-closed-vocab-iter3-seed{42,137}/`
  - Wave 0.5.4: `runs/run-7b-entities-seed42/`, `run-7b-entities-seed137/`
  - Wave 0.5.5: `runs/run-3b-entities-seed42/`, `run-3b-entities-seed137/`
- **Bead epic:** `mb-symi` (closed on this seal); sub-beads `mb-xmgs`,
  `mb-4xtd`, `mb-yfzy`, `mb-hnb4`, `mb-rzpd`, `mb-e10v`, `mb-o4ni`,
  `mb-5r1b`, `mb-qogz` (all closed).

---

**Phase 0.5 SEALED.** ADR 0049 → Accepted. Phase 1A — schema-driven
pipeline graduates to production — awaits Dustin kickoff.
