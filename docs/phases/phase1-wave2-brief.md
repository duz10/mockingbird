# Phase 1 Wave 2 — Implementation brief

> **Read this BEFORE invoking migration-author.** It captures every
> design decision PLAN §7 doesn't fully pin down, so Wave 2 is
> mechanical implementation, not architectural decision-making under
> context pressure. Written end-of-Wave-1 by code-puppy-adeb7b with
> fresh context on PLAN §7.

## Tasks in scope

| bd id    | File / artifact                                  | Delegate          |
|----------|--------------------------------------------------|-------------------|
| `mb-4qg` | `src-tauri/src/db/migrations/001_initial.sql`    | migration-author  |
| `mb-l6d` | `src-tauri/src/db/migrations/002_audit_triggers.sql` | migration-author |
| `mb-7u9` | `src-tauri/src/db/migrations/003_seed_modes.sql` | migration-author  |
| `mb-o0d` | `src-tauri/src/db/{mod.rs,migrations.rs,prompt_loader.rs}` | code-puppy |
| `mb-rzf` | `src-tauri/tests/db_migrations.rs`               | migration-author  |

## Migration 001 — PLAN §7 verbatim, with these clarifications

001 contains everything PLAN §7's "Migration 001 — core tables + FTS5"
code block specifies. Implement it verbatim. The following clarifications
resolve ambiguities I discovered reading the section closely:

1. **Wrap in `BEGIN TRANSACTION; ... COMMIT;`.** PLAN doesn't show the
   transaction frame, but the runner relies on it for atomicity (a
   half-applied migration would brick the DB on retry).
2. **`schema_meta` initial INSERT shows `value = '1'`, then the bottom
   shows `UPDATE schema_meta SET value = '1'`.** The trailing UPDATE
   is a no-op; keep it for symmetry with 002/003 (they each UPDATE the
   schema_version at the end). Future readers will scan for the pattern.
3. **The FTS5 `transcripts_fts_delete` trigger uses the contentless-fts
   delete idiom** (`INSERT INTO transcripts_fts(transcripts_fts, ...) VALUES('delete', ...)`).
   That's correct for `content=` external-content tables — copy exactly.
4. **Raw-transcript immutability is NOT enforced by a SQL trigger in 001.**
   PLAN leaves this to the hook engine. **Decision deferred:** Wave 5 may
   add an `AFTER UPDATE OF text` trigger on `transcripts` that
   `RAISE(ABORT, ...)` when `OLD.stage='raw'` — belt-and-suspenders with
   the hook. Logging this as a separate Wave 5 follow-up task rather than
   bundling into 001, since once 001 ships and is tagged, we can't add the
   trigger to it — would need a `004_raw_immutability_trigger.sql`. **Don't
   add the trigger to 001.** Note in LESSONS that we considered it.
5. **No `DEFAULT (datetime('now'))` columns** — PLAN uses
   `DEFAULT CURRENT_TIMESTAMP` (a SQLite keyword that resolves to UTC).
   Keep this exactly.

## Migration 002 — the four audit-table extrapolation

PLAN shows only the `dictionary` pattern. Here is the full SQL for all
four tables, with explicit column projections. Pattern is consistent:

- **INSERT trigger:** `patch` = a flat JSON object of the row's mutable
  columns (excluding `id` and `created_at`).
- **UPDATE trigger:** `patch` = `{"before": {...}, "after": {...}}`
  containing the same mutable subset. Lets rollback diff.
- **DELETE trigger:** `patch` = the minimal identifying key (so we know
  what was deleted; the full row is in the prior INSERT/UPDATE history).

⚠️ **PLAN bug found:** PLAN's dictionary UPDATE trigger references
`OLD.enabled` / `NEW.enabled` — the `dictionary` table has no `enabled`
column. Removed that field below; documenting the deviation here so it's
reviewable.

```sql
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
```

**Count check:** 4 tables × 3 triggers = 12 audit triggers + 2 FTS5
triggers from 001 = **14 triggers total** at end of migration 002.
Integration test asserts this exact count.

