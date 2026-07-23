-- ──────────────────────────────────────────────────────────────────────
-- Migration 028 — ADR 0065 v2: normal_small hardening + truthful prompt
-- provenance.
--
-- Two append-only changes, both safe on existing rows:
--
--   1. `sessions.effective_prompt_label TEXT` — the ACTUAL prompt
--      (slug + version) the cleaner resolved for this session, e.g.
--      "normal_small v2". NULL on every historical row and on every
--      row where no LLM prompt was resolved (level=None/Light, or the
--      skip-short-utterance preprocessor-only path). The dictation
--      Metadata "Prompt" field prefers this when present and falls back
--      to the mode's canonical prompt version otherwise.
--
--      WHY: pre-028 the Metadata "Prompt: vN" was derived purely from
--      `sessions.prompt_id` → `modes.prompt_id` (the mode's CANONICAL
--      prompt). On a downsized 8 GB Mac the cleaner actually runs the
--      `normal_small` override (ADR 0065) but `sessions.prompt_id` still
--      points at normal@v5 — so the UI displayed "Prompt: v5" while
--      normal_small was the prompt that genuinely ran. That display bug
--      masked the truth (see kennel drawer 91 / session-12 diagnosis).
--      This column lets the UI report the prompt that was REALLY used.
--
--   2. `normal_small` v2 prompt body. v1 (migration 027) shipped the
--      leak-resistance recipe but the weak 3B still (a) added a
--      "Here's your cleaned transcript:" preamble and (b) dropped the
--      speaker's opening sentences (summarized to just the list). v2
--      makes CONTENT FIDELITY rule zero, adds an explicit no-preamble
--      rule with named forbidden openers, and adds a prose+list example
--      demonstrating preservation of throwaway preamble. Append-only:
--      v1 stays addressable for historical session rows;
--      `get_latest_for_mode('normal_small')` now returns v2.
--
-- ADR 0008 compliance: INSERT append-only into `prompts`; ADD COLUMN is
-- additive. No existing prompt row touched; no mode rows repointed.
-- The 7B / Windows path never resolves `normal_small`, so it stays
-- byte-identical and normal@v5 is never re-evaluated.
--
-- schema_version bumped 27 -> 28.
-- ──────────────────────────────────────────────────────────────────────

BEGIN TRANSACTION;

ALTER TABLE sessions ADD COLUMN effective_prompt_label TEXT;

-- The hardened small-model Normal prompt, v2. The token is substituted
-- by `prompt_loader::substitute_prompt_bodies` before this SQL runs; the
-- actual body lives at `src-tauri/src/cleanup/prompts/normal_small_v2.md`.
INSERT INTO prompts (mode_slug, version, body) VALUES
  ('normal_small', 2, '__PROMPT_NORMAL_SMALL_V2_BODY__');

UPDATE schema_meta SET value = '28' WHERE key = 'schema_version';

COMMIT;
