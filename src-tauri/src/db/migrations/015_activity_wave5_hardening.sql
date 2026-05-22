-- 015_activity_wave5_hardening.sql
-- Phase 10 Wave 5. Schema_version 14 → 15.
--
-- Adds the hardening schema for activity-capture:
--
--   activity_exclusion_rules  — user-editable rules that filter events
--                               at capture time (ADR 0043).
--   activity_blocks.raw_events_purged_at  — retention breadcrumb on
--                               Blocks whose raw events have been
--                               purged by the retention sweep (ADR 0042).
--
-- ADR refs: 0042 (retention cascade semantics), 0043 (exclusion
-- rule shape + capture-time enforcement).
--
-- Purely additive. No prior migration is touched. No new triggers.
--
-- Built-in exclusion rules are seeded with `is_builtin = 1`. The UI
-- allows users to disable them (`enabled = 0`) but not delete them.
-- User-created rules use a freshly-generated id and `is_builtin = 0`.
--
-- Timestamps use `CAST(strftime('%s','now') AS INTEGER) * 1000` for
-- portability across SQLite versions (`unixepoch()` is 3.38+).

PRAGMA foreign_keys = ON;

BEGIN TRANSACTION;

-- =============================================================================
-- activity_exclusion_rules
-- =============================================================================
CREATE TABLE activity_exclusion_rules (
  id          TEXT PRIMARY KEY,
  kind        TEXT NOT NULL,                     -- 'app_glob' | 'title_regex' | 'system'
  pattern     TEXT NOT NULL,
  enabled     INTEGER NOT NULL DEFAULT 1,
  is_builtin  INTEGER NOT NULL DEFAULT 0,
  note        TEXT,
  created_at  INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL
);

CREATE INDEX idx_activity_exclusion_rules_enabled
  ON activity_exclusion_rules(enabled, kind);

-- Built-in seed rules. Match ADR 0043 §Built-in rules table.
INSERT INTO activity_exclusion_rules
  (id, kind, pattern, enabled, is_builtin, note, created_at, updated_at)
VALUES
  ('builtin-1password',     'app_glob',    '1Password*',        1, 1, '1Password credentials manager',
     CAST(strftime('%s','now') AS INTEGER) * 1000, CAST(strftime('%s','now') AS INTEGER) * 1000),
  ('builtin-bitwarden',     'app_glob',    'Bitwarden*',        1, 1, 'Bitwarden credentials manager',
     CAST(strftime('%s','now') AS INTEGER) * 1000, CAST(strftime('%s','now') AS INTEGER) * 1000),
  ('builtin-keepass',       'app_glob',    'KeePass*',          1, 1, 'KeePass credentials manager',
     CAST(strftime('%s','now') AS INTEGER) * 1000, CAST(strftime('%s','now') AS INTEGER) * 1000),
  ('builtin-lastpass',      'app_glob',    'LastPass*',         1, 1, 'LastPass credentials manager',
     CAST(strftime('%s','now') AS INTEGER) * 1000, CAST(strftime('%s','now') AS INTEGER) * 1000),
  ('builtin-uac',           'app_glob',    'consent.exe',       1, 1, 'Windows UAC consent dialog',
     CAST(strftime('%s','now') AS INTEGER) * 1000, CAST(strftime('%s','now') AS INTEGER) * 1000),
  ('builtin-winlogon',      'app_glob',    'LogonUI.exe',       1, 1, 'Windows lock screen / sign-in',
     CAST(strftime('%s','now') AS INTEGER) * 1000, CAST(strftime('%s','now') AS INTEGER) * 1000),
  ('builtin-browser-bank',  'title_regex', '(?i)\b(bank|login|password|signin|sign-in)\b',
     1, 1, 'Browser tabs on sign-in / banking pages',
     CAST(strftime('%s','now') AS INTEGER) * 1000, CAST(strftime('%s','now') AS INTEGER) * 1000),
  ('builtin-secure-input',  'system',      'password_field_active',
     1, 1, 'Drop snapshot when UIA reports an active password field',
     CAST(strftime('%s','now') AS INTEGER) * 1000, CAST(strftime('%s','now') AS INTEGER) * 1000);

-- =============================================================================
-- activity_blocks: retention breadcrumb (ADR 0042)
-- =============================================================================
ALTER TABLE activity_blocks ADD COLUMN raw_events_purged_at INTEGER;

UPDATE schema_meta SET value = '15' WHERE key = 'schema_version';

COMMIT;
