//! DB-provenance + default-config helpers for the dictation runtime.
//!
//! Split out of `runtime.rs` (ADR 0063) to keep that module under the
//! 600-line cap. These are cross-platform and are also consumed by the
//! learning + KG ingest paths, so `runtime.rs` re-exports
//! [`bootstrap_provenance_rows`] + [`default_normal_config`] via
//! `pub use` to preserve the `dictation::runtime::*` public paths.

use rusqlite::Connection;

use super::OrchestratorConfig;
use crate::error::{AppError, AppResult};

/// Ensure a `dictionary_snapshots` row + an `example_sets` row exist,
/// seeding empty ones if the DB doesn't have any yet.
///
/// The orchestrator's `NewSession` requires non-null FKs for both.
/// Phase 1's seed migration only populates `prompts` + `modes`; this
/// fills the gap on first launch.
///
/// Returns `(dictionary_snapshot_id, example_set_id)`.
pub fn bootstrap_provenance_rows(conn: &Connection) -> AppResult<(i64, i64)> {
    let dict_id: i64 = match conn
        .query_row(
            "SELECT id FROM dictionary_snapshots ORDER BY id ASC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .ok()
    {
        Some(id) => id,
        None => {
            conn.execute(
                "INSERT INTO dictionary_snapshots (term_ids) VALUES ('[]')",
                [],
            )?;
            conn.last_insert_rowid()
        }
    };
    let example_id: i64 = match conn
        .query_row(
            "SELECT id FROM example_sets ORDER BY id ASC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .ok()
    {
        Some(id) => id,
        None => {
            conn.execute(
                "INSERT INTO example_sets (mode_slug, example_ids) VALUES ('normal', '[]')",
                [],
            )?;
            conn.last_insert_rowid()
        }
    };
    Ok((dict_id, example_id))
}

/// Build the default Wave 4.5 [`OrchestratorConfig`] for Normal mode.
///
/// Phase 5 will swap this for settings-driven config that respects
/// the user's active mode.
pub fn default_normal_config(conn: &Connection) -> AppResult<OrchestratorConfig> {
    let (dict_id, example_id) = bootstrap_provenance_rows(conn)?;

    // Modes table: id=1 normal, id=2 verbose, id=3 fragment.
    let prompt_id: i64 = conn
        .query_row("SELECT prompt_id FROM modes WHERE slug='normal'", [], |r| {
            r.get(0)
        })
        .map_err(|e| AppError::Other(format!("lookup normal-mode prompt_id: {e}")))?;

    Ok(OrchestratorConfig {
        mode_id: 1,
        mode_slug: "normal".into(),
        prompt_id,
        dictionary_snapshot_id: dict_id,
        example_set_id: example_id,
        hotkey_label: "RightAlt".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    #[test]
    fn bootstrap_creates_rows_on_empty_db() {
        let db = Database::open_in_memory().unwrap();
        let (dict, ex) = bootstrap_provenance_rows(&db.conn).unwrap();
        assert!(dict > 0);
        assert!(ex > 0);
    }

    #[test]
    fn bootstrap_is_idempotent() {
        let db = Database::open_in_memory().unwrap();
        let (d1, e1) = bootstrap_provenance_rows(&db.conn).unwrap();
        let (d2, e2) = bootstrap_provenance_rows(&db.conn).unwrap();
        assert_eq!(d1, d2);
        assert_eq!(e1, e2);
    }

    #[test]
    fn default_normal_config_resolves_prompt_id() {
        let db = Database::open_in_memory().unwrap();
        let cfg = default_normal_config(&db.conn).unwrap();
        assert_eq!(cfg.mode_id, 1);
        assert_eq!(cfg.mode_slug, "normal");
        assert!(cfg.prompt_id > 0);
        assert_eq!(cfg.hotkey_label, "RightAlt");
    }
}