## Migration 003 — seed, with token substitution

```sql
-- Tokens __PROMPT_*_BODY__ are substituted by the runner BEFORE
-- execute_batch from contents of src-tauri/src/cleanup/prompts/*.md.
-- Single-quotes inside bodies are doubled by the substitution helper.

BEGIN TRANSACTION;

INSERT INTO prompts (mode_slug, version, body) VALUES
  ('normal',   1, '__PROMPT_NORMAL_BODY__'),
  ('verbose',  1, '__PROMPT_VERBOSE_BODY__'),
  ('fragment', 1, '__PROMPT_FRAGMENT_BODY__');

INSERT INTO modes (slug, display_name, hotkey, provider, model_id, prompt_id, temperature, max_tokens) VALUES
  ('normal',   'Normal',   'Ctrl+Win',       'ollama', 'qwen2.5:3b-instruct-q4_K_M',
    (SELECT id FROM prompts WHERE mode_slug='normal'   AND version=1), 0.3, 2048),
  ('verbose',  'Verbose',  'Ctrl+Shift+Win', 'ollama', 'qwen2.5:3b-instruct-q4_K_M',
    (SELECT id FROM prompts WHERE mode_slug='verbose'  AND version=1), 0.3, 4096),
  ('fragment', 'Fragment', 'Ctrl+Alt+Win',   'ollama', 'gemma2:2b-instruct-q4_K_M',
    (SELECT id FROM prompts WHERE mode_slug='fragment' AND version=1), 0.2, 1024);

UPDATE schema_meta SET value = '3' WHERE key = 'schema_version';

COMMIT;
```

**Deviation from PLAN:** PLAN's seed uses hardcoded `prompt_id = 1, 2, 3`.
That works because the prompts are inserted in order and start at id=1
on a fresh DB — but it's brittle. Using `(SELECT id FROM prompts ...)`
sub-selects is robust to any future reorder. Document this deviation in
LESSONS as "non-trivial PLAN improvement; reviewed at Wave 2".

## Migration runner — file layout and function signatures

### `src-tauri/src/db/mod.rs` (~80 lines target)

```rust
//! Database module — see ADR 0004 for driver rationale.
mod migrations;
mod prompt_loader;

use rusqlite::Connection;
use std::path::Path;
use crate::error::{AppError, AppResult};

pub struct Database {
    pub conn: Connection,
}

impl Database {
    /// Open the database at `path`, apply pending migrations, return
    /// the open connection. Idempotent: calling on a fully-migrated DB
    /// is a no-op aside from PRAGMA application.
    pub fn open<P: AsRef<Path>>(path: P) -> AppResult<Self> {
        let conn = Connection::open(path)?;
        Self::configure_pragmas(&conn)?;
        migrations::apply_all(&conn)?;
        Self::run_integrity_check(&conn)?;
        Ok(Self { conn })
    }

    /// Test-only helper: open an in-memory DB with migrations applied.
    #[cfg(test)]
    pub fn open_in_memory() -> AppResult<Self> {
        let conn = Connection::open_in_memory()?;
        Self::configure_pragmas(&conn)?;
        migrations::apply_all(&conn)?;
        Self::run_integrity_check(&conn)?;
        Ok(Self { conn })
    }

    fn configure_pragmas(conn: &Connection) -> AppResult<()> {
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;\n\
             PRAGMA foreign_keys = ON;\n\
             PRAGMA busy_timeout = 5000;"
        )?;
        Ok(())
    }

    fn run_integrity_check(conn: &Connection) -> AppResult<()> {
        let result: String = conn.query_row(
            "PRAGMA integrity_check;", [], |row| row.get(0)
        )?;
        if result != "ok" {
            return Err(AppError::Other(
                format!("integrity_check returned: {result}")
            ));
        }
        let result: String = conn.query_row(
            "SELECT COUNT(*) FROM pragma_foreign_key_check;", [], |row| {
                row.get::<_, i64>(0).map(|n| n.to_string())
            }
        )?;
        if result != "0" {
            return Err(AppError::Other(
                format!("foreign_key_check found {result} violations")
            ));
        }
        Ok(())
    }
}
```

