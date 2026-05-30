-- ──────────────────────────────────────────────────────────────────────
-- Migration 024 -- KG Phase 1B (ADR 0050). Schema_version 23 -> 24.
--
-- Bead: mb-geds (Chunk 2 of the mb-bjni epic). Charter ADR: 0050
-- (Proposed -- flips Accepted at Chunk 5 seal). Wave brief:
-- docs/knowledge-graph/phase-1b-brief.md.
--
-- ADR 0050 §"DB schema (DDL)" is the canonical record. This migration
-- replicates it byte-for-byte. If the two ever diverge, the ADR wins
-- and a successor migration reconciles.
--
-- FK target verified: sessions(id) INTEGER PRIMARY KEY per
-- src-tauri/src/db/migrations/001_initial.sql line 110.
--
-- Tables added:
--   kg_entities                -- first-class entity rows (people, orgs, projects, etc.)
--   kg_canonical_tags          -- closed-vocab tag rows (v1.1 inert in 1B; row count = 0)
--   kg_entity_mentions         -- per-segment entity provenance (D1)
--   kg_tag_mentions            -- per-segment tag provenance (D1)
--   kg_filing_queue            -- async filing queue (D3)
--
-- VIEWs added:
--   kg_concept_entities_view   -- entries-by-entity projection (D7)
--   kg_concept_tags_view       -- entries-by-tag projection (D7)
--
-- Immutability triggers on kg_entity_mentions + kg_tag_mentions
-- (Principle 1 analog -- mention rows are extracted provenance;
-- reconciliation flows through DELETE + re-INSERT, never UPDATE).
-- These mirror the activity_events_no_update pattern from
-- migration 012.
--
-- Seed: kg_graph_enabled = false (D4). The Rust-side default in
-- SettingKey::default_value is the source of truth; this seed row
-- exists so the Settings UI in Phase 1C has something to bind to
-- before the user toggles. INSERT OR IGNORE keeps re-running the
-- migration in a test harness idempotent.
--
-- ADR refs: 0049 (Phase 0.5 + v1 pivot), 0050 (this charter).
-- Principle 1 (raw immutability): unaffected -- transcripts/sessions
-- raw layer untouched; the mention tables are derived analogs whose
-- immutability is enforced here.
-- Principle 2 (provenance total): per-segment storage on the
-- mention tables is what makes this principle hold for the KG layer.
-- Principle 4 (no telemetry): all KG state is local-only.
--
-- Entity-type vocabulary note: the kg::passes::extract_entities
-- module already serializes EntityType as
-- 'person'|'organization'|'object'|'place'|'project' (see
-- ExtractedEntity + EntityType::as_str in
-- src-tauri/src/kg/passes/extract_entities.rs). The ADR 0050 §"DB
-- schema (DDL)" header lists the older sandbox vocabulary
-- ('location'|'thing' in place of 'place'|'object'); the Rust truth
-- wins and is reproduced below. ADR comment amendment is a Chunk 5
-- seal-time housekeeping item, not a blocker.
-- ──────────────────────────────────────────────────────────────────────

BEGIN TRANSACTION;

-- =============================================================================
-- kg_entities -- one row per canonical entity (Mom, Acme Corp, etc.).
-- =============================================================================
-- Aliases are stored as a JSON array on the entity row rather than a
-- separate kg_entity_aliases table. Empirically the alias count per
-- entity is small (1-4) and aliases are always read together with the
-- canonical row; a JSON-array column is the lower-overhead shape and
-- avoids a join on every entity lookup. v1.1 may revisit if alias
-- counts grow or per-alias provenance becomes a requirement.
CREATE TABLE kg_entities (
  id            INTEGER PRIMARY KEY,
  name          TEXT NOT NULL,                    -- canonical surface form
  entity_type   TEXT NOT NULL,                    -- 'person'|'organization'|'object'|'place'|'project'
  aliases_json  TEXT NOT NULL DEFAULT '[]',       -- JSON array of alternate surfaces
  created_at    TEXT NOT NULL,
  updated_at    TEXT NOT NULL,
  UNIQUE(name, entity_type)
);

CREATE INDEX idx_kg_entities_type ON kg_entities(entity_type, name);

-- =============================================================================
-- kg_canonical_tags -- closed-vocab semantic tags (v1.1 starting point).
-- =============================================================================
-- In 1B this table is created but unpopulated. The two-field schema
-- amendment A2 makes the `tags:` field open-vocab in v1; closed-vocab
-- wiring activates in v1.1 after corpus re-labeling per LESSONS P11.
-- The table exists in 1B so 1C/1D do not need a migration redirect.
CREATE TABLE kg_canonical_tags (
  id           INTEGER PRIMARY KEY,
  slug         TEXT NOT NULL UNIQUE,              -- e.g. 'car-repair'
  display_name TEXT NOT NULL,                     -- e.g. 'Car Repair'
  category     TEXT,                              -- optional grouping; nullable
  created_at   TEXT NOT NULL
);

-- =============================================================================
-- kg_entity_mentions -- per-segment entity provenance (D1).
-- =============================================================================
-- entry_id references sessions(id); a "dictation entry" in the KG is
-- one session. segment_idx is the 0-based pipeline segment ordinal.
-- The (entry_id, segment_idx, entity_id) UNIQUE constraint is THE
-- idempotency guarantee that the kg-filing-idempotent invariant
-- depends on -- re-filing the same entry collapses to existing rows
-- via INSERT OR IGNORE.
CREATE TABLE kg_entity_mentions (
  id            INTEGER PRIMARY KEY,
  entry_id      INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  entity_id     INTEGER NOT NULL REFERENCES kg_entities(id) ON DELETE CASCADE,
  segment_idx   INTEGER NOT NULL,                 -- pipeline segment index, 0-based
  surface_form  TEXT NOT NULL,                    -- exact text the model emitted
  created_at    TEXT NOT NULL,
  UNIQUE(entry_id, segment_idx, entity_id)
);

