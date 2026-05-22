-- 012_activity_capture.sql
-- Phase 10 Wave 1B. Schema_version 11 → 12.
--
-- Adds the sibling-subsystem schema for activity capture (ADR 0036):
--
--   activity_sessions             — one row per (Start … Stop) activity session
--   activity_events               — RAW, IMMUTABLE timeline events (Principle 1)
--   activity_blocks               — DERIVED, EDITABLE Block-level abstractions
--                                   (Wave 3 fills the abstractor; Wave 1B leaves
--                                   the table empty but ready)
--   activity_transcript_segments  — OPTIONAL Layer-2 mic transcription (Wave 4)
--
-- ADR refs: 0036 (sibling-subsystem charter, locked decisions Q1-Q9),
-- 0037 (Command Center is the invocation surface — no chord here).
-- Schema is purely additive. No prior migration is touched.
--
-- IMMUTABILITY: activity_events mirrors transcripts(stage='raw') —
-- the BEFORE UPDATE trigger raises ABORT on any in-place edit
-- (Principle 1 + ADR 0036 §Decision item 6 + Cross-wave invariant #1).
-- DELETE on events is allowed only via session CASCADE; the BEFORE
-- DELETE trigger blocks stray "delete a single event" SQL.
--
-- FUTURE-PROOFING (ADR 0036 Q6): project_id + project_label columns
-- on activity_sessions are nullable and have NO surfaced IPC/UI in
-- v1; they exist so we don't need to amend 012 (sealed past
-- phase-1-complete by the hook engine) when v2 ships multi-project
-- tagging.
--
-- FTS5: deferred to Wave 3 (when activity_blocks.generated_abstract
-- exists in non-trivial quantity). Adding it now would index empty
-- strings.
--
-- Trigger count delta: +2 (the two immutability triggers).
-- `tests/db_migrations.rs::trigger_count_is_N` is updated in this wave.

PRAGMA foreign_keys = ON;

BEGIN TRANSACTION;

-- =============================================================================
-- activity_sessions — one row per Start..Stop session.
-- =============================================================================
CREATE TABLE activity_sessions (
  id                   TEXT PRIMARY KEY,          -- ULID, generated app-side
  started_at           INTEGER NOT NULL,          -- unix epoch ms
  ended_at             INTEGER,                   -- NULL until Stop
  status               TEXT NOT NULL,             -- 'in_progress'|'completed'|'crashed_recovered'|'partial'
  audio_enabled        INTEGER NOT NULL DEFAULT 0,-- 0/1; Wave 4 toggles per-session
  screenshot_enabled   INTEGER NOT NULL DEFAULT 0,-- 0/1; Wave 7 only — Waves 1-6 always 0
  label                TEXT,                      -- user rename; NULL until set or auto-derived
  project_id           TEXT,                      -- Q6: schema future-proofing; NO v1 UI
  project_label        TEXT,                      -- Q6: schema future-proofing; NO v1 UI
  summary_markdown     TEXT,                      -- Wave 3 fills; NULL otherwise
  prompt_set_sha       TEXT,                      -- SHA of prompts/*.md set used in Stage-3
  created_at           INTEGER NOT NULL,
  updated_at           INTEGER NOT NULL
);

CREATE INDEX idx_activity_sessions_started ON activity_sessions(started_at DESC);
CREATE INDEX idx_activity_sessions_status  ON activity_sessions(status, started_at DESC);

-- =============================================================================
-- activity_events — RAW. IMMUTABLE. Principle 1.
-- =============================================================================
CREATE TABLE activity_events (
  id           TEXT PRIMARY KEY,                  -- ULID, generated app-side
  session_id   TEXT NOT NULL REFERENCES activity_sessions(id) ON DELETE CASCADE,
  ts           INTEGER NOT NULL,                  -- unix epoch ms
  kind         TEXT NOT NULL,                     -- 'app_switch'|'context_snapshot'|'idle_start'|'idle_end'|'paused'|'resumed'|'layer_error'
  app_name     TEXT,                              -- e.g. 'chrome.exe'; NULL for idle/control events
  window_title TEXT,                              -- raw window text; NULL for control events
  snapshot_json TEXT,                             -- Wave 1B: NULL or {"app","title"}. Wave 2: full UIA payload.
  created_at   INTEGER NOT NULL
);

CREATE INDEX idx_activity_events_session_ts ON activity_events(session_id, ts);
CREATE INDEX idx_activity_events_kind       ON activity_events(session_id, kind, ts);

-- Principle 1 enforcement: activity_events is write-once.
CREATE TRIGGER activity_events_no_update
BEFORE UPDATE ON activity_events
BEGIN
  SELECT RAISE(ABORT, 'activity_events is immutable (Principle 1; ADR 0036)');
END;

-- Block stray single-row DELETEs. The only sanctioned delete path is
-- CASCADE-on-session-delete (which fires AFTER this trigger's WHEN
-- check passes for in_progress sessions; the CASCADE itself happens
-- through the FK and is not blocked by this trigger). For completed/
-- crashed sessions, even cascading delete should only happen via the
-- session DELETE, not directly against events.
CREATE TRIGGER activity_events_no_delete
BEFORE DELETE ON activity_events
WHEN (
  SELECT 1
  FROM activity_sessions
  WHERE id = OLD.session_id
    AND status != 'in_progress'
)
BEGIN
  SELECT RAISE(ABORT,
    'cannot delete events from a completed session; delete the session row instead');
END;

-- =============================================================================
-- activity_blocks — DERIVED. EDITABLE in v1 only via Wave 3+ Block CRUD (Q7).
-- =============================================================================
CREATE TABLE activity_blocks (
  id                  TEXT PRIMARY KEY,
  session_id          TEXT NOT NULL REFERENCES activity_sessions(id) ON DELETE CASCADE,
  started_at          INTEGER NOT NULL,
  ended_at            INTEGER NOT NULL,
  primary_app         TEXT,
  generated_abstract  TEXT,                       -- Stage-3 LLM output; NULL when Ollama unavailable
  user_edited         INTEGER NOT NULL DEFAULT 0,
  source_event_ids    TEXT NOT NULL,              -- JSON array of activity_events.id values
  prompt_version_sha  TEXT,                       -- per-Block: SHA of the abstractor prompt used
  created_at          INTEGER NOT NULL,
  updated_at          INTEGER NOT NULL
);

CREATE INDEX idx_activity_blocks_session_started
  ON activity_blocks(session_id, started_at);

-- =============================================================================
-- activity_transcript_segments — OPTIONAL Layer-2 (Wave 4).
-- =============================================================================
CREATE TABLE activity_transcript_segments (
  id          TEXT PRIMARY KEY,
  session_id  TEXT NOT NULL REFERENCES activity_sessions(id) ON DELETE CASCADE,
  started_at  INTEGER NOT NULL,
  ended_at    INTEGER NOT NULL,
  text        TEXT NOT NULL,
  source      TEXT NOT NULL,                     -- 'mic' (v1); future 'system' if loopback ever added
  created_at  INTEGER NOT NULL
);

CREATE INDEX idx_activity_segments_session_started
  ON activity_transcript_segments(session_id, started_at);

UPDATE schema_meta SET value = '12' WHERE key = 'schema_version';

COMMIT;