### `src-tauri/src/db/migrations.rs` (~120 lines target)

```rust
use rusqlite::Connection;
use crate::error::{AppError, AppResult};
use super::prompt_loader::substitute_prompt_bodies;

const MIGRATION_001: &str = include_str!("migrations/001_initial.sql");
const MIGRATION_002: &str = include_str!("migrations/002_audit_triggers.sql");
const MIGRATION_003: &str = include_str!("migrations/003_seed_modes.sql");

/// Apply every migration with a version strictly greater than the
/// current `schema_version`. Idempotent: returns Ok early if up-to-date.
pub fn apply_all(conn: &Connection) -> AppResult<()> {
    let current = read_current_version(conn)?;

    if current < 1 {
        conn.execute_batch(MIGRATION_001)?;
    }
    if current < 2 {
        conn.execute_batch(MIGRATION_002)?;
    }
    if current < 3 {
        let prepared = substitute_prompt_bodies(MIGRATION_003);
        conn.execute_batch(&prepared)?;
    }
    Ok(())
}

/// Read `schema_meta.schema_version`. Returns 0 if `schema_meta` doesn't
/// exist yet (a fresh DB).
fn read_current_version(conn: &Connection) -> AppResult<u32> {
    let exists: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master \
         WHERE type='table' AND name='schema_meta';",
        [], |row| row.get(0)
    )?;
    if exists == 0 { return Ok(0); }

    let raw: String = conn.query_row(
        "SELECT value FROM schema_meta WHERE key='schema_version';",
        [], |row| row.get(0)
    ).unwrap_or_else(|_| "0".to_string());

    raw.parse::<u32>().map_err(|e| AppError::Other(
        format!("schema_version not parseable: {raw} ({e})")
    ))
}
```

### `src-tauri/src/db/prompt_loader.rs` (~40 lines target)

```rust
//! Token substitution for migration 003.
//!
//! Prompt bodies live in cleanup/prompts/*.md as the single source of
//! truth. Migration 003 contains tokens like `__PROMPT_NORMAL_BODY__`
//! that are substituted at runtime — keeping prompt edits out of SQL
//! files (which would be sealed after phase-1-complete).

const PROMPT_NORMAL:   &str = include_str!("../cleanup/prompts/normal.md");
const PROMPT_VERBOSE:  &str = include_str!("../cleanup/prompts/verbose.md");
const PROMPT_FRAGMENT: &str = include_str!("../cleanup/prompts/fragment.md");

/// Replace `__PROMPT_*_BODY__` tokens with SQL-escaped prompt bodies.
pub fn substitute_prompt_bodies(sql: &str) -> String {
    sql.replace("__PROMPT_NORMAL_BODY__",   &sql_escape(PROMPT_NORMAL))
       .replace("__PROMPT_VERBOSE_BODY__",  &sql_escape(PROMPT_VERBOSE))
       .replace("__PROMPT_FRAGMENT_BODY__", &sql_escape(PROMPT_FRAGMENT))
}

/// Double single-quotes so the body can sit inside a SQL string literal.
fn sql_escape(s: &str) -> String { s.replace('\'', "''") }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substitution_replaces_all_three_tokens() {
        let template = "x __PROMPT_NORMAL_BODY__ y __PROMPT_VERBOSE_BODY__ z __PROMPT_FRAGMENT_BODY__";
        let out = substitute_prompt_bodies(template);
        assert!(!out.contains("__PROMPT_"));
    }

    #[test]
    fn sql_escape_doubles_single_quotes() {
        assert_eq!(sql_escape("don't"), "don''t");
    }
}
```

## Integration tests — `src-tauri/tests/db_migrations.rs`

