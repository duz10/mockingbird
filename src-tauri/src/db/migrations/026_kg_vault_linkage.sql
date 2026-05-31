-- ──────────────────────────────────────────────────────────────────────
-- Migration 026 -- KG Phase 1E Wave 1E.3 (ADR 0053 §D4 + §D5).
-- Schema_version 25 -> 26.
--
-- Bead: mb-k2pk. Charter ADR: 0053 (Proposed; flips Accepted at the
-- 1E.8 seal). Wave brief: docs/phases/phase-1e.md §"Wave 1E.3".
--
-- Adds the vault-linkage provenance columns to `sessions`. Populated
-- only for rows with `capture_kind IN ('kg-note', 'kg-note-text')`
-- and only after the two-phase commit (ADR 0053 §D4) has run end-
-- to-end. Every existing row + every standard `dictation` row stays
-- NULL on all three columns -- they have no vault projection.
--
-- Columns:
--
--   entry_id        TEXT NULL
--       The KG entry's stable UUID (full form, NOT the `__<id8>`
--       filename suffix). Set by the filing worker as step 5 of the
--       two-phase commit (post file-write). Survives renames in
--       Obsidian; the reverse-watcher (1E.5) keys off this when an
--       inbound file event needs to find its session row.
--
--   vault_path      TEXT NULL
--       Vault-relative POSIX-style path
--       (e.g. `Knowledge Graph/Entries/2026-06-04-foo__abc12345.md`).
--       Populated atomically with `entry_id` after the file write
--       succeeds. Vault-relative (not absolute) so a vault-root
--       relocation in Settings doesn't invalidate every row.
--
--   vault_file_hash TEXT NULL
--       Lowercase hex SHA-256 of the markdown bytes written to disk.
--       CRITICAL for 1E.5 loop-prevention per ADR 0053 §D5: the
--       reverse-watcher compares the hash of an event-fired file
--       against this column to decide "this is OUR write, ignore it"
--       vs "the user touched it, reconcile into DB FTS". Pre-recorded
--       BEFORE the OS file write so the watcher race window is
--       closed -- if the watcher fires on a file we just wrote, it
--       MUST already see our hash.
--
-- NO index added this wave: none of the new columns are query keys
-- yet. 1E.5 may add `idx_sessions_vault_path` for watcher event ->
-- session lookup when that surface lands.
--
-- Backfill: N/A. Existing dictation rows have no vault file. Pre-1E.3
-- KG-note rows (filed during Wave 1B/1C dev testing) were already
-- purged by migration 025's §D6 step, so the live DB has zero KG
-- filings that need linkage backfill. If new KG filings sneak in
-- between 025 ship and 026 ship (none expected; the gap was hours),
-- a P3 sweeper bead will pick them up.
--
-- Principle 1 (raw immutability): unaffected. transcripts unchanged.
-- The three new columns are derived-projection provenance, not raw
-- capture data, so the immutability invariant does not extend to
-- them (mirroring the migration 023 `edit_free_within_5min` and the
-- migration 022 `category` precedents -- both are derived columns
-- that get UPDATEd post-insert).
-- ──────────────────────────────────────────────────────────────────────

BEGIN TRANSACTION;

ALTER TABLE sessions
    ADD COLUMN entry_id TEXT;

ALTER TABLE sessions
    ADD COLUMN vault_path TEXT;

ALTER TABLE sessions
    ADD COLUMN vault_file_hash TEXT;

UPDATE schema_meta
    SET value = '26'
    WHERE key = 'schema_version';

COMMIT;
