-- ──────────────────────────────────────────────────────────────────────
-- Migration 025 -- KG Phase 1D Wave 1D.1 (ADR 0052). Schema_version 24 -> 25.
--
-- Bead: mb-pxzk. Charter ADR: 0052 (Proposed -- flips Accepted at the
-- 1D.6 seal). Wave brief: docs/phases/phase-1d.md §"Wave 1D.1".
--
-- Two responsibilities, one transaction:
--
-- (1) ADDITIVE schema — two new sessions columns + one index:
--     - sessions.capture_kind TEXT NOT NULL DEFAULT 'dictation'
--         Values: 'dictation' | 'kg-note' | 'kg-note-text'.
--         Drives the dictation-tail KG filing source-gate
--         (ADR 0052 §D1). The Wave 1D.1 brief explicitly authorized
--         picking between "reuse existing column" vs "add new
--         column" (ADR 0052 §D1); this migration picks NEW because
--         the pre-existing `sessions.source` column (migration 018,
--         ADR 0046) is the orthogonal audio-origin axis ('desktop'
--         / 'desktop-import' / 'mobile-inbox') consumed by the
--         Iter 2 export-job filter. Mixing the KG capture-intent
--         axis into it would conflate two unrelated dimensions in
--         the same column; the durable rule is "one column, one
--         meaning". No CHECK constraint on the value set (matches
--         migration 018's parse-in-Rust pattern; adding a value
--         later is a Rust-only change).
--     - sessions.category TEXT NULL
--         Consumes mb-oji5 (deferred during 1C → 1D re-scope).
--         Populated by the classify pass at filing time; queryable
--         for Phase 1C's deferred category-axis retrieval. NULL
--         on every existing row + on every dictation-source row
--         (only kg-note filings produce a classify result).
--     - idx_sessions_capture_kind ON sessions(capture_kind, started_at DESC)
--         Composite for the dashboard's "recent KG audio captures"
--         query in Wave 1D.2 (filter by capture_kind='kg-note',
--         order by started_at DESC). Cheap-to-add-now per the
--         migration 018 precedent. No index on `category` -- v1
--         cardinality is 3 (Personal / Professional / Objective);
--         B-tree index is overkill.
--
-- (2) PURGE step (ADR 0052 §D6). Wave 1D.0's sqlite probe found
--     non-zero pre-existing KG state on Dustin's box (3 entities, 11
--     tag mentions, 3 entity mentions, 2 done filings on entries
--     128 + 129 from 2026-05-30 dev test toggles). Per the trigger-
--     direction drift the new ADR fixes, these were never meant to
--     be in the graph. Purging in the same transaction guarantees
--     that any DB that's seen 024-but-not-025 finishes the upgrade
--     either fully purged + fully on the new schema, or not at all.
--     Idempotent: the standard schema_version gate around this file
--     means a re-run is a no-op even if the user manually ran
--     INSERTs against kg_* tables after 025 first landed.
--
--     The DELETE ordering respects FK arrows: mentions reference
--     entities; queue references nothing kg_*. Children first, then
--     parents.
--
--     The kg_canonical_tags table is intentionally NOT purged —
--     migration 024 inserts zero rows into it (the v1.1 closed-vocab
--     workflow is inert), and any future seed rows would survive
--     across this migration unchanged. Verified post-Wave-1D.0:
--     SELECT COUNT(*) FROM kg_canonical_tags = 0.
--
--     The kg_graph_enabled setting is reset to 'false' so a user who
--     enabled it during dev testing isn't surprised by the new
--     source-gate behaviour silently changing their pipeline.
--
-- Principle 1 (raw immutability): unaffected. transcripts/sessions
-- raw layer is untouched; only kg_* derived rows are purged.
-- ADR 0052 §D6 explicitly authorizes this purge as in-scope —
-- dev-test data, not user-meaningful.
--
-- Migration 024's two immutability triggers (trg_kg_entity_mentions_no_update
-- and trg_kg_tag_mentions_no_update) sit on UPDATE; DELETE is allowed.
-- ──────────────────────────────────────────────────────────────────────

BEGIN TRANSACTION;

-- (1) Additive schema.

ALTER TABLE sessions
    ADD COLUMN capture_kind TEXT NOT NULL DEFAULT 'dictation';

ALTER TABLE sessions
    ADD COLUMN category TEXT;

CREATE INDEX IF NOT EXISTS idx_sessions_capture_kind
    ON sessions(capture_kind, started_at DESC);

-- (2) Purge step. Children first.

DELETE FROM kg_entity_mentions;
DELETE FROM kg_tag_mentions;
DELETE FROM kg_filing_queue;
DELETE FROM kg_entities;

-- Reset the toggle so users who enabled the graph during dev testing
-- pick up the new source-gate semantics intentionally (via a fresh
-- toggle flip) rather than silently inheriting them.
UPDATE settings
    SET value = 'false'
    WHERE key = 'kg_graph_enabled';

UPDATE schema_meta
    SET value = '25'
    WHERE key = 'schema_version';

COMMIT;
