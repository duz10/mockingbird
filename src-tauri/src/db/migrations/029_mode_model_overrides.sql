-- ──────────────────────────────────────────────────────────────────────
-- Migration 029 — ADR 0066: per-mode user model-override layer.
--
-- Adds `mode_model_overrides` — a thin side table that holds an OPTIONAL
-- user-pinned cleanup model per mode, kept DELIBERATELY SEPARATE from the
-- `modes` table so the shipped (Windows-parity) `modes.model_id` defaults
-- stay IMMUTABLE.
--
-- Semantics (resolved at dictation time + for the Modes-screen display):
--   * No row for a mode  = "Auto" → use `modes.model_id`, then apply the
--     existing RAM-aware substitution on macOS / no-op everywhere else.
--     THIS IS TODAY'S BEHAVIOUR — unchanged on every platform.
--   * Row present         = use that exact `model_id`, with NO RAM-aware
--     substitution (the user has explicitly pinned a model).
--
-- WHY a side table (not a `modes` column / not reusing `modes.model_id`):
--   On macOS the modes-table model is the parity 7B default that the
--   RAM-aware layer (ADR 0064) substitutes to a 3B at runtime. Writing the
--   user's pick back into `modes.model_id` would (a) lose the "Auto" vs
--   "pinned" distinction and (b) mutate the parity default that the
--   Windows build depends on. A separate, additive table keeps "Auto =
--   today's behaviour" exact and lets revert-to-Auto be a simple DELETE.
--
-- macOS-scoped in practice: the enhanced Modes control that WRITES this
-- table is isMac-gated in the UI, and the dictation-time read is behind
-- `#[cfg(target_os = "macos")]`. On Windows the table simply stays empty
-- and the cleanup path is byte-identical.
--
-- ADR 0008 compliance: append-only, additive CREATE TABLE; no existing
-- table touched, no triggers added (trigger count unchanged).
--
-- schema_version bumped 28 -> 29.
-- ──────────────────────────────────────────────────────────────────────

BEGIN TRANSACTION;

CREATE TABLE IF NOT EXISTS mode_model_overrides (
  mode_slug  TEXT PRIMARY KEY,
  model_id   TEXT NOT NULL,
  created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
);

UPDATE schema_meta SET value = '29' WHERE key = 'schema_version';

COMMIT;
