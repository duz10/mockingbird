-- ──────────────────────────────────────────────────────────────────────
-- Migration 009 — bump default max_tokens for normal + formal modes
--
-- 2026-05-17 user smoketest dictated a 1 m 27 s story in formal mode
-- and got back substantially less polished prose than the raw warranted
-- — the LLM was hitting its max_tokens ceiling and truncating output.
-- formal_v1's prose-polish + structural-promotion behavior generates
-- MORE output tokens than the input, so the old 4096 cap (good for v3
-- normal at 1:1 in/out) was undersized.
--
-- New ceilings, chosen to match the new architecture:
--   - normal: 2048 → 4096  (covers ~5 min of dense speech post-polish)
--   - formal: 4096 → 8192  (covers ~5 min + heading/list expansion)
--
-- 7B-q4 generation at ~50 tok/s means the worst-case 8192-token
-- formal pass takes ~2.7 min on this box — outside the user's
-- expected dictation latency, but the orchestrator's
-- REQUEST_TIMEOUT (60 s) will catch any truly runaway generation
-- and fall back to raw injection. The token ceiling exists to
-- bound cost, not to short-circuit normal use.
--
-- casual stays at 1024: by-design short, and Wave 3 will skip the
-- LLM entirely for short casual utterances anyway.
--
-- ADR 0008: append-only UPDATE. No data destroyed; new sessions
-- pick up the new ceilings, old session rows are untouched.
-- ──────────────────────────────────────────────────────────────────────

BEGIN TRANSACTION;

UPDATE modes SET max_tokens = 4096 WHERE slug = 'normal';
UPDATE modes SET max_tokens = 8192 WHERE slug = 'formal';

UPDATE schema_meta SET value = '9' WHERE key = 'schema_version';

COMMIT;
