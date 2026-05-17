-- ──────────────────────────────────────────────────────────────────────
-- Migration 005 — AI command modes (WisprFlow-parity, local-only)
--
-- Adds three new modes that hijack the cleanup-LLM stage for
-- non-dictation tasks. Same provider abstraction, zero new infra —
-- just different prompts pointing at the same model.
--
--   - rewrite   → "rephrase this same idea differently"
--   - expand    → "elaborate; add detail; same voice"
--   - summarize → "compress to the essential points"
--
-- These ship disabled by default (`enabled = 0`); the user opts in
-- via Settings → Modes. Hotkeys are placeholders (Ctrl+Win+R/E/S);
-- Phase 5 wires the per-mode hotkey picker.
--
-- Provider defaults to ollama / qwen2.5:3b. The user can override
-- per-mode (settings UI Phase 5+).
--
-- Why migration not seed of 003: ADR 0010 forbids editing existing
-- migrations after phase-1-complete. This is the canonical append.
-- Prompt body uses the same `__PROMPT_*_BODY__` token mechanism
-- as 003; the runner (db/prompt_loader.rs) is extended in lockstep.
-- ──────────────────────────────────────────────────────────────────────

BEGIN TRANSACTION;

INSERT INTO prompts (mode_slug, version, body) VALUES
  ('rewrite',   1, '__PROMPT_REWRITE_BODY__'),
  ('expand',    1, '__PROMPT_EXPAND_BODY__'),
  ('summarize', 1, '__PROMPT_SUMMARIZE_BODY__');

INSERT INTO modes (slug, display_name, hotkey, provider, model_id, prompt_id, temperature, max_tokens, enabled) VALUES
  ('rewrite',   'Rewrite',   'Ctrl+Win+R',     'ollama', 'qwen2.5:3b-instruct-q4_K_M',
    (SELECT id FROM prompts WHERE mode_slug='rewrite'   AND version=1), 0.4, 2048, 0),
  ('expand',    'Expand',    'Ctrl+Win+E',     'ollama', 'qwen2.5:3b-instruct-q4_K_M',
    (SELECT id FROM prompts WHERE mode_slug='expand'    AND version=1), 0.5, 4096, 0),
  ('summarize', 'Summarize', 'Ctrl+Win+S',     'ollama', 'qwen2.5:3b-instruct-q4_K_M',
    (SELECT id FROM prompts WHERE mode_slug='summarize' AND version=1), 0.2, 1024, 0);

UPDATE schema_meta SET value = '5' WHERE key = 'schema_version';

COMMIT;
