-- ----------------------------------------------------------------------
-- Migration 030 -- ADR 0067: per-mode user PROMPT-override layer.
--
-- Adds `mode_prompt_overrides` -- a thin side table holding an OPTIONAL
-- user-authored cleanup prompt body per mode, kept DELIBERATELY SEPARATE
-- from the immutable, migration-seeded `prompts` table so the shipped
-- (Windows-parity) prompt defaults stay the source of truth and remain
-- byte-identical. Mirrors the model-override side table from migration
-- 029 (ADR 0066) one-for-one.
--
-- Semantics (resolved at dictation time + for the Modes-screen editor):
--   * No row for a mode = use the shipped default: the latest version in
--     `prompts` for that mode, then the existing macOS tier substitution
--     (e.g. `normal_small`) on a downsized model. THIS IS TODAY'S
--     BEHAVIOUR -- unchanged on every platform.
--   * Row present = use that exact `prompt_body` VERBATIM, with NO tier
--     substitution (an explicit user prompt beats the heuristic).
--
-- Precedence (documented in commands/prompt_override.rs +
-- cleanup/llm_cleaner.rs): user prompt override > macOS tier
-- substitution (normal_small) > mode default.
--
-- WHY a side table (not a `prompts` row / not mutating `prompts`):
--   `prompts` is append-only + immutable per ADR 0008 prompt versioning;
--   the app never writes it at runtime. A separate, additive table keeps
--   "no override = shipped default" exact and lets revert-to-default be a
--   simple DELETE, while the shipped defaults stay the SoT for parity.
--
-- macOS-scoped in practice: the editor that WRITES this table is
-- isMac-gated in the UI, and the dictation-time read is injected only at
-- the macOS effective-model seam (behind `#[cfg(target_os = "macos")]`).
-- On Windows the table simply stays empty and the cleanup path is
-- byte-identical.
--
-- ADR 0008 compliance: append-only, additive CREATE TABLE; no existing
-- table touched, no triggers added (trigger count unchanged).
--
-- schema_version bumped 29 -> 30.
-- ----------------------------------------------------------------------

BEGIN TRANSACTION;

CREATE TABLE IF NOT EXISTS mode_prompt_overrides (
  mode_slug   TEXT PRIMARY KEY,
  prompt_body TEXT NOT NULL,
  created_at  INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
  updated_at  INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
);

UPDATE schema_meta SET value = '30' WHERE key = 'schema_version';

COMMIT;
