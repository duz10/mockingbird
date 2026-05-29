# ADR-0049: Knowledge Graph — Phase 0.5 architectural pivot + v1 charter

- **Status:** Proposed (Accepted upon Wave 0.5.6 REPORT landing + Dustin sign-off)
- **Date:** 2026-05-29
- **Deciders:** Dustin (project lead), Bernard / code-puppy (chartering + implementor)
- **Charter for:** ADR-lateral epic — Knowledge Graph Phase 0.5 (architectural
  pivot) + Phase 1 (v1 production wiring). Sealed via ADR Accepted + STATUS
  update + a Wave 0.5.6 REPORT landing. **No new `phase-*-complete` tag**
  (lateral epic per LESSONS PINNED P5).
- **Supersedes for scope:** ADR 0048's "v1 charter, drafted post-gate"
  placeholder. ADR 0048 itself (the Phase 0 methodology charter) remains
  Accepted and intact — its §G1–§G7 carve-outs and the §3 Q1/Q2/Q3 v1
  decisions (vault subtree, positional routing, files-as-source-of-truth)
  carry forward unchanged.
- **Source spec:** [`docs/knowledge-graph/spec.md`](../knowledge-graph/spec.md)
  (immutable Wave 0 import).
- **Phase 0 evidence:** [`docs/knowledge-graph/REPORT.md`](../knowledge-graph/REPORT.md)
  (final scorecard + go/no-go).
- **External inspirations:**
  - **Asthana (2025)** — *"How I built a personal wiki with Python + an LLM"*
    — hybrid deterministic-glue + LLM pattern; lesson taken: separate the
    deterministic from the probabilistic.
  - **Clark (2025, NVIDIA Sr. AI Engineer)** — *"Schema-driven LLM wiki"* —
    portable Markdown schema as the contract; per-pass specialist routing;
    mobile/desktop role separation. Lesson taken: the schema **is** the
    pipeline contract; Rust modules are contract-consumers.

---

## Context

ADR 0048 (Phase 0 KG validation) Accepted 2026-05-29. The headline:
qwen2.5:3b on the strict-no-regression IAP **hit a structural ceiling**
no prompt edit could ratchet past. Final scorecard: trust gates PASS
(invented_dates=0, junk=100%, segmentation=86.7%); quality gates FAIL
(category=67.3%, entry-type=78.2%, clean-single=6.7%, tag-collapse=9.1%).
Mission as originally specced — "fully organized and connected knowledge
graph from raw user dictation alone" — is **not deliverable on the Phase 0
architecture**.

Two architectural patterns surfaced in parallel — Asthana's hybrid
glue-vs-LLM split and Clark's schema-driven contract — suggest the
Phase 0 architecture itself is the cap, not the model. Phase 0.5 tests
that hypothesis empirically before we commit Phase 1 production wiring.

LESSONS PINNED P9 (promoted from Wave 5 finding) reframes how the IAP
must apply to quality metrics on small local models: **strict
no-regression cannot ratchet on joint-distribution shifts**; quality
metrics need Pareto-frontier acceptance. Phase 0.5 is the first epic to
operate under that revised IAP discipline.

---

## Mission framing

The product mission — Bernard's exact wording from the Phase 0 review —
is **"a fully organized and connected knowledge graph built from raw
user dictation alone."** Phase 0 measured filing quality on a flat
entry schema; Phase 0.5 raises the bar to a *graph*. Connection
(entities, canonical tags as nodes, edges between entries) is the
v1 deliverable, not a v2 stretch.

This reframing is what motivates the architectural pivot. Tag-collapse
9.1% is not "tag quality is mediocre" — it is **the connectivity layer
of the graph is broken**. Category 67.3% is not "filing precision is
poor" — it is **the coarse-grained navigation axis users would lean on
hardest is wrong a third of the time**. The fixes are not "tune the
prompts harder"; the fixes are structural.

---

## Decision

We charter **Phase 0.5 — an architectural pivot epic** that tests
four interventions on the Phase 0 sandbox, then commits Phase 1
(v1 production wiring) conditional on the Phase 0.5 evidence.

The four interventions, **priority-ordered** (each enables the next):

### Move 1 — `SCHEMA.md` as portable contract (structural; Clark)

Replace the current `include_str!`-baked prompts + hardcoded taxonomy
enums + scattered normalization rules with a **single portable
Markdown contract** at `experimental/kg-validation/SCHEMA.md`.

