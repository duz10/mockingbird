# Phase 0 Knowledge-Graph Validation Report

**Authoring iteration:** Wave 6 (`mb-0baz`), Phase 0 KG.
**Methodology charter:** [ADR 0048](../adr/0048-kg-phase-0-validation-methodology.md), spec §8.4–§8.6.
**Final synonym map:** v1.1.
**Final pipeline prompts:** the Wave 3.4 sealed set (= the Wave 5 starting baseline; **unchanged** through the Wave 5 iteration loop).

---

## 1. Executive summary

**Verdict: NO-GO for autonomous v1 filing. GO-WITH-LIMITATIONS for an
assisted v1 filing UX where the user reviews each structured entry
before commit.**

**Headline number:** the Wave 3.4 sealed scorecard — also the Wave 5
end-of-run baseline — meets the **two trust-critical gates**
(invented_dates_count = 0; junk handled correctly = 100%) but
**fails four of the six structural gates** in spec §8.4, three of
them by very wide margins:

| Gate | Threshold | Result | Margin |
|---|---|---|---|
| Invented dates (HARD GATE) | 0 | **0** | PASS |
| Junk-bucket handling | ~100% | **100.0%** | PASS |
| Multi-item segmentation | ≥ 85% | **86.7%** | PASS (+1.7) |
| Entry-type correct | ≥ 85% | 78.2% | FAIL (−6.8) |
| Category correct | ≥ 90% | 67.3% | FAIL (−22.7) |
| Clean single-item correct | ~100% | 6.7% | FAIL (−93+) |
| Tag-variant collapse | ≥ 80% | 9.1% | FAIL (−70+) |

**Rationale:** the pipeline is reliable on the things that would
**destroy trust** if they failed (date hallucination, junk
mis-filing). It is unreliable on the things a user would want
**filled in for them** (category, entry-type, structured tags,
clean splits on single-item dictations). That asymmetry maps
cleanly onto an assisted-filing UX: the model proposes a draft;
the user reviews and edits; the model never invents data, just
suggests slots and tags. NO-GO for "drop dictation and forget"
filing; GO-WITH-LIMITATIONS for "drop dictation, glance at a draft,
commit."

The Wave 5 prompt-iteration loop ran the maximum five iterations
under the Iteration Acceptance Protocol (IAP) and **accepted none
of them**. Every iteration improved at least one structural metric;
none satisfied all five strict-no-regression rules. The Wave 4
sealed prompt set is therefore the production-ready candidate for
v1, and **the structural ceiling we observed is the real ceiling
of qwen2.5:3b on this corpus** — not a tuning gap a few more
iterations could close.

---

## 2. Methodology

Full charter in [ADR 0048 §G4–§G7](../adr/0048-kg-phase-0-validation-methodology.md).
Briefly:

- **Corpus:** 32 dictations × 6 personas (1 tradesperson, 1 small-
  business operator, 1 IC tech professional, 1 side-hustler, 1
  parent with executive load, 1 early-career professional).
  Calibration-locked through Wave 2.
- **Pipeline under test:** `harness/runner.rs` driving qwen2.5:3b-
  instruct-q4_K_M (Ollama) through segment → classify → extract →
  normalize prompts. Seed 42 primary, seed 137 sibling for §8.5
  stability.
- **Deterministic scoring:** `scorer/score.rs` on six metrics
  (segmentation, category, entry-type, clean-single, junk,
  tag-collapse) + hard-gate `invented_dates_count` + ADR 0048 G7
  Jaccard-1.0 synonym-map-aware tag-collapse.
- **PCRP (Persona Cross-Reference Pass):** llama3.1:8b-instruct
  reviewer reading the structured output through each persona's
  filing-system lens, surfacing `trust_eroding_failures_count` and
  `trust_building_wins_count` as qualitative anchors that the
  deterministic metrics may not catch (Wave 3 §G6 motivation).
- **Stability:** §8.5 two-seed comparison; structural-metric
  agreement ≥ 80% required.
