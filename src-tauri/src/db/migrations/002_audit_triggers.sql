-- ──────────────────────────────────────────────────────────────────────
-- Migration 002 — audit triggers + history tables
--
-- Rationale: PLAN-mockingbird-v2.md §7 mandates JSON-patch audit history
-- on all four user-mutable tables (modes, prompts, dictionary,
-- style_examples) so that:
--   1. db/audit.rs::rollback_to_snapshot can replay inverse operations,
--   2. the Phase 8 learning loop can roll back an eval regression,
--   3. Settings → Advanced "undo last change" works.
--
-- PLAN only spells out the dictionary pattern. This migration
-- extrapolates to all four tables per the Wave 2 brief, with explicit
-- column projections (no SELECT *).
--
-- Patch shape conventions:
--   * INSERT → patch = json_object of mutable columns (id and created_at
--     are excluded; the row already has them).
--   * UPDATE → patch = {"before": {...}, "after": {...}} of the same
--     mutable subset. Lets the rollback path compute the inverse diff.
--   * DELETE → patch = a minimal identifying key tuple; the prior
--     INSERT/UPDATE history rows carry the full value.
--
-- PLAN bug worked around: PLAN's dictionary UPDATE trigger references
-- OLD.enabled / NEW.enabled, but the `dictionary` table has no
-- `enabled` column (see migration 001). The trigger below drops that
-- field. Reviewable deviation; documented in docs/LESSONS.md.
--
-- Rollback notes: dropping these triggers is safe (no data loss; just
-- stops new history rows). Dropping the history tables would lose
-- audit data — don't, ever. If a defect is found post-seal, write a
-- new migration that CREATE TRIGGER IF NOT EXISTS-replaces the offender
-- (SQLite has no DROP TRIGGER IF EXISTS in older versions; use guarded
-- form). Hook block-migration-edit-after-phase-1 enforces append-only.
--
-- Count check: 4 tables × 3 ops = 12 audit triggers added here.
-- Combined with the 2 FTS5 triggers from 001, total = 14.
-- ──────────────────────────────────────────────────────────────────────

BEGIN TRANSACTION;

