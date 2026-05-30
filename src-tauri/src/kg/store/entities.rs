//! `kg_entities` row reads + writes.
//!
//! One row per canonical entity (`Mom`, `Acme Corp`, `Project Q3`).
//! Aliases land as a JSON array on the row rather than in a separate
//! table; see migration 024 for the rationale.
//!
//! ## Upsert semantics
//!
//! The model emits the same entity (by canonical `(name, entity_type)`)
//! across many segments and across many dictations. `upsert_entity`
//! collapses these to one row:
//!
//! - First sight: `INSERT` with the supplied aliases JSON-encoded.
//! - Subsequent sights: `UPDATE` merging any new aliases into the
//!   stored JSON array (set-union, sorted by first-seen order),
//!   bumping `updated_at`.
//!
//! Returns the canonical `kg_entities.id` either way — the caller
//! threads it into `kg_entity_mentions.entity_id`.

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::{AppError, AppResult};

/// Upsert one canonical entity. Returns the resolved `kg_entities.id`.
///
/// `entity_type` is the lowercase wire form (`"person"`,
/// `"organization"`, `"object"`, `"place"`, `"project"`) — see
/// [`crate::kg::passes::EntityType::as_str`]. The UNIQUE constraint
/// is on `(name, entity_type)`; the same surface form under two
/// different types is two distinct rows (correct behaviour: "Mark"
/// the person vs "Mark" the project).
pub(crate) fn upsert_entity(
    conn: &Connection,
    name: &str,
    entity_type: &str,
    new_aliases: &[String],
    now_iso: &str,
) -> AppResult<i64> {
    // Try to locate an existing row first; we need its id either
    // way (returned to the caller) and on hit we may need to merge
    // aliases. SELECT-then-INSERT-or-UPDATE is the simplest shape
    // here -- ON CONFLICT DO UPDATE would also work but the JSON
    // merge is hard to express in pure SQL across SQLite versions.
    let existing: Option<(i64, String)> = conn
        .query_row(
            "SELECT id, aliases_json FROM kg_entities \
             WHERE name = ?1 AND entity_type = ?2;",
            params![name, entity_type],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;

    if let Some((id, aliases_json)) = existing {
        let merged = merge_aliases(&aliases_json, new_aliases)?;
        // Only write back if something changed -- saves an audit-trigger-style
        // bump and keeps `updated_at` honest about meaningful events.
        if merged != aliases_json {
            conn.execute(
                "UPDATE kg_entities \
                 SET aliases_json = ?1, updated_at = ?2 \
                 WHERE id = ?3;",
                params![merged, now_iso, id],
            )?;
        }
        return Ok(id);
    }

    // First sight: encode the supplied aliases and insert.
    let initial_aliases = serde_json::to_string(new_aliases)
        .map_err(|e| AppError::Other(format!("alias json encode: {e}")))?;
    conn.execute(
        "INSERT INTO kg_entities (name, entity_type, aliases_json, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?4);",
        params![name, entity_type, initial_aliases, now_iso],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Merge `new_aliases` into the existing JSON-encoded alias array,
/// preserving first-seen order and de-duplicating case-sensitively.
/// Returns the new JSON-encoded string.
fn merge_aliases(existing_json: &str, new_aliases: &[String]) -> AppResult<String> {
    let mut existing: Vec<String> = serde_json::from_str(existing_json)
        .map_err(|e| AppError::Other(format!("alias json decode: {e}")))?;
    for candidate in new_aliases {
        if !existing.iter().any(|s| s == candidate) {
            existing.push(candidate.clone());
        }
    }
    serde_json::to_string(&existing)
        .map_err(|e| AppError::Other(format!("alias json re-encode: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    /// Minimum schema for unit-testing this module in isolation -- just
    /// the kg_entities table. Keeps the test setup tight; full
    /// `apply_all` is exercised by the migration-level test.
    fn make_test_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE kg_entities (
               id INTEGER PRIMARY KEY,
               name TEXT NOT NULL,
               entity_type TEXT NOT NULL,
               aliases_json TEXT NOT NULL DEFAULT '[]',
               created_at TEXT NOT NULL,
               updated_at TEXT NOT NULL,
               UNIQUE(name, entity_type)
             );",
        )
        .unwrap();
        conn
    }

    #[test]
    fn first_upsert_inserts_row_with_supplied_aliases() {
        let conn = make_test_conn();
        let id = upsert_entity(
            &conn,
            "Mom",
            "person",
            &["Mama".into()],
            "2026-05-30T00:00:00Z",
        )
        .unwrap();
        assert_eq!(id, 1);

        let (name, etype, aliases, created, updated): (String, String, String, String, String) =
            conn.query_row(
                "SELECT name, entity_type, aliases_json, created_at, updated_at \
                 FROM kg_entities WHERE id = 1;",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap();
        assert_eq!(name, "Mom");
        assert_eq!(etype, "person");
        assert_eq!(aliases, r#"["Mama"]"#);
        assert_eq!(created, updated, "first sight: created_at == updated_at");
    }

    #[test]
    fn second_upsert_returns_same_id_and_merges_new_aliases() {
        let conn = make_test_conn();
        let id1 = upsert_entity(
            &conn,
            "Mom",
            "person",
            &["Mama".into()],
            "2026-05-30T00:00:00Z",
        )
        .unwrap();
        let id2 = upsert_entity(
            &conn,
            "Mom",
            "person",
            &["Mommy".into(), "Mama".into()],
            "2026-05-30T01:00:00Z",
        )
        .unwrap();
        assert_eq!(id1, id2, "same (name, type) -> same row id");

        let (aliases, updated_at): (String, String) = conn
            .query_row(
                "SELECT aliases_json, updated_at FROM kg_entities WHERE id = ?1;",
                params![id1],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            aliases, r#"["Mama","Mommy"]"#,
            "set-union, first-seen order"
        );
        assert_eq!(updated_at, "2026-05-30T01:00:00Z");
    }

    #[test]
    fn second_upsert_with_same_aliases_does_not_bump_updated_at() {
        let conn = make_test_conn();
        let _ = upsert_entity(
            &conn,
            "Mom",
            "person",
            &["Mama".into()],
            "2026-05-30T00:00:00Z",
        )
        .unwrap();
        let _ = upsert_entity(
            &conn,
            "Mom",
            "person",
            &["Mama".into()],
            "2026-05-30T01:00:00Z",
        )
        .unwrap();
        let updated_at: String = conn
            .query_row(
                "SELECT updated_at FROM kg_entities WHERE id = 1;",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            updated_at, "2026-05-30T00:00:00Z",
            "no-op merge must not bump updated_at"
        );
    }

    #[test]
    fn same_name_different_type_yields_two_rows() {
        let conn = make_test_conn();
        let person = upsert_entity(&conn, "Mark", "person", &[], "t").unwrap();
        let project = upsert_entity(&conn, "Mark", "project", &[], "t").unwrap();
        assert_ne!(person, project);
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM kg_entities;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2);
    }
}
