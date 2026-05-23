-- 018_session_source.sql
--
-- mb-jqhw / ADR 0046: track WHERE the audio for each dictation session came
-- from. The PTT happy path stays default (sessions land with 'desktop'); the
-- "+ Audio file" desktop import button (mb-7vyz) writes 'desktop-import';
-- the future iOS Shortcut courier flow (Iter 3) writes 'mobile-inbox'.
--
-- This is the durable "where did this audio originate" provenance bit. The
-- existing sessions.start_mode column (migration 017) tracks whether the
-- session was triggered by the PTT key or by an in-app Start button; these
-- two dimensions are orthogonal — a 'desktop-import' session is by
-- definition 'in_app' (no PTT was held), but a 'desktop' session may be
-- either 'ptt' or 'in_app'.
--
-- Backfill: every row predating this migration came from the live mic
-- (no headless ingest existed), so DEFAULT 'desktop' is exactly correct.
-- New rows from the existing PTT + in-app Start paths are written
-- explicitly with 'desktop' (see dictation::insert_session_row* updates).
--
-- The CREATE INDEX is for the Iter 2 export job, which will filter
-- sessions by source to project only the originated-on-this-desktop
-- subset out to the synced vault. Cheap to add now (one extra B-tree
-- per ~30-byte row); rebuilding the index later in a non-empty DB
-- would be a noticeable hitch for users with thousands of sessions.
--
-- Purely additive ADD COLUMN; the audit triggers in migration 002 do not
-- cover the `sessions` table, so no trigger update is needed.

ALTER TABLE sessions ADD COLUMN source TEXT NOT NULL DEFAULT 'desktop';
CREATE INDEX IF NOT EXISTS idx_sessions_source ON sessions(source);

UPDATE schema_meta SET value = '18' WHERE key = 'schema_version';
