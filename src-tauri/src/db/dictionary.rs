//! Dictionary repository — user vocabulary substitutions.
//!
//! Stores terms with optional canonical forms. The cleanup pipeline
//! looks these up before the LLM pass to lock in proper-noun spellings
//! deterministically.
//!
//! Snapshots: `create_snapshot` captures the current entry set as a
//! JSON array of ids, stored in `dictionary_snapshots`. Sessions pin
//! their `dictionary_snapshot_id` to the snapshot in force at session
//! start — that's how we get total provenance for cleanup edits.

use rusqlite::{params, Connection};

use crate::error::AppResult;

#[derive(Debug, Clone)]
pub struct NewDictionaryEntry {
    pub term: String,
    pub canonical: Option<String>,
    pub source: String,
    pub confidence: Option<f64>,
    pub app_context: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DictionaryEntry {
    pub id: i64,
    pub term: String,
    pub canonical: Option<String>,
    pub source: String,
    pub confidence: f64,
    pub app_context: Option<String>,
    pub use_count: i64,
    pub last_used_at: Option<String>,
    pub created_at: String,
}

/// Partial-update shape. Outer Option = "should this field be updated?";
/// inner Option (where applicable) = "what value to set". This is the
/// honest way to express updates over nullable columns.
#[derive(Debug, Clone, Default)]
pub struct DictionaryEntryUpdate {
    pub canonical: Option<Option<String>>,
    pub confidence: Option<f64>,
    pub app_context: Option<Option<String>>,
}

pub fn insert(conn: &Connection, new: &NewDictionaryEntry) -> AppResult<i64> {
    conn.execute(
        "INSERT INTO dictionary (term, canonical, source, confidence, app_context) \
         VALUES (?1, ?2, ?3, COALESCE(?4, 1.0), ?5)",
        params![
            new.term,
            new.canonical,
            new.source,
            new.confidence,
            new.app_context,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update(conn: &Connection, id: i64, changes: &DictionaryEntryUpdate) -> AppResult<()> {
    // Apply each field with a guard sub-expression: when the outer
    // Option is None, we COALESCE back to the existing value so the
    // row is effectively unchanged for that column.
    //
    // Building a fully dynamic UPDATE would be cleaner but every
    // field gets the same form, so this stays readable.
    conn.execute(
        "UPDATE dictionary SET \
            canonical   = CASE WHEN ?1 = 1 THEN ?2 ELSE canonical END, \
            confidence  = CASE WHEN ?3 = 1 THEN ?4 ELSE confidence END, \
            app_context = CASE WHEN ?5 = 1 THEN ?6 ELSE app_context END \
         WHERE id = ?7",
        params![
            changes.canonical.is_some() as i64,
            changes.canonical.as_ref().map(|v| v.as_deref()),
            changes.confidence.is_some() as i64,
            changes.confidence,
            changes.app_context.is_some() as i64,
            changes.app_context.as_ref().map(|v| v.as_deref()),
            id,
        ],
    )?;
    Ok(())
}

pub fn delete(conn: &Connection, id: i64) -> AppResult<()> {
    conn.execute("DELETE FROM dictionary WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn get_by_id(conn: &Connection, id: i64) -> AppResult<Option<DictionaryEntry>> {
    query_optional(
        conn,
        "SELECT id, term, canonical, source, confidence, app_context, \
                use_count, last_used_at, created_at \
         FROM dictionary WHERE id = ?1",
        params![id],
    )
}

pub fn find_by_term(
    conn: &Connection,
    term: &str,
    app_context: Option<&str>,
) -> AppResult<Option<DictionaryEntry>> {
    // app_context is part of the UNIQUE key; need IS NULL handling
    // since `= NULL` always evaluates false.
    match app_context {
        Some(ctx) => query_optional(
            conn,
            "SELECT id, term, canonical, source, confidence, app_context, \
                    use_count, last_used_at, created_at \
             FROM dictionary WHERE term = ?1 AND app_context = ?2",
            params![term, ctx],
        ),
        None => query_optional(
            conn,
            "SELECT id, term, canonical, source, confidence, app_context, \
                    use_count, last_used_at, created_at \
             FROM dictionary WHERE term = ?1 AND app_context IS NULL",
            params![term],
        ),
    }
}

pub fn list_all(conn: &Connection) -> AppResult<Vec<DictionaryEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, term, canonical, source, confidence, app_context, \
                use_count, last_used_at, created_at \
         FROM dictionary ORDER BY id",
    )?;
    let rows = stmt.query_map([], row_to_entry)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn bump_usage(conn: &Connection, id: i64) -> AppResult<()> {
    conn.execute(
        "UPDATE dictionary SET use_count = use_count + 1, \
            last_used_at = CURRENT_TIMESTAMP WHERE id = ?1",
        params![id],
    )?;
    Ok(())
}

/// Snapshot the current dictionary state. Returns the new snapshot's
/// id (suitable for `sessions.dictionary_snapshot_id`).
///
/// Encoding: `term_ids` column is a JSON array of every current entry's
/// id, sorted ascending for determinism. Empty dictionary → `"[]"`.
pub fn create_snapshot(conn: &Connection) -> AppResult<i64> {
    let ids: Vec<i64> = {
        let mut stmt = conn.prepare("SELECT id FROM dictionary ORDER BY id")?;
        let rows = stmt.query_map([], |r| r.get::<_, i64>(0))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        out
    };
    let json = serde_json::to_string(&ids)
        .map_err(|e| crate::error::AppError::Other(format!("serialize term_ids: {e}")))?;
    conn.execute(
        "INSERT INTO dictionary_snapshots (term_ids) VALUES (?1)",
        params![json],
    )?;
    Ok(conn.last_insert_rowid())
}

fn query_optional(
    conn: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> AppResult<Option<DictionaryEntry>> {
    let mut stmt = conn.prepare(sql)?;
    let mut rows = stmt.query_map(params, row_to_entry)?;
    match rows.next() {
        Some(Ok(e)) => Ok(Some(e)),
        Some(Err(e)) => Err(e.into()),
        None => Ok(None),
    }
}

fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<DictionaryEntry> {
    Ok(DictionaryEntry {
        id: row.get(0)?,
        term: row.get(1)?,
        canonical: row.get(2)?,
        source: row.get(3)?,
        confidence: row.get(4)?,
        app_context: row.get(5)?,
        use_count: row.get(6)?,
        last_used_at: row.get(7)?,
        created_at: row.get(8)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::error::AppError;

    fn fresh() -> Database {
        Database::open_in_memory().unwrap()
    }

    #[test]
    fn insert_and_round_trip() {
        let db = fresh();
        let id = insert(
            &db.conn,
            &NewDictionaryEntry {
                term: "Bernarrd".into(),
                canonical: Some("Bernard".into()),
                source: "user".into(),
                confidence: None,
                app_context: None,
            },
        )
        .unwrap();
        let e = get_by_id(&db.conn, id).unwrap().unwrap();
        assert_eq!(e.term, "Bernarrd");
        assert_eq!(e.canonical.as_deref(), Some("Bernard"));
        assert_eq!(e.confidence, 1.0);
        assert_eq!(e.use_count, 0);
    }

    #[test]
    fn unique_term_app_context_is_enforced() {
        // SQL UNIQUE treats NULL as distinct (`NULL != NULL`), so duplicates
        // with `app_context: None` would BOTH succeed despite the UNIQUE
        // constraint — a famous SQLite gotcha. Pin the test on a non-null
        // app_context where UNIQUE actually fires. (Wave 3 brief flagged
        // this; Phase 6 may want a unique INDEX with COALESCE if we need
        // null-equal-null semantics — that's a future migration.)
        let db = fresh();
        let new = NewDictionaryEntry {
            term: "Foo".into(),
            canonical: None,
            source: "user".into(),
            confidence: None,
            app_context: Some("vscode".into()),
        };
        insert(&db.conn, &new).unwrap();
        let err = insert(&db.conn, &new).unwrap_err();
        assert!(matches!(err, AppError::Sqlite(_)));
    }

    #[test]
    fn update_canonical_round_trips() {
        let db = fresh();
        let id = insert(
            &db.conn,
            &NewDictionaryEntry {
                term: "foo".into(),
                canonical: Some("Foo".into()),
                source: "user".into(),
                confidence: None,
                app_context: None,
            },
        )
        .unwrap();
        update(
            &db.conn,
            id,
            &DictionaryEntryUpdate {
                canonical: Some(Some("FOO".into())),
                ..Default::default()
            },
        )
        .unwrap();
        let e = get_by_id(&db.conn, id).unwrap().unwrap();
        assert_eq!(e.canonical.as_deref(), Some("FOO"));
    }

    #[test]
    fn update_with_no_changes_is_noop() {
        let db = fresh();
        let id = insert(
            &db.conn,
            &NewDictionaryEntry {
                term: "a".into(),
                canonical: Some("A".into()),
                source: "user".into(),
                confidence: Some(0.5),
                app_context: Some("vscode".into()),
            },
        )
        .unwrap();
        update(&db.conn, id, &DictionaryEntryUpdate::default()).unwrap();
        let e = get_by_id(&db.conn, id).unwrap().unwrap();
        assert_eq!(e.canonical.as_deref(), Some("A"));
        assert_eq!(e.confidence, 0.5);
        assert_eq!(e.app_context.as_deref(), Some("vscode"));
    }

    #[test]
    fn delete_removes_row_and_fires_audit() {
        let db = fresh();
        let id = insert(
            &db.conn,
            &NewDictionaryEntry {
                term: "x".into(),
                canonical: None,
                source: "user".into(),
                confidence: None,
                app_context: None,
            },
        )
        .unwrap();
        delete(&db.conn, id).unwrap();
        assert!(get_by_id(&db.conn, id).unwrap().is_none());
        let history_count: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM _history_dictionary WHERE row_id = ?1 AND operation = 'DELETE'",
                params![id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(history_count, 1);
    }

    #[test]
    fn find_by_term_with_and_without_app_context() {
        let db = fresh();
        insert(
            &db.conn,
            &NewDictionaryEntry {
                term: "ctx".into(),
                canonical: Some("CTX".into()),
                source: "user".into(),
                confidence: None,
                app_context: Some("vscode".into()),
            },
        )
        .unwrap();
        insert(
            &db.conn,
            &NewDictionaryEntry {
                term: "ctx".into(),
                canonical: Some("CTX-global".into()),
                source: "user".into(),
                confidence: None,
                app_context: None,
            },
        )
        .unwrap();
        let with_ctx = find_by_term(&db.conn, "ctx", Some("vscode"))
            .unwrap()
            .unwrap();
        assert_eq!(with_ctx.canonical.as_deref(), Some("CTX"));
        let without = find_by_term(&db.conn, "ctx", None).unwrap().unwrap();
        assert_eq!(without.canonical.as_deref(), Some("CTX-global"));
    }

    #[test]
    fn bump_usage_increments_count_and_sets_timestamp() {
        let db = fresh();
        let id = insert(
            &db.conn,
            &NewDictionaryEntry {
                term: "z".into(),
                canonical: None,
                source: "user".into(),
                confidence: None,
                app_context: None,
            },
        )
        .unwrap();
        bump_usage(&db.conn, id).unwrap();
        bump_usage(&db.conn, id).unwrap();
        let e = get_by_id(&db.conn, id).unwrap().unwrap();
        assert_eq!(e.use_count, 2);
        assert!(e.last_used_at.is_some());
    }

    #[test]
    fn create_snapshot_captures_current_ids_as_json_array() {
        let db = fresh();
        let id1 = insert(
            &db.conn,
            &NewDictionaryEntry {
                term: "a".into(),
                canonical: None,
                source: "user".into(),
                confidence: None,
                app_context: None,
            },
        )
        .unwrap();
        let id2 = insert(
            &db.conn,
            &NewDictionaryEntry {
                term: "b".into(),
                canonical: None,
                source: "user".into(),
                confidence: None,
                app_context: None,
            },
        )
        .unwrap();
        let snap_id = create_snapshot(&db.conn).unwrap();
        let term_ids_json: String = db
            .conn
            .query_row(
                "SELECT term_ids FROM dictionary_snapshots WHERE id = ?1",
                params![snap_id],
                |r| r.get(0),
            )
            .unwrap();
        let parsed: Vec<i64> = serde_json::from_str(&term_ids_json).unwrap();
        assert_eq!(parsed, vec![id1, id2]);
    }

    #[test]
    fn create_snapshot_with_empty_dictionary_yields_empty_json_array() {
        let db = fresh();
        let snap_id = create_snapshot(&db.conn).unwrap();
        let term_ids_json: String = db
            .conn
            .query_row(
                "SELECT term_ids FROM dictionary_snapshots WHERE id = ?1",
                params![snap_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(term_ids_json, "[]");
    }
}
