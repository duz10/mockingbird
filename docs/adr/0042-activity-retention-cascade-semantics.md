# ADR 0042 — Activity retention cascade semantics

- **Status:** Accepted
- **Date:** 2026-05-26
- **Phase:** 10 (Wave 5 — Hardening)
- **Author:** code-puppy (Bernard) on behalf of Dustin
- **Supersedes:** none
- **Superseded by:** none

## Context

Phase 10 Wave 5 ships a configurable retention TTL on the activity-capture
schema. The user can set per-table TTLs:

- `activity_events` (RAW timeline data — Principle 1)
- `activity_transcript_segments` (Layer-2 audio transcription)
- `activity_blocks` (DERIVED Layer-3 abstracts)

Defaults ship as `0 = forever` (privacy-by-default-but-don't-purge-without-
permission). The user opts in to TTLs via the Activity Settings tab.

The non-trivial decision: **what happens to a Block when the raw events
that produced it are purged for retention?** Three reasonable answers
were on the table (verbatim from Wave 5 kickoff):

- **(a) Block survives.** `source_event_ids` becomes stale but
  `generated_abstract` is preserved — it's already a summary, the raw
  events were just provenance.
- **(b) Block deleted alongside its events.** Raw retention propagates
  transitively.
- **(c) Block records a `raw_purged_at` timestamp + UI shows
  "summary; raw events purged for retention" with no regen possible.**

## Decision

**Option (c) — retention propagates to a `raw_events_purged_at`
breadcrumb on the Block, but the Block row + its `generated_abstract`
survive. Regenerate-summary is disabled in the UI when this column is
non-null.**

This is (a) and (c) hybridized: the *practical* behavior of (a) — the
abstract survives — plus the *honesty* of (c) — the user can see the
underlying provenance has been pruned and re-summarization is no longer
possible from raw.

If the user explicitly sets a `blocks_ttl_days` (separate setting, also
`0 = forever` by default), Blocks AGE OUT BY THEMSELVES against their
own `started_at` timestamp. That's a separate axis — it does NOT
trigger when raw events under them are pruned.

If the user deletes a **session** via the existing
`activity_delete_session` IPC, the cascade is the existing FK-CASCADE
chain: session → events + blocks + transcript_segments all go. Retention
sweep DOES NOT delete session rows; only their child rows.

## Schema effect

```sql
ALTER TABLE activity_blocks ADD COLUMN raw_events_purged_at INTEGER;
```

Nullable, NULL = raw still present. Stamped by the retention sweep
in a single UPDATE pass *before* the DELETE pass against events (so
in-flight readers see the breadcrumb appear before the rows
underneath disappear).

## Rationale

1. **Abstracts are the user-valuable artefact.** Raw events are
   provenance. Throwing the abstract away because its raw aged out
   would be a UX regression for "I purge my events monthly but want
   to see what I worked on last summer".
2. **Honesty matters.** Showing the abstract without acknowledging
   the raw is gone would mislead users about what they can
   re-derive. The breadcrumb makes it visible.
3. **Principle 1 holds.** The sweep DELETEs raw rows; it does NOT
   UPDATE them. The Block UPDATE that stamps `raw_events_purged_at`
   is against the derived `activity_blocks` table, not `activity_events`.

## Sweep order (binding)

For each session whose `activity_events` rows are about to be deleted:

1. UPDATE `activity_blocks` SET `raw_events_purged_at = <now_ms>` WHERE
   `session_id = ?` AND `raw_events_purged_at IS NULL`.
2. DELETE FROM `activity_events` WHERE `session_id = ?` AND
   `ts < <cutoff_ms>`.
3. (Separate sweep pass) DELETE FROM `activity_transcript_segments`
   WHERE `ts < <cutoff_ms>`.
4. (Separate sweep pass) DELETE FROM `activity_blocks` WHERE
   `started_at < <blocks_cutoff_ms>` (only if `blocks_ttl_days > 0`).

The sweep is wrapped in a single transaction so partial failures don't
leave breadcrumbs without the underlying delete.

## Alternatives considered

- **(a) pure** — abstract survives silently. Rejected: surprises the
  user when "regenerate summary" is greyed out with no explanation.
- **(b)** — Block deleted alongside. Rejected: the abstract is the
  user-facing artefact; losing it because its raw aged out is the
  exact UX regression we want to avoid.
- **(c) pure** (regen disabled but raw breadcrumb without abstract
  preservation). Rejected: this is just (b) with extra steps.

## UI surface

When `raw_events_purged_at IS NOT NULL` on a Block:

- Activity Detail page Block card shows a small ⓘ "raw events purged
  YYYY-MM-DD" annotation.
- "Regenerate summary" action is disabled with the same tooltip.
- Drill-down event timeline shows an empty state with the same
  annotation instead of a stale list.

## Test plan

- Pure-Rust `retention::sweep_with_cascade` unit test (throwaway crate):
  insert session + 5 events + 1 Block referencing those events; sweep
  with cutoff after 3 events; assert: Block survives, `raw_events_purged_at`
  is stamped, only 3 events remain.
- Idempotency test: re-run sweep with same cutoff; assert
  `raw_events_purged_at` unchanged + no row count delta.
- Principle-1 invariant test: assert the sweep issues no UPDATE
  against `activity_events` (covered by the existing trigger plus
  this ADR's sweep-order contract).