-- ──────────────────────────────────────────────────────────────────────
-- 1. _history_modes
-- ──────────────────────────────────────────────────────────────────────
CREATE TABLE _history_modes (
  id        INTEGER PRIMARY KEY,
  row_id    INTEGER NOT NULL,
  operation TEXT NOT NULL,
  patch     TEXT NOT NULL,
  at        TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TRIGGER modes_audit_insert AFTER INSERT ON modes BEGIN
  INSERT INTO _history_modes (row_id, operation, patch) VALUES (
    NEW.id, 'INSERT', json_object(
      'slug', NEW.slug, 'display_name', NEW.display_name, 'hotkey', NEW.hotkey,
      'provider', NEW.provider, 'model_id', NEW.model_id, 'prompt_id', NEW.prompt_id,
      'temperature', NEW.temperature, 'max_tokens', NEW.max_tokens, 'enabled', NEW.enabled
    )
  );
END;

CREATE TRIGGER modes_audit_update AFTER UPDATE ON modes BEGIN
  INSERT INTO _history_modes (row_id, operation, patch) VALUES (
    NEW.id, 'UPDATE', json_object(
      'before', json_object(
        'slug', OLD.slug, 'display_name', OLD.display_name, 'hotkey', OLD.hotkey,
        'provider', OLD.provider, 'model_id', OLD.model_id, 'prompt_id', OLD.prompt_id,
        'temperature', OLD.temperature, 'max_tokens', OLD.max_tokens, 'enabled', OLD.enabled
      ),
      'after', json_object(
        'slug', NEW.slug, 'display_name', NEW.display_name, 'hotkey', NEW.hotkey,
        'provider', NEW.provider, 'model_id', NEW.model_id, 'prompt_id', NEW.prompt_id,
        'temperature', NEW.temperature, 'max_tokens', NEW.max_tokens, 'enabled', NEW.enabled
      )
    )
  );
END;

CREATE TRIGGER modes_audit_delete AFTER DELETE ON modes BEGIN
  INSERT INTO _history_modes (row_id, operation, patch) VALUES (
    OLD.id, 'DELETE', json_object('slug', OLD.slug)
  );
END;

-- ──────────────────────────────────────────────────────────────────────
-- 2. _history_prompts
-- ──────────────────────────────────────────────────────────────────────
CREATE TABLE _history_prompts (
  id        INTEGER PRIMARY KEY,
  row_id    INTEGER NOT NULL,
  operation TEXT NOT NULL,
  patch     TEXT NOT NULL,
  at        TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TRIGGER prompts_audit_insert AFTER INSERT ON prompts BEGIN
  INSERT INTO _history_prompts (row_id, operation, patch) VALUES (
    NEW.id, 'INSERT', json_object(
      'mode_slug', NEW.mode_slug, 'version', NEW.version, 'body', NEW.body
    )
  );
END;

CREATE TRIGGER prompts_audit_update AFTER UPDATE ON prompts BEGIN
  INSERT INTO _history_prompts (row_id, operation, patch) VALUES (
    NEW.id, 'UPDATE', json_object(
      'before', json_object('mode_slug', OLD.mode_slug, 'version', OLD.version, 'body', OLD.body),
      'after',  json_object('mode_slug', NEW.mode_slug, 'version', NEW.version, 'body', NEW.body)
    )
  );
END;

CREATE TRIGGER prompts_audit_delete AFTER DELETE ON prompts BEGIN
  INSERT INTO _history_prompts (row_id, operation, patch) VALUES (
    OLD.id, 'DELETE', json_object('mode_slug', OLD.mode_slug, 'version', OLD.version)
  );
END;

-- ──────────────────────────────────────────────────────────────────────
-- 3. _history_dictionary  (PLAN bug: no `enabled` column on dictionary)
-- ──────────────────────────────────────────────────────────────────────
CREATE TABLE _history_dictionary (
  id        INTEGER PRIMARY KEY,
  row_id    INTEGER NOT NULL,
  operation TEXT NOT NULL,
  patch     TEXT NOT NULL,
  at        TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TRIGGER dictionary_audit_insert AFTER INSERT ON dictionary BEGIN
  INSERT INTO _history_dictionary (row_id, operation, patch) VALUES (
    NEW.id, 'INSERT', json_object(
      'term', NEW.term, 'canonical', NEW.canonical, 'source', NEW.source,
      'confidence', NEW.confidence, 'app_context', NEW.app_context
    )
  );
END;

CREATE TRIGGER dictionary_audit_update AFTER UPDATE ON dictionary BEGIN
  INSERT INTO _history_dictionary (row_id, operation, patch) VALUES (
    NEW.id, 'UPDATE', json_object(
      'before', json_object(
        'term', OLD.term, 'canonical', OLD.canonical, 'source', OLD.source,
        'confidence', OLD.confidence, 'app_context', OLD.app_context
      ),
      'after', json_object(
        'term', NEW.term, 'canonical', NEW.canonical, 'source', NEW.source,
        'confidence', NEW.confidence, 'app_context', NEW.app_context
      )
    )
  );
END;

CREATE TRIGGER dictionary_audit_delete AFTER DELETE ON dictionary BEGIN
  INSERT INTO _history_dictionary (row_id, operation, patch) VALUES (
    OLD.id, 'DELETE', json_object('term', OLD.term, 'app_context', OLD.app_context)
  );
END;

-- ──────────────────────────────────────────────────────────────────────
-- 4. _history_style_examples
-- ──────────────────────────────────────────────────────────────────────
CREATE TABLE _history_style_examples (
  id        INTEGER PRIMARY KEY,
  row_id    INTEGER NOT NULL,
  operation TEXT NOT NULL,
  patch     TEXT NOT NULL,
  at        TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TRIGGER style_examples_audit_insert AFTER INSERT ON style_examples BEGIN
  INSERT INTO _history_style_examples (row_id, operation, patch) VALUES (
    NEW.id, 'INSERT', json_object(
      'mode_slug', NEW.mode_slug, 'session_id', NEW.session_id,
      'raw_input', NEW.raw_input, 'ideal_output', NEW.ideal_output,
      'app_context', NEW.app_context, 'source', NEW.source,
      'rank', NEW.rank, 'enabled', NEW.enabled
    )
  );
END;

CREATE TRIGGER style_examples_audit_update AFTER UPDATE ON style_examples BEGIN
  INSERT INTO _history_style_examples (row_id, operation, patch) VALUES (
    NEW.id, 'UPDATE', json_object(
      'before', json_object(
        'raw_input', OLD.raw_input, 'ideal_output', OLD.ideal_output,
        'app_context', OLD.app_context, 'rank', OLD.rank, 'enabled', OLD.enabled
      ),
      'after', json_object(
        'raw_input', NEW.raw_input, 'ideal_output', NEW.ideal_output,
        'app_context', NEW.app_context, 'rank', NEW.rank, 'enabled', NEW.enabled
      )
    )
  );
END;

CREATE TRIGGER style_examples_audit_delete AFTER DELETE ON style_examples BEGIN
  INSERT INTO _history_style_examples (row_id, operation, patch) VALUES (
    OLD.id, 'DELETE', json_object('mode_slug', OLD.mode_slug, 'source', OLD.source)
  );
END;

UPDATE schema_meta SET value = '2' WHERE key = 'schema_version';

COMMIT;
