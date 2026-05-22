-- 014_activity_audio_provenance.sql
-- Phase 10 Wave 4. Schema_version 13 → 14.
--
-- Adds per-session audio-pipeline provenance columns on
-- `activity_sessions`. Layer 2 (Whisper-driven mic + system loopback
-- transcription) needs to record:
--
--   - audio_whisper_model      — the GGUF model used (e.g.
--                                "whisper-large-v3-turbo-q5_0")
--   - audio_chunk_window_ms    — the chunker window in ms (mirrors
--                                ChunkerConfig.chunk_samples /
--                                sample_rate; typically 30_000)
--
-- Both columns are NULL-defaulted because:
--   1. Pre-Wave-4 sessions had no audio pipeline at all (null is
--      semantically correct for "this session didn't run audio").
--   2. Sessions with `audio_enabled = 0` also need null here — they
--      capture no audio.
--
-- Per Principle 2 (provenance is total), Wave 4 writes these on every
-- session whose `audio_enabled = 1`. Wave 3 and earlier sessions stay
-- null forever, which is fine (no audio = no audio provenance).
--
-- The transcript-segment-to-Block stitching strategy is documented in
-- ADR 0041. The Whisper model + chunk-window choices are documented
-- in ADR 0029 (chunked Whisper) + ADR 0011 (model storage); they get
-- recorded per-session here so a future model swap is queryable
-- from the DB alone.

PRAGMA foreign_keys = ON;

BEGIN TRANSACTION;

ALTER TABLE activity_sessions ADD COLUMN audio_whisper_model   TEXT;
ALTER TABLE activity_sessions ADD COLUMN audio_chunk_window_ms INTEGER;

UPDATE schema_meta SET value = '14' WHERE key = 'schema_version';

COMMIT;