The schema encodes:

- Pass definitions (segment, classify, extract, normalize) with
  input/output contracts.
- Prompt templates per LLM-driven pass.
- Normalization rules (the deterministic tag-canonicalization logic
  that currently lives in `passes/normalize.rs`).
- Taxonomy: categories, entry types, canonical tag vocabulary
  (initially open; closed by Move 3).
- Per-pass model selection (default 7b in Phase 0.5; per-pass
  specialists deferred to v1.1+).
- Schema version field — the Rust loader asserts a known version on
  startup.
- **Reserved structure for user-preference rules** (v1.1 surface;
  Phase 0.5 leaves it empty but defines the slot).

The pipeline reads from SCHEMA.md at runtime. Rust modules become
**contract-consumers**, not contract-holders. This is Clark's
architectural shift.

**Why this is structural, not cosmetic:** every subsequent move
(embeddings classifier, closed tag vocab, entity extraction) needs a
single source of truth for the taxonomy. Without SCHEMA.md, each move
forks the truth across three or four files and they drift. With
SCHEMA.md, the truth has one home and every consumer reads from it.

### Move 2 — Embeddings classifier replaces LLM classify (closes Gaps 2+3)

Phase 0 category 67.3% + entry-type 78.2% are the two largest
structural gaps. Hypothesis: small local LLMs are poor at consistently
applying a coarse-grained finite-set label, but a **local embeddings
model** comparing the segment to exemplars of each category /
entry-type label can hit it reliably.

**Default model:** `nomic-embed-text` (768-dim, fully local, ~270 MB,
zero GPU contention with the LLM pass).

**Bootstrap exemplars:** the 32-pair corpus answer keys are
already-labeled ground truth. Each category gets exemplars built from
all answer-key segments labeled with that category; same for
entry-type. New segment → cosine similarity to each label's centroid
(or to its nearest exemplar) → assign the winning label.

