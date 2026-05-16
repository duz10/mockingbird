# ADR-0010: Raw-transcript immutability

- **Status:** Accepted
- **Date:** 2026-05-15
- **Deciders:** Dustin, code-puppy-adeb7b

## Context

PLAN §7 and §12.3 declare that rows in `transcripts` with
`stage='raw'` are **immutable**. This is the foundational invariant
that makes the cleanup pipeline, learning loop, and eval framework
all reproducible. Without it, "what did the user actually say at
T0?" becomes unanswerable after any later "correction".

## Decision

Once written, a row in `transcripts` where `stage='raw'` is never
UPDATEd. Corrections, cleanups, re-runs, and human edits ALL produce
**new rows** with `stage='cleaned'` (or future stages) and a
`parent_id` FK back to the raw row.

Enforcement:

1. **Schema** (Phase 1): SQLite trigger on `transcripts` that raises
   `RAISE(ABORT, 'raw transcripts are immutable')` for any
   `UPDATE OF * WHEN stage='raw'`.
2. **Hook** (live): `block-raw-transcript-edit` scans Rust source
   for `UPDATE transcripts ... stage='raw'` patterns at write time
   and refuses the change.
3. **Judge** (per iteration): the `invariants` judge re-verifies
   the schema trigger is intact.

## Consequences

- **Positive:** total provenance. Every claim of the form "the user
  said X" is reproducible from immutable raw data.
- **Negative:** more rows per session (raw + cleaned + per-cleaning
  variants). SQLite handles this fine; storage cost is tiny next to
  the audio files we don't ship long-term anyway.
- **Neutral:** the `data-model` skill is the binding reference.

## Alternatives considered

- **Soft-delete + restore:** loses the "what did we see at T0" answer
  if the soft-delete column itself is mutated.
- **Append-only log with materialized current-row view:** essentially
  what we're doing, but with extra plumbing. Our approach is the
  simpler "raw is one row, cleaned is more rows" expression of that.

## Cross-references

- PLAN §7 (schema), §12.3 (invariant)
- `.code_puppy/skills/data-model/SKILL.md`
- `scripts/hooks/block-raw-transcript-edit.py`
- AGENTS.md "Principles" #1
