---
description: Execute one iteration of Phase 1 (Foundation). See docs/phases/phase1.md.
---

Phase 1: Foundation. Read `docs/phases/phase1.md` for the binding
plan (planning-agent writes it after Phase 0 closes). Standard
required-reading + iteration-mandate + definition-of-done apply
from `.code_puppy/AGENTS.md`.

Phase summary (per PLAN §10): Tauri v2 app opens to tray, SQLite
migrations 001-003 applied, settings round-trip, FTS5 search smoke
test passes. Key deliverables: `src-tauri/Cargo.toml`, `tauri.conf.json`
with the recording-window block from Section 9, the three migrations
including FTS5 + audit triggers, the migration runner, logging with
PII scrubbing, typed settings, tray placeholder, ADR 0004 (rusqlite
vs sqlx).
