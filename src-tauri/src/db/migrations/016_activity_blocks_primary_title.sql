-- 016_activity_blocks_primary_title.sql
-- Post-phase-10 hotfix. Schema_version 15 → 16.
--
-- Adds the missing `activity_blocks.primary_title` column. Migration
-- 012 created `activity_blocks` WITHOUT this column, but production
-- code in `activity/blocks_persist.rs` (INSERT + SELECT), `blocker.rs`,
-- `abstractor.rs`, `assembler.rs`, `export.rs`, and `pdf_export.rs`
-- has always referenced `primary_title` as a non-null `String` field
-- on the `ActivityBlockRow` struct.
--
-- WHY this slipped past the Phase 10 Wave 6 judges (LESSONS P2 +
-- new entry "Schema-vs-code drift slips past green judges when
-- `cargo test --release` is the gated-but-link-only step"):
--
--   * The cargo gate on this box runs `test --release --no-run`
--     because the test runner exits STATUS_ENTRYPOINT_NOT_FOUND. The
--     link-clean check proves Rust types + trait surfaces are sound
--     but never executes a single SQL statement.
--   * The Wave 6 `provenance-is-total` judge is a file-diff / static
--     reasoning check; it doesn't open SQLite and exercise the INSERT
--     / SELECT path against the migration-applied schema.
--   * The test at `db/migrations.rs::migration_013_adds_label_and_fts`
--     does INSERT a row with `primary_title` (line ~366) and would
--     have failed loudly — but it never ran on this box, and CI for
--     this project is the developer's box.
--
-- Live-fire failure (Dustin's smoke test, 2026-05-26):
--   `sqlite error: no such column: primary_title`
-- raised by `blocks_persist::list_blocks()` on the activity session
-- detail UI, blocking the entire activity summary render.
--
-- DEFAULT '' so existing rows (Wave 1B-5 shipped with 0 rows in
-- production, but a hand-fixtured DB would still apply cleanly).
-- The column is NOT NULL to match the Rust struct field's `String`
-- type — anything writing a Block now provides a real title from
-- the blocker / abstractor pipeline.
--
-- Purely additive ADD COLUMN. No prior migration is touched. No
-- triggers. Idempotent via the schema_version gate.

PRAGMA foreign_keys = ON;

BEGIN TRANSACTION;

ALTER TABLE activity_blocks
  ADD COLUMN primary_title TEXT NOT NULL DEFAULT '';

UPDATE schema_meta SET value = '16' WHERE key = 'schema_version';

COMMIT;
