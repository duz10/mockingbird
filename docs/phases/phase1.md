# Phase 1 — Foundation

**Phase entry tag:** `phase-0-complete` (commit `9dddae0`)
**Phase exit tag:** `phase-1-complete` (target — SEALS migrations 001-003 forever)
**Planner:** planning-agent (session 1b10a8)
**Implementor:** code-puppy-adeb7b (with `migration-author` as the
active agent for Wave 2 SQL work)
**Estimated iterations:** 3–4

> Binding spec lives in PLAN-mockingbird-v2.md §7 (data model + migrations),
> §9 (recording-window config block), §10 Phase 1, §5 (deps). This doc
> operationalizes them.

## Resolved decisions

### ADR 0004 — rusqlite over sqlx (Accepted)

Mockingbird's DB workload is single-process, single-writer, modest-volume,
with heavy use of raw SQL features (FTS5, JSON triggers, multi-table
`_history_*` audit, `execute_batch` for migrations). `rusqlite` with
`features = ["bundled"]` ships SQLite statically, gives deterministic
FTS5/JSON1 surface across dev machines, needs no build-time DB or
codegen step, and keeps error handling synchronous and obvious. sqlx's
compile-time query checking buys nothing here because every cross-row
invariant lives in triggers we control. Drop `tauri-plugin-sql` from
the Cargo manifest — all DB access lives in Rust, the React side talks
to typed `#[tauri::command]` wrappers.

### Phase 1 Cargo deps (subset of PLAN §5)

Include: `tauri`, `tauri-plugin-tray`, `rusqlite (bundled)`, `serde`,
`serde_json`, `tokio` (basic features only — no full runtime), `tracing`,
`tracing-subscriber`, `tracing-appender`, `chrono`, `uuid`, `thiserror`,
`windows` (path/registry only), `tempfile` (dev), `rstest` (dev),
`proptest` (dev), `mockall` (dev).

**Defer to Phase 2:** `whisper-rs`, `cpal`, `ort`, `enigo`. These pull
in CUDA / native build deps and would make Phase 1 unbuildable on
machines without cmake/nvcc — which is the current state of Dustin's
machine per STATUS.md blocked-on.

## Task waves

### Wave 1 — Decisions, scaffolding, prompt stubs (Iteration 1)

| id                 | title                                                | priority | agent       |
|--------------------|------------------------------------------------------|----------|-------------|
| `p1-adr-0004`      | Write ADR 0004 (rusqlite vs sqlx)                    | 1        | code-puppy  |
| `p1-cargo-toml`    | Workspace + `src-tauri/Cargo.toml` (Phase-1 deps)   | 1        | code-puppy  |
| `p1-tauri-conf`    | `src-tauri/tauri.conf.json` with recording-window   | 1        | code-puppy  |
| `p1-skeleton`      | Rust skeletons: main.rs, lib.rs, error.rs           | 1        | code-puppy  |
| `p1-prompt-stubs`  | `cleanup/prompts/{normal,verbose,fragment}.md`      | 2        | code-puppy  |
| `p1-data-model-doc`| `docs/DATA_MODEL.md` reference (canonical=PLAN §7)  | 2        | code-puppy  |

### Wave 2 — Migrations (Iteration 1 or 2; depends on Wave 1)

| id                 | title                                                | priority | agent              |
|--------------------|------------------------------------------------------|----------|--------------------|
| `p1-mig-001`       | `001_initial.sql` — full PLAN §7 + FTS5             | 1        | migration-author   |
| `p1-mig-002`       | `002_audit_triggers.sql` — all 4 tables, 12 triggers | 1        | migration-author   |
| `p1-mig-003`       | `003_seed_modes.sql` — prompts loaded via runner    | 1        | migration-author   |
| `p1-mig-runner`    | `src-tauri/src/db/{mod.rs,migrations.rs}`           | 1        | code-puppy         |
| `p1-mig-tests`     | `src-tauri/tests/db_migrations.rs` integration tests | 1       | migration-author   |

