-- ──────────────────────────────────────────────────────────────────────
-- Migration 027 — ADR 0065: tier-gated small-model Normal prompt.
--
-- Ships the prompt body for `normal_small` — a hardened variant of
-- normal@v5 used ONLY on the macOS RAM-aware downsize path (when an
-- 8 GB Mac substitutes the 3B for the 7B parity model AND the active
-- mode is Normal). It keeps Normal's behaviour (bulleted lists,
-- grammar fixes, paragraph breaks) but ports casual_v2's
-- leak-resistance recipe: distinctive `Speech:`/`Cleaned:` labels
-- instead of mirror-prone `**Input:**/**Output:**` markdown, an
-- explicit "never echo the example scaffolding" rule, and de-risked
-- example content (the leak-prone "3 PM tomorrow / bring the slides"
-- declarative example from v5 is removed entirely).
--
-- Why a new mode_slug instead of bumping `normal` to v6:
--   normal@v5 is the Windows / 7B parity prompt and MUST stay
--   byte-identical and never re-evaluated (user directive). The
--   small-model prompt is a SEPARATE, parallel slug (the same pattern
--   migration 020 used for `normal_additive`). The runtime selects it
--   via `LlmCleaner::prompt_mode_override`, set only at the single
--   `#[cfg(target_os = "macos")]` seam in
--   `dictation/runtime_cleaner.rs::make_default_cleaner`. On non-macOS
--   the override is never set, so `normal` continues to resolve to
--   normal@v5 unchanged.
--
-- ADR 0008 compliance:
--   - INSERT append-only into `prompts`. No existing prompt row
--     touched. No mode rows repointed — `normal_small` is looked up
--     by mode_slug directly from the override branch in
--     `cleanup/llm_cleaner.rs::run_cleanup`, not via the modes table's
--     prompt_id foreign key.
--   - schema_version bumped 26 -> 27.
-- ──────────────────────────────────────────────────────────────────────

BEGIN TRANSACTION;

-- The hardened small-model Normal prompt. The token is substituted by
-- `prompt_loader::substitute_prompt_bodies` before this SQL runs; the
-- actual body lives at `src-tauri/src/cleanup/prompts/normal_small.md`.
INSERT INTO prompts (mode_slug, version, body) VALUES
  ('normal_small', 1, '__PROMPT_NORMAL_SMALL_BODY__');

UPDATE schema_meta SET value = '27' WHERE key = 'schema_version';

COMMIT;
