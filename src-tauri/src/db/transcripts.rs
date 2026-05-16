//! Transcripts repository.
//!
//! Raw transcripts are IMMUTABLE — this module deliberately has no
//! `update_raw`, `upsert_raw`, or any path that issues `UPDATE
//! transcripts WHERE stage = 'raw'`. The hook `block-raw-transcript-edit`
//! scans non-test code for those patterns; this module would refuse
//! to ship if someone snuck one in. See ADR 0010.

use rusqlite::{params, Connection};

use crate::error::{AppError, AppResult};

/// Lifecycle stage of a transcript row. The storage column is TEXT;
/// this enum is the typed view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    /// Direct STT output. Immutable.
    Raw,
    /// After the cleanup LLM pass.
    Cleaned,
    /// What was actually injected (post user-edit, post-Tier-0 pass-through, etc).
    Final,
}

impl Stage {
    /// Canonical lowercase string form used in the SQL column.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Raw => "raw",
            Self::Cleaned => "cleaned",
            Self::Final => "final",
        }
    }

    /// Strict parse: rejects unknown strings, case-sensitive.
    pub fn parse(s: &str) -> AppResult<Self> {
        match s {
            "raw" => Ok(Self::Raw),
            "cleaned" => Ok(Self::Cleaned),
            "final" => Ok(Self::Final),
            other => Err(AppError::Other(format!(
                "invalid transcript stage: {other:?}"
            ))),
        }
    }
}

/// A transcript row.
#[derive(Debug, Clone)]
pub struct Transcript {
    pub id: i64,
    pub session_id: i64,
    pub stage: Stage,
    pub text: String,
    pub model_used: Option<String>,
    pub created_at: String,
}