**Acceptance:** Pareto-frontier IAP head-to-head vs the 7b LLM
classify baseline (Move 1's first run). Accept if category **OR**
entry-type lifts meaningfully (≥10 pts) AND the other doesn't regress
beyond a per-metric tolerance band (defined in Wave 0.5.2 brief).

**Why this closes Gaps 2+3 specifically:** embeddings are
purpose-built for "how similar is this text to these labeled
exemplars"; small LLMs are not. The Phase 0 ceiling on category +
entry-type is consistent with an architectural mismatch, not a tuning
gap.

### Move 3 — Closed canonical tag vocabulary + new-tag-request flow (closes Gap 4)

Phase 0 tag-collapse 9.1% means the freeform raw-topic-tags
extraction surface produces wildly varying tags for semantically
identical concepts (`car-repair` / `auto-repair` / `vehicle-fix` etc.).
The synonym map v1.1 + deterministic normalization in
`passes/normalize.rs` lifted this only marginally because **the
underlying tag space is unbounded**.

**Closed vocabulary, seeded from corpus + Bernard's synonym-map v1.1**
(`experimental/kg-validation/judge-calibration/synonym-map.json`).
The extract pass produces tags **only** from the closed vocabulary.

**New-tag-request flow:** when the model genuinely encounters a
concept the vocabulary does not cover, it emits a `proposed_new_tag`
field alongside the closed-vocab tags it did pick. A post-LLM
validator filters obvious junk (1-char, profanity, dup of existing).
Surviving requests batch into a review queue (v1.1 UX; Phase 0.5
just measures the request rate + accept rate).

**Why this closes Gap 4 specifically:** tag-collapse is a finite-set
problem. An unbounded extractor cannot solve a finite-set problem
reliably. A bounded extractor + a controlled growth path can.

### Move 4 — Entity extraction probe (mission-enabling; conditional v1 inclusion)

The "connected" half of "fully organized and connected" demands
entity extraction. Phase 0.5 runs an **entity-extraction probe** as a
research wave (Wave 0.5.4) before committing entities to v1.

**Architecture under test:**

- LLM-driven extraction with structured output (named persons,
  organizations, projects, places).
- Entity disambiguation via lexical similarity (Levenshtein) +
  embedding similarity (nomic-embed-text on entity surface forms).
- Entity canonicalization keyed to the same SCHEMA.md taxonomy slot
  that Move 3 builds.

**New metric — entity-quality:** Bernard hand-labels ~10–15 corpus
dictations with ground-truth entities (Wave 0.5.4 sub-task) so we can
score precision + recall.

**v1 inclusion gate:**

- entity-quality ≥ 50% → entity layer in v1 (conditional on Wave 0.5.6
  final review).
- entity-quality < 50% → flag v1 as **tags-only** (no entity nodes;
  re-attempt v1.1 with per-pass specialist routing per Clark).

**Why this is a probe, not a commitment:** entity extraction on
small local models is the highest-variance unknown in the four moves.
It might just work; it might be unsalvageable until we route a
specialist model. Better to find out on a probe than to spec it into
v1 and discover the cap mid-build.

---

## Model strategy

**Default model for Phase 0.5 (all waves):** `qwen2.5:7b-instruct-q4_K_M`.

Rationale:

- Phase 0 demonstrated 3b is at-or-below the structural ceiling for
  category / entry-type / clean-single / tag-collapse, *even on the
  current architecture*. Pivoting the architecture without raising
  the model would confound the experiment.
- Hardware floor commitment (red flag accepted): users without GPU
  headroom for 7b-q4 (~5 GB VRAM working set, ~4.7 GB on disk) will
  not get the v1 graph layer. Mitigated by **Move-1 SCHEMA.md per-pass
  model selection** + the **opt-in graph layer** commitment below —
  dictation users are unaffected.

**Cross-test probes (Wave 0.5.5 only):**

- `qwen2.5:3b-instruct-q4_K_M` (architecture-vs-scale isolation).
- `qwen2.5:14b-instruct-q4_K_M` (headroom probe; pull at wave entry
  if not on disk).
- `gemma2:9b` (cross-family schema portability probe; already on disk
  from Wave 3.3).

The cross-test isolates **how much of the lift comes from architecture
vs scale**. If 3b on the pivoted architecture closes most of the gap,
the v1 hardware floor relaxes. If 14b lifts negligibly above 7b on the
pivoted architecture, we have headroom evidence the v1.1 specialist
routing won't help further on this corpus.

**Per-pass specialist routing (Clark "Nemotron pattern"):** deferred
to v1.1+. The Phase 0.5 contract is that SCHEMA.md *supports* per-pass
model selection (the slot exists); we just default everything to 7b
for Phase 0.5 to keep the experiment readable.

---

## Phase 0.5 wave structure

| Wave | Title | Sub-bead | Depends on |
|---|---|---|---|
| 0.5.0 | This ADR + epic + sub-bead structure | (this iteration) | ADR 0048 Accepted |
| 0.5.1 | SCHEMA.md refactor + 7b baseline + parity gate | (sub-bead 1) | 0.5.0 |
| 0.5.2 | Embeddings classifier (nomic-embed-text) | (sub-bead 2) | 0.5.1 |
| 0.5.3 | Closed canonical tag vocabulary + new-tag flow | (sub-bead 3) | 0.5.1 |
| 0.5.4 | Entity extraction probe + new metric | (sub-bead 4) | 0.5.1, 0.5.3 |
| 0.5.5 | qwen2.5:3b cross-test on pivoted architecture | (sub-bead 5) | 0.5.2, 0.5.3, 0.5.4 |
| 0.5.6 | REPORT.md + Phase 1 GO/NO-GO | (sub-bead 6) | 0.5.5; **Dustin sign-off gate** |

Each wave seals via:

1. Sandbox gate green (vanilla `cargo fmt/clippy/test` from the
   sandbox — sandbox has no CUDA / whisper-rs / ort deps; LESSONS P2
   sidestepped).
2. STATUS.md in-flight block updated.
3. Wave-boundary commit with the sub-bead ID + this ADR id.
4. (Where relevant) updated REPORT-like scorecard artifact under
   `docs/knowledge-graph/` or `experimental/kg-validation/runs/`.

---

## Iteration Acceptance Protocol (Phase 0.5)

Per LESSONS PINNED P9, the IAP splits by metric cost-of-regression:

### Strict no-regression (trust-critical; ANY regression rejects)

- `invented_dates_count` must remain **0**. Hard gate.
- Junk-bucket handling (is_junk fixtures yielding `entries = []`)
  must remain **100%**.
- Sandbox isolation (no production file under `src-tauri/` or `ui/`
  modified). Hook-enforced.

### Pareto-frontier (quality metrics; cross-metric trade-offs allowed)

For each of: category, entry-type, segmentation, clean-single,
tag-collapse, PCRP trust_eroding_failures_count.

Accept an iteration if:

1. **No metric is meaningfully worse** than the wave's entry baseline,
   where "meaningfully worse" is defined per-metric in the wave brief
   (e.g. ≤ 5 pts tolerance band for percentage metrics, ≤ +2 absolute
   for PCRP trust_eroding count).
2. **Aggregate weighted score** (same weights as ADR 0048 §8.4)
   improves OR holds.
3. **Stability ≥ 80%** on three structural-metric agreements
   between seed 42 and seed 137 (unchanged from ADR 0048 §8.5).

This explicitly allows e.g. a Move-2 iteration that lifts category
+15 pts while regressing tag-collapse −3 pts, because tag-collapse
will be fixed by Move 3 in the next wave; the trade is acceptable
*at the epic level*.

### Per-wave entry baselines

Each wave records its entry baseline at wave start (the previous wave's
exit scorecard). Within-wave IAP measures against that baseline, not
against the Phase 0 baseline. This is what makes the Pareto-frontier
discipline coherent across waves: each wave only has to not-regress
against what the previous wave shipped.

---

## Success criteria for Phase 0.5 GO

At Wave 0.5.6, **at least 3 of**:

- category lift ≥ 10 pts from Phase 0 baseline (67.3% → ≥77.3%)
- entry-type lift ≥ 10 pts from Phase 0 baseline (78.2% → ≥88.2%)
- tag-collapse lift ≥ 10 pts from Phase 0 baseline (9.1% → ≥19.1%)
- clean-single lift ≥ 10 pts from Phase 0 baseline (6.7% → ≥16.7%)

AND **all of**:

- Hard gate intact: `invented_dates_count = 0`.
- PCRP trust_eroding_failures_count ≤ Phase 0 baseline (≤ 8).
- Stability ≥ 80% on seed-42 vs seed-137 structural-metric agreements
  (preserves ADR 0048 §8.5).

Below this bar → **Phase 0.5 NO-GO**; we ship Phase 0's
assisted-filing UX (REPORT.md §8) and revisit the architectural pivot
post-v1 with longitudinal user data.

---

## Phase 1 commitments (binding once Phase 0.5 GOs)

These are recorded **now**, before Phase 0.5 starts, so the Wave 0.5.6
GO/NO-GO decision has a known target rather than re-litigating scope:

- **Schema-driven pipeline graduates from sandbox to production.**
  The SCHEMA.md contract becomes the production source of truth for
  pass prompts + taxonomy. The current sandbox loader becomes the
  production loader (with the sandbox-vs-production reuse-by-reference
  discipline of ADR 0048 §5.2 carried forward).
- **SQLite schema extensions** in a single new migration: `entities`
  table, `canonical_tags` table, `edges` table (entry-to-entity,
  entry-to-tag, entity-to-entity co-occurrence). Migration number
  assigned at Phase 1 wave 1 entry.
- **Concept pages as computed views**, not stored entities. A "concept
  page" for `marketing-lead` is a SQL view over entries + edges; not a
  separate file or row. Files-as-source-of-truth (ADR 0048 §3 Q3)
  preserved: the Markdown files are the entries; the DB indexes
  relationships derived from them.
- **Retrieval UX:** chronological + entity + tag + category + free-text
  search + date range. Six retrieval axes at v1.
- **Migration backfill** for existing entries: pre-Phase 1 entries
  get classified + tagged + entity-extracted in a one-shot backfill
  job. User-visible as a one-time progress overlay (reuse ADR 0046
  Wave 4 import-progress pattern).
- **Graph layer is OPT-IN.** **Binding design commitment.** Existing
  dictation users are unaffected; intake UX is unchanged. The graph
  layer activates only when the user explicitly enables it in
  Settings → Knowledge Graph. Default: off. Mission-scope cohesion is
  preserved because the graph layer is additive, not replacement, and
  the dictation experience is the foundation.
- **Live-dictation latency target:** entries appear in the graph
  within ~1 min of dictation completing. An async queue with a
  visible status indicator ("Filing... 23s remaining") is acceptable;
  blocking the dictation post-paste is not.

---

## v1.1 deferrals (post-ship, empirically driven)

Recorded so the v1 charter (this ADR) doesn't pre-emptively spec
features that should be data-driven:

- **Async quality linter** — background pass that flags drafts the
  user is likely to edit (low-confidence classifications, ambiguous
  dates). Implementation depends on the `sessions.edit_free_within_5min`
  signal already shipped by ADR 0047 — we want the v1 longitudinal
  data first.
- **SCHEMA.md learning loop** — capture user corrections, surface
  patterns, propose schema edits. Requires v1 user-correction UX to
  exist first.
- **`synthesize` operation** — Clark's "compress N entries on the same
  topic into a digest" pass. Architecturally a 5th pass; gated on the
  graph layer being healthy.
- **Per-pass specialist routing** — SCHEMA.md slot exists from Phase
  0.5 Move 1; defaults to all-7b through v1; v1.1 starts assigning
  smaller specialists per-pass based on Phase 0.5 + v1 data.
- **Obsidian vault export of the graph itself** — entities + tags +
  edges projected into the vault as their own files. Beyond entries,
  which already project per ADR 0046.

## v1.2+ deferrals

- **Mobile/desktop role separation** (Clark): mobile is a thin
  ingestion + draft-review surface; desktop owns the full pipeline
  + retrieval. Charter when the mobile platform lands beyond
  ADR 0046's iOS Shortcut.

---

## Red flags acknowledged

These are surfaced for the record; each is mitigated by an explicit
provision above.

1. **Hardware floor — 7b default.** Mitigated: opt-in graph layer;
   dictation experience unchanged; SCHEMA.md per-pass model selection
   lets a future v1.1 substitute smaller specialists.
2. **Latency budget.** Async queue with status indicator commitment
   above. Hard target: graph backlog drains within ~1 min of
   dictation.
3. **Entity extraction is high variance.** Probe-then-decide design
   in Move 4 (tags-only fallback if entity-quality < 50%).
4. **SCHEMA.md refactor parity gate is non-trivial.** Wave 0.5.1
   Step 4 demands byte-identical output on the same seed; if the
   refactor accidentally perturbs prompt formatting, this catches
   it. Halt + iterate.
5. **Compounding validation requires longitudinal data.**
   Acknowledged. The Phase 0 corpus is a snapshot; real-world drift
   over months of use can't be measured here. Mitigated by the v1.1
   linter + the user-correction signal already shipped.
6. **Mission scope cohesion via opt-in graph.** The product
   identity stays "voice dictation with great cleanup"; the graph is
   a power-user surface that doesn't change the entry-level UX.
7. **3-consecutive-Pareto-reject HALT condition.** If a wave's IAP
   rejects three iterations in a row, the architectural assumption
   under test is suspect — halt + surface, don't push to the
   per-wave iteration cap.
8. **Wave 0.5.6 boundary halt.** Mandatory. Dustin reviews 0.5.1–0.5.5
   evidence on disk before authorizing v1 charter finalization.

---

## Sandbox isolation discipline (carried forward from ADR 0048)

ADR 0048 §5.1–5.4 sandbox discipline applies unchanged. Phase 0.5
work lives entirely under `experimental/kg-validation/`. New files
permitted; production files (`src-tauri/**`, `ui/**`, `migrations/**`)
**read-only** until the Phase 1 charter (a successor wave inside this
ADR) explicitly opens them. The `block-production-edits` discipline
is the same one ADR 0048 used.

The single exception window: **Phase 1 (production wiring)** will
necessarily edit production files. That window opens only after this
ADR moves Accepted, the Phase 0.5 GO/NO-GO landed, and a Phase 1 wave
brief is authored. Until then, all Phase 0.5 work is sandbox-only.

---

## Beads inheritance from ADR 0048

The following ADR 0048 sub-beads CLOSED with the Phase 0 seal — they
are the foundation Phase 0.5 builds on:

`mb-4wxw`, `mb-w1lw`, `mb-i9l1`, `mb-t7w5`, `mb-901u`, `mb-i4us`,
`mb-nbel`, `mb-57a1`, `mb-jz5r`, `mb-he98`, `mb-ojm5`, `mb-0baz`.

Phase 0.5 mints its own epic + sub-beads (one per wave 0.5.1–0.5.6),
each referencing this ADR id in its description.

---

## Acceptance criteria for this ADR

This ADR moves to **Accepted** when:

1. Wave 0.5.6 REPORT.md lands at `docs/knowledge-graph/REPORT-phase-0-5.md`
   with a defensible GO or NO-GO recommendation.
2. Dustin reviews the wave-by-wave evidence on disk and signs off on
   the recommendation.
3. STATUS.md is updated to reflect the seal (move this ADR from "in
   flight" to "sealed lateral epics").

NO-GO is an acceptable terminal state — the ADR still moves Accepted
because the architectural pivot was honestly attempted and measured.
The Phase 0 assisted-filing UX (REPORT.md §8) becomes the v1 surface
in that case.

---

## Amendments

### A1 (Wave 0.5.2 empirical update) — Move 1 mechanism revised

**Original Move 1 (as proposed):** Local embeddings (`nomic-embed-text`)
with nearest-neighbor classification over a labeled exemplar pool
replaces LLM category/type reasoning. Hypothesis: deterministic,
cheap, continuously learning from new exemplars.

**Empirical finding (Wave 0.5.2, `runs/iter-2-embed-classify/` (nearest)
vs `runs/iter-2-embed-centroid/` (centroid) vs `runs/iter-1-7b-fix/`
(7b LLM + SCHEMA.md baseline) on the 32-pair corpus, seed 42):**

| Metric | 7b LLM + SCHEMA.md | embed-NN | embed-centroid |
|---|---|---|---|
| Category | 81.5% | 70.4% (-11.1) | 66.7% (-14.8) |
| Entry-type | 88.9% | 68.5% (-20.4) | 75.9% (-13.0) |
| Clean single-item | 33.3% | 13.3% (-20.0) | 13.3% (-20.0) |

Both prototype methods named in the original Move 1 design regressed
materially. The dual-method failure pattern points at architecture, not
hyperparameter — at this corpus scale (46 exemplars across ~25
category×type buckets ≈ 2 exemplars/bucket), pretrained general-purpose
embeddings under-resolve the semantic distinctions the 7b LLM with
calibrated SCHEMA.md handles cleanly.

**Revised Move 1:**

- The Move 1 *outcome* (close Gaps 2 + 3, lift category and entry-type
  above their bars) is achieved by **SCHEMA.md + 7b LLM classification**
  (Wave 0.5.1 delivered +14.2 / +10.7 / +26.6 on category / type /
  clean-single respectively from Phase 0 baseline; both seeds clean on
  the hard gate).
- The embeddings infrastructure built in Wave 0.5.2
  (`experimental/kg-validation/src/embeddings.rs`,
  `experimental/kg-validation/src/exemplars.rs`,
  `experimental/kg-validation/src/bin/embed-reclassify.rs`) is
  **preserved for entity disambiguation in Move 4** (Wave 0.5.4),
  where similarity over a small number of candidate aliases is the
  actual problem shape embeddings solve well.
- Speculative embedding-based classification for v1 is dropped. Future
  reconsideration only if: (a) corpus ≥ 500 entries/user routinely AND
  (b) per-class exemplar count ≥ 100 AND (c) measured LLM-only baseline
  shows ceiling on category/type metrics.

**Architectural meta-finding (v1 commitment, not just a Phase 0.5 tactic):**
at 7b scale with a calibrated SCHEMA.md (LESSONS P10), the LLM is itself
a sufficient structural component for classification at this corpus
size. The "deterministic code layer over LLM" pattern (Asthana) wins at
small-model scale where the LLM ceiling is low; the "schema-driven LLM
as the structural piece" pattern (Clark) wins at mid-model scale where
the LLM ceiling is high enough that additional deterministic layers add
complexity without quality gain. v1 commits to the latter for
classification; entity disambiguation in Move 4 keeps the embeddings
layer as a *similarity* tool (its natural problem shape), not a
classification tool.

**Impact on later waves:**

- Wave 0.5.3 (`mb-rzpd`) — unchanged. Closed canonical tag vocabulary +
  new-tag-request flow is mechanism-independent of classifier choice;
  proceeds as scheduled.
- Wave 0.5.4 (`mb-o4ni`) — embeddings reused for entity disambiguation
  (lexical-candidate-set → embedding similarity tie-break), not for
  category/type classification.
- Wave 0.5.5 (`mb-5r1b`) — 3b cross-test runs on the pivoted architecture
  (SCHEMA.md `small-conservative` profile + closed vocab + entity probe),
  NOT on an embeddings-classify variant.
- Wave 0.5.6 (`mb-qogz`) — REPORT.md includes A1 as a first-class
  finding ("Move 2 falsified at 32-pair scale"), not a footnote.

Bead closures referenced by this amendment: `mb-hnb4` (escalation),
`mb-yfzy` (Wave 0.5.2 task).
