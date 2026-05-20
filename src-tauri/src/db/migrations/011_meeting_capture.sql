-- 011_meeting_capture.sql
-- Phase MC. Schema_version 10 → 11.
--
-- Adds the sibling-subsystem schema for meeting recording:
--
--   meeting_sessions        — one row per (Start … Stop) meeting
--   meeting_transcripts     — per-channel formatted prose + raw segments JSON
--   meeting_transcripts_fts — FTS5 mirror of the formatted text column,
--                             porter+unicode61 tokenizer (matches existing
--                             transcripts_fts pattern from migration 001)
--   meeting_transcripts_fts_insert / _delete — trigger pair maintaining the
--                             FTS shadow table (matches existing pattern;
--                             no _update trigger because formatted rows are
--                             write-once like raw dictation transcripts)
--
-- ADR refs: 0026 (sibling-subsystem charter), 0027 (chord activation),
-- 0028 (twin-stream capture), 0029 (long-form chunked Whisper), 0030
-- (whisper segment exposure). The schema is purely additive — no
-- existing migration is touched (binding rule, ADR 0010). All Phase MC
-- runtime settings are NEW typed `SettingKey` variants resolved by the
-- Rust layer's `default_value()`; this migration does NOT seed
-- `settings` rows.
--
-- Trigger count delta: +2 (the two FTS triggers). The
-- `tests/db_migrations.rs::trigger_count_is_N` assertion is updated in
-- the same wave to reflect this.

PRAGMA foreign_keys = ON;

BEGIN TRANSACTION;

CREATE TABLE meeting_sessions (
  id                       INTEGER PRIMARY KEY,
  uuid                     TEXT NOT NULL UNIQUE,
  title                    TEXT,                       -- user-given, nullable
  started_at               TEXT NOT NULL,              -- ISO-8601 UTC
  ended_at                 TEXT NOT NULL,              -- ISO-8601 UTC
  status                   TEXT NOT NULL,              -- 'complete'|'partial'|'demoted'|'interrupted'|'failed'
  error_message            TEXT,
  source                   TEXT NOT NULL,              -- 'mic'|'system'|'both'
  total_duration_ms        INTEGER NOT NULL,
  mic_duration_ms          INTEGER,
  sys_duration_ms          INTEGER,
  hotkey_pressed           TEXT NOT NULL,
  audio_blob_path          TEXT,                       -- canonical meeting.wav path
  whisper_model_id         TEXT NOT NULL,              -- e.g. 'whisper-large-v3-turbo-q5_0'
  formatter_version        TEXT NOT NULL,              -- 'mc-v1' — bump to regenerate transcripts later
  chunk_count_mic          INTEGER,
  chunk_count_sys          INTEGER,
  stt_latency_ms           INTEGER,
  formatter_latency_ms     INTEGER
);

CREATE INDEX idx_meeting_sessions_started ON meeting_sessions(started_at DESC);
CREATE INDEX idx_meeting_sessions_status  ON meeting_sessions(status, started_at DESC);

CREATE TABLE meeting_transcripts (
  id                       INTEGER PRIMARY KEY,
  meeting_session_id       INTEGER NOT NULL REFERENCES meeting_sessions(id) ON DELETE CASCADE,
  channel                  TEXT NOT NULL,              -- 'mic'|'system'|'merged'
  stage                    TEXT NOT NULL,              -- 'raw_segments'|'formatted'  (raw_segments is JSON)
  text                     TEXT NOT NULL,              -- formatted prose OR segments JSON
  created_at               TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  UNIQUE(meeting_session_id, channel, stage)
);

CREATE INDEX idx_meeting_transcripts_session ON meeting_transcripts(meeting_session_id);

-- FTS5 search across formatted meeting transcripts.
-- Mirrors the existing transcripts_fts pattern; same porter+unicode61
-- tokenizer. The shadow-table approach (content='...', content_rowid='id')
-- means INSERTs/DELETEs into meeting_transcripts must be reflected via
-- the trigger pair below. No _update trigger: rows are write-once
-- (formatted transcripts are an output of the deterministic formatter;
-- regenerating means bumping `formatter_version` + writing a new row,
-- not editing the old one).
CREATE VIRTUAL TABLE meeting_transcripts_fts USING fts5(
  text,
  content='meeting_transcripts',
  content_rowid='id',
  tokenize='porter unicode61'
);

CREATE TRIGGER meeting_transcripts_fts_insert
  AFTER INSERT ON meeting_transcripts BEGIN
  INSERT INTO meeting_transcripts_fts(rowid, text) VALUES (new.id, new.text);
END;

CREATE TRIGGER meeting_transcripts_fts_delete
  AFTER DELETE ON meeting_transcripts BEGIN
  INSERT INTO meeting_transcripts_fts(meeting_transcripts_fts, rowid, text)
    VALUES('delete', old.id, old.text);
END;

UPDATE schema_meta SET value = '11' WHERE key = 'schema_version';

COMMIT;
