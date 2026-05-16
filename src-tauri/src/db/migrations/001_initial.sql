-- ──────────────────────────────────────────────────────────────────────
-- Migration 001 — core tables + FTS5
--
-- Rationale: bootstraps the Mockingbird schema per PLAN-mockingbird-v2.md
-- §7. Creates schema_meta versioning, the four audited tables (modes,
-- prompts, dictionary, style_examples) along with their supporting
-- snapshot/set tables, sessions + transcripts (the raw/cleaned/final
-- record), the contentless transcripts_fts virtual table with its
-- AFTER INSERT / AFTER DELETE sync triggers, corrections, settings,
-- and learning_runs. Index choices match PLAN §7 verbatim.
--
-- Rollback notes: this migration is the genesis of the schema; the only
-- "rollback" is to delete the database file. Once `phase-1-complete`
-- ships, the hook `block-migration-edit-after-phase-1` seals this file
-- forever. Any defect found here MUST be repaired by a follow-up
-- migration (004+), never by editing 001 in place.
--
-- Notes on this file:
--   * PRAGMA statements at the top are idempotent and survive re-runs.
--   * `transcripts_fts` uses content=external (contentless idiom); the
--     DELETE trigger therefore uses the special `'delete'` sentinel
--     insert into the fts table itself.
--   * Raw-transcript immutability is enforced by the hook engine, not
--     by a SQL trigger here. A belt-and-suspenders trigger may land in
--     a future migration (deferred per Wave 2 brief §1.4).
--   * `DEFAULT CURRENT_TIMESTAMP` resolves to UTC per SQLite docs.
-- ──────────────────────────────────────────────────────────────────────

PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

BEGIN TRANSACTION;

CREATE TABLE schema_meta (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

INSERT INTO schema_meta (key, value) VALUES ('schema_version', '1');

CREATE TABLE prompts (
  id          INTEGER PRIMARY KEY,
  mode_slug   TEXT NOT NULL,
  version     INTEGER NOT NULL,
  body        TEXT NOT NULL,
  created_at  TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  UNIQUE(mode_slug, version)
);

CREATE TABLE modes (
  id            INTEGER PRIMARY KEY,
  slug          TEXT NOT NULL UNIQUE,
  display_name  TEXT NOT NULL,
  hotkey        TEXT NOT NULL,
  provider      TEXT NOT NULL,
  model_id      TEXT NOT NULL,
  prompt_id     INTEGER NOT NULL REFERENCES prompts(id),
  temperature   REAL NOT NULL DEFAULT 0.3,
  max_tokens    INTEGER NOT NULL DEFAULT 2048,
  enabled       INTEGER NOT NULL DEFAULT 1,
  created_at    TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at    TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE dictionary (
  id            INTEGER PRIMARY KEY,
  term          TEXT NOT NULL,
  canonical     TEXT,
  source        TEXT NOT NULL,
  confidence    REAL NOT NULL DEFAULT 1.0,
  app_context   TEXT,
  use_count     INTEGER NOT NULL DEFAULT 0,
  last_used_at  TEXT,
  created_at    TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  UNIQUE(term, app_context)
);

CREATE INDEX idx_dictionary_term ON dictionary(term);
CREATE INDEX idx_dictionary_use ON dictionary(use_count DESC, last_used_at DESC);

CREATE TABLE dictionary_snapshots (
  id          INTEGER PRIMARY KEY,
  term_ids    TEXT NOT NULL,
  created_at  TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE style_examples (
  id          INTEGER PRIMARY KEY,
  mode_slug   TEXT NOT NULL,
  session_id  INTEGER,
  raw_input   TEXT NOT NULL,
  ideal_output TEXT NOT NULL,
  app_context TEXT,
  source      TEXT NOT NULL,
  rank        REAL NOT NULL DEFAULT 0,
  enabled     INTEGER NOT NULL DEFAULT 1,
  created_at  TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_style_examples_mode ON style_examples(mode_slug, enabled, rank DESC);

CREATE TABLE example_sets (
  id           INTEGER PRIMARY KEY,
  mode_slug    TEXT NOT NULL,
  example_ids  TEXT NOT NULL,
  created_at   TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE sessions (
  id                       INTEGER PRIMARY KEY,
  uuid                     TEXT NOT NULL UNIQUE,
  mode_id                  INTEGER NOT NULL REFERENCES modes(id),
  hotkey_pressed           TEXT NOT NULL,
  started_at               TEXT NOT NULL,
  recording_ended_at       TEXT NOT NULL,
  processing_completed_at  TEXT,
  status                   TEXT NOT NULL,
  error_message            TEXT,
  foreground_app           TEXT,
  foreground_window_title  TEXT,
  audio_duration_ms        INTEGER NOT NULL,
  audio_blob_path          TEXT,
  prompt_id                INTEGER REFERENCES prompts(id),
  dictionary_snapshot_id   INTEGER REFERENCES dictionary_snapshots(id),
  example_set_id           INTEGER REFERENCES example_sets(id),
  stt_latency_ms           INTEGER,
  cleanup_latency_ms       INTEGER,
  injection_latency_ms     INTEGER
);

CREATE INDEX idx_sessions_started ON sessions(started_at DESC);
CREATE INDEX idx_sessions_mode ON sessions(mode_id, started_at DESC);
CREATE INDEX idx_sessions_app ON sessions(foreground_app, started_at DESC);

CREATE TABLE transcripts (
  id           INTEGER PRIMARY KEY,
  session_id   INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  stage        TEXT NOT NULL,                -- 'raw' | 'cleaned' | 'final'
  text         TEXT NOT NULL,
  model_used   TEXT,
  created_at   TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  UNIQUE(session_id, stage)
);

CREATE INDEX idx_transcripts_session ON transcripts(session_id);

-- FTS5 search across all transcript stages
CREATE VIRTUAL TABLE transcripts_fts USING fts5(
  text,
  content='transcripts',
  content_rowid='id',
  tokenize='porter unicode61'
);

CREATE TRIGGER transcripts_fts_insert AFTER INSERT ON transcripts BEGIN
  INSERT INTO transcripts_fts(rowid, text) VALUES (new.id, new.text);
END;

CREATE TRIGGER transcripts_fts_delete AFTER DELETE ON transcripts BEGIN
  INSERT INTO transcripts_fts(transcripts_fts, rowid, text) VALUES('delete', old.id, old.text);
END;

-- transcripts(stage='raw') is IMMUTABLE — no fts update trigger needed
-- (cleaned/final are also written once; learning loop edits style_examples instead)

CREATE TABLE corrections (
  id            INTEGER PRIMARY KEY,
  session_id    INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  before_text   TEXT NOT NULL,
  after_text    TEXT NOT NULL,
  detection_method TEXT NOT NULL,
  classification   TEXT,
  classified_at    TEXT,
  created_at       TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE settings (
  key    TEXT PRIMARY KEY,
  value  TEXT NOT NULL
);

CREATE TABLE learning_runs (
  id                          INTEGER PRIMARY KEY,
  started_at                  TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  completed_at                TEXT,
  sessions_analyzed           INTEGER,
  corrections_classified      INTEGER,
  examples_added              INTEGER,
  examples_removed            INTEGER,
  dictionary_terms_added      INTEGER,
  eval_correction_rate_before REAL,
  eval_correction_rate_after  REAL,
  rolled_back                 INTEGER NOT NULL DEFAULT 0,
  notes                       TEXT
);

UPDATE schema_meta SET value = '1' WHERE key = 'schema_version';

COMMIT;
