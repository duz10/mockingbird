# Wave 3 — final results (JVP HALT on Gate 3)

**Status:** PARTIAL → **HALTED**. Pipeline runs sealed, structural metrics
sealed, stability sealed, JVP attempted (Gate 3 STOP), PCRP attempted on
run-a. **Tag metric and JVP verdict marked INVALID per ADR 0048 §G5.**
Wave 4 remains **blocked** pending judge revalidation per Wave-3 dispatch
standing-rule #6.

- Bead: `mb-57a1` (LEFT OPEN — judge validation failure, not implementer scope)
- ADR: 0048 (G5 + G6 amendments shipped earlier this wave: `be5fc79`)
- Calibration v2 fix: `7f8ff1c` (see `docs/knowledge-graph/wave-3-results.md`
  section "Calibration v2 fix" below)
- Pipeline model: `qwen2.5:3b-instruct-q4_K_M`
- Primary judge model: `llama3.1:8b-instruct-q4_K_M` (pulled 2026-05-29)
- Cross-judge model: `gemma2:9b` (pulled 2026-05-29)
- Persona-reviewer model: `llama3.1:8b-instruct-q4_K_M` (different family
  from pipeline per §G4)

## TL;DR

| Layer | Outcome | Notes |
|---|---|---|
| Pipeline runs (both seeds) | ✅ 32/32 each, 0 parse errors | unchanged from partial |
| Structural metrics (run-a) | 4 ✅ / 3 ❌ | hard gate holds (0 invented dates) |
| Stability (run-a vs run-b) | ✅ 100% date / 96–98% categorical | unchanged from partial |
| Tag-variant collapse metric | 81.8% (45/55) — **INVALID** | judge halted Gate 3 |
| JVP overall | **❌ HALT** | Gate 3 STOP (cross-judge 57.1% < 85%) |
| PCRP | 8 trust-eroding / 9 trust-building | informational; default NO-GO per §G6 |
| Wave 4 (invariant judges) | 🛑 **NOT STARTED** | dispatch contract pre-condition unmet |

## Runs

| Run | Seed | Pipeline duration | Dictations | Success | Errors |
|---|---|---|---|---|---|
| run-a-baseline | 42 | 172 s | 32 | 32 | 0 |
| run-b-stability | 137 | 168 s | 32 | 32 | 0 |

Zero per-pass parse errors on either run. Pipeline is stable under load.

## Calibration v2 fix (Gate 1 fairness)

The Wave-3.1 self-review (prior dispatch) flagged that the original
`cal-eq-001` pair — `["car-repair", "auto"]` vs
`["car-repair", "auto-maintenance"]` — was **lexically identical** to
the judge prompt's first in-context example. Including it in the
calibration set would inflate Gate 1's verdict-correct on memorization
rather than reasoning.

Fix (commit `7f8ff1c`): replaced cal-eq-001 with
`["birthday", "gift"]` vs `["birthday", "birthday-gift"]` — same
anchored-synonym pattern, fresh vocabulary disjoint from every prompt
example and every other calibration pair. Bumped
`calibration_set_id` v1 → v2. Loader round-trip test updated. 81/81
sandbox tests still green.

Gate 1 with v2: **11/12 = 91.7% PASS** (judge correctly classified the
fresh `birthday`/`gift`/`birthday-gift` pair as Equivalent). The single
miss was `cal-eq-004` (`["doctor-appointment"]` vs `["doctor", "appointment"]`)
— a borderline compound-vs-decomposition call the judge marked
NotEquivalent. The Gate 1 pass with v2 is real reasoning, not the
inflated v1 memorization signal.

## SCORE_SUMMARY — run-a-baseline (seed 42)

| Metric | Result | Threshold | Verdict |
|---|---|---|---|
| Clean single-item correct | 6.7% (1/15) | ~100% | ❌ FAIL |
| Segmentation correct (multi-item) | 86.7% (13/15) | ≥ 85% | ✅ PASS |
| Category correct | 67.3% (37/55) | ≥ 90% | ❌ FAIL |
| Entry-type correct | 78.2% (43/55) | ≥ 85% | ❌ FAIL |
| **Invented dates count (HARD GATE)** | **0** | **0** | ✅ PASS |
| Tag-variant collapse correct | 81.8% (45/55) | ≥ 80% | ⚠️ **INVALID — JVP HALT** |
| Junk-bucket handled correctly | 100.0% (2/2) | ~100% | ✅ PASS |

The tag-variant-collapse number is **mechanically above threshold but
marked invalid** by ADR 0048 §G5: the LLM judge that produced those 55
verdicts failed cross-judge agreement at 57.1% (threshold ≥ 85%). Until
judge validity is re-established, this metric cannot feed downstream
gating.

## SCORE_SUMMARY — run-b-stability (seed 137) — NOT RE-SCORED

