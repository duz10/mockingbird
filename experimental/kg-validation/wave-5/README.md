# Wave 5 — Wiggum prompt-iteration loop (cap 5)

This directory tracks the Iteration Acceptance Protocol (IAP) loop per
ADR 0048 §G7 + the Wave 5 kickoff brief. Pipeline runs live under
`../runs/iter-N-a/` (seed 42) + `../runs/iter-N-b/` (seed 137) which
are gitignored; THIS directory holds the durable artifacts.

## Files

- `iter-0-baseline.json` — immutable Wave 3 sealed numbers (run-a-baseline).
- `baseline-current.json` — the running baseline. On every IAP Accept,
  the candidate's metrics overwrite this; on every Reject, it's
  unchanged. Iter 0 = `iter-0-baseline.json`.
- `ITERATION_JOURNAL.md` — append-only journal, one `## Iter N` block
  per IAP run.

## Per-iteration recipe

```powershell
# 1. Author the prompt change. Single prompt per iteration so cause-effect
#    is attributable (kickoff rule).

# 2. Run pipeline seed 42.
cargo run --release --bin run-corpus -- `
    --run-id iter-N-a --seed 42 --output-dir runs

# 3. Score seed-42 run.
cargo run --release --bin score-run -- `
    --run-dir runs/iter-N-a

# 4. Run pipeline seed 137.
cargo run --release --bin run-corpus -- `
    --run-id iter-N-b --seed 137 --output-dir runs

# 5. Score seed-137 run WITH stability vs seed-42.
cargo run --release --bin score-run -- `
    --run-dir runs/iter-N-b --stability-vs iter-N-a

# 6. IAP check.
cargo run --release --bin iap-check -- `
    --baseline-snapshot wave-5/baseline-current.json `
    --candidate-run-dir runs/iter-N-a `
    --stability-run-dir runs/iter-N-b `
    --iteration N --label "short prompt-change description" `
    --journal wave-5/ITERATION_JOURNAL.md `
    --update-baseline wave-5/baseline-current.json
# Exit 0 = Accept (baseline advanced), exit 2 = Reject (revert prompt change),
# exit 1 = CLI/IO error.

# 7. Synonym map sweep (parallel track; not against iter cap).
#    Inspect runs/iter-N-a/SCORE_SUMMARY.md "Top near-misses" table;
#    add ONE-AT-A-TIME entries that pass Bernard's discipline rules
#    (person tags NEVER collapse domain; specificity preserved;
#    domain overlap ≠ equivalence). Commit as `[synonym-map]`.

# 8. Commit iteration: prompt + journal + baseline-current.
git add prompts/ wave-5/baseline-current.json wave-5/ITERATION_JOURNAL.md
git commit -m "feat(kg-wave-5): iter N — <label> [accept|reject] [mb-ojm5]"
```

## IAP rules (binding)

1. **Aggregate same-or-better**: weighted sum ≥ baseline.
   Weights: hard-gate=4, segmentation=2, category=2, entry-type=2,
   tag-collapse=2, clean-single=1, junk=1 (sum=14).
2. **No per-metric regression**: strict — any single metric below
   previous baseline = REJECT.
3. **Hard gate intact**: invented_dates_count == 0.
4. **Stability**: re-run at seed 137, structural-metric agreement
   (segmentation / category / entry-type) ≥ 80%.
5. **PCRP trust-eroding count ≤ previous baseline**.

REJECT → revert prompt change, log failure, iteration counter still
advances. ACCEPT → commit prompt, advance baseline, log delta.

## Stop conditions

- All §8.4 thresholds met → seal early, advance to Wave 6.
- 5 iterations exhausted → seal best result, advance to Wave 6 with
  honest data.
- 5-attempt rule on a single iteration → escalate via STATUS +
  escalation-tagged bead.
- 5 consecutive IAP rejects → halt + surface (kickoff standing rule).
