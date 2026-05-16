//! Style examples repository — few-shot inputs/outputs for cleanup LLM.
//!
//! Phase 1 ships minimal CRUD. Ranking, automatic selection from
//! corrections, and per-session materialization arrive in Phase 8.

use rusqlite::{params, Connection};

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone)]
pub struct NewStyleExample {
    pub mode_slug: String,
    pub session_id: Option<i64>,
    pub raw_input: String,
    pub ideal_output: String,
    pub app_context: Option<String>,
    pub source: String,
    pub rank: f64,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct StyleExample {
    pub id: i64,
    pub mode_slug: String,
    pub session_id: Option<i64>,
    pub raw_input: String,
    pub ideal_output: String,
    pub app_context: Option<String>,
    pub source: String,
    pub rank: f64,
    pub enabled: bool,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct ExampleSet {
    pub id: i64,
    pub mode_slug: String,
    pub example_ids: Vec<i64>,
    pub created_at: String,
}

pub fn insert(conn: &Connection, new: &NewStyleExample) -> AppResult<i64> {
    conn.execute(
        "INSERT INTO style_examples \
            (mode_slug, session_id, raw_input, ideal_output, app_context, source, rank, enabled) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            new.mode_slug,
            new.session_id,
            new.raw_input,
            new.ideal_output,
            new.app_context,
            new.source,
            new.rank,
            new.enabled as i64,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn delete(conn: &Connection, id: i64) -> AppResult<()> {
    conn.execute("DELETE FROM style_examples WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn set_enabled(conn: &Connection, id: i64, enabled: bool) -> AppResult<()> {
    conn.execute(
        "UPDATE style_examples SET enabled = ?1 WHERE id = ?2",
        params![enabled as i64, id],
    )?;
    Ok(())
}

pub fn get_by_id(conn: &Connection, id: i64) -> AppResult<Option<StyleExample>> {
    let mut stmt = conn.prepare(
        "SELECT id, mode_slug, session_id, raw_input, ideal_output, \
                app_context, source, rank, enabled, created_at \
         FROM style_examples WHERE id = ?1",
    )?;
    let mut rows = stmt.query_map(params![id], row_to_example)?;
    match rows.next() {
        Some(Ok(e)) => Ok(Some(e)),
        Some(Err(e)) => Err(e.into()),
        None => Ok(None),
    }
}

pub fn list_for_mode(
    conn: &Connection,
    mode_slug: &str,
    enabled_only: bool,
) -> AppResult<Vec<StyleExample>> {
    let sql = if enabled_only {
        "SELECT id, mode_slug, session_id, raw_input, ideal_output, \
                app_context, source, rank, enabled, created_at \
         FROM style_examples WHERE mode_slug = ?1 AND enabled = 1 \
         ORDER BY rank DESC, id"
    } else {
        "SELECT id, mode_slug, session_id, raw_input, ideal_output, \
                app_context, source, rank, enabled, created_at \
         FROM style_examples WHERE mode_slug = ?1 \
         ORDER BY rank DESC, id"
    };
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params![mode_slug], row_to_example)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Create an immutable example_set. Stored as a JSON array of ids.
pub fn create_example_set(
    conn: &Connection,
    mode_slug: &str,
    example_ids: &[i64],
) -> AppResult<i64> {
    let json = serde_json::to_string(example_ids)
        .map_err(|e| AppError::Other(format!("serialize example_ids: {e}")))?;
    conn.execute(
        "INSERT INTO example_sets (mode_slug, example_ids) VALUES (?1, ?2)",
        params![mode_slug, json],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn get_example_set(conn: &Connection, id: i64) -> AppResult<Option<ExampleSet>> {
    let row = conn
        .query_row(
            "SELECT id, mode_slug, example_ids, created_at FROM example_sets WHERE id = ?1",
            params![id],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            },
        )
        .optional()?;
    let Some((id, mode_slug, ids_json, created_at)) = row else {
        return Ok(None);
    };
    let example_ids: Vec<i64> = serde_json::from_str(&ids_json)
        .map_err(|e| AppError::Other(format!("parse example_ids: {e}")))?;
    Ok(Some(ExampleSet {
        id,
        mode_slug,
        example_ids,
        created_at,
    }))
}

fn row_to_example(row: &rusqlite::Row<'_>) -> rusqlite::Result<StyleExample> {
    Ok(StyleExample {
        id: row.get(0)?,
        mode_slug: row.get(1)?,
        session_id: row.get(2)?,
        raw_input: row.get(3)?,
        ideal_output: row.get(4)?,
        app_context: row.get(5)?,
        source: row.get(6)?,
        rank: row.get(7)?,
        enabled: row.get::<_, i64>(8)? != 0,
        created_at: row.get(9)?,
    })
}

// `Connection::query_row(...).optional()` requires the OptionalExtension trait.
use rusqlite::OptionalExtension;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    fn fresh() -> Database {
        Database::open_in_memory().unwrap()
    }

    fn sample(mode: &str) -> NewStyleExample {
        NewStyleExample {
            mode_slug: mode.into(),
            session_id: None,
            raw_input: "hello".into(),
            ideal_output: "Hello.".into(),
            app_context: None,
            source: "manual".into(),
            rank: 0.0,
            enabled: true,
        }
    }

    #[test]
    fn insert_and_round_trip() {
        let db = fresh();
        let id = insert(&db.conn, &sample("normal")).unwrap();
        let got = get_by_id(&db.conn, id).unwrap().unwrap();
        assert_eq!(got.raw_input, "hello");
        assert_eq!(got.ideal_output, "Hello.");
        assert!(got.enabled);
        assert_eq!(got.rank, 0.0);
    }

    #[test]
    fn list_for_mode_respects_enabled_only_flag() {
        let db = fresh();
        let enabled_id = insert(
            &db.conn,
            &NewStyleExample {
                enabled: true,
                ..sample("normal")
            },
        )
        .unwrap();
        let disabled_id = insert(
            &db.conn,
            &NewStyleExample {
                enabled: false,
                raw_input: "off".into(),
                ..sample("normal")
            },
        )
        .unwrap();
        let only_enabled = list_for_mode(&db.conn, "normal", true).unwrap();
        let all = list_for_mode(&db.conn, "normal", false).unwrap();
        assert_eq!(only_enabled.len(), 1);
        assert_eq!(only_enabled[0].id, enabled_id);
        assert_eq!(all.len(), 2);
        assert!(all.iter().any(|e| e.id == disabled_id));
    }

    #[test]
    fn set_enabled_toggles_value() {
        let db = fresh();
        let id = insert(&db.conn, &sample("normal")).unwrap();
        set_enabled(&db.conn, id, false).unwrap();
        let e = get_by_id(&db.conn, id).unwrap().unwrap();
        assert!(!e.enabled);
        set_enabled(&db.conn, id, true).unwrap();
        let e = get_by_id(&db.conn, id).unwrap().unwrap();
        assert!(e.enabled);
    }

    #[test]
    fn delete_removes_row_and_fires_audit() {
        let db = fresh();
        let id = insert(&db.conn, &sample("normal")).unwrap();
        delete(&db.conn, id).unwrap();
        assert!(get_by_id(&db.conn, id).unwrap().is_none());
        let n: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM _history_style_examples \
                 WHERE row_id = ?1 AND operation = 'DELETE'",
                params![id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn create_example_set_stores_ids_as_json() {
        let db = fresh();
        let ids = vec![10i64, 20, 30];
        let set_id = create_example_set(&db.conn, "normal", &ids).unwrap();
        let set = get_example_set(&db.conn, set_id).unwrap().unwrap();
        assert_eq!(set.example_ids, ids);
        assert_eq!(set.mode_slug, "normal");
    }

    #[test]
    fn create_example_set_with_empty_ids_works() {
        let db = fresh();
        let set_id = create_example_set(&db.conn, "fragment", &[]).unwrap();
        let set = get_example_set(&db.conn, set_id).unwrap().unwrap();
        assert!(set.example_ids.is_empty());
    }

    #[test]
    fn get_example_set_returns_none_for_missing() {
        let db = fresh();
        assert!(get_example_set(&db.conn, 999).unwrap().is_none());
    }
}
