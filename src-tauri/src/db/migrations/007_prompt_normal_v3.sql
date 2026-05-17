-- ──────────────────────────────────────────────────────────────────────
-- Migration 007 — normal-mode prompt v3 (preserve intro framing)
--
-- Why: 2026-05-17 smoketest feedback. v2 told the LLM to render
-- bullets for "make a list" cues — which it did — but also stripped
-- introductory framing like "I'm going to put together a list of
-- keyboard supplies". User got bare bullets with no topic context.
-- v3 keeps the v2 cue-detection rules but adds an explicit rule:
-- when the speaker named the list ("here's a list of X"), keep that
-- as a lead-in line before the bullets.
--
-- ADR 0008 compliance:
--   - INSERT a new prompts row (mode_slug='normal', version=3), do
--     NOT UPDATE the v2 row. v1 and v2 both stay addressable for any
--     session row that points at them.
--   - UPDATE modes.normal.prompt_id to point at v3 so new dictations
--     pick it up automatically.
--   - Append-only-migration invariant: a future v4 will be migration
--     008. Never edit a past migration file.
--
-- Body source: src-tauri/src/cleanup/prompts/normal_v3.md, substituted
-- at runtime via prompt_loader.rs (the `__PROMPT_*_BODY__` token on
-- the INSERT line below). See migration 006's warning block for why
-- the comment uses the asterisk-prose form: writing the literal token
-- in a `--` comment would inject the prompt body into the comment and
-- crash the SQL parser. Don't do that.
-- ──────────────────────────────────────────────────────────────────────

BEGIN TRANSACTION;

INSERT INTO prompts (mode_slug, version, body) VALUES
  ('normal', 3, '__PROMPT_NORMAL_V3_BODY__');

UPDATE modes
   SET prompt_id = (SELECT id FROM prompts WHERE mode_slug='normal' AND version=3)
 WHERE slug = 'normal';

UPDATE schema_meta SET value = '7' WHERE key = 'schema_version';

COMMIT;
