//! Prompts repository — READ ONLY.
//!
//! Prompts are append-only, seeded by migration 003, and bumped via
//! future migrations (per ADR 0008 prompt versioning). The application
//! never inserts, updates, or deletes prompts at runtime — that's a
//! schema migration.

use rusqlite::{params, Connection};

use crate::error::AppResult;

/// A prompt row.
#[derive(Debug, Clone)]
pub struct Prompt {
    pub id: i64,
    pub mode_slug: String,
    pub version: i64,
    pub body: String,
    pub created_at: String,
}

/// Lookup by primary key.
pub fn get_by_id(conn: &Connection, id: i64) -> AppResult<Option<Prompt>> {
    query_optional(
        conn,
        "SELECT id, mode_slug, version, body, created_at FROM prompts WHERE id = ?1",
        params![id],
    )
}

/// Lookup by (mode_slug, version). UNIQUE constraint guarantees ≤ 1 hit.
pub fn get_by_mode_and_version(
    conn: &Connection,
    mode_slug: &str,
    version: i64,
) -> AppResult<Option<Prompt>> {
    query_optional(
        conn,
        "SELECT id, mode_slug, version, body, created_at FROM prompts \
         WHERE mode_slug = ?1 AND version = ?2",
        params![mode_slug, version],
    )
}

/// Latest version for a mode. Used at session start to pin
/// `sessions.prompt_id` to the current prompt.
pub fn get_latest_for_mode(conn: &Connection, mode_slug: &str) -> AppResult<Option<Prompt>> {
    query_optional(
        conn,
        "SELECT id, mode_slug, version, body, created_at FROM prompts \
         WHERE mode_slug = ?1 ORDER BY version DESC LIMIT 1",
        params![mode_slug],
    )
}

/// All versions for a mode, sorted highest-version first.
pub fn list_for_mode(conn: &Connection, mode_slug: &str) -> AppResult<Vec<Prompt>> {
    let mut stmt = conn.prepare(
        "SELECT id, mode_slug, version, body, created_at FROM prompts \
         WHERE mode_slug = ?1 ORDER BY version DESC",
    )?;
    let rows = stmt.query_map(params![mode_slug], row_to_prompt)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

fn query_optional(
    conn: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> AppResult<Option<Prompt>> {
    let mut stmt = conn.prepare(sql)?;
    let mut rows = stmt.query_map(params, row_to_prompt)?;
    match rows.next() {
        Some(Ok(p)) => Ok(Some(p)),
        Some(Err(e)) => Err(e.into()),
        None => Ok(None),
    }
}

fn row_to_prompt(row: &rusqlite::Row<'_>) -> rusqlite::Result<Prompt> {
    Ok(Prompt {
        id: row.get(0)?,
        mode_slug: row.get(1)?,
        version: row.get(2)?,
        body: row.get(3)?,
        created_at: row.get(4)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    #[test]
    fn seeded_prompts_are_discoverable_after_fresh_migrate() {
        let db = Database::open_in_memory().unwrap();
        for slug in ["normal", "verbose", "fragment"] {
            let p = get_by_mode_and_version(&db.conn, slug, 1).unwrap();
            assert!(p.is_some(), "missing seeded prompt for {slug}");
            assert!(!p.unwrap().body.is_empty(), "empty body for {slug}");
        }
    }

    #[test]
    fn get_latest_for_mode_returns_highest_version() {
        let db = Database::open_in_memory().unwrap();
        let latest = get_latest_for_mode(&db.conn, "normal").unwrap().unwrap();
        assert_eq!(latest.version, 1);
        assert_eq!(latest.mode_slug, "normal");
    }

    #[test]
    fn get_by_mode_and_version_returns_none_for_missing() {
        let db = Database::open_in_memory().unwrap();
        assert!(get_by_mode_and_version(&db.conn, "normal", 99)
            .unwrap()
            .is_none());
        assert!(get_by_mode_and_version(&db.conn, "no-such-mode", 1)
            .unwrap()
            .is_none());
    }

    #[test]
    fn list_for_mode_orders_by_version_desc() {
        let db = Database::open_in_memory().unwrap();
        let list = list_for_mode(&db.conn, "normal").unwrap();
        assert_eq!(list.len(), 1, "seed has v1 only");
        // Confirm ordering invariant holds (even with 1 row).
        let versions: Vec<i64> = list.iter().map(|p| p.version).collect();
        let mut sorted = versions.clone();
        sorted.sort_by(|a, b| b.cmp(a));
        assert_eq!(versions, sorted);
    }

    #[test]
    fn get_by_id_returns_seeded_prompt() {
        let db = Database::open_in_memory().unwrap();
        let by_mode = get_by_mode_and_version(&db.conn, "fragment", 1)
            .unwrap()
            .unwrap();
        let by_id = get_by_id(&db.conn, by_mode.id).unwrap().unwrap();
        assert_eq!(by_id.mode_slug, "fragment");
    }
}