```rust
//! End-to-end migration tests using rstest fixtures + tempfile DBs.

use mockingbird_lib::db::Database;
use tempfile::tempdir;

/// Fresh-tempdb fixture: returns a Database with all migrations applied.
fn fresh_db() -> Database {
    Database::open_in_memory().expect("migrate fresh in-memory db")
}

#[test]
fn schema_version_is_3_after_apply() {
    let db = fresh_db();
    let v: String = db.conn.query_row(
        "SELECT value FROM schema_meta WHERE key='schema_version'",
        [], |r| r.get(0)
    ).unwrap();
    assert_eq!(v, "3");
}

#[test]
fn all_expected_tables_exist() {
    let db = fresh_db();
    let expected = [
        "schema_meta", "prompts", "modes", "dictionary", "dictionary_snapshots",
        "style_examples", "example_sets", "sessions", "transcripts",
        "corrections", "settings", "learning_runs",
        "_history_modes", "_history_prompts", "_history_dictionary", "_history_style_examples",
    ];
    for t in expected {
        let n: i64 = db.conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
            [t], |r| r.get(0)
        ).unwrap();
        assert_eq!(n, 1, "missing table: {t}");
    }
}

#[test]
fn trigger_count_is_14() {
    // 2 FTS5 triggers (transcripts_fts_insert/delete) + 4 tables * 3 audit = 14.
    let db = fresh_db();
    let n: i64 = db.conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='trigger'",
        [], |r| r.get(0)
    ).unwrap();
    assert_eq!(n, 14);
}

#[test]
fn seeded_modes_and_prompts_present() {
    let db = fresh_db();
    let prompts: i64 = db.conn.query_row(
        "SELECT COUNT(*) FROM prompts", [], |r| r.get(0)
    ).unwrap();
    assert_eq!(prompts, 3);
    let modes: i64 = db.conn.query_row(
        "SELECT COUNT(*) FROM modes", [], |r| r.get(0)
    ).unwrap();
    assert_eq!(modes, 3);

    // Seed inserts MUST fire audit triggers (migration 002 ran before 003).
    let history_prompts: i64 = db.conn.query_row(
        "SELECT COUNT(*) FROM _history_prompts WHERE operation='INSERT'",
        [], |r| r.get(0)
    ).unwrap();
    assert_eq!(history_prompts, 3, "audit triggers should fire on seed");
    let history_modes: i64 = db.conn.query_row(
        "SELECT COUNT(*) FROM _history_modes WHERE operation='INSERT'",
        [], |r| r.get(0)
    ).unwrap();
    assert_eq!(history_modes, 3);
}

#[test]
fn audit_update_records_before_and_after() {
    let db = fresh_db();
    db.conn.execute(
        "INSERT INTO dictionary (term, canonical, source) VALUES ('foo','Foo','user')",
        []
    ).unwrap();
    db.conn.execute(
        "UPDATE dictionary SET canonical='FOO' WHERE term='foo'", []
    ).unwrap();
    let n: i64 = db.conn.query_row(
        "SELECT COUNT(*) FROM _history_dictionary WHERE operation='UPDATE'",
        [], |r| r.get(0)
    ).unwrap();
    assert_eq!(n, 1);

    let patch: String = db.conn.query_row(
        "SELECT patch FROM _history_dictionary WHERE operation='UPDATE'",
        [], |r| r.get(0)
    ).unwrap();
    assert!(patch.contains("\"before\""));
    assert!(patch.contains("\"after\""));
    assert!(patch.contains("FOO"));
    assert!(patch.contains("Foo"));
}

#[test]
fn fts5_round_trip_finds_inserted_transcript() {
    let db = fresh_db();

    // Need a session row first (transcripts.session_id is NOT NULL FK).
    db.conn.execute(
        "INSERT INTO sessions (uuid, mode_id, hotkey_pressed, started_at, recording_ended_at, status, audio_duration_ms) \
         VALUES ('test-uuid', 1, 'Ctrl+Win', '2026-05-15T00:00:00Z', '2026-05-15T00:00:05Z', 'complete', 5000)",
        []
    ).unwrap();
    db.conn.execute(
        "INSERT INTO transcripts (session_id, stage, text) VALUES (1, 'raw', 'hello world from fts5')",
        []
    ).unwrap();

    let hit: String = db.conn.query_row(
        "SELECT t.text FROM transcripts t \
         JOIN transcripts_fts f ON f.rowid = t.id \
         WHERE transcripts_fts MATCH 'hello'",
        [], |r| r.get(0)
    ).unwrap();
    assert_eq!(hit, "hello world from fts5");
}

#[test]
fn apply_all_is_idempotent() {
    let db = fresh_db();
    // Second call must be a no-op (assertion: doesn't panic, schema_version still 3).
    mockingbird_lib::db::migrations::apply_all(&db.conn)
        .expect("second apply_all should be Ok");
    let v: String = db.conn.query_row(
        "SELECT value FROM schema_meta WHERE key='schema_version'",
        [], |r| r.get(0)
    ).unwrap();
    assert_eq!(v, "3");
}
```