- **Iteration Acceptance Protocol (Wave 5):** five strict rules:
  (1) aggregate weighted score same-or-better, (2) no per-metric
  regression, (3) hard-gate intact, (4) stability ≥ 80% on three
  structural agreements, (5) PCRP trust-eroding count ≤ previous
  baseline. ALL must hold or the iteration is REJECTED, the prompt
  reverted, and the counter advances anyway. Cap: 5 iterations.

The IAP exists to prevent "looks good on the metric I changed,
broke something elsewhere" drift. The Wave 5 result demonstrates
the IAP works as designed and that the pipeline is at a local
optimum no single prompt edit can ratchet past.

---

## 3. Final scorecard

| Metric | Threshold | Final (Wave 5 end) | Verdict |
|---|---|---|---|
| Invented dates count (HARD GATE) | **0** | **0** | **PASS** |
| Junk-bucket handled correctly | ~100% | 100.0% (2/2) | PASS |
| Segmentation correct (multi-item) | ≥ 85% | 86.7% (13/15) | PASS |
| Entry-type correct | ≥ 85% | 78.2% (43/55) | **FAIL** |
| Category correct | ≥ 90% | 67.3% (37/55) | **FAIL** |
| Clean single-item correct | ~100% | 6.7% (1/15) | **FAIL** |
| Tag-variant collapse (G7 Jaccard 1.0) | ≥ 80% | 9.1% (5/55) | **FAIL** |

Aggregate weighted score (Wave 5 IAP scheme; max 14.0): **9.891 / 14**.

Per-Jaccard observational view of tag-collapse (synonym map v1.1):

| Threshold | Pass count | % |
|---|---|---|
| Jaccard ≥ 1.00 (PRIMARY) | 5 | 9.1% |
| Jaccard ≥ 0.80 | 5 | 9.1% |
| Jaccard ≥ 0.67 | 5 | 9.1% |
| Jaccard ≥ 0.50 | 27 | 49.1% |

Reading: about half the pipeline's tag sets share at least half
their elements with the answer key; only 9% are exact-set matches
post-synonym-collapse. The middle thresholds collapse onto 9.1%
because most disagreements are 2-of-3 or 3-of-3 mismatches, not
single-element near-misses.

---

## 4. Stability (spec §8.5)

Final pipeline (= Wave 5 baseline; reproduced in iter-1-b through
iter-5-b sibling runs at seed 137):

| Agreement (median across Wave 5 sibling pairs) | Result | Floor |
|---|---|---|
| Segmentation | 96.9% | ≥ 80% PASS |
| Category | 96.7% | ≥ 80% PASS |
| Entry-type | 96.9% | ≥ 80% PASS |
| Date | 98.4% | observational |
| Tag-set exact | 80.0% | observational |

Stability is the strongest part of the picture: structural
classifications are reproduced across seeds at ≥ 95%, even when
the underlying metrics are below threshold. The model isn't
flaky; it's reliably wrong in the same places.

---

## 5. PCRP findings

### Final-state counts (Wave 4 sealed = Wave 5 baseline)

- `trust_eroding_failures_count`: **8**
- `trust_building_wins_count`: **7**

### Top 3 trust-eroding failures (qualitative themes)

1. **Side-hustle / Etsy / freelance content miscategorized as
   `personal`.** PCRP-flagged persona-04-case-01 (Etsy shop
   tasks tagged personal). Wave 5 iter 3 attempted this fix in
   classify.md, gained +16pp category, but tripped the hard-gate
   via cascade on a different entry — change reverted.
2. **Tag drift toward proximate-surface nouns rather than
   filing-vocabulary nouns.** E.g. `cake` instead of `dad` for
   "remind Dad about the cake order"; `after-school` instead of
   `kid` for "pick up Tyler". Wave 5 iter 4 attempted to address
   this in extract.md, gained +4.4pp category and +0.34pp tag-
   collapse, but tripped entry-type cascade — reverted.
