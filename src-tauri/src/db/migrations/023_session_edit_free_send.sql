-- ──────────────────────────────────────────────────────────────────────
-- Migration 023 — mb-v2fa / ADR 0047 §Wave 2.5: edit-free-send metric.
--
-- Adds the single nullable column that proves whether the rest of the
-- ADR 0047 epic actually worked. The whole "let the model NOT
-- consolidate, let the user pull Compress on demand" thesis is a bet
-- that injected text needs fewer follow-up edits. This column is the
-- inverse-of-success heuristic that lets us *check the bet* across a
-- representative population of sessions, in aggregate, on-device.
--
-- Column semantics (read these as a state machine):
--
--   NULL  =  "not observed yet" -- the session has not yet been
--            injected (still processing, aborted, in-app, headless
--            ingest, secure-input-blocked, etc) OR the 5-min
--            observation window has not yet elapsed. The aggregation
--            in `commands/insights.rs` deliberately treats NULL as
--            "excluded from the denominator": sessions that never
--            inject (in-app, abort, file import) are not part of
--            the population this metric measures.
--   1     =  "injected, no edit-equivalent action observed within
--            5 min of injection". The optimistic default flipped on
--            by `mark_injected_for_edit_metric` when the orchestrator
--            sees InjectionOutcome::Ok or OkClipboardNotRestored.
--   0     =  "injected, then the user did something inside the 5-min
--            window that signals dissatisfaction". Today the
--            edit-equivalent actions are:
--              (a) running an on-demand LLM pass on the session
--                  (LlmPassCard run -- they wanted different output)
--              (b) clicking the Dictations detail Copy button
--                  (they're moving the text by hand because the
--                  injection didn't land where it needed to OR
--                  they're shipping the text to a second target)
--            Each call site lives in `commands/sessions.rs` and
--            invokes `sessions::mark_edit_observed`, which is a
--            no-op if processing_completed_at is more than 5 min
--            ago or if the session was never injected. This makes
--            the 5-min anchor a property of the helper, not of
--            individual call sites.
--
-- ADR 0008 compliance: pure ADD COLUMN (default NULL). No existing
-- rows touched; legacy rows stay NULL forever -- they predate the
-- column and we have no way to retroactively know if the user
-- edited within 5 min back then. The Insights aggregation handles
-- legacy rows the same way it handles never-injected rows: excluded.
--
-- Principle 1 ("raw transcripts are immutable") -- unaffected. This
-- migration touches the `sessions` table only. The `mark_edit_observed`
-- helper also touches only the `sessions` table; the `transcripts`
-- audit triggers remain the sole guard on raw immutability.
--
-- Principle 4 ("no telemetry") -- this is a LOCAL-ONLY column. The
-- Insights aggregation reads it on-device for the "Your usage" tab
-- tile. There is no code path that sends it anywhere; the binary
-- doesn't talk to a backend.
-- ──────────────────────────────────────────────────────────────────────

BEGIN TRANSACTION;

ALTER TABLE sessions ADD COLUMN edit_free_within_5min INTEGER;

UPDATE schema_meta SET value = '23' WHERE key = 'schema_version';

COMMIT;
