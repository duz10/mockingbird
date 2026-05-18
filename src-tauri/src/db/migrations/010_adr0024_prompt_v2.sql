-- ──────────────────────────────────────────────────────────────────────
-- Migration 010 — ADR 0024 Wave C: prompt v2 bumps across all 3 modes
--
-- 2026-05-17 baseline mode-eval (`docs/cleanup/eval-baseline-*.md`)
-- revealed three concrete problems with the casual_v1 / normal_v4 /
-- formal_v1 prompts shipped in migration 008:
--
--   1. **Casual hallucination on long input.** Fixture 06_implicit_long
--      (an 8-item architecture description) made the 3B model regress
--      to the v1 prompt's "grocery list" few-shot example and emit
--      `"hey can you grab milk, eggs, and bread on the way home thanks"`
--      as the cleaned output. Zero must-preserve hits. Worst-possible
--      failure mode: user dictates technical content, gets unrelated
--      chat pasted into VS Code. See `docs/cleanup/eval-findings-v1.md`
--      § P1.
--
--   2. **Formal flattening of intensity markers.** "really really
--      important...fix it now" → "critically important...immediately"
--      semantically preserved but lexically dropped the speaker's
--      urgency cues. See findings § P3.
--
--   3. **Formal silent paraphrase of proper nouns** (risk surface; not
--      observed in baseline but the v1 prompt had no protection rule).
--
-- This migration ships:
--
--   * `casual_v2` — explicit anti-substitution rule, reordered examples
--     (short → long with the long-preservation case in the
--     most-influential slot), added a technical-content preservation
--     demonstration.
--   * `normal_v5` — minor revision of v4 adding the anti-substitution
--     rule + proper-noun-verbatim guidance. v4 was already at 96.8%
--     preservation in baseline; v5 targets parity-or-better.
--   * `formal_v2` — adds proper-noun-verbatim rule, emotional-intensity
--     preservation rule, a "when uncertain, copy" tiebreaker, and an
--     intensity-preserving example. Fixes a content-dropping example in
--     v1 ("I am compiling a list" silently dropped "of things").
--
--   * Mode repointing: each mode's `prompt_id` is updated to the new
--     latest version. Old v1/v4 rows are preserved (ADR 0008 append-
--     only) for historical session provenance.
--
--   * **Casual temperature drop: 0.4 → 0.2.** The v1 baseline showed
--     the model wandering at 0.4 under load; lowering the temperature
--     reduces creative drift without changing the prompt. The lower
--     register is still well within casual's stylistic range.
--
-- ADR 0008 compliance:
--   - INSERTs append-only. v1/v4 prompt rows are untouched.
--   - mode rows updated only at `prompt_id` and `temperature`
--     columns; the rest of the row (model_id, max_tokens, etc.) is
--     unchanged.
--   - schema_version bumped 9 → 10.
-- ──────────────────────────────────────────────────────────────────────

BEGIN TRANSACTION;

-- 1. New prompt rows. Versions are per-mode-slug monotonic: casual was
--    at v1, normal at v4, formal at v1.
INSERT INTO prompts (mode_slug, version, body) VALUES
  ('casual', 2, '__PROMPT_CASUAL_V2_BODY__'),
  ('normal', 5, '__PROMPT_NORMAL_V5_BODY__'),
  ('formal', 2, '__PROMPT_FORMAL_V2_BODY__');

-- 2. Repoint each mode at its new latest prompt.
UPDATE modes
   SET prompt_id = (SELECT id FROM prompts WHERE mode_slug='casual' AND version=2)
 WHERE slug = 'casual';

UPDATE modes
   SET prompt_id = (SELECT id FROM prompts WHERE mode_slug='normal' AND version=5)
 WHERE slug = 'normal';

UPDATE modes
   SET prompt_id = (SELECT id FROM prompts WHERE mode_slug='formal' AND version=2)
 WHERE slug = 'formal';

-- 3. Casual temperature drop — see header note for rationale.
UPDATE modes SET temperature = 0.2 WHERE slug = 'casual';

UPDATE schema_meta SET value = '10' WHERE key = 'schema_version';

COMMIT;