`run-b` was NOT re-scored with judges this session. With the primary
judge invalid per Gate 3, the tag metric on run-b would also be invalid;
spending another ~45 minutes of LLM time to produce an
already-known-invalid number is not a defensible budget choice. The
structural metrics from the prior `--skip-jvp --skip-pcrp` partial run
are still on disk and unchanged:

| Metric | Result | Threshold | Verdict |
|---|---|---|---|
| Clean single-item correct | 13.3% (2/15) | ~100% | ❌ FAIL |
| Segmentation correct (multi-item) | 86.7% (13/15) | ≥ 85% | ✅ PASS |
| Category correct | 70.9% (39/55) | ≥ 90% | ❌ FAIL |
| Entry-type correct | 76.4% (42/55) | ≥ 85% | ❌ FAIL |
| **Invented dates count (HARD GATE)** | **0** | **0** | ✅ PASS |
| Tag-variant collapse correct | — | ≥ 80% | ⏸ skipped (judge invalid) |
| Junk-bucket handled correctly | 100.0% (2/2) | ~100% | ✅ PASS |

## Stability run-a vs. run-b (spec §8.5) — unchanged

| Dimension | Agreement |
|---|---|
| compared dictations | 32 (65 entries) |
| segmentation agreement | 96.9% (31/32) |
| category agreement | 96.9% (63/65) |
| entry-type agreement | 98.5% (64/65) |
| date agreement | **100.0%** (65/65) |
| tag-set exact agreement | 83.1% (54/65) |

The pipeline is highly deterministic across seeds. Small metric variance
(e.g. category 67.3% → 70.9%) reflects local-LLM noise on a small sample
— ~2 entries' verdicts change between seeds. This stability is the
**iteration substrate** for Wave 5 prompt work, independent of the judge
problem.

## JVP — run-a-baseline detail

| Gate | Outcome | Detail |
|---|---|---|
| 1. Calibration set | ✅ Pass | 11/12 (91.7%) verdict-correct on v2 (threshold ≥ 90%) |
| 2. Reasoning audit | ✅ Pass | 70/70 (100.0%) verdicts had structurally-valid reasoning (threshold ≥ 95%) |
| 3. Cross-judge (`gemma2:9b`) | 🛑 **Stop** | 4/7 (57.1%) primary↔cross agreement on 10% sample (STOP < 85%) |
| 4. Distribution sanity | ✅ Pass | 45/70 (64.3%) verdicts equivalent (in-band 40–80%) |
| 5. Determinism re-check | ⚠️ Warn | 0/5 byte-identical re-runs at fixed seed |
| **Overall** | **🛑 HALT** | per §G5: any Gate STOP ⇒ overall HALT |

### Gate 3 STOP — root-cause analysis

Three disagreements on the 10% sample (n=7):

1. `persona-01-case-01 entry[0]`: cross-judge errored with a transient
   network failure (`error sending request for url`). **Infra noise**, not
   a real verdict disagreement. If excluded, agreement is 4/6 = 66.7% —
   still STOP.
2. `persona-03-case-04 entry[1]`: primary=Equivalent, cross=NotEquivalent.
3. `persona-04-case-03 entry[0]`: primary=Equivalent, cross=NotEquivalent.

Both genuine disagreements are in the same direction: **`llama3.1:8b` is
more permissive than `gemma2:9b` on equivalence calls.** This is the
exact failure mode Gate 3 exists to detect. Combined with the Gate 4
distribution result (judge marked 64.3% of all verdicts Equivalent — the
high end of the in-band range), the structural picture is:

> The primary judge skews toward Equivalent verdicts on the real corpus,
> in a way the cross-judge does not corroborate. The 81.8% tag-collapse
> metric is therefore likely inflated.

Gate 1 (calibration) passing at 91.7% is not contradictory: the
calibration set is small and deliberately unambiguous, so the
primary→permissive bias doesn't show up there. The real corpus has many
fuzzy decomposition / superset cases where llama3.1 leans Equivalent
and gemma2:9b leans NotEquivalent.

### Gate 5 WARN — determinism

