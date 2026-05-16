#![allow(missing_docs)] // Self-documenting field set; module-level doc is the API surface.

//! Audit-log reader + rollback engine.
//!
//! Reads `_history_*` tables (populated by migration 002 triggers) and
//! can restore any audited row to any prior state.
//!
//! ## Algorithm
//!
//! `state_at(table, row_id, ts)` walks history backward: finds the
//! latest history entry with `at <= ts`, then:
//!   - `INSERT` → patch is the full row projection → `Some(patch)`
//!   - `UPDATE` → patch is `{before, after}` → `Some(patch.after)`
//!   - `DELETE` → row didn't exist at cutoff → `None`
//!   - no history → `None`
//!
//! `rollback_row_to_timestamp(table, row_id, ts)` applies the result:
//!   - target=Some, row exists → `UPDATE`
//!   - target=Some, row absent → `INSERT` (with explicit id)
//!   - target=None, row exists → `DELETE`
//!   - target=None, row absent → no-op
//!
//! The rollback's own UPDATE/INSERT/DELETE fires the audit triggers,
//! leaving a trail of the rollback itself. That's intentional — replays
//! are first-class history.
//!
//! ## SQL injection surface
//!
//! Table and column names are interpolated from `AuditedTable` enum
//! variants and `&'static str` constants. User input never reaches SQL
//! identifiers. All values bind through rusqlite parameters.

use rusqlite::{params, params_from_iter, types::Value as SqlValue, Connection, OptionalExtension};

use crate::error::{AppError, AppResult};

/// Which audited table you're operating on. Gates all dynamic SQL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditedTable {
    Modes,
    Prompts,
    Dictionary,
    StyleExamples,
}

impl AuditedTable {
    pub fn table_name(self) -> &'static str {
        match self {
            Self::Modes => "modes",
            Self::Prompts => "prompts",
            Self::Dictionary => "dictionary",
            Self::StyleExamples => "style_examples",
        }
    }
    pub fn history_table(self) -> &'static str {
        match self {
            Self::Modes => "_history_modes",
            Self::Prompts => "_history_prompts",
            Self::Dictionary => "_history_dictionary",
            Self::StyleExamples => "_history_style_examples",
        }
    }
    /// Mutable column list. **Must mirror migration 002's trigger
    /// projections exactly.** Wave-2 brief is the source of truth; any
    /// schema change adding a column to a mutable list must also update
    /// the migration and this list in the same PR.
    pub fn mutable_columns(self) -> &'static [&'static str] {
        match self {
            Self::Modes => columns::MODES_MUTABLE,
            Self::Prompts => columns::PROMPTS_MUTABLE,
            Self::Dictionary => columns::DICTIONARY_MUTABLE,
            Self::StyleExamples => columns::STYLE_EXAMPLES_MUTABLE,
        }
    }
}

