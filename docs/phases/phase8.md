# Phase 8 — Learning loop

**Status:** ✅ COMPLETE (Wave 1 — backend only; Settings → Advanced UI deferred to Phase 5/6 UI sprint)
**Started:** 2026-05-18
**Sealed:** 2026-05-18 (commit pending; tag `phase-8-complete`)

## Goal (PLAN §10)

> Nightly Task Scheduler job that classifies user corrections, promotes
> them to the dictionary/style examples, and rolls back on regression.

## What landed

### New module tree (all under 600-line cap)

| Module | Purpose | Lines |
|---|---|---:|
| `learning/mod.rs` | Module root + ASCII pipeline diagram | 65 |
| `learning/corrections.rs` | Repo for `corrections` table | 280 |
| `learning/runs.rs` | Repo for `learning_runs` table | 220 |
| `learning/classifier.rs` | `Classifier` trait + `LlmClassifier` + `HeuristicClassifier` (deterministic fallback) | 295 |
| `learning/promoter.rs` | Apply classifier verdicts: dictionary insert / style-example insert / per-mode prune-to-50 | 295 |
| `learning/eval.rs` | `EvalProvider` trait + `DefaultEvalProvider` (corrections-per-session ratio) + `FixedEvalProvider` (tests) | 210 |
| `learning/runner.rs` | End-to-end `run_once`: classify → promote → eval → commit-or-rollback (single SQLite txn) | 390 |
| `learning/scheduler.rs` | `Scheduler` trait + `WinTaskScheduler` (`schtasks.exe`) + `RecordingScheduler` (tests) | 200 |
| `bin/learn.rs` | CLI entry point invoked by Task Scheduler | 110 |

### Pipeline (per PLAN §10)

```
   corrections (last 7 d) ─▶ classifier ─▶ {new_vocab, style_change,
                                            mistranscription, noise}
         │                                       │
         │           new_vocab ──────────────────┼──▶ dictionary
         │           style_change ───────────────┼──▶ style_examples
         ▼                                       ▼
   eval(replay last 24 h, compute correction rate)
         │
         ├── after_rate <= before_rate ─▶ COMMIT
         └── after_rate >  before_rate ─▶ ROLLBACK (whole batch)
```

### Key design decisions

- **Single SQLite transaction** for the entire batch — partial-state
  DBs are impossible. The `learning_runs` meta-row is inserted in a
  separate implicit transaction so it survives even when the main
  txn rolls back.
- **Pluggable `EvalProvider`** — the v1 `DefaultEvalProvider` uses a
  cheap corrections-per-session ratio. Phase 8 Wave 2 can swap in a
  session-replay-based evaluator without touching the runner. The
  trait is the seam.
- **LLM with heuristic fallback** — `bin/learn.rs` health-checks
  Ollama; falls back to `HeuristicClassifier` if Ollama isn't
  reachable. The nightly loop still makes progress on a box where
  the user uninstalled Ollama.
- **No DELETE for pruning** — style-example pruning sets
  `enabled = 0` (audit-friendly per ADR 0010). The few-shot selector
  already filters by `enabled = 1`.
- **`schtasks.exe` not COM** — `Scheduler::install` shells out
  to the documented Windows CLI rather than linking the (huge,
  stale) Task Scheduler COM surface.

### Classifier verdict matrix

| Verdict | Promoter action | Test coverage |
|---|---|---|
| `new_vocab` | Insert into `dictionary` as `source='learned'` (skips if term exists) | ✅ `promote_new_vocab_inserts_dictionary_entry`, `promote_new_vocab_skips_if_term_exists` |
| `style_change` | Insert `(raw → ideal)` into `style_examples` with `rank=0.6` | ✅ `promote_style_change_inserts_style_example` |
| `mistranscription` | No-op | ✅ `promote_mistranscription_is_noop` |
| `noise` | No-op | ✅ `promote_noise_is_noop` |

### Tests

- **34 new pure unit tests** (lib) — all green.
- **2 `#[ignore]`d live tests**: `scheduler::live_install_uninstall_round_trip`
  (actually mutates Windows Task Scheduler).
- **Simulated 50-correction dataset** test (`simulated_50_corrections_dataset_completes_within_eval_window`)
  proves the runner handles a realistic batch end-to-end.
- **Synthetic regression test** (`regression_path_rolls_back_and_records_run`)
  uses `FixedEvalProvider` to assert the rollback path zeroes out
  all promotions AND keeps the original correction unclassified
  (so a future run can retry).

## Phase total

- ~1,600 LoC new Rust (`learning/*` + `bin/learn.rs`).
- 34 new unit tests; 2 new `#[ignore]`d live tests.
- 0 new ADRs (the EvalProvider trait was a refactor in response to a
  test-revealed deadlock, captured in this doc rather than a separate ADR).
- 0 new migrations (schema already had `corrections` + `learning_runs`
  from migration 001).

## Carry-forward to Phase 5/6 UI

- **Settings → Advanced → Learning history view** — list `learning_runs`
  rows, show `rolled_back` badge, expose a "rerun pending classifications"
  button.
- **"This was wrong" right-click on History items** — Tauri command
  that calls `corrections::insert(NewCorrection { detection_method: "manual", ... })`.
- **First-launch task-scheduler install** — Phase 7 polish wave should
  call `WinTaskScheduler::install(installed_learn_exe_path)` on first
  successful settings save.

## Carry-forward to Phase 8 Wave 2

- **Clipboard-monitor correction detection** — listen for `WM_CLIPBOARDUPDATE`,
  compare clipboard text against most recent injection, infer corrections
  when the user edits within 60 s. Inserts with `detection_method = "clipboard_undo"`.
- **Session-replay-based eval** — replace `DefaultEvalProvider` with one
  that re-runs the cleanup pipeline on each session's `raw` transcript
  and measures divergence from `final`.
