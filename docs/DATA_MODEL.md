# Data model — reference

> **Canonical source:** `PLAN-mockingbird-v2.md` Section 7. This file
> is a convenience copy for offline reference. If the two ever
> disagree, **PLAN.md wins** and this file gets a follow-up sync PR.

This document mirrors PLAN §7 structure: migration 001 (core tables +
FTS5), migration 002 (audit triggers), migration 003 (seed modes +
prompts). For the verbatim DDL and rationale, read PLAN §7 directly.

## Where the database lives

`%APPDATA%\Mockingbird\mockingbird.db` (Windows v1; macOS Phase 9
will resolve via `~/Library/Application Support/Mockingbird/`).

- **WAL mode** (`PRAGMA journal_mode = WAL`).
- **Foreign keys ON** (`PRAGMA foreign_keys = ON`).
- Schema version tracked in `schema_meta` (key=`schema_version`,
  value=integer-as-text).

## The four audited tables

| Table             | Purpose                                                    |
|-------------------|------------------------------------------------------------|
| `modes`           | Default / Email / Code / Casual configuration              |
| `prompts`         | Versioned cleanup-LLM prompt bodies (one per mode/version) |
| `dictionary`      | User vocabulary substitutions (Bernarrd → Bernard)         |
| `style_examples`  | Few-shot examples for the cleanup prompt                   |

Each gets a parallel `_history_<name>` table populated by migration 002
triggers, with `op_type` (INSERT/UPDATE/DELETE), `op_at`, and a JSON
projection of the row before/after.

## The provenance tables

| Table                    | Role                                                  |
|--------------------------|-------------------------------------------------------|
| `sessions`               | One dictation session. FKs into `modes`, `prompts`, `dictionary_snapshots`, `example_sets`. All FKs are NOT NULL — provenance is total. |
| `transcripts`            | The raw/cleaned/final text. Raw rows are **immutable** (ADR 0010). |
| `dictionary_snapshots`   | Copy-on-write snapshot of the dictionary at session time. |
| `example_sets`           | Pointer to the few-shot set used at session time.     |
| `corrections`            | User-supplied corrections (Phase 6+).                 |

## FTS5

Migration 001 creates `transcripts_fts` virtual table + INSERT/DELETE
triggers that mirror the `text` column. The history viewer (Phase 6)
uses this for sub-second search.

## Settings, learning runs

- `settings` — typed key/value, JSON-encoded values (Phase-1 settings
  module is the typed facade).
- `learning_runs` — one row per Phase-8 nightly run, with metrics +
  rollback pointer.

## Invariants (binding)

1. **Raw transcripts immutable** — ADR 0010. Enforced by:
   - SQLite trigger (`RAISE(ABORT)` on UPDATE of a raw row)
   - hook `block-raw-transcript-edit` (refuses risky source patterns)
   - judge `raw-immutability-static-check` (grep audit at phase exit)
2. **Provenance total** — every session row's FKs are NOT NULL.
3. **Migrations append-only after Phase 1** — hook
   `block-migration-edit-after-phase-1` enforces post-tag.

## How to read PLAN §7 vs this doc

When implementing a new feature that touches the schema, the order of
reading is:

1. PLAN §7 — the canonical DDL
2. `.code_puppy/skills/data-model/SKILL.md` — the binding rules
3. This file — quick reference for shape and table names
4. The actual migration .sql files (post-Phase-1) for what shipped

When PLAN §7 and a shipped migration disagree, the migration wins for
the currently-shipped behavior; PLAN gets updated to reconcile via PR.
