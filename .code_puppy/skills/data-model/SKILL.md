---
name: data-model
description: Mockingbird database schema, table-by-table invariants, and the rules that keep provenance total. Activate this skill whenever you are about to touch anything under `src-tauri/src/db/`, write a migration, design a new table, or reason about transcript lifecycle.
---

# Mockingbird data model

## Files of record

- `PLAN.md` Section 7 — full DDL for the three first-shipped migrations
- `src-tauri/src/db/migrations/00{1,2,3}_*.sql` — sealed after `phase-1-complete`
- `src-tauri/src/db/transcripts.rs` (and siblings) — typed CRUD

## Hard rules

1. **Raw is immutable.** A row in `transcripts` with `stage='raw'` is never
   updated. The corrected transcript is a *new row* with `stage='cleaned'`
   and a `parent_id` referencing the raw row. Hook `block-raw-transcript-edit`
   refuses code that emits `UPDATE transcripts ... stage='raw'`.

2. **Provenance is total.** Every cleaning event records:
   - `prompt_version` (string, semver-ish)
   - `dictionary_snapshot_id` (FK to a copy-on-write dictionary version)
   - `examples_set_id` (FK to the corrected examples used as few-shot)
   So later iterations can answer "why did v0.3 say *X* but v0.4 say *Y*?"

3. **Migrations are append-only after Phase 1.** Once tag
   `phase-1-complete` exists, files `001_initial.sql`, `002_provenance.sql`,
   `003_examples.sql` are frozen. Add `00N_*.sql` for new changes.
   Hook `block-migration-edit-after-phase-1` enforces.

4. **No destructive operations in migrations after Phase 1.** No
   `DROP COLUMN`, no `DROP TABLE`. If a column becomes vestigial, leave
   it and write a follow-up note in `docs/LESSONS.md`.

## Where new code goes

- New table → new migration + new module under `src-tauri/src/db/`
- New per-row metadata that's append-only → consider a side table with a
  FK rather than widening the parent
- Cleanup variants → new row, never `UPDATE`

## Common mistakes to refuse

- `UPDATE transcripts SET text=? WHERE id=?` on a raw row → wrong.
- Storing prompt text inline on the transcript row → wrong; store
  `prompt_version` FK.
- "Just this once" editing migration 001 to fix a typo → wrong;
  add a migration that overlays the fix.

## Cross-references

- Section 8 (cleanup pipeline) — how cleaned rows are produced
- Appendix A "Never do" — committed list
- ADRs 0001 (provenance design), 0002 (immutability) — write if missing
