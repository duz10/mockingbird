-- ──────────────────────────────────────────────────────────────────────
-- Migration 008 — Wave 2 of ADR 0022: three focused transcription modes
--
-- Background. 2026-05-17 smoketest showed `normal_v3` dropping
-- introductory preamble on a multi-sentence list utterance (the Santa
-- "making a list and checking it twice" test). Root cause: 3B-q4
-- model + 4.8 KB monolithic prompt = rules buried below the
-- attention budget, so the model defaults to its summarisation
-- prior. See ADR 0022 (Wave 2) and LESSONS 2026-05-17
-- phase5-postship-8 + phase5-postship-9.
--
-- This migration ships:
--
--   1. **Three focused prompts** (`casual_v1`, `normal_v4`,
--      `formal_v1`), each ~1.5 KB, with a NON-NEGOTIABLE
--      "PRESERVE EVERY SENTENCE" rule front-loaded. The
--      preprocessor (Wave 1) already does fillers/punctuation/
--      capitalisation, so these prompts focus only on style
--      transformation + list rendering decisions.
--
--   2. **Two new modes** (`casual`, `formal`) added to the
--      `modes` table. `normal` is repointed at `normal_v4`.
--      `verbose` and `fragment` are marked `enabled = 0` —
--      rows preserved for provenance (any old session row that
--      references them still resolves correctly).
--
--   3. **Smart-default models** for the user's hardware (RTX
--      2060 / 6 GB VRAM, Whisper-large-turbo also resident):
--        - casual  → qwen2.5:3b-instruct-q4_K_M  (fast, ~2-3s)
--        - normal  → qwen2.5:7b-instruct-q4_K_M  (reliable, ~3-4s)
--        - formal  → qwen2.5:7b-instruct-q4_K_M  (reliable + polish)
--      Temperature dropped to 0.1 for normal+formal (content
--      preservation > creativity); 0.4 for casual (light
--      register adjustment OK).
--
--   4. **Active-mode rescue**: any user whose
--      `dictation.active_mode_slug` setting points at the
--      now-disabled `verbose` or `fragment` is migrated to
--      `normal` so the next dictation doesn't silently use a
--      disabled mode.
--
-- ADR 0008 compliance:
--   - INSERTs are append-only. The old `normal_v3` prompt row
--     is left in place; only `modes.normal.prompt_id` is updated
--     to point at `normal_v4`.
--   - `verbose` / `fragment` rows are NOT deleted — `enabled = 0`
--     is the documented soft-disable mechanism (migration 005's
--     `enabled` column was introduced for exactly this purpose).
--   - A future Wave 3 prompt iteration ships as migration 009.
--     Never edit a past migration file.
--
-- Body source: `src-tauri/src/cleanup/prompts/{casual_v1,normal_v4,
-- formal_v1}.md`, substituted at runtime via `prompt_loader.rs`.
-- ──────────────────────────────────────────────────────────────────────

BEGIN TRANSACTION;

-- 1. New prompt rows. mode_slug is the dictation mode they target;
--    version is per-mode-slug monotonic.
INSERT INTO prompts (mode_slug, version, body) VALUES
  ('casual', 1, '__PROMPT_CASUAL_V1_BODY__'),
  ('normal', 4, '__PROMPT_NORMAL_V4_BODY__'),
  ('formal', 1, '__PROMPT_FORMAL_V1_BODY__');

-- 2a. New `casual` mode. Lower max_tokens because casual output is
--     usually short. Hotkey is a placeholder until Phase 5's chord
--     picker ships — user can override via Modes editor.
INSERT INTO modes (slug, display_name, hotkey, provider, model_id,
                   prompt_id, temperature, max_tokens, enabled) VALUES
  ('casual', 'Casual', 'Ctrl+Win+C', 'ollama',
   'qwen2.5:3b-instruct-q4_K_M',
   (SELECT id FROM prompts WHERE mode_slug='casual' AND version=1),
   0.4, 1024, 1);

-- 2b. New `formal` mode. Higher max_tokens because formal output
--     often expands (filler → polished prose, structure additions).
INSERT INTO modes (slug, display_name, hotkey, provider, model_id,
                   prompt_id, temperature, max_tokens, enabled) VALUES
  ('formal', 'Formal', 'Ctrl+Win+F', 'ollama',
   'qwen2.5:7b-instruct-q4_K_M',
   (SELECT id FROM prompts WHERE mode_slug='formal' AND version=1),
   0.1, 4096, 1);

-- 3. Repoint `normal` at the new v4 prompt. Bump model to 7B and
--    drop temperature to 0.1 for content-fidelity. The user installed
--    qwen2.5:7b in this iteration specifically to back this default.
UPDATE modes
   SET prompt_id   = (SELECT id FROM prompts WHERE mode_slug='normal' AND version=4),
       model_id    = 'qwen2.5:7b-instruct-q4_K_M',
       temperature = 0.1
 WHERE slug = 'normal';

-- 4. Soft-disable `verbose` + `fragment`. They're superseded by
--    `casual`/`normal`/`formal` but the rows survive so any
--    historical session row that referenced them still resolves.
UPDATE modes
   SET enabled = 0
 WHERE slug IN ('verbose', 'fragment');

-- 5. Active-mode rescue. If a user had `verbose` or `fragment`
--    selected, point them at `normal` so the next dictation works.
--    Done as UPSERT so it works whether or not the setting row
--    exists (it always does post-Phase-4, but defence-in-depth).
INSERT INTO settings (key, value) VALUES ('dictation.active_mode_slug', 'normal')
ON CONFLICT(key) DO UPDATE SET value = 'normal'
WHERE value IN ('verbose', 'fragment');

UPDATE schema_meta SET value = '8' WHERE key = 'schema_version';

COMMIT;