/// Insert the immutable raw transcript for a session.
///
/// Errors if a raw transcript already exists for that session via the
/// schema's `UNIQUE(session_id, stage)` constraint.
pub fn insert_raw(conn: &Connection, session_id: i64, text: &str) -> AppResult<i64> {
    conn.execute(
        "INSERT INTO transcripts (session_id, stage, text, model_used) \
         VALUES (?1, 'raw', ?2, NULL)",
        params![session_id, text],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Insert the cleaned transcript. `model_used` is required for cleaned
/// (we always know which model produced it).
pub fn insert_cleaned(
    conn: &Connection,
    session_id: i64,
    text: &str,
    model_used: &str,
) -> AppResult<i64> {
    conn.execute(
        "INSERT INTO transcripts (session_id, stage, text, model_used) \
         VALUES (?1, 'cleaned', ?2, ?3)",
        params![session_id, text, model_used],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Insert the final transcript (what was actually injected). `model_used`
/// is optional because Tier-0 pass-through has no second model.
pub fn insert_final(
    conn: &Connection,
    session_id: i64,
    text: &str,
    model_used: Option<&str>,
) -> AppResult<i64> {
    conn.execute(
        "INSERT INTO transcripts (session_id, stage, text, model_used) \
         VALUES (?1, 'final', ?2, ?3)",
        params![session_id, text, model_used],
    )?;
    Ok(conn.last_insert_rowid())
}

/// All transcripts for a session, ordered raw → cleaned → final.
pub fn get_by_session(conn: &Connection, session_id: i64) -> AppResult<Vec<Transcript>> {
    let mut stmt = conn.prepare(
        "SELECT id, session_id, stage, text, model_used, created_at \
         FROM transcripts WHERE session_id = ?1 \
         ORDER BY CASE stage WHEN 'raw' THEN 0 WHEN 'cleaned' THEN 1 ELSE 2 END",
    )?;
    let rows = stmt.query_map(params![session_id], row_to_transcript)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Lookup a single stage.
pub fn get_stage(
    conn: &Connection,
    session_id: i64,
    stage: Stage,
) -> AppResult<Option<Transcript>> {
    let mut stmt = conn.prepare(
        "SELECT id, session_id, stage, text, model_used, created_at \
         FROM transcripts WHERE session_id = ?1 AND stage = ?2",
    )?;
    let mut rows = stmt.query_map(params![session_id, stage.as_str()], row_to_transcript)?;
    match rows.next() {
        Some(Ok(t)) => Ok(Some(t)),
        Some(Err(e)) => Err(e.into()),
        None => Ok(None),
    }
}

fn row_to_transcript(row: &rusqlite::Row<'_>) -> rusqlite::Result<Transcript> {
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
    Ok(Transcript {
        id: row.get(0)?,
        session_id: row.get(1)?,
        stage,
        text: row.get(3)?,
        model_used: row.get(4)?,
        created_at: row.get(5)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    /// Minimal session row for transcript tests. Bypasses
    /// sessions::insert's provenance enforcement on purpose; provenance
    /// is exercised in sessions.rs's own tests and the integration suite.
    fn session_fixture(conn: &Connection) -> i64 {
        let uuid = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO sessions (uuid, mode_id, hotkey_pressed, started_at, \
             recording_ended_at, status, audio_duration_ms) \
             VALUES (?1, 1, 'Ctrl+Win', '2026-05-15T00:00:00Z', \
             '2026-05-15T00:00:05Z', 'complete', 5000)",
            params![uuid],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    #[test]
    fn insert_raw_returns_id_and_round_trips() {
        let db = Database::open_in_memory().unwrap();
        let session_id = session_fixture(&db.conn);
        let id = insert_raw(&db.conn, session_id, "hello world").unwrap();
        assert!(id > 0);
        let t = get_stage(&db.conn, session_id, Stage::Raw)
            .unwrap()
            .unwrap();
        assert_eq!(t.text, "hello world");
        assert_eq!(t.stage, Stage::Raw);
        assert_eq!(t.model_used, None);
    }

    #[test]
    fn duplicate_raw_for_same_session_errors() {
        let db = Database::open_in_memory().unwrap();
        let session_id = session_fixture(&db.conn);
        insert_raw(&db.conn, session_id, "first").unwrap();
        let err = insert_raw(&db.conn, session_id, "second").unwrap_err();
        assert!(
            matches!(err, AppError::Sqlite(_)),
            "expected UNIQUE violation"
        );
    }

    #[test]
    fn insert_cleaned_carries_model_used() {
        let db = Database::open_in_memory().unwrap();
        let session_id = session_fixture(&db.conn);
        insert_cleaned(&db.conn, session_id, "Hello world.", "qwen2.5:3b").unwrap();
        let t = get_stage(&db.conn, session_id, Stage::Cleaned)
            .unwrap()
            .unwrap();
        assert_eq!(t.model_used.as_deref(), Some("qwen2.5:3b"));
    }

    #[test]
    fn insert_final_allows_no_model() {
        let db = Database::open_in_memory().unwrap();
        let session_id = session_fixture(&db.conn);
        insert_final(&db.conn, session_id, "Hello.", None).unwrap();
        let t = get_stage(&db.conn, session_id, Stage::Final)
            .unwrap()
            .unwrap();
        assert_eq!(t.model_used, None);
    }

    #[test]
    fn get_by_session_returns_stages_in_order() {
        let db = Database::open_in_memory().unwrap();
        let session_id = session_fixture(&db.conn);
        // Insert in non-pipeline order; expect get_by_session to sort.
        insert_final(&db.conn, session_id, "FINAL", None).unwrap();
        insert_raw(&db.conn, session_id, "RAW").unwrap();
        insert_cleaned(&db.conn, session_id, "CLEAN", "m").unwrap();
        let stages: Vec<Stage> = get_by_session(&db.conn, session_id)
            .unwrap()
            .into_iter()
            .map(|t| t.stage)
            .collect();
        assert_eq!(stages, vec![Stage::Raw, Stage::Cleaned, Stage::Final]);
    }

    #[test]
    fn get_stage_returns_none_for_missing() {
        let db = Database::open_in_memory().unwrap();
        let session_id = session_fixture(&db.conn);
        assert!(get_stage(&db.conn, session_id, Stage::Raw)
            .unwrap()
            .is_none());
    }

    #[test]
    fn stage_parse_accepts_canonical_strings_only() {
        assert_eq!(Stage::parse("raw").unwrap(), Stage::Raw);
        assert_eq!(Stage::parse("cleaned").unwrap(), Stage::Cleaned);
        assert_eq!(Stage::parse("final").unwrap(), Stage::Final);
        assert!(Stage::parse("RAW").is_err());
        assert!(Stage::parse("bogus").is_err());
        assert!(Stage::parse("").is_err());
    }
}
