-- ──────────────────────────────────────────────────────────────────────
-- Migration 021 — ADR 0047 §Wave 2.3: repoint `casual` mode at the 7B
-- Qwen-Instruct Q4_K_M model.
--
-- Background. ADR 0022's own Context section identified the 3B model
-- as below the "headroom for restraint-heavy cleanup" threshold:
-- small models default to the summarisation prior under prompt
-- ambiguity. `casual` was left at 3B in migration 008 ONLY because
-- the latency tax of a 7B call on one-liners was unacceptable.
--
-- Wave 2A (commit 7330884, mb-da5t) shipped the LLM-skip-on-short-
-- utterance path: short, non-listy utterances now bypass the LLM
-- entirely and return the deterministic preprocessor output in ~5 ms.
-- The 7B latency tax therefore only applies to genuinely-long casual
-- dictations (>12 words OR listy), which is the population where the
-- 3B's over-consolidation symptom actually bites. The 3B compromise
-- has done its job and is no longer load-bearing.
--
-- This migration repoints `casual` at the same Qwen-7B-Q4_K_M model
-- that already backs `normal` + `formal` (set in migration 008 / ADR
-- 0022 Wave 2). The existing `casual_v2` few-shots are already 7B-
-- compatible (authored in migration 010 / ADR 0024 Wave C). No
-- prompt-body changes; no temperature changes (already 0.2 from
-- migration 010); only the `model_id` column moves.
--
-- Ordering precondition (binding):
--   The LLM-skip-on-short-utterance path in `cleanup/llm_cleaner.rs`
--   must already be in place. Without it, casual one-liners pay the
--   full 7B latency on every utterance, which is the failure mode
--   migration 008 specifically engineered around. The skip path
--   landed in commit 7330884 (Wave 2A); this migration is dependent
--   on that commit and is sequenced after it in the migration chain.
--
-- Model availability:
--   `qwen2.5:7b-instruct-q4_K_M` is the same model already backing
--   `normal` + `formal` since migration 008. Existing installs that
--   have been running normal or formal already have it pulled; fresh
--   installs follow the documented "Ollama auto-pulls on first
--   /api/generate call" behaviour (the OllamaProvider's error path
--   in `cleanup/llm_cleaner.rs` falls back to raw on provider error,
--   so a missing model degrades gracefully to no-cleanup-this-time
--   rather than hard-failing the dictation).
--
-- ADR 0008 compliance:
--   Pure UPDATE against one row of the `modes` table. No INSERTs.
--   Prompt rows untouched. Other `modes` columns untouched. The
--   row's prompt_id still resolves to `casual_v2` (the latest
--   `mode_slug='casual'` prompt as of migration 010); the body of
--   that prompt is unchanged and 7B-compatible by design.
-- ──────────────────────────────────────────────────────────────────────

BEGIN TRANSACTION;

UPDATE modes
   SET model_id = 'qwen2.5:7b-instruct-q4_K_M'
 WHERE slug = 'casual';

UPDATE schema_meta SET value = '21' WHERE key = 'schema_version';

COMMIT;
