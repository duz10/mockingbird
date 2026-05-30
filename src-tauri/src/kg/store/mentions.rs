//! `kg_entity_mentions` + `kg_tag_mentions` per-segment writes.
//!
//! Both tables enforce idempotency via UNIQUE constraints:
//!
//! - `kg_entity_mentions`: UNIQUE(entry_id, segment_idx, entity_id)
//! - `kg_tag_mentions`:    UNIQUE(entry_id, segment_idx, tag_slug)
//!
//! Writes use `INSERT OR IGNORE` so re-filing the same entry collapses
//! to existing rows -- the kg-filing-idempotent invariant.
//!
//! Update is blocked at the SQL level by triggers (`kg_entity_mentions_no_update`
//! / `kg_tag_mentions_no_update` per migration 024). Callers reconcile
//! by `DELETE`-then-re-`INSERT`, preserving the "what the model said
//! when" audit trail.

use rusqlite::{params, Connection};

use crate::error::AppResult;

/// Persist one (entry, segment_idx, entity_id) mention. No-op if the
/// triple already exists. `surface_form` is the exact text the model
/// emitted (so the post-hoc UI can render the original wording even
/// when it differs from the canonical entity name).
pub(crate) fn insert_entity_mention(
    conn: &Connection,
    entry_id: i64,
    entity_id: i64,
    segment_idx: i64,
    surface_form: &str,
    now_iso: &str,
) -> AppResult<()> {
    conn.execute(
        "INSERT OR IGNORE INTO kg_entity_mentions \
           (entry_id, entity_id, segment_idx, surface_form, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5);",
        params![entry_id, entity_id, segment_idx, surface_form, now_iso],
    )?;
    Ok(())
}

/// Persist one (entry, segment_idx, tag_slug) mention. No-op if the
/// triple already exists. `canonical_tag_id` is `None` in 1B (the
/// `kg_canonical_tags` table is inert until v1.1); the column stays
/// NULLable on the wire for forward compatibility.
pub(crate) fn insert_tag_mention(
    conn: &Connection,
    entry_id: i64,
    segment_idx: i64,
    tag_slug: &str,
    now_iso: &str,
) -> AppResult<()> {
    conn.execute(
        "INSERT OR IGNORE INTO kg_tag_mentions \
           (entry_id, canonical_tag_id, segment_idx, tag_slug, created_at) \
         VALUES (?1, NULL, ?2, ?3, ?4);",
        params![entry_id, segment_idx, tag_slug, now_iso],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    /// Sets up the FK-graph minimum: sessions stub + kg_entities stub
    /// + the two mention tables with the real UNIQUE constraints. FK
    /// targets are stubs (no audit triggers etc.) -- we're testing
    /// the mention-table behaviour in isolation.
    fn make_test_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE sessions (id INTEGER PRIMARY KEY);
             CREATE TABLE kg_entities (id INTEGER PRIMARY KEY);
             CREATE TABLE kg_canonical_tags (id INTEGER PRIMARY KEY);
             CREATE TABLE kg_entity_mentions (
               id INTEGER PRIMARY KEY,
               entry_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
               entity_id INTEGER NOT NULL REFERENCES kg_entities(id) ON DELETE CASCADE,
               segment_idx INTEGER NOT NULL,
               surface_form TEXT NOT NULL,
               created_at TEXT NOT NULL,
               UNIQUE(entry_id, segment_idx, entity_id)
             );
             CREATE TABLE kg_tag_mentions (
               id INTEGER PRIMARY KEY,
               entry_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
               canonical_tag_id INTEGER REFERENCES kg_canonical_tags(id) ON DELETE SET NULL,
               segment_idx INTEGER NOT NULL,
               tag_slug TEXT NOT NULL,
               created_at TEXT NOT NULL,
               UNIQUE(entry_id, segment_idx, tag_slug)
             );
             INSERT INTO sessions (id) VALUES (1);
             INSERT INTO kg_entities (id) VALUES (1), (2);",
        )
        .unwrap();
        conn
    }

    #[test]
    fn entity_mention_idempotent_via_unique_constraint() {
        let conn = make_test_conn();
        insert_entity_mention(&conn, 1, 1, 0, "Mom", "t").unwrap();
        insert_entity_mention(&conn, 1, 1, 0, "Mom", "t").unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM kg_entity_mentions;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            count, 1,
            "duplicate (entry,seg,entity) collapses to one row"
        );
    }

    #[test]
    fn entity_mention_different_segments_yield_separate_rows() {
        let conn = make_test_conn();
        insert_entity_mention(&conn, 1, 1, 0, "Mom", "t").unwrap();
        insert_entity_mention(&conn, 1, 1, 1, "Mom", "t").unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM kg_entity_mentions;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2, "same entity in different segments = two rows");
    }

    #[test]
    fn entity_mention_different_entities_same_segment_yield_separate_rows() {
        let conn = make_test_conn();
        insert_entity_mention(&conn, 1, 1, 0, "Mom", "t").unwrap();
        insert_entity_mention(&conn, 1, 2, 0, "Acme", "t").unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM kg_entity_mentions;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn tag_mention_idempotent_via_unique_constraint() {
        let conn = make_test_conn();
        insert_tag_mention(&conn, 1, 0, "family", "t").unwrap();
        insert_tag_mention(&conn, 1, 0, "family", "t").unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM kg_tag_mentions;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn tag_mention_writes_canonical_id_as_null_in_1b() {
        let conn = make_test_conn();
        insert_tag_mention(&conn, 1, 0, "family", "t").unwrap();
        let canonical: Option<i64> = conn
            .query_row(
                "SELECT canonical_tag_id FROM kg_tag_mentions WHERE id = 1;",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            canonical.is_none(),
            "1B leaves canonical_tag_id NULL (open-vocab is primary)"
        );
    }
}
