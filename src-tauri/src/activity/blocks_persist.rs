//! DB persistence for `activity_blocks` + `activity_sessions.summary_markdown`.
//!
//! Phase 10 Wave 3. The migration-012 schema reserved these columns
//! but had no callers; this module is the first writer. Migration 013
//! adds the `activity_blocks.label` column (ADR 0040 §Decision item 4)
//! that supports the rename path here.
//!
//! ## Invariants
//!
//! - `activity_blocks` rows are MUTABLE (per migration 012 — they
//!   carry a `user_edited` flag). `activity_events` are NOT — this
//!   module never UPDATEs an event row.
//! - Every row written carries non-null `prompt_version_sha` +
//!   `source_event_ids` (Principle 2: provenance is total).
//! - Block ordering inside a session is `ORDER BY started_at ASC,
//!   id ASC` — same shape as event ordering.

#![allow(missing_docs)]

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::activity::ids::new_event_id; // shared UUID generator
use crate::error::{AppError, AppResult};

/// One persisted Block row, projection for the IPC layer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ActivityBlockRow {
    pub id: String,
    pub session_id: String,
    pub started_at: i64,
    pub ended_at: i64,
    pub primary_app: String,
    /// User-set label (added in migration 013). NULL until renamed.
    pub label: Option<String>,
    /// Best-effort title at the start of the Block. Stored in the
    /// `generated_abstract` adjacent column to keep the row body
    /// self-describing in the absence of an LLM summary.
    pub primary_title: String,
    /// LLM-or-template summary text.
    pub generated_abstract: Option<String>,
    /// True iff the user has edited any of label / abstract.
    pub user_edited: bool,
    /// JSON array of source `activity_events.id` values (Principle 2).
    pub source_event_ids: String,
    /// Prompt-set fingerprint at write time (abstractor's
    /// `current_prompt_set_sha`, or the no-payload sentinel).
    pub prompt_version_sha: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Insert one Block row. `source_event_ids` is the caller-provided
/// JSON encoding of the contributing event ids (the blocker hands a
/// `Vec<String>`; the abstractor + commands layer encode it once and
/// pass through here).
#[allow(clippy::too_many_arguments)]
pub fn insert_block(
    conn: &Connection,
    session_id: &str,
    started_at: i64,
    ended_at: i64,
    primary_app: &str,
    primary_title: &str,
    generated_abstract: Option<&str>,
    source_event_ids_json: &str,
    prompt_version_sha: &str,
    now_ms: i64,
) -> AppResult<String> {
    let id = new_event_id();
    conn.execute(
        "INSERT INTO activity_blocks \
         (id, session_id, started_at, ended_at, primary_app, primary_title, \
          generated_abstract, source_event_ids, prompt_version_sha, user_edited, \
          label, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0, NULL, ?10, ?10)",
        params![
            id,
            session_id,
            started_at,
            ended_at,
            primary_app,
            primary_title,
            generated_abstract,
            source_event_ids_json,
            prompt_version_sha,
            now_ms,
        ],
    )?;
    Ok(id)
}

/// List the Blocks for a session, chronologically.
pub fn list_blocks(conn: &Connection, session_id: &str) -> AppResult<Vec<ActivityBlockRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, session_id, started_at, ended_at, primary_app, primary_title, \
                label, generated_abstract, user_edited, source_event_ids, \
                prompt_version_sha, created_at, updated_at \
         FROM activity_blocks \
         WHERE session_id = ?1 \
         ORDER BY started_at ASC, id ASC",
    )?;
    let rows = stmt
        .query_map(params![session_id], row_to_block)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Update a Block's user-facing label. Sets `user_edited = 1` and
/// bumps `updated_at`.
pub fn rename_block(
    conn: &Connection,
    block_id: &str,
    new_label: Option<&str>,
    now_ms: i64,
) -> AppResult<()> {
    let n = conn.execute(
        "UPDATE activity_blocks SET label = ?1, user_edited = 1, updated_at = ?2 \
         WHERE id = ?3",
        params![new_label, now_ms, block_id],
    )?;
    if n == 0 {
        return Err(AppError::ActivityPersist(format!(
            "no such activity block: {block_id}"
        )));
    }
    Ok(())
}

