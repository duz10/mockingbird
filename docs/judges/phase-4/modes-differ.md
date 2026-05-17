# Judge: cleanup-modes-differ (Phase 4)

**Target:** `src-tauri/src/cleanup/provider.rs` (StubCleanupProvider mode-routing), `src-tauri/src/cleanup/llm_cleaner.rs`, `src-tauri/tests/dictation_orchestrator.rs::llm_cleanup_runs_in_orchestrator_and_injects_cleaned_text`

**Question:** When the same raw transcript is run through the cleanup pipeline under three (or more) different mode slugs, do the resulting outputs differ pairwise?

**Pass criteria — ALL of:**

1. `pwsh scripts/cargo-with-cuda.ps1 test --release --lib -- cleanup::provider::tests::stub_modes_produce_distinguishable_output --exact` → `1 passed`.
2. `pwsh scripts/cargo-with-cuda.ps1 test --release --lib -- cleanup::llm_cleaner::tests::clean_modes_produce_different_output --exact` → `1 passed`.
3. `pwsh scripts/cargo-with-cuda.ps1 test --release --test dictation_orchestrator -- llm_cleanup_runs_in_orchestrator_and_injects_cleaned_text --exact` → `1 passed`. This is the integration-level proof: full orchestrator + LlmCleaner + StubCleanupProvider → injected text differs from raw.

**On failure:** block `phase-4-complete` tag.

**Last run:** 2026-05-18 — GREEN (all three tests pass).