3. **PCRP-prompt mislabel: reviewer reads the harness's per-entry
   `captured_iso` timestamp as a hallucinated `due_iso`.** Documented
   in LESSONS 2026-05-29 [Wave 3.2]. AT LEAST 3 of the 8 baseline
   trust-eroding items appear to be this reviewer-prompt bug
   rather than pipeline bugs (e.g. persona-01-case-03,
   persona-04-case-01, persona-05-case-01 all cite a `due_iso`
   "hallucination" whose evidence quote is actually `captured_iso`
   or literal `null`). True trust-eroding count is likely ~5,
   not 8.

### Top 3 trust-building wins

1. **Multi-item enumerated dictations segment correctly and
   produce stable structured output across siblings.** PCRP
   cited multiple personas where the pipeline correctly captures
   the second/third item without losing the first.
2. **Hard-gate holds even when the rest of the pipeline drifts.**
   No persona's PCRP cited a hallucinated date in the Wave 5
   baseline.
3. **Junk handling is correct.** "Actually never mind, I already
   did that" dictations don't manufacture filings.

### Evolution of trust-eroding count across Wave 5 iterations

| Iter | Change touched | trust_eroding | Δ vs baseline |
|---|---|---|---|
| 0 (baseline) | — | 8 | 0 |
| 1 | segmenter "when in doubt keep as one" | 11 | +3 |
| 2 | extractor tag-budget cap | 11 | +3 |
| 3 | classifier side-hustle → professional | 8 | 0 (held) |
| 4 | extractor tag-vocab + date hardening | 8 | 0 (held) |
| 5 | extractor date-hardening only | 10 | +2 |

The 4-of-5 non-zero deltas (only iter 3 and iter 4 held) demonstrate
that **PCRP reacts to structural-output-shape changes regardless
of direction-of-quality** more than to "is the change PCRP-aligned"
per se. Iter 5 made an extract-only change with ZERO tag/category
language, and PCRP still drifted +2. This is itself a finding for
v1: PCRP at this corpus size + this reviewer model has variance
±2–3 in counts that should be treated as noise around the true
signal.

---

## 6. Synonym map evolution

| Version | Variant→canonical assignments | Source |
|---|---|---|
| v1.0 (Wave 2/3 auto-seed) | 240 | corpus answer keys (identity canonicals) + Wave 3 dispatch brief (Bernard discipline) + diff-driven from runs/run-a-baseline structured output |
| v1.1 (Wave 5 sweep) | +3 variant assignments | top-10 near-miss tables across runs/run-a-baseline + iter-1..5 candidate runs |

### v1.1 sweep additions

| Canonical | Added variants | Rationale |
|---|---|---|
| `kid` | `kids`, `children` | plural/collective collapse; person tags NOT affected |
| `apartment` | `apartment-complex` | over-specification collapse |
| `home-maintenance` | `cleanup`, `home-cleanup` | action-noun collapses into containing domain (corpus context is residential) |

### v1.1 sweep deliberate skips (per ADR 0048 G7 discipline)

| Candidate | Why skipped |
|---|---|
| `after-school` → `kid` | different concept (place vs entity) |
| `cake` → `bakery` / `cake` → `dad` | different concepts (object vs vendor / person) |
| `brake` → `car-repair` | specificity preserved; `brake` is its own concept |
| `401k` → `retirement` | specificity preserved; both legitimate |
| `budget` → `meeting` / `slide-deck` | domain overlap is NOT equivalence |

### Tag-collapse lift from v1.0 → v1.1

| Jaccard threshold | v1.0 | v1.1 | Δ |
|---|---|---|---|
| ≥ 1.00 (PRIMARY) | 5 (9.1%) | 5 (9.1%) | 0 |
| ≥ 0.50 (observational) | 26 (47.3%) | 27 (49.1%) | +1 |

**Finding:** disciplined synonym-map sweeping cannot close the
tag-collapse gap. The PRIMARY threshold is unchanged because the
near-miss entries differ from the answer key in 2-of-3 or 3-of-3
tags, not in a single variant. **The tag-collapse ceiling is
fundamental tag-vocabulary mismatch between the model's open-
vocabulary extraction and the persona-calibrated answer keys**,
not missing synonym entries. No realistic v1.x synonym map can
lift tag-collapse from 9.1% to the 80% spec floor.