/// Overwrite a Block's `generated_abstract` with a user-provided
/// string. Sets `user_edited = 1`.
pub fn rewrite_abstract(
    conn: &Connection,
    block_id: &str,
    new_abstract: &str,
    now_ms: i64,
) -> AppResult<()> {
    let n = conn.execute(
        "UPDATE activity_blocks SET generated_abstract = ?1, user_edited = 1, updated_at = ?2 \
         WHERE id = ?3",
        params![new_abstract, now_ms, block_id],
    )?;
    if n == 0 {
        return Err(AppError::ActivityPersist(format!(
            "no such activity block: {block_id}"
        )));
    }
    Ok(())
}

/// Delete one Block. The session's `summary_markdown` becomes stale
/// (caller should regenerate); we don't auto-regenerate because that
/// would re-call the LLM uninvited.
pub fn delete_block(conn: &Connection, block_id: &str) -> AppResult<()> {
    let n = conn.execute(
        "DELETE FROM activity_blocks WHERE id = ?1",
        params![block_id],
    )?;
    if n == 0 {
        return Err(AppError::ActivityPersist(format!(
            "no such activity block: {block_id}"
        )));
    }
    Ok(())
}

/// Merge `source_ids` into `target_id`. The target absorbs the
/// sources' time range (min/max) + source_event_ids (concatenated),
/// then the source rows are deleted. The target's `user_edited`
/// flag is set.
pub fn merge_blocks(
    conn: &mut Connection,
    target_id: &str,
    source_ids: &[String],
    now_ms: i64,
) -> AppResult<()> {
    if source_ids.is_empty() {
        return Ok(()); // no-op
    }
    let tx = conn.transaction()?;
    // Pull the rows we need.
    let target = load_block_unlocked(&tx, target_id)?;
    let mut combined_event_ids = parse_event_ids(&target.source_event_ids);
    let mut min_start = target.started_at;
    let mut max_end = target.ended_at;
    for sid in source_ids {
        if sid == target_id {
            continue;
        }
        let s = load_block_unlocked(&tx, sid)?;
        combined_event_ids.extend(parse_event_ids(&s.source_event_ids));
        min_start = min_start.min(s.started_at);
        max_end = max_end.max(s.ended_at);
        tx.execute("DELETE FROM activity_blocks WHERE id = ?1", params![sid])?;
    }
    let merged_json = serde_json::to_string(&combined_event_ids).map_err(|e| {
        AppError::ActivityPersist(format!("merge_blocks: source_event_ids encode failed: {e}"))
    })?;
    tx.execute(
        "UPDATE activity_blocks SET started_at = ?1, ended_at = ?2, \
         source_event_ids = ?3, user_edited = 1, updated_at = ?4 \
         WHERE id = ?5",
        params![min_start, max_end, merged_json, now_ms, target_id],
    )?;
    tx.commit()?;
    Ok(())
}

/// Split one Block at `split_at_ms` (a timestamp within the Block's
/// span). The original Block's `ended_at` becomes `split_at_ms`; a
/// new Block is created covering `[split_at_ms, original.ended_at]`.
/// The new Block inherits the abstract + label (caller can rewrite
/// either afterwards). `user_edited` is set on both halves.
pub fn split_block(
    conn: &mut Connection,
    block_id: &str,
    split_at_ms: i64,
    now_ms: i64,
) -> AppResult<String> {
    let tx = conn.transaction()?;
    let original = load_block_unlocked(&tx, block_id)?;
    if split_at_ms <= original.started_at || split_at_ms >= original.ended_at {
        return Err(AppError::ActivityPersist(format!(
            "split_at_ms ({split_at_ms}) is outside block range \
             [{}..{}]",
            original.started_at, original.ended_at
        )));
    }
    // Trim the original.
    tx.execute(
        "UPDATE activity_blocks SET ended_at = ?1, user_edited = 1, updated_at = ?2 \
         WHERE id = ?3",
        params![split_at_ms, now_ms, block_id],
    )?;
    // Insert the right half.
    let new_id = new_event_id();
    tx.execute(
        "INSERT INTO activity_blocks \
         (id, session_id, started_at, ended_at, primary_app, primary_title, label, \
          generated_abstract, user_edited, source_event_ids, prompt_version_sha, \
          created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, ?9, ?10, ?11, ?11)",
        params![
            new_id,
            original.session_id,
            split_at_ms,
            original.ended_at,
            original.primary_app,
            original.primary_title,
            original.label,
            original.generated_abstract,
            original.source_event_ids,
            original.prompt_version_sha,
            now_ms,
        ],
    )?;
    tx.commit()?;
    Ok(new_id)
}

