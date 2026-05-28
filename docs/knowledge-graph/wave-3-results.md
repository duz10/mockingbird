# Wave 3 — partial baseline + stability results

**Status:** PARTIAL — pipeline runs + 5-of-6 structural metrics + stability
shipped. **JVP + PCRP + tag metric BLOCKED** on judge-model pull
(`llama3.1:8b-instruct-q4_K_M` and ideally `gemma2:9b` not on disk on this
box; per Wave-3 dispatch policy 4–9 GB model pulls are user-initiated).

- Bead: `mb-57a1` (left open — JVP/PCRP unfinished)
- ADR: 0048 (G5 + G6 amendments shipped this wave: `be5fc79`)
- Pipeline model: `qwen2.5:3b-instruct-q4_K_M`
- Dispatched judge model: `llama3.1:8b-instruct-q4_K_M` (NOT pulled)
- Dispatched cross-judge: `gemma2:9b` (NOT pulled — Gate 3 would have
  demoted to WARN-only per §G5 in any case)

## Runs

| Run | Seed | Duration | Dictations | Success | Errors |
|---|---|---|---|---|---|
| run-a-baseline | 42 | 172 s | 32 | 32 | 0 |
| run-b-stability | 137 | 168 s | 32 | 32 | 0 |

Zero per-pass parse errors on either run. Pipeline is stable under load.

## SCORE_SUMMARY — run-a-baseline (seed 42)

| Metric | Result | Threshold | Verdict |
|---|---|---|---|
| Clean single-item correct | 6.7% (1/15) | ~100% | ❌ FAIL |
| Segmentation correct (multi-item) | 86.7% (13/15) | ≥ 85% | ✅ PASS |
| Category correct | 67.3% (37/55) | ≥ 90% | ❌ FAIL |
| Entry-type correct | 78.2% (43/55) | ≥ 85% | ❌ FAIL |
| **Invented dates count (HARD GATE)** | **0** | **0** | ✅ PASS |
| Tag-variant collapse correct | — | ≥ 80% | ⏸ skipped (no judge) |
| Junk-bucket handled correctly | 100.0% (2/2) | ~100% | ✅ PASS |

## SCORE_SUMMARY — run-b-stability (seed 137)

| Metric | Result | Threshold | Verdict |
|---|---|---|---|
| Clean single-item correct | 13.3% (2/15) | ~100% | ❌ FAIL |
| Segmentation correct (multi-item) | 86.7% (13/15) | ≥ 85% | ✅ PASS |
| Category correct | 70.9% (39/55) | ≥ 90% | ❌ FAIL |
| Entry-type correct | 76.4% (42/55) | ≥ 85% | ❌ FAIL |
| **Invented dates count (HARD GATE)** | **0** | **0** | ✅ PASS |
| Tag-variant collapse correct | — | ≥ 80% | ⏸ skipped (no judge) |
| Junk-bucket handled correctly | 100.0% (2/2) | ~100% | ✅ PASS |

## Stability run-a vs. run-b (spec §8.5)

| Dimension | Agreement |
|---|---|
| compared dictations | 32 (65 entries) |
| segmentation agreement | 96.9% (31/32) |
| category agreement | 96.9% (63/65) |
| entry-type agreement | 98.5% (64/65) |
| date agreement | **100.0%** (65/65) |
| tag-set exact agreement | 83.1% (54/65) |

The pipeline is highly deterministic across seeds. The 3–6 point metric
variance between run-a and run-b (e.g. category 67.3% → 70.9%) reflects
local-LLM noise on a small sample — only ~2 entries' verdicts changed
between the runs. Tag-set agreement is the lowest at 83.1% as expected
(open vocabulary).

## Findings — what the numbers actually say

### ✅ Wins

1. **The hard gate holds.** 0 invented dates across 110 produced entries.
   This is the single load-bearing invariant of the system and it cleared
   on both runs with seed-stability.
2. **Junk handling is perfect.** Both `case-05` dictations (per personas 01
   and 05) correctly produced 0 entries. The segmenter recognizes
   intentionally-aborted speech.
3. **Multi-item segmentation passes threshold.** 86.7% on the harder
   multi-item bucket. The 5-item peak-hard case (`persona-05-case-03`) was
   segmented correctly into 5 entries on both runs.
4. **Pipeline determinism is real.** 100% date agreement across seeds;
   96–98% on categorical fields. We have an iteration substrate.

### ❌ Failures + diagnosis

1. **Over-segmentation is the dominant Wave-3 failure mode.** 9 of 15
   single-item dictations (60%) got split into 2 entries by the segmenter.
   This cascades into the abysmal 6.7% clean-single-item score: only
   `persona-01-case-06` was both correctly segmented AND correctly
   classified end-to-end. Of the 6 correctly-segmented singles, 5 had
   category or entry-type errors. **The segmenter prompt needs to learn
   "when in doubt, keep as one entry" — this is the most important Wave
   5 / Wiggum-loop target.**
2. **Category correctness 67% < 90% threshold by 23 points.** Largest
   single gap to the spec §8.4 targets. The classify pass needs the most
   prompt work; likely benefits from more in-context examples covering
   the personal/professional/objective boundary.
3. **Entry-type correctness 78% < 85% threshold by 7 points.** Within
   plausible reach of one iteration's prompt work.

### ⏸ Not yet measurable

1. **Tag-variant collapse** — the LLM judge isn't wired (model missing).
   Cannot report whether tag normalization is producing
   judge-recognized-equivalent sets.
2. **JVP gates** — by extension, judge validation cannot run; the tag
   metric would be invalid even if computed.
3. **PCRP** — requires an LLM reviewer model (ideally non-qwen for
   different-family discipline per §G4). Not yet run.

## What's needed to unblock the rest of Wave 3

Pull one or both of:

```
ollama pull llama3.1:8b-instruct-q4_K_M    # ~5 GB, primary judge (REQUIRED)
ollama pull gemma2:9b                      # ~5 GB, Gate 3 cross-judge (optional)
```

Once the primary judge is on disk:

```
cd experimental/kg-validation
.\target\release\score-run.exe --run-dir runs\run-a-baseline
.\target\release\score-run.exe --run-dir runs\run-b-stability --stability-vs run-a-baseline
```

Each scoring invocation runs JVP (5 gates) + tag metric + PCRP. ETA per
run is dominated by judge calls: ~1 call per produced tag set × 55 entries
+ 12 calibration pairs + 6 personas × 1 PCRP call ≈ 75–90 judge calls per
run. At ~3–8 s/call for `llama3.1:8b` on this box, ~5–10 minutes per
scoring invocation.

## Open bead / next-wave gating

`mb-57a1` remains **open**. Wave 4 (invariant judges + dry-run rig) is
**blocked** until JVP completes — standing-rule #6 in the Wave-3 dispatch
explicitly requires JVP green + PCRP complete + stability findings on
disk. Stability is on disk; JVP + PCRP are not.

The pipeline-side findings (over-segmentation; category weakness)
are real Wave-5 targets and don't need to wait for Wave 4 to be planned —
but executing Wave 5 also requires a working judge for scoring iteration
deltas.
