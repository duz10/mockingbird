# ADR-0004: rusqlite over sqlx

- **Status:** Accepted
- **Date:** 2026-05-15
- **Deciders:** Dustin, code-puppy-adeb7b, planning-agent (session 1b10a8)

## Context

PLAN.md Section −1 item 9 and Section 10 Phase 1 reserved a decision
between two Rust SQLite drivers: **rusqlite** (synchronous, FFI-direct)
and **sqlx** (async, compile-time-checked queries). Both can drive
Mockingbird's local SQLite database. The choice affects every Rust db
module from Phase 1 onward and surfaces in trigger-heavy and FTS5-heavy
code paths.

Mockingbird's workload:

- Single-process, single-writer, modest volume (one row per dictation)
- Heavy use of raw SQL features: FTS5 virtual tables, JSON triggers,
  multi-table `_history_*` audit, `execute_batch` for migrations
- DB access from inside Tauri command handlers (already on worker threads)
- No external db server, no codegen step desired, no build-time db needed

PLAN §5 also lists `tauri-plugin-sql` as a candidate. That plugin lives
on the JS side and exposes a SQL API to React; we don't need that —
we want typed `#[tauri::command]` wrappers, not raw SQL leaking to the
frontend.

## Decision

Use **`rusqlite`** with `features = ["bundled"]`. Drop `tauri-plugin-sql`
from `Cargo.toml`. All database access is in Rust; React talks only to
typed `#[tauri::command]` wrappers.

## Consequences

- **Positive:**
  - `bundled` ships SQLite statically → deterministic FTS5 / JSON1
    surface across every dev machine, no system-SQLite version drift.
  - No build-time database, no codegen step (sqlx-offline mode adds
    toolchain friction we don't need).
  - Synchronous, obvious error handling. DB calls run inside Tauri
    command handlers already on worker threads — adding tokio just to
    appease sqlx buys nothing.
  - `execute_batch` pattern for migration files reads cleanly.
- **Negative:**
  - No compile-time query checking. Mitigated: every cross-row
    invariant lives in triggers we control, integration tests
    exercise every query path, and `mockall` boundaries let us
    test repository methods in isolation.
  - Schema changes require rebuilding the `rusqlite::bundled` SQLite
    less than once a year — fine for our cadence.
- **Neutral:**
  - The decision is revisited at Phase 4 entry; if cloud LLM batch
    work shows up that benefits from async db, revisit then.

## Alternatives considered

- **sqlx (async, compile-time checks):** the compile-time check
  benefit is mostly cancelled by our trigger-heavy schema (the checker
  can't see triggers). The async surface complicates `execute_batch`
  for migrations. Buys complexity, returns little.
- **`tauri-plugin-sql` only:** exposes SQL to the JS side; we deliberately
  do not want that. Type-safe boundaries belong in Rust.
- **Diesel:** ORM that obscures the SQL we explicitly want to write
  directly (especially trigger-coupled tables). Overkill.

## Cross-references

- PLAN §5 (tech stack), §7 (data model), §10 Phase 1
- `.code_puppy/skills/data-model/SKILL.md`
- `.code_puppy/agents/migration-author.json`
- (this phase) `src-tauri/Cargo.toml`, `src-tauri/src/db/`