/// Update the session's rendered summary + the prompt-set fingerprint
/// used to make it. `summary_markdown` is the assembler's full
/// Markdown output; `prompt_set_sha` is the abstractor's prompt
/// fingerprint at generation time (provenance — Principle 2).
pub fn update_session_summary(
    conn: &Connection,
    session_id: &str,
    summary_markdown: &str,
    prompt_set_sha: &str,
    now_ms: i64,
) -> AppResult<()> {
    let n = conn.execute(
        "UPDATE activity_sessions SET summary_markdown = ?1, prompt_set_sha = ?2, \
         updated_at = ?3 WHERE id = ?4",
        params![summary_markdown, prompt_set_sha, now_ms, session_id],
    )?;
    if n == 0 {
        return Err(AppError::ActivityPersist(format!(
            "no such activity session: {session_id}"
        )));
    }
    Ok(())
}

/// Read just the `summary_markdown` column (for the export / copy
/// commands — no need to drag the events list along).
pub fn get_session_summary(conn: &Connection, session_id: &str) -> AppResult<Option<String>> {
    let mut stmt = conn.prepare("SELECT summary_markdown FROM activity_sessions WHERE id = ?1")?;
    match stmt.query_row(params![session_id], |row| row.get::<_, Option<String>>(0)) {
        Ok(v) => Ok(v),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

fn row_to_block(row: &rusqlite::Row<'_>) -> rusqlite::Result<ActivityBlockRow> {
    Ok(ActivityBlockRow {
        id: row.get(0)?,
        session_id: row.get(1)?,
        started_at: row.get(2)?,
        ended_at: row.get(3)?,
        primary_app: row.get(4)?,
        primary_title: row.get(5)?,
        label: row.get(6)?,
        generated_abstract: row.get(7)?,
        user_edited: row.get::<_, i64>(8)? != 0,
        source_event_ids: row.get(9)?,
        prompt_version_sha: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
    })
}

fn load_block_unlocked(conn: &Connection, id: &str) -> AppResult<ActivityBlockRow> {
    let mut stmt = conn.prepare(
        "SELECT id, session_id, started_at, ended_at, primary_app, primary_title, \
                label, generated_abstract, user_edited, source_event_ids, \
                prompt_version_sha, created_at, updated_at \
         FROM activity_blocks WHERE id = ?1",
    )?;
    match stmt.query_row(params![id], row_to_block) {
        Ok(r) => Ok(r),
        Err(rusqlite::Error::QueryReturnedNoRows) => Err(AppError::ActivityPersist(format!(
            "no such activity block: {id}"
        ))),
        Err(e) => Err(e.into()),
    }
}

fn parse_event_ids(json: &str) -> Vec<String> {
    serde_json::from_str(json).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity::persist::insert_session;
    use crate::db::migrations;

    fn fresh_db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        migrations::apply_all(&c).unwrap();
        c
    }

    #[test]
    fn insert_and_list_blocks_round_trips() {
        let c = fresh_db();
        let sid = insert_session(&c, 1_000).unwrap();
        let bid = insert_block(
            &c,
            &sid,
            1_000,
            5_000,
            "code.exe",
            "main.rs",
            Some("The user edited main.rs."),
            "[]",
            "abstract_v1-deadbeef",
            1_000,
        )
        .unwrap();
        let rows = list_blocks(&c, &sid).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, bid);
        assert!(!rows[0].user_edited);
        assert_eq!(rows[0].prompt_version_sha, "abstract_v1-deadbeef");
    }

    #[test]
    fn rename_block_sets_user_edited_flag() {
        let c = fresh_db();
        let sid = insert_session(&c, 1_000).unwrap();
        let bid = insert_block(
            &c, &sid, 1_000, 2_000, "a.exe", "t", None, "[]", "v1", 1_000,
        )
        .unwrap();
        rename_block(&c, &bid, Some("My Label"), 1_500).unwrap();
        let rows = list_blocks(&c, &sid).unwrap();
        assert_eq!(rows[0].label.as_deref(), Some("My Label"));
        assert!(rows[0].user_edited);
        assert!(rows[0].updated_at >= 1_500);
    }

    #[test]
    fn rewrite_abstract_overwrites_and_marks_edited() {
        let c = fresh_db();
        let sid = insert_session(&c, 1_000).unwrap();
        let bid = insert_block(
            &c,
            &sid,
            1_000,
            2_000,
            "a.exe",
            "t",
            Some("original"),
            "[]",
            "v1",
            1_000,
        )
        .unwrap();
        rewrite_abstract(&c, &bid, "user-supplied summary", 1_500).unwrap();
        let rows = list_blocks(&c, &sid).unwrap();
        assert_eq!(
            rows[0].generated_abstract.as_deref(),
            Some("user-supplied summary")
        );
        assert!(rows[0].user_edited);
    }

    #[test]
    fn delete_block_removes_row() {
        let c = fresh_db();
        let sid = insert_session(&c, 1_000).unwrap();
        let bid = insert_block(
            &c, &sid, 1_000, 2_000, "a.exe", "t", None, "[]", "v1", 1_000,
        )
        .unwrap();
        delete_block(&c, &bid).unwrap();
        assert!(list_blocks(&c, &sid).unwrap().is_empty());
    }

    #[test]
    fn delete_block_unknown_id_errors() {
        let c = fresh_db();
        let err = delete_block(&c, "ghost").unwrap_err();
        assert!(matches!(err, AppError::ActivityPersist(_)));
    }

    #[test]
    fn merge_blocks_absorbs_sources_and_deletes_them() {
        let mut c = fresh_db();
        let sid = insert_session(&c, 1_000).unwrap();
        let a = insert_block(
            &c,
            &sid,
            1_000,
            2_000,
            "a.exe",
            "t",
            None,
            r#"["e1"]"#,
            "v1",
            1_000,
        )
        .unwrap();
        let b = insert_block(
            &c,
            &sid,
            2_000,
            5_000,
            "a.exe",
            "t",
            None,
            r#"["e2","e3"]"#,
            "v1",
            2_000,
        )
        .unwrap();
        let cc = insert_block(
            &c,
            &sid,
            5_000,
            7_000,
            "a.exe",
            "t",
            None,
            r#"["e4"]"#,
            "v1",
            5_000,
        )
        .unwrap();

        merge_blocks(&mut c, &a, &[b.clone(), cc.clone()], 10_000).unwrap();

        let rows = list_blocks(&c, &sid).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, a);
        assert_eq!(rows[0].started_at, 1_000);
        assert_eq!(rows[0].ended_at, 7_000);
        assert!(rows[0].user_edited);
        let ids: Vec<String> = serde_json::from_str(&rows[0].source_event_ids).unwrap();
        assert!(ids.contains(&"e1".to_string()));
        assert!(ids.contains(&"e4".to_string()));
    }

    #[test]
    fn split_block_trims_original_and_inserts_right_half() {
        let mut c = fresh_db();
        let sid = insert_session(&c, 1_000).unwrap();
        let bid = insert_block(
            &c,
            &sid,
            1_000,
            5_000,
            "a.exe",
            "t",
            Some("orig"),
            "[]",
            "v1",
            1_000,
        )
        .unwrap();
        let new_id = split_block(&mut c, &bid, 3_000, 6_000).unwrap();
        let rows = list_blocks(&c, &sid).unwrap();
        assert_eq!(rows.len(), 2);
        // Chronological order: original first, then new.
        let orig = rows.iter().find(|r| r.id == bid).unwrap();
        let right = rows.iter().find(|r| r.id == new_id).unwrap();
        assert_eq!(orig.ended_at, 3_000);
        assert_eq!(right.started_at, 3_000);
        assert_eq!(right.ended_at, 5_000);
        assert_eq!(right.generated_abstract.as_deref(), Some("orig"));
        assert!(right.user_edited && orig.user_edited);
    }

    #[test]
    fn split_block_out_of_range_errors() {
        let mut c = fresh_db();
        let sid = insert_session(&c, 1_000).unwrap();
        let bid = insert_block(
            &c, &sid, 1_000, 5_000, "a.exe", "t", None, "[]", "v1", 1_000,
        )
        .unwrap();
        assert!(split_block(&mut c, &bid, 500, 6_000).is_err());
        assert!(split_block(&mut c, &bid, 9_999, 6_000).is_err());
    }

    #[test]
    fn update_session_summary_writes_and_reads_back() {
        let c = fresh_db();
        let sid = insert_session(&c, 1_000).unwrap();
        update_session_summary(&c, &sid, "# Header", "abstract_v1-cafef00d", 2_000).unwrap();
        let back = get_session_summary(&c, &sid).unwrap();
        assert_eq!(back.as_deref(), Some("# Header"));
    }

    #[test]
    fn get_session_summary_returns_none_for_unknown() {
        let c = fresh_db();
        let v = get_session_summary(&c, "ghost").unwrap();
        assert!(v.is_none());
    }

    // ----------------------------------------------------------
    // Phase 10 Wave 6.B — provenance-is-total judge fixtures.
    // ADR 0044 / AGENTS.md Principle 2 ("provenance is total").
    // ----------------------------------------------------------

    /// Seed one session + N blocks with the given `prompt_version_sha`
    /// values. Each block gets an `[event_id]` source list pointing
    /// at a freshly-inserted event row (so the FK walk in C5 is
    /// satisfied for the non-purged path).
    fn seed_session_with_blocks(c: &Connection, shas: &[&str]) -> (String, Vec<String>) {
        use crate::activity::persist::{insert_event, insert_session};
        let sid = insert_session(c, 1_000).unwrap();
        let mut block_ids = Vec::new();
        for (i, sha) in shas.iter().enumerate() {
            let ts = 1_000 + (i as i64) * 1_000;
            let eid =
                insert_event(c, &sid, ts, "app_switch", Some("a.exe"), Some("t"), None).unwrap();
            let src_json = serde_json::to_string(&vec![eid]).unwrap();
            let bid = insert_block(
                c,
                &sid,
                ts,
                ts + 500,
                "a.exe",
                "t",
                Some("summary"),
                &src_json,
                sha,
                ts,
            )
            .unwrap();
            block_ids.push(bid);
        }
        (sid, block_ids)
    }

    #[test]
    fn all_blocks_have_prompt_version_sha() {
        let c = fresh_db();
        let _ = seed_session_with_blocks(
            &c,
            &[
                "template_no_payload_v1",
                "abstract_v1-deadbeef",
                "abstract_v2_audio-cafef00d",
            ],
        );
        let n: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM activity_blocks \
                 WHERE prompt_version_sha IS NULL OR prompt_version_sha = ''",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            n, 0,
            "every block must carry a non-empty prompt_version_sha"
        );
    }

    #[test]
    fn prompt_version_sha_is_known_family() {
        let c = fresh_db();
        let _ = seed_session_with_blocks(
            &c,
            &[
                "template_no_payload_v1",
                "abstract_v1-deadbeef",
                "abstract_v2_audio-cafef00d",
                // 40-hex LLM-prompt SHA fallback (criterion 3 escape hatch).
                "abcdef0123456789abcdef0123456789abcdef01",
            ],
        );
        let v1_re = regex::Regex::new(r"^abstract_v1-[0-9a-f]{8}$").unwrap();
        let v2_re = regex::Regex::new(r"^abstract_v2_audio-[0-9a-f]{8}$").unwrap();
        let sha_re = regex::Regex::new(r"^[0-9a-f]{40,64}$").unwrap();
        let mut stmt = c
            .prepare("SELECT prompt_version_sha FROM activity_blocks")
            .unwrap();
        let rows: Vec<String> = stmt
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert!(!rows.is_empty(), "fixture must seed at least one block");
        for v in rows {
            let ok = v == "template_no_payload_v1"
                || v1_re.is_match(&v)
                || v2_re.is_match(&v)
                || sha_re.is_match(&v);
            assert!(ok, "unknown prompt_version_sha family: {v:?}");
        }
    }

    #[test]
    fn source_event_ids_is_valid_json_array_of_strings() {
        let c = fresh_db();
        let _ = seed_session_with_blocks(&c, &["template_no_payload_v1", "abstract_v1-deadbeef"]);
        // Also seed a block with an empty (but parseable) array — the
        // template-no-payload path produces these.
        use crate::activity::persist::insert_session;
        let sid = insert_session(&c, 9_000).unwrap();
        insert_block(
            &c,
            &sid,
            9_000,
            9_500,
            "a.exe",
            "t",
            None,
            "[]",
            "template_no_payload_v1",
            9_000,
        )
        .unwrap();

        let mut stmt = c
            .prepare("SELECT id, source_event_ids FROM activity_blocks")
            .unwrap();
        let rows: Vec<(String, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert!(!rows.is_empty());
        for (bid, json) in rows {
            let parsed: Result<Vec<String>, _> = serde_json::from_str(&json);
            assert!(
                parsed.is_ok(),
                "block {bid} source_event_ids is not a JSON array of strings: {json:?}"
            );
        }
    }

    #[test]
    fn source_event_ids_reference_existing_rows_or_block_is_purged() {
        let c = fresh_db();
        let (sid, block_ids) =
            seed_session_with_blocks(&c, &["abstract_v1-deadbeef", "abstract_v1-cafef00d"]);
        // Sanity: both blocks' source events exist (FK walk OK).
        for bid in &block_ids {
            let json: String = c
                .query_row(
                    "SELECT source_event_ids FROM activity_blocks WHERE id = ?1",
                    params![bid],
                    |r| r.get(0),
                )
                .unwrap();
            let ids: Vec<String> = serde_json::from_str(&json).unwrap();
            for eid in &ids {
                let n: i64 = c
                    .query_row(
                        "SELECT COUNT(*) FROM activity_events WHERE id = ?1",
                        params![eid],
                        |r| r.get(0),
                    )
                    .unwrap();
                assert_eq!(n, 1, "event id {eid} referenced by block {bid} missing");
            }
        }

        // Now delete one block's underlying events + mark the block as
        // purged. The carve-out: dangling refs are legal iff
        // raw_events_purged_at IS NOT NULL.
        let purged_block = &block_ids[0];
        let json: String = c
            .query_row(
                "SELECT source_event_ids FROM activity_blocks WHERE id = ?1",
                params![purged_block],
                |r| r.get(0),
            )
            .unwrap();
        let ids: Vec<String> = serde_json::from_str(&json).unwrap();
        for eid in &ids {
            c.execute("DELETE FROM activity_events WHERE id = ?1", params![eid])
                .unwrap();
        }
        c.execute(
            "UPDATE activity_blocks SET raw_events_purged_at = ?1 WHERE id = ?2",
            params![999_999i64, purged_block],
        )
        .unwrap();

        // Run the judge's invariant: every block either has a live FK
        // walk OR raw_events_purged_at is set.
        let mut stmt = c
            .prepare(
                "SELECT id, source_event_ids, raw_events_purged_at \
                 FROM activity_blocks WHERE session_id = ?1",
            )
            .unwrap();
        let rows: Vec<(String, String, Option<i64>)> = stmt
            .query_map(params![sid], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        for (bid, json, purged_at) in rows {
            let ids: Vec<String> = serde_json::from_str(&json).unwrap();
            if ids.is_empty() {
                continue;
            }
            let all_present = ids.iter().all(|eid| {
                let n: i64 = c
                    .query_row(
                        "SELECT COUNT(*) FROM activity_events WHERE id = ?1",
                        params![eid],
                        |r| r.get(0),
                    )
                    .unwrap_or(0);
                n == 1
            });
            assert!(
                all_present || purged_at.is_some(),
                "block {bid} has dangling source_event_ids AND raw_events_purged_at is NULL"
            );
        }
    }
}
