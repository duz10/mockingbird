-- Migration 004 — injection_status column on sessions.
--
-- Wave 4 of Phase 3 introduces per-row injection outcome persistence.
-- This is append-only-to-the-migration-list (ADR 0010 + PLAN §12 #5
-- bind us to never EDIT existing migrations; adding new ones is fine).
--
-- The column is NULLABLE so:
--   1. Pre-Phase-3 session rows (none on disk yet, but the schema
--      stays forwards-compatible for users who upgrade across phases).
--   2. Sessions currently in-flight (between insert + processing
--      completion) have a sensible NULL state — not yet decided.
--
-- Canonical values (string match to InjectionOutcome::as_db_str):
--   'ok'
--   'ok_clipboard_not_restored'
--   'aborted_secure'
--   'aborted_user_opt_out'
--   'aborted_focus_changed'
--   'failed_clipboard_locked'
--   'failed_send_input'

ALTER TABLE sessions ADD COLUMN injection_status TEXT;

-- Bump schema version.
UPDATE schema_meta SET value = '4' WHERE key = 'schema_version';
