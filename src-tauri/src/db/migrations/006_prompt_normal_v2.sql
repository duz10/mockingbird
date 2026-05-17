-- ──────────────────────────────────────────────────────────────────────
-- Migration 006 — normal-mode prompt v2 (structure cues)
--
-- Why: phase-5 smoketest (2026-05-17) surfaced that users expect
-- explicit verbal cues like "make a list" to produce real markdown
-- lists in the cleaned output. v1 of the normal prompt was deliberately
-- structure-averse ("do not invent structure that isn't implied by the
-- speech"). v2 keeps that conservatism as the default, then carves out
-- a documented set of explicit cues ("make a list", "step one / step
-- two", "heading", "bold", "code", "new paragraph") that DO trigger
-- markdown rendering.
--
-- ADR 0008 compliance:
--   - We INSERT a new prompts row (mode_slug='normal', version=2)
--     rather than UPDATE-ing the v1 row. v1 stays addressable for
--     every existing session row that recorded prompt_id pointing at
--     the v1 prompt — provenance preserved.
--   - The modes.normal row is then UPDATEd to point at the v2 prompt
--     id, so new dictations pick up v2.
--   - Append-only-migration invariant: this file is added, not edited.
--     Future v3 will be migration 007.
--
-- Body source: src-tauri/src/cleanup/prompts/normal_v2.md, substituted
-- at runtime via db/prompt_loader.rs (the `__PROMPT_NORMAL_V2_BODY__`
-- token below). The v1 file (normal.md) stays frozen at v1 forever
-- as the on-disk record of what shipped originally.
-- ──────────────────────────────────────────────────────────────────────

BEGIN TRANSACTION;

INSERT INTO prompts (mode_slug, version, body) VALUES
  ('normal', 2, '__PROMPT_NORMAL_V2_BODY__');

UPDATE modes
   SET prompt_id = (SELECT id FROM prompts WHERE mode_slug='normal' AND version=2)
 WHERE slug = 'normal';

UPDATE schema_meta SET value = '6' WHERE key = 'schema_version';

COMMIT;
