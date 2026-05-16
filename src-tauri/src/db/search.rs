//! FTS5 search across all transcripts.
//!
//! Phase 1 uses conservative phrase-escaping: the raw query is doubled-
//! quote escaped and wrapped in a single FTS5 phrase. This prevents
//! operator injection (AND/OR/NEAR/prefix) AND SQL injection while
//! giving deterministic substring matching.
//!
//! Phase 6 will introduce operator-aware parsing for the history viewer.

use rusqlite::{params, Connection};

use super::transcripts::Stage;
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone)]
pub struct SearchHit {
    pub transcript_id: i64,
    pub session_id: i64,
    pub stage: Stage,
    pub snippet: String,
    /// SQLite bm25() score. Lower is a better match.
    pub bm25_rank: f64,
}

/// Convert a free-form query into an FTS5-safe phrase. Public so unit
/// tests can pin the escaping rules.
///
/// Result is wrapped in `"…"` with internal `"` doubled. Any FTS5
/// operators or special characters become literal text inside the
/// phrase.
pub fn sanitize_query(raw: &str) -> String {
    let escaped = raw.replace('"', "\"\"");
    format!("\"{escaped}\"")
}

/// Full-text search across all transcripts. Returns hits ordered by
/// bm25 rank (best match first), limited to `limit` rows.
pub fn search(conn: &Connection, query: &str, limit: usize) -> AppResult<Vec<SearchHit>> {
    let sanitized = sanitize_query(query);
    // ORDER BY bm25() ASC because lower rank = better match. We coerce
    // limit to i64; rusqlite has no native usize binding.
    let mut stmt = conn.prepare(
        "SELECT t.id, t.session_id, t.stage, \
                snippet(transcripts_fts, 0, '<mark>', '</mark>', '…', 16), \
                bm25(transcripts_fts) \
         FROM transcripts_fts \
         JOIN transcripts t ON t.id = transcripts_fts.rowid \
         WHERE transcripts_fts MATCH ?1 \
         ORDER BY bm25(transcripts_fts) ASC \
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![sanitized, limit as i64], row_to_hit)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Lightweight count, used by the Wave-5 `fts5-smoke` judge and the
/// `fts_smoke_test` Tauri command. Doesn't materialize hits.
pub fn smoke_test_count(conn: &Connection, query: &str) -> AppResult<usize> {
    let sanitized = sanitize_query(query);
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM transcripts_fts WHERE transcripts_fts MATCH ?1",
        params![sanitized],
        |r| r.get(0),
    )?;
    Ok(count as usize)
}

fn row_to_hit(row: &rusqlite::Row<'_>) -> rusqlite::Result<SearchHit> {
    let stage_str: String = row.get(2)?;
    let stage = Stage::parse(&stage_str).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            2,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                e.to_string(),
            )),
        )
    })?;
    Ok(SearchHit {
        transcript_id: row.get(0)?,
        session_id: row.get(1)?,
        stage,
        snippet: row.get(3)?,
        bm25_rank: row.get(4)?,
    })
}

// AppError must be reachable for tests that explicitly check error
// kinds (none today, but keep the import path warm).
#[allow(dead_code)]
fn _force_apperror_import(e: AppError) -> AppError {
    e
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use rusqlite::params;

    fn session_fixture(conn: &Connection) -> i64 {
        conn.execute(
            "INSERT INTO sessions (uuid, mode_id, hotkey_pressed, started_at, \
             recording_ended_at, status, audio_duration_ms) \
             VALUES (?1, 1, 'Ctrl+Win', '2026-05-15T00:00:00Z', \
             '2026-05-15T00:00:05Z', 'complete', 5000)",
            params![uuid::Uuid::new_v4().to_string()],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    #[test]
    fn sanitize_wraps_in_quotes_and_doubles_internal_quotes() {
        assert_eq!(sanitize_query("hello"), "\"hello\"");
        assert_eq!(sanitize_query("hello world"), "\"hello world\"");
        // Input `say "hi"` (8 chars) → wrap + double quotes → `"say ""hi"""` (12 chars).
        assert_eq!(sanitize_query("say \"hi\""), "\"say \"\"hi\"\"\"");
    }

    #[test]
    fn sanitize_treats_operators_as_literal_text() {
        // None of AND/OR/NEAR are operators inside a phrase.
        let out = sanitize_query("AND OR NEAR");
        assert_eq!(out, "\"AND OR NEAR\"");
    }

    #[test]
    fn sanitize_does_not_unwrap_sql_special_chars() {
        // Phase-1 invariant: nothing gets stripped, only quote-escaped.
        let out = sanitize_query("DROP TABLE; --");
        assert_eq!(out, "\"DROP TABLE; --\"");
    }

    #[test]
    fn search_returns_empty_for_no_matches() {
        let db = Database::open_in_memory().unwrap();
        let session = session_fixture(&db.conn);
        crate::db::transcripts::insert_raw(&db.conn, session, "hello world").unwrap();
        let hits = search(&db.conn, "xyzzy_no_match", 10).unwrap();
        assert!(hits.is_empty());
    }

    #[test]
    fn search_finds_inserted_raw_transcript() {
        let db = Database::open_in_memory().unwrap();
        let session = session_fixture(&db.conn);
        crate::db::transcripts::insert_raw(&db.conn, session, "hello mockingbird").unwrap();
        let hits = search(&db.conn, "mockingbird", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].stage, Stage::Raw);
        assert!(hits[0].snippet.contains("mockingbird"));
    }

    #[test]
    fn search_respects_limit() {
        let db = Database::open_in_memory().unwrap();
        for _ in 0..5 {
            let session = session_fixture(&db.conn);
            crate::db::transcripts::insert_raw(&db.conn, session, "hello universe").unwrap();
        }
        let hits = search(&db.conn, "hello", 3).unwrap();
        assert_eq!(hits.len(), 3);
    }

    #[test]
    fn search_ranks_better_matches_higher() {
        let db = Database::open_in_memory().unwrap();
        let s1 = session_fixture(&db.conn);
        let s2 = session_fixture(&db.conn);
        // s1 has "needle" once; s2 has it three times — should rank higher (lower bm25).
        crate::db::transcripts::insert_raw(&db.conn, s1, "haystack with one needle").unwrap();
        crate::db::transcripts::insert_raw(&db.conn, s2, "needle needle needle haystack").unwrap();
        let hits = search(&db.conn, "needle", 10).unwrap();
        assert_eq!(hits.len(), 2);
        // bm25 ASC means first hit should be the denser match.
        assert!(hits[0].bm25_rank <= hits[1].bm25_rank);
        assert_eq!(hits[0].session_id, s2);
    }

    #[test]
    fn smoke_test_count_returns_zero_for_no_matches() {
        let db = Database::open_in_memory().unwrap();
        let n = smoke_test_count(&db.conn, "nothing_here").unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn smoke_test_count_returns_positive_after_insert() {
        let db = Database::open_in_memory().unwrap();
        let session = session_fixture(&db.conn);
        crate::db::transcripts::insert_raw(&db.conn, session, "smoke test passes").unwrap();
        let n = smoke_test_count(&db.conn, "smoke").unwrap();
        assert_eq!(n, 1);
    }
}
