-- 013_activity_blocks_fts.sql
-- Phase 10 Wave 3. Schema_version 12 → 13.
--
-- Adds two things needed by the Wave-3 summarization pipeline:
--
--   1. activity_blocks.label TEXT
--      Block rename has nowhere to write in the migration-012 schema.
--      ADR 0040 §Decision item 4 walks the alternatives; the answer
--      is "add a column" because reusing primary_app or
--      generated_abstract conflates provenance with user intent.
--
--   2. activity_blocks_fts  (FTS5 contentless shadow)
--      Search over the Block abstracts + user-set labels. Mirrors
--      the meeting_transcripts FTS pattern in migration 011: INSERT
--      / UPDATE / DELETE triggers keep the shadow in sync.
--
-- IMPORTANT: activity_events is intentionally NOT indexed (ADR 0040
-- §Decision item 5). FTS5 only covers Block-derived data.
--
-- The schema is purely additive. No prior migration is touched.

PRAGMA foreign_keys = ON;

BEGIN TRANSACTION;

-- =============================================================================
-- New column: activity_blocks.label
-- =============================================================================
ALTER TABLE activity_blocks ADD COLUMN label TEXT;

-- =============================================================================
-- FTS5 contentless shadow over (label, generated_abstract).
-- =============================================================================
CREATE VIRTUAL TABLE activity_blocks_fts USING fts5(
  label,
  generated_abstract,
  content=''                          -- contentless: we own the rowids
);

-- Backfill: at migration time, the table is empty (Wave 1B + 2 wrote
-- zero Block rows). The insert below is a no-op in practice, but it
-- guards against the case of running this migration on a hand-fixtured
-- DB that already has Block rows.
INSERT INTO activity_blocks_fts(rowid, label, generated_abstract)
SELECT rowid, label, COALESCE(generated_abstract, '')
FROM activity_blocks
WHERE generated_abstract IS NOT NULL OR label IS NOT NULL;

-- Keep the FTS shadow in lockstep with the source table.
CREATE TRIGGER activity_blocks_ai
AFTER INSERT ON activity_blocks
BEGIN
  INSERT INTO activity_blocks_fts(rowid, label, generated_abstract)
  VALUES (NEW.rowid, NEW.label, COALESCE(NEW.generated_abstract, ''));
END;

CREATE TRIGGER activity_blocks_au
AFTER UPDATE ON activity_blocks
BEGIN
  -- Delete-then-insert is the canonical contentless-FTS5 update
  -- pattern (the special-form delete uses the OLD values).
  INSERT INTO activity_blocks_fts(activity_blocks_fts, rowid, label, generated_abstract)
  VALUES('delete', OLD.rowid, OLD.label, COALESCE(OLD.generated_abstract, ''));
  INSERT INTO activity_blocks_fts(rowid, label, generated_abstract)
  VALUES (NEW.rowid, NEW.label, COALESCE(NEW.generated_abstract, ''));
END;

CREATE TRIGGER activity_blocks_ad
AFTER DELETE ON activity_blocks
BEGIN
  INSERT INTO activity_blocks_fts(activity_blocks_fts, rowid, label, generated_abstract)
  VALUES('delete', OLD.rowid, OLD.label, COALESCE(OLD.generated_abstract, ''));
END;

UPDATE schema_meta SET value = '13' WHERE key = 'schema_version';

COMMIT;