0/5 re-runs at fixed seed produced byte-identical output. **The verdict
may still be identical even when bytes differ** (the chain-of-thought
prose varies; the `VERDICT:` line is what's parsed). Gate 5 is currently
strict byte-identity which is overly aggressive for a chain-of-thought
judge. The Warn here is non-blocking; recommend Wave-5 follow-up to
either:

- (a) compare parsed verdicts only, or
- (b) lower the temperature further (currently 0.2), or
- (c) accept that local Ollama re-runs are non-deterministic and
  promote the verdict-only check to "the" determinism gate.

## PCRP — run-a-baseline detail

- Reviewer: `llama3.1:8b-instruct-q4_K_M`
- 13 samples across 6 personas selected per §G6 algorithm
- **trust_eroding_failures: 8**
- **trust_building_wins: 9**
- Final-run go/no-go condition (ADR §G6):
  *"PCRP final-run ≥ 5 trust-eroding failures AND scores not exceeding
  thresholds by >5pts → REPORT.md defaults NO-GO."*
  - trust_eroding 8 ≥ 5 ✓
  - no passing metric exceeds threshold by >5pts (seg pass by 1.7pts,
    tag pass by 1.8pts, junk perfect but small N)
  - **→ default NO-GO** (informational; Wave is already halting on JVP)

### Top trust-eroding themes (cross-persona pattern)

1. **Personal/professional miscategorization on side-hustle content**
   — persona-04-case-01 (Etsy / craft vinyl tagged `personal`),
   persona-03-case-01 (work committee tagged `personal`). The classify
   pass needs side-hustle/freelance examples (these are Wave-1 calibration
   locks that didn't propagate into the classify few-shots).
2. **Topic-tag drift** — `wholesaler`/`craft`/`vinyl` instead of
   `supplies`/`etsy`/`craft-vinyl`; `insurance`/`renter`/`home` instead
   of `insurance`/`renters`. Extract pass picks proximate-noun tags, not
   the persona's filing vocabulary.
3. **Date-extraction MISSES** (not invented dates — the inverse).
   persona-04-case-01 dictation says "before the weekend" with capture
   2026-06-14 (Sunday); answer key expects `2026-06-20` (Sat) on entry[0];
   pipeline emitted no due date. PCRP reviewer mislabeled this as
   "hallucinated" but the structural hard-gate is correct — the failure
   mode here is the **opposite** (under-extraction, not over-extraction).
   Recommend Wave-5 prompt tuning to give extract more confidence on
   the "before the weekend" / "by end of week" patterns.
4. **Over-segmentation** of single-item dictations — already the
   dominant structural finding (clean-single 6.7%); PCRP corroborates
   this is the persona-visible failure too.

### Top trust-building themes

1. **Junk handled correctly** — both `case-05` dictations produce zero
   entries (persona-01-case-05, persona-05-case-05).
2. **Multi-item peak-hard segmentation works** — persona-05-case-03
   (5-item rambler) is correctly split into 5 entries with correct dates
   on the items that had them.
3. **Explicit dates extract reliably** — "before Friday", "by the 19th",
   etc., produce the right ISO date when stated clearly. The
   under-extraction problem is on the **soft** date phrases, not the
   hard ones.

## What's needed to unblock the rest of Wave 3 + Wave 4

This is now a **judge-validation problem**, not a model-pulls problem.
Options forward, in rough order of cost:

### Option A — Tune the judge prompt (cheapest)

Add an explicit rule to the judge prompt biasing it toward
**NotEquivalent on superset/decomposition disagreement**, with one
concrete in-context example of a fuzzy disagreement where the verdict
is NotEquivalent. This should reduce the primary-judge equivalence rate
from 64.3% toward the in-band middle (~55–60%) and likely close the
cross-judge agreement gap.

Re-run JVP only (no need to re-run scoring; the tag verdicts are
re-issued by Gate 1 + a fresh tag-judge sweep). ~10 min iteration.

### Option B — Swap primary judge to a larger / different model

`gemma2:9b` (already on disk) or `qwen2.5:14b` could replace `llama3.1:8b`
as primary. The §G4 different-family discipline still allows this — the
constraint is "different family from the pipeline (`qwen2.5:3b`)". A
`gemma2`-as-primary / `llama3.1`-as-cross-judge swap is fully compliant.
Higher LLM cost per scoring run (gemma2:9b is ~30% slower than llama3.1:8b)
but may resolve the asymmetry cleanly.

### Option C — Add more borderline pairs to the calibration set

Current v2 calibration set is 7 unambiguous-Equivalent + 5
unambiguous-Different. Gate 1 with the current set can only detect
**egregious** judge failures; it doesn't measure the judge's behavior
on fuzzy cases. Adding 5–8 hand-graded borderline pairs would make
Gate 1's pass rate a meaningful signal for the cases Gate 3 actually
catches.

This is a calibration upgrade, not a fix in itself — but it pairs well
with Option A or B.

### Option D — Loosen Gate 3 thresholds (NOT recommended)

Currently STOP < 85%, WARN 85–95%, PASS ≥ 95%. Loosening to STOP < 50%
would let this run pass with WARN. **This is a documentation change
masquerading as a fix** and would undermine the validation protocol.
Reject unless Dustin explicitly chooses to defer judge-validity work
to a later phase.

## Open bead / next-wave gating

`mb-57a1` remains **open**. Wave 4 (invariant judges + dry-run rig) is
**blocked** until JVP completes with overall `Proceed` or
`ProceedWithWarnings` — Wave-3 dispatch standing-rule #6 explicitly
requires JVP green before Wave 4 starts. JVP overall is currently `Halt`.

Pipeline-side findings (over-segmentation, category weakness,
side-hustle classification, soft-date under-extraction) are real
Wave-5 prompt-tuning targets and are now well-documented for whichever
agent picks up Wave 5 — but executing Wave 5 still requires a working
judge to score iteration deltas, so judge-validation is on the critical
path.
