-- ──────────────────────────────────────────────────────────────────────
-- Migration 003 — seed prompts + modes
--
-- Rationale: ships the three default cleanup modes (normal / verbose /
-- fragment) and their version-1 prompts. Prompt bodies are NOT
-- hand-pasted here; the runner substitutes the `__PROMPT_*_BODY__`
-- tokens below from the contents of src-tauri/src/cleanup/prompts/*.md
-- via include_str! (see db/prompt_loader.rs). That keeps the prompt
-- source of truth in Markdown files reviewable as prose, and avoids
-- editing SQL when prompts evolve.
--
-- Deviation from PLAN §7: PLAN uses hardcoded `prompt_id = 1, 2, 3` in
-- the modes INSERT, relying on auto-increment order on a fresh DB. We
-- use `(SELECT id FROM prompts WHERE mode_slug=? AND version=1)`
-- sub-selects instead — robust to future reorder and explicit about
-- the relationship. Documented in docs/LESSONS.md.
--
-- Rollback notes: this seed runs once on a fresh DB. The audit
-- triggers from migration 002 record the INSERTs in
-- `_history_prompts` / `_history_modes`, so `rollback_to_snapshot`
-- can undo them. Don't `DELETE FROM prompts` here — append a new
-- migration if a default needs to change post-seal.
-- ──────────────────────────────────────────────────────────────────────

BEGIN TRANSACTION;

INSERT INTO prompts (mode_slug, version, body) VALUES
  ('normal',   1, '__PROMPT_NORMAL_BODY__'),
  ('verbose',  1, '__PROMPT_VERBOSE_BODY__'),
  ('fragment', 1, '__PROMPT_FRAGMENT_BODY__');

INSERT INTO modes (slug, display_name, hotkey, provider, model_id, prompt_id, temperature, max_tokens) VALUES
  ('normal',   'Normal',   'Ctrl+Win',       'ollama', 'qwen2.5:3b-instruct-q4_K_M',
    (SELECT id FROM prompts WHERE mode_slug='normal'   AND version=1), 0.3, 2048),
  ('verbose',  'Verbose',  'Ctrl+Shift+Win', 'ollama', 'qwen2.5:3b-instruct-q4_K_M',
    (SELECT id FROM prompts WHERE mode_slug='verbose'  AND version=1), 0.3, 4096),
  ('fragment', 'Fragment', 'Ctrl+Alt+Win',   'ollama', 'gemma2:2b-instruct-q4_K_M',
    (SELECT id FROM prompts WHERE mode_slug='fragment' AND version=1), 0.2, 1024);

UPDATE schema_meta SET value = '3' WHERE key = 'schema_version';

COMMIT;
