# Phase 0 Knowledge Graph — Invariant judges (Wave 4)

ADR 0048 chartered Phase 0 KG as a sandbox-isolated lateral epic. This
directory documents the six invariant judges authored in Wave 4
(`mb-he98`) that gate any seal claim.

> **Where the code lives:** `experimental/kg-validation/src/judges/`
> (one Rust module per judge, fixture pairs co-located in `#[cfg(test)]`).
> The orchestrator binary is `experimental/kg-validation/src/bin/run-judges.rs`.

## The six judges

| # | Name | Mechanism | What it asserts | Source module |
|---|---|---|---|---|
| 1 | `hard_gate_invented_dates_zero` | reads `SCORE.json::per_metric::invented_dates_count` | Equals `0` for every supplied run (spec §8.4 absolute floor). | `judges/hard_gate.rs` |
| 2 | `thresholds_match_spec_8_4` | reads each run's `SCORE.json` | Per-metric pass/fail vs. spec §8.4 floors: segmentation ≥ 85%, category ≥ 90%, entry-type ≥ 85%, tag-variant-collapse ≥ 80%, junk-correct = 100%, invented dates = 0, clean-single ≈ 100%. | `judges/thresholds.rs` |
| 3 | `stability_meets_spec_8_5` | reads the `stability` block of a `SCORE.json` | Structural-metric agreement ≥ 80% (segmentation / category / entry-type / tag-set-exact) AND date-agreement = 100%. `tag_set_exact_agreement` is reported but **not gated** (ADR 0048 §G7: open-vocab metric, misses point to synonym-map gaps not pipeline regression). | `judges/stability.rs` |
| 4 | `sandbox_isolation_phase0_kg` | `git diff --name-only <baseline-ref> HEAD` | Every changed file is inside the allowed sandbox surface: `experimental/kg-validation/`, `docs/`, `STATUS.md`, `.beads/`, `.code_puppy/`. Default baseline ref: `phase-0-kg-start` (anchor tag at the commit just before Wave 0 began). | `judges/sandbox_isolation.rs` |
| 5 | `determinism_seed42_byte_identical` | re-invokes `run-corpus --seed 42` on a 3-dictation subset and byte-compares the produced `structured/<id>.json` against the baseline run's same files | At fixed seed, the same code + same model + same Ollama options produce byte-identical structured outputs. **Optional / opt-in** via `--enable-determinism` — the rest of the suite stays usable offline. | `judges/determinism.rs` |
| 6 | `pcrp_completeness_and_trust` | reads `PERSONA_REVIEW.md` + `SCORE.json` from the final-iteration run dir | PCRP markdown is present AND (`trust_eroding_failures_count ≤ 5` OR at least one structural metric exceeds its §8.4 floor by > 5 pts). Encodes the §G6 escape valve. | `judges/pcrp_completeness.rs` |

(JVP-completeness — the seventh judge from the original draft suite —
was retired in ADR 0048 §G7 along with the LLM tag-equivalence judge.
The deterministic synonym-map metric removed the need for an LLM
validator on tag-collapse; JVP source survives in
`src/scoring/judge_validation.rs` for future LLM-judged metrics.)

## Invocation (canonical dry-run)

From the repo root, no extra env needed (the sandbox crate has no
CUDA / whisper-rs / ort deps — vanilla Cargo works):

```powershell
cd experimental\kg-validation
cargo build --release --bin run-judges
.\target\release\run-judges.exe `
    --runs runs/run-a-baseline,runs/run-b-stability `
    --final-run runs/run-a-baseline
```

That runs five judges mechanically and skips `determinism` (verdict
`PASS` with "SKIPPED" reasoning). To run the determinism judge live
(re-invokes Ollama; ~30s per dictation × 3):

```powershell
.\target\release\run-judges.exe `
    --runs runs/run-a-baseline,runs/run-b-stability `
    --final-run runs/run-a-baseline `
    --enable-determinism `
    --run-corpus .\target\release\run-corpus.exe `
    --corpus-dir corpus `
    --model qwen2.5:3b-instruct-q4_K_M
```

Exit codes: `0` = all six PASS, `1` = at least one FAIL, `2` =
argument error.

## Wave 4 status (smoke against the on-disk Wave 3 runs)

| Judge | Verdict | Notes |
|---|---|---|
| `hard_gate_invented_dates_zero` | ✅ PASS | `0 / 55` in both runs — hard gate holds. |
| `thresholds_match_spec_8_4` | ❌ FAIL | Expected. Category 67–71% (floor 90%), entry-type 76–78% (floor 85%), tag-collapse 9–11% (floor 80%), clean-single 7–13% (floor ~100%). **These are the canonical Wave 5 prompt-iteration inputs**; the FAIL is the judge surfacing the gap, not a defect. |
| `stability_meets_spec_8_5` | ✅ PASS | 96.9 / 96.9 / 98.5 / 100 across segmentation / category / entry-type / date. Tag-set exact 83.1% reported but not gated. |
| `sandbox_isolation_phase0_kg` | ✅ PASS (post-commit) | Initial smoke flagged a stale root-`.gitignore` entry for `experimental/kg-validation/runs/`. Removed in this iteration — the sandbox-local `.gitignore` already covers `/runs`, `/target`, and `/smoke-corpus`. Verdict goes green after the cleanup commit. |
| `determinism_seed42_byte_identical` | ⚪ SKIPPED | Opt-in. Live run deferred until Wave 5 prompt iteration stabilizes the run-a baseline. |
| `pcrp_completeness_and_trust` | ❌ FAIL | §G6 NO-GO. `trust_eroding_failures_count = 8` (parsed from the canonical markdown bullet form) AND no structural metric exceeds its floor by > 5 pts. This is the **expected** signal that Wave 5 prompt iteration is required before any Phase 0 seal claim. |

The 3-PASS / 1-PASS-post-commit / 1-SKIP / 2-FAIL outcome is the
canonical "Wave 4 done, Wave 5 needs to ship prompt iteration before
Wave 6 can attempt a seal." The thresholds + PCRP fails are not bugs;
they are the judges doing their job.

## What's next (`mb-ojm5`, blocked on `mb-he98`)

Wave 5 — Wiggum loop, cap 5: iterate `prompts/segment.md`,
`prompts/classify.md`, and `prompts/extract.md` against the corpus,
re-run `run-corpus`, re-run `run-judges`, and stop when the
`thresholds` and `pcrp_completeness` judges flip green (or the loop
caps out, at which point ADR 0048 picks up a §G8 escalation). The
synonym map (`judge-calibration/synonym-map.json`) iterates in
parallel using the top-10 near-miss aggregation that `score-run`
already emits.

The `determinism` judge should be re-enabled at Wave 6 once a
candidate "green" baseline run is on disk to compare against.