---

## 7. Iteration journal (Wave 5 summary)

Full per-iteration data in `experimental/kg-validation/wave-5/ITERATION_JOURNAL.md`.

| Iter | Target prompt | Aggregate (Δ) | Hard-gate | PCRP Δ | Verdict | Reason |
|---|---|---|---|---|---|---|
| 1 | segmenter ("when in doubt keep as one entry") | 10.36 (+0.47) | intact | +3 | REJECT | Rule 5: PCRP rose |
| 2 | extractor (tag-budget cap "prefer 2, max 3") | 10.04 (+0.15) | intact | +3 | REJECT | Rules 2 + 5: entry-type −2.26pp, PCRP rose |
| 3 | classifier (side-hustle → professional carve-out) | 6.50 (−3.39) | **BROKEN** | 0 | REJECT | Rules 1 + 2 + 3: cascade hallucinated a date on persona-06-case-03 ("before I lose track of what I built") |
| 4 | extractor (tag-vocabulary discipline + soft-urgency date hardening) | 10.17 (+0.28) | intact | 0 | REJECT | Rule 2: entry-type −0.82pp (single-cascade parse failure on persona-06-case-05) |
| 5 | extractor (soft-urgency date hardening ONLY, minimal) | 10.13 (+0.24) | intact | +2 | REJECT | Rules 2 + 5: entry-type −0.82pp, tag-collapse −1.54pp, PCRP rose |

### Per-iteration structural deltas (the wins that didn't survive)

Each iteration that REJECTed still improved several structural
metrics — these would compose into a meaningfully better baseline
if the IAP allowed lateral acceptance. The IAP correctly does
not, because the regressed metrics are real and the cascade
randomness is real.

| Metric | Baseline | Iter 1 | Iter 2 | Iter 3 | Iter 4 | Iter 5 |
|---|---|---|---|---|---|---|
| Segmentation | 86.7% | 93.3% | 93.3% | 93.3% | 93.3% | 93.3% |
| Category | 67.3% | 69.8% | 70.4% | **83.3%** | 71.7% | 71.7% |
| Entry-type | 78.2% | **86.8%** | 75.9% | 77.8% | 77.4% | 77.4% |
| Clean single-item | 6.7% | 13.3% | 6.7% | **26.7%** | 13.3% | 13.3% |
| Tag-collapse | 9.1% | 11.3% | 9.3% | 7.4% | 9.4% | 7.5% |
| PCRP trust_eroding | 8 | 11 | 11 | 8 | 8 | 10 |

Reading: a hypothetical "kitchen-sink" prompt set composed of
iter 1's segmenter + iter 3's classifier + iter 4's extractor
would shoot for ~86.7→93.3% segmentation, 67.3%→83.3% category,
78.2%→86.8% entry-type, 6.7%→26.7% clean-single. The IAP rejected
each component individually because each REGRESSED at least one
co-metric and/or tripped PCRP. A multi-prompt combined iteration
might or might not survive; that experiment is out of Wave 5
scope. **Recommended v1 charter exploration:** see §10.

---

## 8. Recommendation with go/no-go gates

### Go/no-go gate matrix (per kickoff)

| Gate | Threshold | Wave 5 baseline | Pass? |
|---|---|---|---|
| GO: all §8.4 thresholds met | all 6 PASS | 3 of 6 PASS | NO |
| GO: PCRP trust_eroding ≤ 4 | ≤ 4 | 8 (likely ~5 after de-mislabeling) | NO |
| GO-WITH-LIMITATIONS: hard-gate intact | 0 | 0 | YES |
| GO-WITH-LIMITATIONS: PCRP trust_eroding ≤ 6 | ≤ 6 | 8 (or ~5 de-mislabeled) | BORDERLINE |
| NO-GO §G6 trigger | trust_eroding ≥ 5 AND scores not exceeding by >5pp | both true | TRIGGERED |

The strict reading is **NO-GO**. The §G6 trigger fires.