mod columns {
    pub const MODES_MUTABLE: &[&str] = &[
        "slug",
        "display_name",
        "hotkey",
        "provider",
        "model_id",
        "prompt_id",
        "temperature",
        "max_tokens",
        "enabled",
    ];
    pub const PROMPTS_MUTABLE: &[&str] = &["mode_slug", "version", "body"];
    pub const DICTIONARY_MUTABLE: &[&str] =
        &["term", "canonical", "source", "confidence", "app_context"];
    pub const STYLE_EXAMPLES_MUTABLE: &[&str] = &[
        "mode_slug",
        "session_id",
        "raw_input",
        "ideal_output",
        "app_context",
        "source",
        "rank",
        "enabled",
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Operation {
    Insert,
    Update,
    Delete,
}

impl Operation {
    pub fn parse(s: &str) -> AppResult<Self> {
        match s {
            "INSERT" => Ok(Self::Insert),
            "UPDATE" => Ok(Self::Update),
            "DELETE" => Ok(Self::Delete),
            other => Err(AppError::Other(format!("invalid operation: {other:?}"))),
        }
    }
}

#[derive(Debug, Clone)]
pub struct HistoryEntry {
    pub id: i64,
    pub row_id: i64,
    pub operation: Operation,
    pub patch: serde_json::Value,
    pub at: String,
}

/// All history entries for a specific row, oldest-first.
pub fn list_history(
    conn: &Connection,
    table: AuditedTable,
    row_id: i64,
) -> AppResult<Vec<HistoryEntry>> {
    let sql = format!(
        "SELECT id, row_id, operation, patch, at FROM {} \
         WHERE row_id = ?1 ORDER BY at ASC, id ASC",
        table.history_table()
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![row_id], row_to_history_entry)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// All history entries for a table, oldest-first. For Phase 6's
/// table-level audit UI.
pub fn list_history_for_table(
    conn: &Connection,
    table: AuditedTable,
) -> AppResult<Vec<HistoryEntry>> {
    let sql = format!(
        "SELECT id, row_id, operation, patch, at FROM {} \
         ORDER BY at ASC, id ASC",
        table.history_table()
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], row_to_history_entry)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Compute the row's state at-or-before `at_or_before_ts`.
///
/// `Some(state)` if the row existed at that time; `None` otherwise.
/// State is a JSON object matching the mutable-column projection from
/// migration 002.
pub fn state_at(
    conn: &Connection,
    table: AuditedTable,
    row_id: i64,
    at_or_before_ts: &str,
) -> AppResult<Option<serde_json::Value>> {
    let sql = format!(
        "SELECT operation, patch FROM {} \
         WHERE row_id = ?1 AND at <= ?2 \
         ORDER BY at DESC, id DESC LIMIT 1",
        table.history_table()
    );
    let row = conn
        .query_row(&sql, params![row_id, at_or_before_ts], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })
        .optional()?;
    let Some((op_str, patch_str)) = row else {
        return Ok(None);
    };
    let op = Operation::parse(&op_str)?;
    let patch: serde_json::Value = serde_json::from_str(&patch_str)
        .map_err(|e| AppError::Other(format!("parse audit patch: {e}")))?;

    Ok(match op {
        Operation::Insert => Some(patch),
        Operation::Update => patch.get("after").cloned(),
        Operation::Delete => None,
    })
}

/// Restore a single row to its state at-or-before `at_or_before_ts`.
pub fn rollback_row_to_timestamp(
    conn: &Connection,
    table: AuditedTable,
    row_id: i64,
    at_or_before_ts: &str,
) -> AppResult<()> {
    let target = state_at(conn, table, row_id, at_or_before_ts)?;
    let exists: i64 = conn.query_row(
        &format!(
            "SELECT EXISTS(SELECT 1 FROM {} WHERE id = ?1)",
            table.table_name()
        ),
        params![row_id],
        |r| r.get(0),
    )?;
    let row_exists = exists == 1;
    restore_row(conn, table, row_id, target.as_ref(), row_exists)
}

/// Roll every row in the table back to its state at-or-before
/// `at_or_before_ts`. Walks distinct row_ids from history.
pub fn rollback_table_to_timestamp(
    conn: &Connection,
    table: AuditedTable,
    at_or_before_ts: &str,
) -> AppResult<()> {
    let row_ids: Vec<i64> = {
        let sql = format!(
            "SELECT DISTINCT row_id FROM {} ORDER BY row_id",
            table.history_table()
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], |r| r.get::<_, i64>(0))?;
        let mut v = Vec::new();
        for r in rows {
            v.push(r?);
        }
        v
    };
    for row_id in row_ids {
        rollback_row_to_timestamp(conn, table, row_id, at_or_before_ts)?;
    }
    Ok(())
}

fn restore_row(
    conn: &Connection,
    table: AuditedTable,
    row_id: i64,
    target: Option<&serde_json::Value>,
    row_exists: bool,
) -> AppResult<()> {
    let cols = table.mutable_columns();
    match (target, row_exists) {
        (Some(state), true) => {
            // UPDATE table SET c1=?1, c2=?2, ... WHERE id=?N
            let setters: Vec<String> = cols
                .iter()
                .enumerate()
                .map(|(i, c)| format!("{c} = ?{}", i + 1))
                .collect();
            let sql = format!(
                "UPDATE {} SET {} WHERE id = ?{}",
                table.table_name(),
                setters.join(", "),
                cols.len() + 1
            );
            let mut values: Vec<SqlValue> = Vec::with_capacity(cols.len() + 1);
            for c in cols {
                values.push(json_to_sqlite_value(state.get(c).cloned())?);
            }
            values.push(SqlValue::Integer(row_id));
            conn.execute(&sql, params_from_iter(values))?;
        }
        (Some(state), false) => {
            // INSERT INTO table (id, c1, c2, ...) VALUES (?1, ?2, ?3, ...)
            let mut col_list = vec!["id"];
            col_list.extend(cols.iter().copied());
            let placeholders: Vec<String> = (1..=col_list.len()).map(|i| format!("?{i}")).collect();
            let sql = format!(
                "INSERT INTO {} ({}) VALUES ({})",
                table.table_name(),
                col_list.join(", "),
                placeholders.join(", ")
            );
            let mut values: Vec<SqlValue> = Vec::with_capacity(col_list.len());
            values.push(SqlValue::Integer(row_id));
            for c in cols {
                values.push(json_to_sqlite_value(state.get(c).cloned())?);
            }
            conn.execute(&sql, params_from_iter(values))?;
        }
        (None, true) => {
            conn.execute(
                &format!("DELETE FROM {} WHERE id = ?1", table.table_name()),
                params![row_id],
            )?;
        }
        (None, false) => {
            // No-op: target says "row didn't exist", and it doesn't. Done.
        }
    }
    Ok(())
}

fn json_to_sqlite_value(v: Option<serde_json::Value>) -> AppResult<SqlValue> {
    use serde_json::Value as JV;
    let Some(v) = v else {
        return Ok(SqlValue::Null);
    };
    Ok(match v {
        JV::Null => SqlValue::Null,
        JV::Bool(b) => SqlValue::Integer(b as i64),
        JV::Number(n) => {
            if let Some(i) = n.as_i64() {
                SqlValue::Integer(i)
            } else if let Some(f) = n.as_f64() {
                SqlValue::Real(f)
            } else {
                return Err(AppError::Other(format!("unsupported JSON number: {n}")));
            }
        }
        JV::String(s) => SqlValue::Text(s),
        other => {
            return Err(AppError::Other(format!(
                "unsupported JSON value in audit patch: {other}"
            )))
        }
    })
}

fn row_to_history_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<HistoryEntry> {
    let op_str: String = row.get(2)?;
    let op = Operation::parse(&op_str).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            2,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                e.to_string(),
            )),
        )
    })?;
    let patch_str: String = row.get(3)?;
    let patch: serde_json::Value = serde_json::from_str(&patch_str).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            3,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                e.to_string(),
            )),
        )
    })?;
    Ok(HistoryEntry {
        id: row.get(0)?,
        row_id: row.get(1)?,
        operation: op,
        patch,
        at: row.get(4)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::dictionary::{self, NewDictionaryEntry};
    use crate::db::Database;

    fn fresh() -> Database {
        Database::open_in_memory().unwrap()
    }

    /// Force the latest history row in `table` to have `at = '<base>'`
    /// where base is a deterministic synthetic timestamp. Returns the
    /// timestamp string. This lets tests skirt SQLite's 1-second
    /// CURRENT_TIMESTAMP granularity without sleeping.
    fn pin_latest_at(conn: &Connection, table: AuditedTable, ts: &str) {
        let sql = format!(
            "UPDATE {0} SET at = ?1 WHERE id = (SELECT MAX(id) FROM {0})",
            table.history_table()
        );
        conn.execute(&sql, params![ts]).unwrap();
    }

    fn insert_term(conn: &Connection, term: &str, canonical: &str) -> i64 {
        dictionary::insert(
            conn,
            &NewDictionaryEntry {
                term: term.into(),
                canonical: Some(canonical.into()),
                source: "user".into(),
                confidence: None,
                app_context: None,
            },
        )
        .unwrap()
    }

    #[test]
    fn list_history_returns_entries_oldest_first() {
        let db = fresh();
        let id = insert_term(&db.conn, "foo", "Foo");
        dictionary::update(
            &db.conn,
            id,
            &dictionary::DictionaryEntryUpdate {
                canonical: Some(Some("FOO".into())),
                ..Default::default()
            },
        )
        .unwrap();
        let history = list_history(&db.conn, AuditedTable::Dictionary, id).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].operation, Operation::Insert);
        assert_eq!(history[1].operation, Operation::Update);
    }

    #[test]
    fn state_at_returns_insert_payload_immediately_after_insert() {
        let db = fresh();
        let id = insert_term(&db.conn, "foo", "Foo");
        pin_latest_at(&db.conn, AuditedTable::Dictionary, "2026-05-15 10:00:00");
        let state = state_at(
            &db.conn,
            AuditedTable::Dictionary,
            id,
            "2026-05-15 10:00:00",
        )
        .unwrap()
        .unwrap();
        assert_eq!(state.get("term").and_then(|v| v.as_str()), Some("foo"));
        assert_eq!(state.get("canonical").and_then(|v| v.as_str()), Some("Foo"));
    }

    #[test]
    fn state_at_returns_before_first_update() {
        let db = fresh();
        let id = insert_term(&db.conn, "foo", "Foo");
        pin_latest_at(&db.conn, AuditedTable::Dictionary, "2026-05-15 10:00:00");

        dictionary::update(
            &db.conn,
            id,
            &dictionary::DictionaryEntryUpdate {
                canonical: Some(Some("FOO".into())),
                ..Default::default()
            },
        )
        .unwrap();
        pin_latest_at(&db.conn, AuditedTable::Dictionary, "2026-05-15 10:00:05");

        // Query state at t=10:00:02 — between insert (10:00:00) and update (10:00:05).
        let state = state_at(
            &db.conn,
            AuditedTable::Dictionary,
            id,
            "2026-05-15 10:00:02",
        )
        .unwrap()
        .unwrap();
        // Must reflect the insert's "Foo", not the update's "FOO".
        assert_eq!(state.get("canonical").and_then(|v| v.as_str()), Some("Foo"));
    }

    #[test]
    fn state_at_returns_after_payload_after_update() {
        let db = fresh();
        let id = insert_term(&db.conn, "foo", "Foo");
        pin_latest_at(&db.conn, AuditedTable::Dictionary, "2026-05-15 10:00:00");
        dictionary::update(
            &db.conn,
            id,
            &dictionary::DictionaryEntryUpdate {
                canonical: Some(Some("FOO".into())),
                ..Default::default()
            },
        )
        .unwrap();
        pin_latest_at(&db.conn, AuditedTable::Dictionary, "2026-05-15 10:00:05");

        let state = state_at(
            &db.conn,
            AuditedTable::Dictionary,
            id,
            "2026-05-15 10:00:10",
        )
        .unwrap()
        .unwrap();
        assert_eq!(state.get("canonical").and_then(|v| v.as_str()), Some("FOO"));
    }

    #[test]
    fn state_at_returns_none_after_delete() {
        let db = fresh();
        let id = insert_term(&db.conn, "foo", "Foo");
        pin_latest_at(&db.conn, AuditedTable::Dictionary, "2026-05-15 10:00:00");
        dictionary::delete(&db.conn, id).unwrap();
        pin_latest_at(&db.conn, AuditedTable::Dictionary, "2026-05-15 10:00:05");

        let state = state_at(
            &db.conn,
            AuditedTable::Dictionary,
            id,
            "2026-05-15 10:00:10",
        )
        .unwrap();
        assert!(state.is_none());
    }

    #[test]
    fn state_at_returns_none_before_any_history() {
        let db = fresh();
        let id = insert_term(&db.conn, "foo", "Foo");
        pin_latest_at(&db.conn, AuditedTable::Dictionary, "2026-05-15 10:00:00");
        let state = state_at(
            &db.conn,
            AuditedTable::Dictionary,
            id,
            "2026-05-15 09:00:00",
        )
        .unwrap();
        assert!(state.is_none());
    }

    #[test]
    fn rollback_row_to_timestamp_restores_prior_state() {
        let db = fresh();
        let id = insert_term(&db.conn, "foo", "Foo");
        pin_latest_at(&db.conn, AuditedTable::Dictionary, "2026-05-15 10:00:00");

        dictionary::update(
            &db.conn,
            id,
            &dictionary::DictionaryEntryUpdate {
                canonical: Some(Some("FOO".into())),
                ..Default::default()
            },
        )
        .unwrap();
        pin_latest_at(&db.conn, AuditedTable::Dictionary, "2026-05-15 10:00:05");

        rollback_row_to_timestamp(
            &db.conn,
            AuditedTable::Dictionary,
            id,
            "2026-05-15 10:00:02",
        )
        .unwrap();

        let restored = dictionary::get_by_id(&db.conn, id).unwrap().unwrap();
        assert_eq!(restored.canonical.as_deref(), Some("Foo"));

        // The rollback's UPDATE fired the audit trigger → 3 entries total
        // (insert, update, rollback's update).
        let history = list_history(&db.conn, AuditedTable::Dictionary, id).unwrap();
        assert_eq!(history.len(), 3);
        assert_eq!(history[2].operation, Operation::Update);
    }

    #[test]
    fn rollback_row_to_missing_state_deletes_the_live_row() {
        let db = fresh();
        // The row exists now, but at timestamp T it didn't (we query a ts before insert).
        let id = insert_term(&db.conn, "x", "X");
        pin_latest_at(&db.conn, AuditedTable::Dictionary, "2026-05-15 10:00:00");

        rollback_row_to_timestamp(
            &db.conn,
            AuditedTable::Dictionary,
            id,
            "2026-05-15 09:00:00",
        )
        .unwrap();

        assert!(dictionary::get_by_id(&db.conn, id).unwrap().is_none());
    }

    #[test]
    fn rollback_table_to_timestamp_walks_every_row() {
        let db = fresh();
        let a = insert_term(&db.conn, "a", "A");
        let b = insert_term(&db.conn, "b", "B");
        // Pin both INSERT history rows to t=10:00:00.
        db.conn
            .execute(
                "UPDATE _history_dictionary SET at = '2026-05-15 10:00:00'",
                [],
            )
            .unwrap();

        dictionary::update(
            &db.conn,
            a,
            &dictionary::DictionaryEntryUpdate {
                canonical: Some(Some("AAA".into())),
                ..Default::default()
            },
        )
        .unwrap();
        dictionary::update(
            &db.conn,
            b,
            &dictionary::DictionaryEntryUpdate {
                canonical: Some(Some("BBB".into())),
                ..Default::default()
            },
        )
        .unwrap();
        // Pin both UPDATE history rows to t=10:00:05.
        db.conn
            .execute(
                "UPDATE _history_dictionary SET at = '2026-05-15 10:00:05' \
                 WHERE operation = 'UPDATE'",
                [],
            )
            .unwrap();

        rollback_table_to_timestamp(&db.conn, AuditedTable::Dictionary, "2026-05-15 10:00:02")
            .unwrap();

        let row_a = dictionary::get_by_id(&db.conn, a).unwrap().unwrap();
        let row_b = dictionary::get_by_id(&db.conn, b).unwrap().unwrap();
        assert_eq!(row_a.canonical.as_deref(), Some("A"));
        assert_eq!(row_b.canonical.as_deref(), Some("B"));
    }

    #[test]
    fn list_history_for_table_returns_all_rows() {
        let db = fresh();
        insert_term(&db.conn, "a", "A");
        insert_term(&db.conn, "b", "B");
        let history = list_history_for_table(&db.conn, AuditedTable::Dictionary).unwrap();
        assert_eq!(history.len(), 2);
        assert!(history.iter().all(|h| h.operation == Operation::Insert));
    }

    #[test]
    fn operation_parse_round_trips() {
        for s in ["INSERT", "UPDATE", "DELETE"] {
            let op = Operation::parse(s).unwrap();
            assert!(matches!(
                op,
                Operation::Insert | Operation::Update | Operation::Delete
            ));
        }
        assert!(Operation::parse("BOGUS").is_err());
    }
}
