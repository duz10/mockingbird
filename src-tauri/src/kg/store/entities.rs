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
use serde::Serialize;

use super::{fetch_session_title_excerpt, EntryRef};
use crate::error::{AppError, AppResult};

/// Drill-down payload for the concept modal (Wave 1C.4 / ADR 0051 D4).
///
/// Returned by [`entity_detail`]. Mirrors what the modal renders:
/// header (canonical name + entity_type + aliases), body counters
/// (mention_count + total_entries), and the most-recent N entries
/// (cap at `recent_limit`).
///
/// `mention_count` is the total row count from `kg_entity_mentions`
/// for this entity ("how many times has the model called this out");
/// `total_entries` is the count of DISTINCT entries that contain at
/// least one such mention ("how many dictations is this in"). The
/// two diverge when an entity is mentioned multiple times within
/// one dictation, which is the common case.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityDetail {
    pub entity_id: i64,
    pub canonical_name: String,
    pub entity_type: String,
    pub aliases: Vec<String>,
    pub mention_count: i64,
    pub total_entries: u32,
    pub recent_entries: Vec<EntryRef>,
}

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

/// Fetch the modal payload for one entity. Returns
/// `AppError::Other("entity not found: <id>")` when the id does not
/// resolve, so the IPC layer surfaces a clean error toast instead of
/// the UI rendering a blank modal.
///
/// `recent_limit` caps the size of `recent_entries`; the UI passes
/// 50 per ADR 0051 D4 ("Cap visible: 50 recent"). Total counts
/// (`mention_count`, `total_entries`) are computed across the FULL
/// mention set regardless of `recent_limit` so the modal's footer
/// ("Total entries: N") is honest about how many entries the cap is
/// hiding.
///
/// Ordering of `recent_entries`: most-recent dictation first by
/// `sessions.started_at DESC`. Ties (same `started_at` to the
/// second) break by `sessions.id DESC` so the order is deterministic
/// for tests + snapshot diffs.
pub(crate) fn entity_detail(
    conn: &Connection,
    entity_id: i64,
    recent_limit: u32,
) -> AppResult<EntityDetail> {
    // Header row. Aliases land as JSON-encoded strings (per the
    // upsert path's storage convention); decode to Vec<String> for
    // the wire.
    let row: Option<(String, String, String)> = conn
        .query_row(
            "SELECT name, entity_type, aliases_json FROM kg_entities WHERE id = ?1;",
            params![entity_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let (canonical_name, entity_type, aliases_json) =
        row.ok_or_else(|| AppError::Other(format!("entity not found: {entity_id}")))?;
    let aliases: Vec<String> = serde_json::from_str(&aliases_json)
        .map_err(|e| AppError::Other(format!("alias json decode: {e}")))?;

    // Counters. COUNT-vs-SUM per LESSONS 2026-05-30 Wave 1C.2
    // Finding 3 -- COUNT over zero rows returns 0, not NULL.
    let mention_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM kg_entity_mentions WHERE entity_id = ?1;",
        params![entity_id],
        |r| r.get(0),
    )?;
    let total_entries_i64: i64 = conn.query_row(
        "SELECT COUNT(DISTINCT entry_id) FROM kg_entity_mentions WHERE entity_id = ?1;",
        params![entity_id],
        |r| r.get(0),
    )?;
    let total_entries = u32::try_from(total_entries_i64).unwrap_or(u32::MAX);

    // Recent entries. GROUP BY collapses multi-mention-per-entry
    // back to one row per entry; ORDER BY started_at DESC, id DESC
    // gives a deterministic newest-first ordering with a stable tie
    // break (sessions.id is AUTOINCREMENT-monotonic so id-DESC
    // matches "most recently inserted" for same-second ties).
    let mut stmt = conn.prepare(
        "SELECT s.id, s.started_at \
         FROM kg_entity_mentions m \
         JOIN sessions s ON s.id = m.entry_id \
         WHERE m.entity_id = ?1 \
         GROUP BY s.id \
         ORDER BY s.started_at DESC, s.id DESC \
         LIMIT ?2;",
    )?;
    let rows = stmt.query_map(params![entity_id, recent_limit as i64], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
    })?;
    let mut recent_entries = Vec::new();
    for row in rows {
        let (id, started_at) = row?;
        recent_entries.push(EntryRef {
            entry_id: id,
            title: fetch_session_title_excerpt(conn, id)?,
            captured_iso: started_at,
            category: None, // mb-oji5 parking lot
        });
    }

    Ok(EntityDetail {
        entity_id,
        canonical_name,
        entity_type,
        aliases,
        mention_count,
        total_entries,
        recent_entries,
    })
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

    // ============================================================
    // entity_detail (Wave 1C.4 / ADR 0051 D1)
    // ============================================================

    /// Bigger fixture: kg_entities + sessions + transcripts +
    /// kg_entity_mentions. Mirrors the persistent state the modal
    /// reads against.
    ///
    /// Seeded entities:
    ///   id 1: Mom (person), aliases ["Mama"]
    ///   id 2: Acme (organization), aliases []
    ///
    /// Seeded sessions (== entry_ids):
    ///   200 -> started_at 2026-05-25, final transcript "Reminder to call Mom about taxes"
    ///   201 -> started_at 2026-05-26, final transcript "Acme contract review"
    ///   202 -> started_at 2026-05-27, cleaned transcript "  Spoke with Mom and Acme  "
    ///   203 -> started_at 2026-05-28, no transcripts (empty title)
    ///
    /// Mentions:
    ///   Mom  in 200 (seg 0), 200 (seg 2), 202 (seg 0)            -> 3 mentions, 2 entries
    ///   Acme in 201 (seg 0), 202 (seg 1), 203 (seg 0)            -> 3 mentions, 3 entries
    fn seed_detail_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE sessions (
               id INTEGER PRIMARY KEY,
               started_at TEXT NOT NULL
             );
             CREATE TABLE transcripts (
               id INTEGER PRIMARY KEY,
               session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
               stage TEXT NOT NULL,
               text TEXT NOT NULL,
               UNIQUE(session_id, stage)
             );
             CREATE TABLE kg_entities (
               id INTEGER PRIMARY KEY,
               name TEXT NOT NULL,
               entity_type TEXT NOT NULL,
               aliases_json TEXT NOT NULL DEFAULT '[]',
               created_at TEXT NOT NULL,
               updated_at TEXT NOT NULL,
               UNIQUE(name, entity_type)
             );
             CREATE TABLE kg_entity_mentions (
               id INTEGER PRIMARY KEY,
               entry_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
               entity_id INTEGER NOT NULL REFERENCES kg_entities(id) ON DELETE CASCADE,
               segment_idx INTEGER NOT NULL,
               surface_form TEXT NOT NULL,
               created_at TEXT NOT NULL,
               UNIQUE(entry_id, segment_idx, entity_id)
             );

             INSERT INTO kg_entities (id, name, entity_type, aliases_json, created_at, updated_at) VALUES
               (1, 'Mom',  'person',       '[\"Mama\"]', 't', 't'),
               (2, 'Acme', 'organization', '[]',         't', 't');

             INSERT INTO sessions (id, started_at) VALUES
               (200, '2026-05-25T10:00:00Z'),
               (201, '2026-05-26T10:00:00Z'),
               (202, '2026-05-27T10:00:00Z'),
               (203, '2026-05-28T10:00:00Z');

             INSERT INTO transcripts (session_id, stage, text) VALUES
               (200, 'final',   'Reminder to call Mom about taxes'),
               (201, 'final',   'Acme contract review'),
               (202, 'cleaned', '  Spoke with Mom and Acme  ');

             INSERT INTO kg_entity_mentions (entry_id, entity_id, segment_idx, surface_form, created_at) VALUES
               (200, 1, 0, 'Mom',  't'),
               (200, 1, 2, 'Mom',  't'),
               (202, 1, 0, 'Mom',  't'),
               (201, 2, 0, 'Acme', 't'),
               (202, 2, 1, 'Acme', 't'),
               (203, 2, 0, 'Acme', 't');",
        )
        .unwrap();
        conn
    }

    #[test]
    fn entity_detail_returns_header_counts_and_recent_entries() {
        let conn = seed_detail_conn();
        let d = entity_detail(&conn, 1, 50).unwrap();
        assert_eq!(d.entity_id, 1);
        assert_eq!(d.canonical_name, "Mom");
        assert_eq!(d.entity_type, "person");
        assert_eq!(d.aliases, vec!["Mama".to_string()]);
        assert_eq!(d.mention_count, 3, "3 mention rows (Mom x2 in 200 + x1 in 202)");
        assert_eq!(d.total_entries, 2, "distinct entries: 200 + 202");
        // Newest first: 202 before 200.
        let ids: Vec<i64> = d.recent_entries.iter().map(|e| e.entry_id).collect();
        assert_eq!(ids, vec![202, 200]);
    }

    #[test]
    fn entity_detail_picks_cleaned_when_final_missing_and_trims_whitespace() {
        let conn = seed_detail_conn();
        let d = entity_detail(&conn, 1, 50).unwrap();
        // entry 202: only `cleaned` stage exists; the whitespace was
        // padded on both sides in the fixture -- truncate_title trims.
        let row_202 = d.recent_entries.iter().find(|e| e.entry_id == 202).unwrap();
        assert_eq!(row_202.title, "Spoke with Mom and Acme");
        // entry 200: `final` stage present, used.
        let row_200 = d.recent_entries.iter().find(|e| e.entry_id == 200).unwrap();
        assert_eq!(row_200.title, "Reminder to call Mom about taxes");
    }

    #[test]
    fn entity_detail_recent_limit_caps_list_but_not_counts() {
        let conn = seed_detail_conn();
        let d = entity_detail(&conn, 2, 1).unwrap(); // Acme: 3 entries, cap at 1
        assert_eq!(d.recent_entries.len(), 1);
        assert_eq!(d.recent_entries[0].entry_id, 203, "newest first");
        // Counts still reflect the FULL set, not the capped list.
        assert_eq!(d.mention_count, 3);
        assert_eq!(d.total_entries, 3);
    }

    #[test]
    fn entity_detail_missing_id_returns_err() {
        let conn = seed_detail_conn();
        let err = entity_detail(&conn, 9999, 50).unwrap_err();
        match err {
            AppError::Other(msg) => assert!(
                msg.contains("entity not found: 9999"),
                "unexpected error message: {msg}"
            ),
            other => panic!("expected AppError::Other, got {other:?}"),
        }
    }

    #[test]
    fn entity_detail_entry_with_no_transcript_has_empty_title_not_err() {
        let conn = seed_detail_conn();
        let d = entity_detail(&conn, 2, 50).unwrap(); // Acme touches 203
        let row_203 = d.recent_entries.iter().find(|e| e.entry_id == 203).unwrap();
        assert_eq!(
            row_203.title, "",
            "missing transcript -> empty title, UI handles"
        );
        // captured_iso still populated; row stays in the list.
        assert_eq!(row_203.captured_iso, "2026-05-28T10:00:00Z");
    }

    #[test]
    fn entity_detail_category_is_always_none_in_1c4() {
        // mb-oji5 parking lot: category is not persisted yet, so the
        // DTO field stays None. Pinning the contract here so a future
        // mb-oji5 land-day knows it has to flip this assertion.
        let conn = seed_detail_conn();
        let d = entity_detail(&conn, 1, 50).unwrap();
        assert!(d.recent_entries.iter().all(|e| e.category.is_none()));
    }

    #[test]
    fn entity_detail_orders_ties_by_id_desc() {
        // Same-second started_at: id-DESC is the tiebreaker so the
        // ordering is deterministic for snapshot tests.
        let conn = seed_detail_conn();
        conn.execute_batch(
            "INSERT INTO sessions (id, started_at) VALUES
               (210, '2026-05-27T10:00:00Z'),
               (211, '2026-05-27T10:00:00Z');
             INSERT INTO kg_entity_mentions (entry_id, entity_id, segment_idx, surface_form, created_at) VALUES
               (210, 1, 0, 'Mom', 't'),
               (211, 1, 0, 'Mom', 't');",
        )
        .unwrap();
        let d = entity_detail(&conn, 1, 50).unwrap();
        // Same-second 211 > 210; both newer than 202 (also same
        // 'started_at' second; tie -> id desc).
        let ids: Vec<i64> = d.recent_entries.iter().map(|e| e.entry_id).collect();
        // Newest 'started_at' is 2026-05-27 (3 entries: 211, 210, 202)
        // then 2026-05-25 entry 200.
        assert_eq!(ids, vec![211, 210, 202, 200]);
    }

    #[test]
    fn entity_detail_dto_serializes_camel_case() {
        let d = EntityDetail {
            entity_id: 1,
            canonical_name: "Mom".into(),
            entity_type: "person".into(),
            aliases: vec!["Mama".into()],
            mention_count: 3,
            total_entries: 2,
            recent_entries: vec![EntryRef {
                entry_id: 200,
                title: "hi".into(),
                captured_iso: "t".into(),
                category: None,
            }],
        };
        let json = serde_json::to_string(&d).unwrap();
        assert!(json.contains("\"entityId\":1"));
        assert!(json.contains("\"canonicalName\":\"Mom\""));
        assert!(json.contains("\"entityType\":\"person\""));
        assert!(json.contains("\"mentionCount\":3"));
        assert!(json.contains("\"totalEntries\":2"));
        assert!(json.contains("\"recentEntries\":"));
        assert!(json.contains("\"entryId\":200"));
        assert!(json.contains("\"capturedIso\":\"t\""));
        assert!(json.contains("\"category\":null"));
    }
}