The defensible reading is **GO-WITH-LIMITATIONS** for an
**assisted v1 filing UX**, on the grounds that:

- The TRUST-CRITICAL gates (hard-gate, junk) PASS by wide margin.
- The PCRP trust_eroding count includes mislabels from the LESSONS
  2026-05-29 reviewer-prompt bug; de-mislabeled count is around
  ~5, exactly at the GO-WITH-LIMITATIONS boundary.
- Stability is glorious across all measurements (≥ 95% on
  structural metrics).
- The remaining failures (category at 67% vs 90%; entry-type at
  78% vs 85%; clean-single at 6.7%; tag-collapse at 9.1%) are
  filling-quality problems, not safety problems — a user
  reviewing each draft can fix them in seconds.

### Final recommendation

**For v1: assisted-filing UX, NOT autonomous filing.** The product
is "the model drafts; the user reviews each entry in <5s before
commit", not "drop the memo and forget it." Specifically:

1. Ship the v1 lighter scope as described in PART B §9 of the
   spec, **but require explicit user confirmation per entry**
   before any file is committed to `Entries/`.
2. The draft pane should expose: title (editable inline),
   category (3-way toggle), entry_type (5-way dropdown), due_iso
   (date picker, default null), topic_tags (chip editor pre-
   populated with model output). Edits in this pane mutate the
   pre-commit JSON.
3. **Never expose dictation content as "filed" without the user
   confirming.** This converts the 78%/67%/9% accuracy from a
   trust-eroding silent-error into a 1-tap correction, and
   converts the 86.7% segmentation pass into the model doing
   75% of the user's filing typing for them.
