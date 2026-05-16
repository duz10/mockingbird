//! Database module — local SQLite, single-writer.
//!
//! Driver choice: `rusqlite` with `features = ["bundled"]`. See
//! `docs/adr/0004-rusqlite-over-sqlx.md` for the rationale.
//!
//! Public surface:
//! - [`Database`] — owns the connection and gates open-time setup
//! - [`apply_migrations`] — test-friendly shim around the internal runner
//!
//! Migrations 001-003 are SEALED forever once `phase-1-complete` lands.
//! The hook `block-migration-edit-after-phase-1` enforces this.

pub(crate) mod migrations;
pub(crate) mod prompt_loader;

// Wave 3 — repository modules.
pub mod audit;
pub mod dictionary;
pub mod examples;
pub mod prompts;
pub mod search;
pub mod sessions;
pub mod transcripts;

use rusqlite::Connection;
use std::path::Path;

use crate::error::{AppError, AppResult};

/// Owned SQLite connection with migrations + PRAGMAs already applied.
///
/// Phase 1 Wave 4 will move instances of this into Tauri's `State`
/// management; Wave 2 only needs `Database::open(...)` to work from
/// `lib.rs::run()`'s `.setup(...)` callback and for integration tests
/// to call `Database::open_in_memory()`.
pub struct Database {
    /// The live connection. `pub` so test code in
    /// `src-tauri/tests/db_migrations.rs` can run ad-hoc SQL; in Wave 3
    /// the repository modules will be the only callers and this can
    /// quietly become `pub(crate)`.
    pub conn: Connection,
}

impl Database {
    /// Open the database at `path`, apply pending migrations, run the
    /// integrity check, return the open connection.
    ///
    /// Idempotent: calling on a fully-migrated DB is a no-op aside
    /// from PRAGMA application.
    pub fn open<P: AsRef<Path>>(path: P) -> AppResult<Self> {
        let conn = Connection::open(path)?;
        Self::configure_pragmas(&conn)?;
        migrations::apply_all(&conn)?;
        Self::run_integrity_check(&conn)?;
        Ok(Self { conn })
    }

    /// In-memory variant for tests. **Must be `pub`** (not
    /// `#[cfg(test)]`) because integration tests in `src-tauri/tests/`
    /// are a separate crate and can't see `#[cfg(test)]` items.
    pub fn open_in_memory() -> AppResult<Self> {
        let conn = Connection::open_in_memory()?;
        Self::configure_pragmas(&conn)?;
        migrations::apply_all(&conn)?;
        Self::run_integrity_check(&conn)?;
        Ok(Self { conn })
    }

    fn configure_pragmas(conn: &Connection) -> AppResult<()> {
        // WAL: better concurrency + crash safety. busy_timeout: ride out
        // brief lock contention without erroring. foreign_keys: enforce
        // FK constraints (off by default in SQLite, a famous foot-gun).
        //
        // Note: journal_mode is a `PRAGMA ... = VALUE` that returns the
        // resolved mode; execute_batch is fine because we don't need
        // the return value here.
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;\n\
             PRAGMA foreign_keys = ON;\n\
             PRAGMA busy_timeout = 5000;",
        )?;
        Ok(())
    }

    fn run_integrity_check(conn: &Connection) -> AppResult<()> {
        let result: String = conn.query_row("PRAGMA integrity_check;", [], |row| row.get(0))?;
        if result != "ok" {
            return Err(AppError::Other(format!(
                "integrity_check returned: {result}"
            )));
        }

        // pragma_foreign_key_check is a table-valued PRAGMA; query its
        // row count to surface any violations without parsing rows.
        let violations: i64 = conn.query_row(
            "SELECT COUNT(*) FROM pragma_foreign_key_check;",
            [],
            |row| row.get(0),
        )?;
        if violations != 0 {
            return Err(AppError::Other(format!(
                "foreign_key_check found {violations} violations"
            )));
        }
        Ok(())
    }
}

/// Public shim around the internal migrations runner.
///
/// Exists so integration tests in `src-tauri/tests/db_migrations.rs`
/// can verify the runner's idempotency without us having to expose
/// `db::migrations::apply_all` publicly. Keeping `migrations` itself
/// `pub(crate)` enforces "migrations are an implementation detail" at
/// the type system level.
pub fn apply_migrations(conn: &Connection) -> AppResult<()> {
    migrations::apply_all(conn)
}