CREATE INDEX idx_kg_entity_mentions_entry  ON kg_entity_mentions(entry_id);
CREATE INDEX idx_kg_entity_mentions_entity ON kg_entity_mentions(entity_id, entry_id);

-- =============================================================================
-- kg_tag_mentions -- per-segment tag provenance (D1).
-- =============================================================================
-- In 1B tag_slug is the open-vocab string the model emitted. The
-- foreign key to kg_canonical_tags is NULLable so open-vocab tags
-- land cleanly; the canonical-tag join activates in v1.1 once the
-- table is populated.
CREATE TABLE kg_tag_mentions (
  id               INTEGER PRIMARY KEY,
  entry_id         INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  canonical_tag_id INTEGER REFERENCES kg_canonical_tags(id) ON DELETE SET NULL,
  segment_idx      INTEGER NOT NULL,
  tag_slug         TEXT NOT NULL,                 -- open-vocab string (v1 primary key)
  created_at       TEXT NOT NULL,
  UNIQUE(entry_id, segment_idx, tag_slug)
);

CREATE INDEX idx_kg_tag_mentions_entry ON kg_tag_mentions(entry_id);
CREATE INDEX idx_kg_tag_mentions_slug  ON kg_tag_mentions(tag_slug, entry_id);

-- =============================================================================
-- kg_filing_queue -- async filing queue (D3).
-- =============================================================================
-- FIFO state machine: pending -> processing -> done | failed.
-- The worker reaps `done` rows older than 30 days on startup (failure
-- rows are kept forever in 1B; Phase 1C surfaces a failures UI).
-- UNIQUE(entry_id) means re-enqueueing the same entry is a no-op
-- INSERT OR IGNORE -- the kg-filing-idempotent invariant in action.
CREATE TABLE kg_filing_queue (
  id                    INTEGER PRIMARY KEY,
  entry_id              INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  state                 TEXT NOT NULL,            -- 'pending'|'processing'|'done'|'failed'
  enqueued_at           TEXT NOT NULL,
  processing_started_at TEXT,                     -- set when state -> processing
  finished_at           TEXT,                     -- set when state -> done|failed
  attempt_count         INTEGER NOT NULL DEFAULT 0,
  last_error            TEXT,                     -- diagnostic message on failure
  UNIQUE(entry_id)
);

CREATE INDEX idx_kg_filing_queue_state ON kg_filing_queue(state, enqueued_at);

-- =============================================================================
-- Concept-page VIEWs (D7).
-- =============================================================================
-- These are projections of the mention tables. NOT stored -- recomputed
-- on each SELECT. Phase 1C will index/materialize ONLY if production
-- latency surfaces an issue (YAGNI; no view-cache infrastructure in 1B).

CREATE VIEW kg_concept_entities_view AS
SELECT
  e.id                AS entity_id,
  e.name              AS entity_name,
  e.entity_type       AS entity_type,
  m.entry_id          AS entry_id,
  MIN(m.segment_idx)  AS first_segment_idx,
  COUNT(*)            AS mention_count,
  MAX(m.created_at)   AS most_recent_mention_at
FROM kg_entities e
JOIN kg_entity_mentions m ON m.entity_id = e.id
GROUP BY e.id, m.entry_id;

CREATE VIEW kg_concept_tags_view AS
SELECT
  m.tag_slug          AS tag_slug,
  c.id                AS canonical_tag_id,
  c.display_name      AS canonical_display_name,
  m.entry_id          AS entry_id,
  MIN(m.segment_idx)  AS first_segment_idx,
  COUNT(*)            AS mention_count,
  MAX(m.created_at)   AS most_recent_mention_at
FROM kg_tag_mentions m
LEFT JOIN kg_canonical_tags c ON c.id = m.canonical_tag_id
GROUP BY m.tag_slug, m.entry_id;

-- =============================================================================
-- Immutability triggers on mention tables (Principle 2 analog).
-- =============================================================================
-- Mention rows are extracted provenance: once written, they are not
-- edited in place. Reconciliation (e.g. re-filing an entry after a
-- pipeline change) flows through DELETE + re-INSERT, preserving the
-- audit trail of "what the model said when". DELETE is allowed
-- because (a) Phase 1D backfill needs to be able to wipe + re-file
-- and (b) the FK CASCADE from sessions has to flow through. The
-- activity_events_no_delete pattern from migration 012 is NOT
-- mirrored here -- mention rows are derived, not raw observations.

CREATE TRIGGER kg_entity_mentions_no_update
BEFORE UPDATE ON kg_entity_mentions
BEGIN
  SELECT RAISE(ABORT, 'kg_entity_mentions is write-once (Principle 2; ADR 0050)');
END;

CREATE TRIGGER kg_tag_mentions_no_update
BEFORE UPDATE ON kg_tag_mentions
BEGIN
  SELECT RAISE(ABORT, 'kg_tag_mentions is write-once (Principle 2; ADR 0050)');
END;

-- =============================================================================
-- Seed: KgGraphEnabled = false (D4).
-- =============================================================================
-- The actual default lives in SettingKey::default_value (Rust source
-- of truth). This row exists so the Settings UI in Phase 1C has
-- something to bind to before the user toggles. INSERT OR IGNORE so
-- re-running the migration in a test harness is idempotent (and so a
-- user who has already toggled the value via the Settings facade
-- isn't reset to false on schema-meta replay).
INSERT OR IGNORE INTO settings (key, value)
VALUES ('kg_graph_enabled', 'false');

UPDATE schema_meta SET value = '24' WHERE key = 'schema_version';

COMMIT;