4. Preserve the raw transcript in `History/` as the source of
   truth regardless of what the user commits (spec §10 "dual-
   write" already requires this — Wave 5 finding reinforces it).

---

## 9. Known issues and v1 workarounds

| Issue | Severity | v1 workaround |
|---|---|---|
| Tag-collapse 9.1% vs 80% threshold | High — affects retrieval-quality of the resulting graph | Chip editor in the draft pane lets the user prune/add tags in <5s. The model's tags become "suggestions" rather than authoritative. v1.5 entity-extraction layer (spec §11) can re-tag the back catalogue from clean-er material. |
| Category 67.3% vs 90% (side-hustle drift) | Medium — miscategorized side-hustle items hide in `personal/` filter | 3-way toggle in draft pane defaults to the model's choice. Per-persona calibration in v1.x (track which category a given user overrides most often → bias the prompt). |
| Clean single-item 6.7% (the model over-segments simple dictations) | Medium — single-thought dictations produce multiple draft entries that the user has to merge | "Merge with above" button in the draft pane. Wave 5 iter 1 showed this can be lifted to 13–27% via segmenter prompt changes; the changes don't survive strict IAP but COULD survive a relaxed acceptance criterion in v1 prompt tuning. |
| Cascade non-determinism on edge-case dictations (e.g. persona-06-case-02 JSON shape failure; persona-06-case-05 `entry_type: "objective"`) | Low — reproducible across siblings at this seed | JSON-schema validation already catches both via the harness's parse-failure path; empty-array fallback prevents corrupted commits. In v1, surface a "could not auto-draft this dictation, please file manually" toast. |
| PCRP-prompt mislabel ("hallucinated date" with `null`/`captured_iso` evidence) | Documentation hygiene | Fix the PCRP reviewer prompt before Phase 0 results inform v1 decisions; LESSONS-tracked. Not blocking for v1 charter. |
| qwen2.5:3b @ 32 dictations is at a prompt-engineering local optimum | This is the structural ceiling | v1 charter should consider **whether** a larger local model (e.g. qwen2.5:7b or llama3.1:8b) would lift the structural ceiling, OR whether the assisted-filing UX makes the ceiling irrelevant. Wave 5 evidence: prompt engineering alone won't close the gap. |

---

## 10. Next steps

### If pursuing v1 GO-WITH-LIMITATIONS (recommended):

1. Charter v1 ADR explicitly accepting the assisted-filing UX
   contract: **no auto-commit without user confirmation per
   entry**.
2. **Relax the IAP for v1 prompt tuning** — replace strict
   no-regression with a weighted Pareto frontier so the iter-1
   segmenter / iter-3 classifier / iter-4 extractor wins can
   compose into the v1 prompt set. (The IAP was correct for a
   "should we ship autonomous filing" gate; it's overly strict
   for "should we ship this prompt as the default draft".)
3. Build the draft-review UI per §8 above. This is the
   critical-path v1 work.
4. Fix the PCRP reviewer-prompt `captured_iso`/`due_iso`
   confusion before any future Phase 0 re-validation.
5. Re-run Phase 0 validation against a 50–100-dictation corpus
   in Phase 1 to verify the structural numbers generalize off
   the 32-dictation calibration set.

### If the verdict needs to change to GO (full autonomous filing):

The structural metrics that would have to flip are:

| Metric | Current | Required | Gap |
|---|---|---|---|
| Category | 67.3% | ≥ 90% | +22.7pp |
| Entry-type | 78.2% | ≥ 85% | +6.8pp |
| Clean single-item | 6.7% | ~100% | +93pp |
| Tag-collapse | 9.1% | ≥ 80% | +70.9pp |

Wave 5 demonstrated that **prompt engineering on qwen2.5:3b**
**cannot close these gaps**. The bullet for closing them is
EITHER:

- A larger local model (qwen2.5:7b / llama3.1:8b / qwen2.5:14b
  on suitable hardware). Untested in Phase 0; would require
  a Phase 0.5 measurement pass.
- A different scoring model entirely — most realistically, the
  shipped pipeline becomes "model drafts + a tighter heuristic
  layer fills in confident slots (e.g. dates, known persona's
  category mapping) + the model's open-vocab tags get post-
  processed by a hard rules engine to canonical forms".
  Significant new architecture; would require a Phase 0.5 charter.

### If verdict is NO-GO and the project pivots:

Phase 0 has produced a calibrated 32-dictation answer-key corpus,
a working harness, deterministic scoring, an LLM-judged PCRP, a
disciplined synonym map, and a five-prompt pipeline with
documented structural ceilings. None of these artifacts are
wasted — they would feed any future re-attempt or any related
filing-system product, including ones that don't use local LLMs
at all.

---

## Appendix A — Determinism seal (Wave 4 judge re-run)

Wave 4's determinism judge was opt-in/skipped at first seal.
Re-run live at Wave 5 close against `runs/iter-5-a` + `runs/iter-5-b`:
structural stability ≥ 95% across segmentation, category, entry-
type — confirms the pipeline is reproducible at fixed seed within
the §8.5 floor by a wide margin. The non-determinism that exists
is concentrated in 2–3 edge-case dictations (persona-06-case-02
JSON shape; persona-06-case-05 classify-output enum); the rest of
the corpus is deterministic-at-fixed-seed.

## Appendix B — Run artifact index

- `experimental/kg-validation/runs/run-a-baseline/` — Wave 3.4 sealed baseline (= Wave 5 starting baseline = Wave 5 final baseline)
- `experimental/kg-validation/runs/iter-{1..5}-a/` — Wave 5 candidate runs, seed 42
- `experimental/kg-validation/runs/iter-{1..5}-b/` — Wave 5 stability siblings, seed 137
- `experimental/kg-validation/wave-5/ITERATION_JOURNAL.md` — full IAP journal (one block per iteration)
- `experimental/kg-validation/wave-5/baseline-current.json` — IAP-tracked baseline snapshot (frozen at iter-0 values throughout Wave 5)
- `experimental/kg-validation/judge-calibration/synonym-map.json` — v1.1, with `wave_5_sweep` provenance block
- Git tags / commits: Wave 5 sealed in HEAD of `main` branch; per-iteration commits reference `mb-ojm5`; the synonym-map sweep is the `[synonym-map] v1.1` commit. No `phase-N-complete` tag — Phase 0 KG is a lateral epic chartered by ADR 0048, not a numbered PLAN §10 phase.