⚠️ **Visibility note:** the idempotency test calls
`mockingbird_lib::db::migrations::apply_all` — that requires `db` to be
`pub mod` and `migrations` to be `pub mod`, OR an integration-test-only
re-export. Cleanest: make `db` `pub`, keep `migrations` `pub(crate)`,
and add a `pub fn apply_migrations(conn: &Connection) -> AppResult<()>`
shim in `db/mod.rs` that the integration test calls. Document this.

## Wiring into `lib.rs`

Add `pub mod db;` and `pub mod error;` to `lib.rs` exports. The
`run()` function gains:

```rust
.setup(|app| {
    let app_data = app.path().app_data_dir()?;
    std::fs::create_dir_all(&app_data)?;
    let db_path = app_data.join("mockingbird.db");
    let _db = db::Database::open(&db_path)
        .map_err(|e| tauri::Error::from(std::io::Error::new(
            std::io::ErrorKind::Other, e.to_string()
        )))?;
    // _db will move into managed state in Wave 4 (commands need it).
    tracing::info!(?db_path, "database ready");
    Ok(())
})
```

(In Wave 4, `_db` gets `.manage(db)`-ed so `#[tauri::command]`s can
inject `tauri::State<Database>`. Not Wave 2's job.)

## Exit checklist for Wave 2

- [ ] `cargo check --workspace` green
- [ ] `cargo clippy --all-targets -- -D warnings` clean
- [ ] `cargo test --workspace` green (incl. 7+ integration tests above)
- [ ] `cargo fmt --check` clean
- [ ] `bd close mb-4qg mb-l6d mb-7u9 mb-o0d mb-rzf`
- [ ] STATUS.md updated to "Wave 2 ✅; Waves 3-5 queued"
- [ ] LESSONS.md: anything non-obvious encountered (e.g. rusqlite
      FTS5 trigger syntax surprises, `_history_*` row count drift)
- [ ] Commit message: `feat(phase-1-wave-2): migrations 001-003 + runner + tests`
- [ ] **DO NOT TAG phase-1-complete YET.** Tag lands at Wave 5 after
      DB repos + app shell + judges.

## Known risks for Wave 2 (carry from phase1.md)

1. `rusqlite-bundled` + FTS5: first test failure here is almost certainly
   a build-flag issue, not your SQL. Sanity check: `cargo run --example`
   a tiny FTS5 round-trip standalone if integration tests fail mysteriously.
2. `include_str!` with Windows backslashes: use forward slashes only.
3. The integration test for `audit_update_records_before_and_after`
   depends on the seed already having put 3 prompts and 3 modes in
   `_history_*` (assertions check exact counts elsewhere — that test
   is robust to that).
4. **Don't add `pub mod migrations` to public API.** Keep migrations
   `pub(crate)` and expose a single `db::apply_migrations(&conn)` shim
   for the integration test (and a `Database::open_in_memory()` helper
   for tests). Migrations are an internal implementation detail.

## Out of scope for Wave 2 (do these in Waves 3-5)

- DB repository modules (`db/transcripts.rs`, `db/sessions.rs`, etc.)
- Settings facade
- Logging module
- Tray module
- `#[tauri::command]` handlers
- Tag `phase-1-complete`
