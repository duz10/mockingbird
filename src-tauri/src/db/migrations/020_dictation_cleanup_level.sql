-- ──────────────────────────────────────────────────────────────────────
-- Migration 020 — ADR 0047 §Wave 2.1: dictation cleanup-level dial.
--
-- Ships the prompt body for the new `Medium` level of the
-- DictationCleanupLevel dial: an additive-only prompt that may insert
-- punctuation, paragraph breaks, and list structure but may not remove
-- or modify content words. Stored under a dedicated mode_slug
-- (`normal_additive`) so it is reachable independently of the existing
-- casual / normal / formal tone modes -- the level dial is ORTHOGONAL
-- to the tone dial (ADR 0047 §Wave 2.1 "tone becomes orthogonal").
--
-- Why a new mode_slug instead of bumping `normal` to v6:
--   The additive prompt is NOT the latest `normal` voice -- it is a
--   different mode of operation that the Medium level uses across all
--   three tone modes. Storing as `normal` v6 would silently change
--   default Normal-tone behaviour at level=High, which is the
--   "preserve existing behaviour at upgrade" invariant for this
--   migration.
--
-- Setting registration:
--   The new SettingKey::DictationCleanupLevel (and the existing
--   SettingKey::LlmSkipWordThreshold from Wave 2.2) live in the
--   typed-settings registry; their defaults are emitted by
--   `Settings::get` whenever the row is missing, so no INSERT into
--   `settings` is needed here. New users get the defaults from the
--   enum; existing users start with empty rows and the same defaults.
--
-- ADR 0008 compliance:
--   - INSERT append-only into `prompts`. No existing prompt row
--     touched. No mode rows repointed -- the additive prompt is
--     looked up by mode_slug directly from the Medium-level branch
--     in `cleanup/llm_cleaner.rs::run_cleanup`, not via the modes
--     table's prompt_id foreign key.
--   - schema_version bumped 19 -> 20.
-- ──────────────────────────────────────────────────────────────────────

BEGIN TRANSACTION;

-- The additive-only prompt body. The token is substituted by
-- `prompt_loader::substitute_prompt_bodies` before this SQL runs;
-- the actual body lives at `src-tauri/src/cleanup/prompts/normal_v6_additive.md`
-- (committed in commit 5daa977, ADR 0047 §Wave 2.1).
INSERT INTO prompts (mode_slug, version, body) VALUES
  ('normal_additive', 1, '__PROMPT_NORMAL_V6_ADDITIVE_BODY__');

UPDATE schema_meta SET value = '20' WHERE key = 'schema_version';

COMMIT;