### Wave 3 — DB repository modules (Iteration 2; depends on Wave 2)

| id                 | title                                                | priority |
|--------------------|------------------------------------------------------|----------|
| `p1-db-transcripts`| `db/transcripts.rs` (no `update_raw` — hook scans)  | 1        |
| `p1-db-search`     | `db/search.rs` (FTS5 query helper)                  | 1        |
| `p1-db-sessions`   | `db/sessions.rs`                                    | 2        |
| `p1-db-prompts`    | `db/prompts.rs` (read-only)                         | 2        |
| `p1-db-dictionary` | `db/dictionary.rs`                                  | 2        |
| `p1-db-examples`   | `db/examples.rs` (minimal — full ranking in P8)     | 3        |
| `p1-db-audit`      | `db/audit.rs` (rollback helpers + tests)            | 2        |

### Wave 4 — App shell (Iteration 2 or 3; parallel after Wave 2)

| id                  | title                                              | priority |
|---------------------|----------------------------------------------------|----------|
| `p1-logging`        | `src-tauri/src/logging.rs` (rotation + PII scrub)  | 1        |
| `p1-settings-model` | `src-tauri/src/settings/model.rs` (typed keys)     | 1        |
| `p1-settings-mod`   | `src-tauri/src/settings/mod.rs` (facade over table)| 1        |
| `p1-tray`           | `src-tauri/src/tray.rs` (placeholder menu)         | 2        |
| `p1-commands`       | `src-tauri/src/commands.rs` (#[tauri::command])    | 1        |
| `p1-app-wire`       | `lib.rs::run()` wires migrations + tray + commands | 1        |

### Wave 5 — Docs, verification, seal (Iteration 3 or 4)

| id                    | title                                            | priority |
|-----------------------|--------------------------------------------------|----------|
| `p1-settings-doc`     | Flesh out `docs/SETTINGS.md` per `model.rs`      | 1        |
| `p1-lefthook-verify`  | Confirm lefthook runs real fmt/clippy/test       | 2        |
| `p1-status-update`    | STATUS.md → Phase 1 ✅ → Phase 2 queued          | 1        |
| `p1-lessons-append`   | `docs/LESSONS.md` Phase-1 findings               | 1        |
| `p1-judges-run`       | Run end-of-phase judges (see §C)                 | 1        |
| `p1-commit-tag`       | Final commit + `git tag phase-1-complete` (SEAL) | 1        |
| `p1-bd-sync`          | Close p1-* + seed Phase 2 tasks                  | 1        |

## Exit criteria

**PLAN §10 Phase 1 verbatim:**
> Tauri v2 app opens to tray, SQLite migrations 001-003 applied, settings round-trip, FTS5 search smoke test passes.

**Operationalized:**
1. `cargo build --release` green on Windows without CUDA toolchain.
2. `cargo test` green (incl. migration integration + FTS5 round-trip + audit-rollback).
3. `cargo clippy -- -D warnings` clean.
4. `cargo tauri dev` → main hidden + tray icon + menu opens.
5. `set_setting("theme","dark")` → restart → `get_setting("theme")` returns `"dark"`.
6. `fts_smoke_test("hello")` returns an inserted raw transcript.
7. `git tag --list "phase-*"` includes `phase-1-complete`.
8. STATUS.md current phase = "Phase 2 (queued)".
9. Hook `block-migration-edit-after-phase-1` confirmed live (try-and-expect-block, then discard).

## Judges at phase exit

| Judge                              | Run? | Notes                                                |
|------------------------------------|------|------------------------------------------------------|
| `build-passes`                     | YES  | `cargo build --release`                              |
| `tests-pass`                       | YES  | `cargo test --workspace`                             |
| `lint-clean`                       | YES  | `cargo clippy -- -D warnings`; `cargo fmt --check`   |
| `migrations-applied` *(new)*       | YES  | Fresh tempdb → runner → assert schema_version=3      |
| `fts5-smoke` *(new)*               | YES  | Insert raw, FTS5 match returns it                    |
| `adr-recorded`                     | YES  | ADR 0004 exists, Accepted, follows template          |
| `plan-aligned`                     | YES  | 001/002/003 schema diff'd against PLAN §7            |
| `status-updated`                   | YES  | Last-judge-run line present                          |
| `raw-immutability-static-check` *(new)* | YES | `grep "UPDATE transcripts" src-tauri/src/` returns 0 hits in non-test code |
| `agents-md-present`                | passthrough |                                               |
| `hook-config-valid`                | passthrough |                                               |

Three NEW judge prompts to add to `.code_puppy/judges-template.json` as
part of `p1-judges-run`: `migrations-applied`, `fts5-smoke`,
`raw-immutability-static-check`.

## Risks (top 8)

1. **`whisper-rs cuda` would fail to build without CUDA.** → Defer to Phase 2 Cargo.toml (encoded in this plan).
2. **rusqlite `bundled` + FTS5 build flag verification.** → First migration test confirms or fails fast.
3. **Tauri v2 recording-window config quirks** (`focus:false` + `skipTaskbar:true` + `transparent:true`). → Phase 1 declares config only; if Tauri schema rejects, fall back to building the recording window via `WebviewWindowBuilder` in Rust at startup (Phase 5 polishes anyway).
4. **`include_str!` Windows path separators.** → Forward slashes only.
5. **`%APPDATA%` resolution + WAL on first run.** → `app.path().app_data_dir()` (Tauri 2 API), `mkdir -p` on startup, log resolved path.
6. **`block-migration-edit-after-phase-1` premature fires.** → Hook checks tag existence, not branch; pre-seal edits OK.
7. **Audit triggers on `prompts` fire during seed (003).** → Integration test expects 6 history rows after fresh-run.
8. **5-attempt rule on `tauri.conf.json` schema.** → Switch to minimal config + programmatic `WebviewWindowBuilder` if Wave 1 burns 3 attempts.

## Out of scope (DEFER to later phases)

- `whisper-rs`, `cpal`, `ort`, `enigo` deps → Phase 2
- Global hotkey + text injection → Phase 3
- Cleanup LLM provider abstraction → Phase 4
- Recording UX (real recording window, audio meter) → Phase 5
- History viewer / data UI → Phase 6
- `ui/` (React + Tailwind) → introduced in Phase 5; Phase 1 has no JS
- Code signing + updater wiring → Phase 7 (key already generated)
- Learning loop → Phase 8

## Iteration plan

| Iteration | Scope                              | Notes                                    |
|-----------|------------------------------------|------------------------------------------|
| 1         | Wave 1 (scaffolding + ADR 0004)    | ✅ commit `8e70d7c`.                     |
| 2         | Wave 2 (migrations + runner + tests)| **Pre-resolved in `phase1-wave2-brief.md`** — all design decisions PLAN doesn't pin down are captured there. Wave 2 is mechanical implementation. |
| 3         | Wave 3 + Wave 4 (DB repos + app shell) | Likely combinable; depends on Wave 2 surprises. |
| 4         | Wave 5 (docs + seal) — buffer for any 5-attempt-rule escalation | Tag `phase-1-complete`. |

## Wave-specific briefs

- **Wave 2:** `docs/phases/phase1-wave2-brief.md` — exhaustive
  implementation brief: full audit-trigger SQL for all 4 tables
  (extrapolated from PLAN's dictionary-only example), runner file
  layout (`db/mod.rs` + `db/migrations.rs` + `db/prompt_loader.rs`)
  with function signatures, token-substitution strategy for migration
  003, integration-test specs (7 tests), known PLAN bugs flagged
  (dictionary trigger references non-existent `enabled` column),
  deviations recorded. Read BEFORE invoking `migration-author`.
