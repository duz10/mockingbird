# Judge: learning-eval (Phase 8)

**Target:** `src-tauri/src/learning/runner.rs`, `src-tauri/src/learning/eval.rs`

**Question:** Does the learning loop (a) commit when the eval metric does not regress, and (b) roll back when it does, leaving zero side effects?

**Pass criteria — ALL of:**

1. `pwsh scripts/cargo-with-cuda.ps1 test --release --lib -- learning::runner::tests::happy_run_promotes_and_commits --exact` → `1 passed`. Asserts: 2 corrections classified, 1 dictionary term added, 1 style example added, `rolled_back = false`, classified rows are present in DB.

2. `pwsh scripts/cargo-with-cuda.ps1 test --release --lib -- learning::runner::tests::regression_path_rolls_back_and_records_run --exact` → `1 passed`. Asserts: `rolled_back = true`, no style example present, original correction still unclassified (so future runs retry), `learning_runs` row persists with the regression note.

3. `pwsh scripts/cargo-with-cuda.ps1 test --release --lib -- learning::runner::tests::simulated_50_corrections_dataset_completes_within_eval_window --exact` → `1 passed`. Proves the runner handles a realistic 50-row batch.

4. `pwsh scripts/cargo-with-cuda.ps1 test --release --lib -- learning::runner::tests::noop_when_nothing_pending --exact` → `1 passed`. Proves the no-op path still inserts a `learning_runs` row for observability.

**On failure:** block `phase-8-complete` tag.

**Last run:** 2026-05-18 — GREEN (4/4 pass).
