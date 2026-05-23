-- 017_dictation_start_mode.sql
--
-- mb-tfyp: track which start path produced each dictation session.
--
-- ADR 0045 amended ADR 0037 to permit programmatic dictation start/stop
-- via the `dictation_start` / `dictation_stop` IPC, in addition to the
-- original Right Alt PTT path. Both modes drive the same FSM and
-- orchestrator, but the resulting sessions are semantically distinct:
--
--   * `ptt`    — PTT-triggered session. A real "target app" was focused
--                at key-down; the cleaned text gets pasted there. Focus
--                drift between key-up and inject is a real concern and
--                the abort path applies.
--
--   * `in_app` — Programmatically-triggered session (in-app Start
--                button or any other future `dictation_start` caller).
--                Mockingbird IS the focus the entire time; there is no
--                target app, no paste, and the focus-change abort
--                heuristic does NOT apply. The list pill should show
--                `IN_APP` (neutral), not `ABORTED_FOCUS_CHANGED` (red).
--
-- Backfill: existing rows predate ADR 0045 and were all PTT, so
-- DEFAULT 'ptt' is exactly correct. New rows are written explicitly by
-- the orchestrator (see `dictation::SessionState::start_mode`).
--
-- Purely additive ADD COLUMN. The audit triggers in migration 002 do
-- not cover `sessions`, so no trigger update is needed.

ALTER TABLE sessions ADD COLUMN start_mode TEXT NOT NULL DEFAULT 'ptt';

UPDATE schema_meta SET value = '17' WHERE key = 'schema_version';
