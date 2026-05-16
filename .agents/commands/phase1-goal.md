---
description: Execute one iteration of Phase 1 (Foundation). See docs/phases/phase1.md.
---

Phase 1: Foundation. Read `docs/phases/phase1.md` for the binding
plan (25 tasks across 5 waves; planning-agent session 1b10a8).
Standard required-reading + iteration-mandate + definition-of-done
apply from `.code_puppy/AGENTS.md`.

**Before starting the active wave**, also read any wave-specific brief
at `docs/phases/phase1-wave{M}-brief.md`. Briefs pre-resolve design
decisions PLAN doesn't fully pin down and are written by code-puppy at
iteration boundaries with full context. They are binding for the
active wave; deviate only with a recorded reason in LESSONS.md.

Phase summary (per PLAN §10): Tauri v2 app opens to tray, SQLite
migrations 001-003 applied, settings round-trip, FTS5 search smoke
test passes. Key deliverables: `src-tauri/Cargo.toml`, `tauri.conf.json`,
the three migrations including FTS5 + audit triggers, the migration
runner, logging with PII scrubbing, typed settings, tray placeholder,
ADR 0004 (rusqlite vs sqlx).

**Wave 1 ✅** (commit `8e70d7c`): ADR 0004, Cargo manifests, tauri.conf.json,
Rust skeletons, prompt stubs, DATA_MODEL.md, .gitattributes.

**Wave 2 (next):** migrations 001-003 + runner + integration tests.
Read `docs/phases/phase1-wave2-brief.md` FIRST — it has the audit-trigger
SQL for all 4 tables extrapolated, runner architecture, file layout,
function signatures, and 7 integration-test specs. Then
`invoke_agent("migration-author", ...)` for the SQL deliverables.
