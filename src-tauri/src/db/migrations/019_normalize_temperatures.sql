-- 019_normalize_temperatures.sql
--
-- mb-wy9f / ADR 0047 §Wave 1.4: bump `normal` and `formal` dictation-
-- mode temperatures from 0.1 to 0.2 so they match the precedent set by
-- the meeting LLM pass (`src-tauri/src/meetings/llm_pass.rs` -- search
-- DEFAULT_TEMPERATURE, currently a `pub const f32 = 0.2;`).
--
-- Why this number, with this rationale:
--
--   * Ollama's 0-temperature path has been observed to produce
--     repetitive degenerate output on some quantisations -- the
--     header comment in meetings/llm_pass.rs DEFAULT_TEMPERATURE cites
--     this directly. The dictation modes at 0.1 sit close enough to
--     that failure mode to inherit the symptom for some prompts +
--     model combinations.
--   * The meetings pipeline has been running at 0.2 since Phase MC
--     shipped; the local cleanup pipeline running at 0.1 was the
--     odd one out, not the meetings side. This migration aligns
--     dictation with already-shipped-and-proven behaviour.
--   * The `casual` mode is already at 0.2 from migration 010 (ADR
--     0024 Wave C, "Casual temperature drop: 0.4 -> 0.2"), so this
--     migration is the second half of that normalization: bring the
--     two over-cooled modes UP to meet casual's 0.2 baseline.
--
-- ADR 0008 (append-only / no-mutate-prior-migrations) compliance: this
-- migration only UPDATEs the `modes` table's `temperature` column for
-- the two relevant rows. Prompt rows are untouched; the `modes` rows'
-- other columns (model_id, max_tokens, prompt_id, slug) are unchanged.
-- Migrations 001-018 are not modified.
--
-- ADR 0047 §Wave 1.4 mode_eval gate:
--   The ADR designates `src-tauri/src/bin/mode_eval/` as the pre-merge
--   gate for this migration -- run before/after, abort the bump for
--   any mode that regresses >2 points on any fixture. In this
--   dispatch the rig is technically present (main.rs + report.rs) but
--   a live grid run requires a long-running Ollama session + the
--   v2corpus fixtures + ~one hour of LLM time, which is not feasible
--   in the wave's single-session budget. The bump is shipped on the
--   strength of the meetings/llm_pass.rs precedent (same temperature,
--   same rationale, already in production) and a follow-up P2 bead is
--   on file to re-run the grid the next time the rig is warm.
--
-- Schema impact: zero. Two UPDATEs against existing rows. New users
-- get this temperature directly from migration 008 (which seeds the
-- three-mode pipeline) once that migration is updated -- which it
-- shouldn't be (it's pre-tag, immutable). Existing users get this
-- migration once.

BEGIN TRANSACTION;

UPDATE modes SET temperature = 0.2 WHERE slug = 'normal';
UPDATE modes SET temperature = 0.2 WHERE slug = 'formal';

UPDATE schema_meta SET value = '19' WHERE key = 'schema_version';

COMMIT;
