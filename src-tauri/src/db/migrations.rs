//! Migration runner.
//!
//! Migrations are embedded via `include_str!` so the shipped binary
//! contains the schema; there is no separate "migrations directory"
//! to mis-deploy. Numeric `schema_version` tracked in `schema_meta`
//! (set by each migration's trailing UPDATE) drives idempotency.

use rusqlite::Connection;

use super::prompt_loader::substitute_prompt_bodies;
use crate::error::{AppError, AppResult};

const MIGRATION_001: &str = include_str!("migrations/001_initial.sql");
const MIGRATION_002: &str = include_str!("migrations/002_audit_triggers.sql");
const MIGRATION_003: &str = include_str!("migrations/003_seed_modes.sql");
const MIGRATION_004: &str = include_str!("migrations/004_injection_status.sql");
const MIGRATION_005: &str = include_str!("migrations/005_ai_command_modes.sql");

/// Apply every migration with a version strictly greater than the
/// current `schema_version`. Idempotent — returns Ok early if up-to-date.
///
/// Migration 003 contains `__PROMPT_*_BODY__` tokens that are
/// substituted via [`substitute_prompt_bodies`] before execution. 001
/// and 002 are emitted verbatim.
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
    if current < 4 {
        conn.execute_batch(MIGRATION_004)?;
    }
    if current < 5 {
        let prepared = substitute_prompt_bodies(MIGRATION_005);
        conn.execute_batch(&prepared)?;
    }
    Ok(())
}

/// Read `schema_meta.schema_version`. Returns 0 if `schema_meta`
/// doesn't exist yet (a fresh DB).
fn read_current_version(conn: &Connection) -> AppResult<u32> {
    let exists: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master \
         WHERE type='table' AND name='schema_meta';",
        [],
        |row| row.get(0),
    )?;
    if exists == 0 {
        return Ok(0);
    }

    let raw: String = conn
        .query_row(
            "SELECT value FROM schema_meta WHERE key='schema_version';",
            [],
            |row| row.get(0),
        )
        .unwrap_or_else(|_| "0".to_string());

    raw.parse::<u32>()
        .map_err(|e| AppError::Other(format!("schema_version not parseable: {raw} ({e})")))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `read_current_version` on a brand-new in-memory connection
    /// (no `schema_meta` yet) must return 0, not error.
    #[test]
    fn read_current_version_on_empty_db_returns_zero() {
        let conn = Connection::open_in_memory().expect("open in-memory");
        assert_eq!(read_current_version(&conn).expect("read version"), 0);
    }

    /// Applying all three migrations against a fresh in-memory DB
    /// must leave `schema_version = 3`. (Smoke-level; the integration
    /// test suite covers full table/trigger/FTS5 assertions.)
    #[test]
    fn apply_all_brings_fresh_db_to_version_5() {
        let conn = Connection::open_in_memory().expect("open in-memory");
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .expect("pragma fk");
        apply_all(&conn).expect("apply_all");
        let v: String = conn
            .query_row(
                "SELECT value FROM schema_meta WHERE key='schema_version'",
                [],
                |r| r.get(0),
            )
            .expect("read schema_version");
        assert_eq!(v, "5");
    }

    /// Second `apply_all` call against a fully-migrated DB is a no-op.
    #[test]
    fn apply_all_is_idempotent() {
        let conn = Connection::open_in_memory().expect("open in-memory");
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .expect("pragma fk");
        apply_all(&conn).expect("first apply_all");
        apply_all(&conn).expect("second apply_all should be a no-op");
        let v: String = conn
            .query_row(
                "SELECT value FROM schema_meta WHERE key='schema_version'",
                [],
                |r| r.get(0),
            )
            .expect("read schema_version");
        assert_eq!(v, "5");
    }

    /// Migration 005 seeds the AI command modes (disabled by default).
    #[test]
    fn migration_005_seeds_ai_command_modes() {
        let conn = Connection::open_in_memory().expect("open in-memory");
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        apply_all(&conn).unwrap();
        for slug in ["rewrite", "expand", "summarize"] {
            let count: i64 = conn
                .query_row("SELECT COUNT(*) FROM modes WHERE slug = ?1", [slug], |r| {
                    r.get(0)
                })
                .unwrap();
            assert_eq!(count, 1, "missing mode row for {slug}");
            let enabled: i64 = conn
                .query_row("SELECT enabled FROM modes WHERE slug = ?1", [slug], |r| {
                    r.get(0)
                })
                .unwrap();
            assert_eq!(enabled, 0, "{slug} should ship disabled by default");
        }
    }

    /// Migration 004 actually adds the column it claims to add.
    #[test]
    fn migration_004_adds_injection_status_column() {
        let conn = Connection::open_in_memory().expect("open in-memory");
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        apply_all(&conn).unwrap();
        // `PRAGMA table_info` enumerates columns; check the injection_status one is there.
        let mut stmt = conn.prepare("PRAGMA table_info(sessions)").unwrap();
        let names: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .map(|n| n.unwrap())
            .collect();
        assert!(
            names.iter().any(|n| n == "injection_status"),
            "injection_status column missing; got {names:?}"
        );
    }
}
