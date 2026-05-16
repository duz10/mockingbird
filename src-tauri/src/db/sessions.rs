//! Sessions repository — the central provenance pivot.
//!
//! `NewSession` requires provenance FKs (`prompt_id`,
//! `dictionary_snapshot_id`, `example_set_id`) at the type level, even
//! though the SQL schema allows them to be NULL. This is the
//! application-layer enforcement of the "provenance is total" rule
//! (PLAN §12, AGENTS.md principle 2). The schema and the API
//! deliberately disagree.

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone)]
pub struct NewSession {
    pub uuid: String,
    pub mode_id: i64,
    pub hotkey_pressed: String,
    pub started_at: String,
    pub recording_ended_at: String,
    pub status: SessionStatus,
    pub foreground_app: Option<String>,
    pub foreground_window_title: Option<String>,
    pub audio_duration_ms: i64,
    pub audio_blob_path: Option<String>,

    // Provenance — REQUIRED at application layer.
    pub prompt_id: i64,
    pub dictionary_snapshot_id: i64,
    pub example_set_id: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    Recording,
    Processing,
    #[default]
    Complete,
    Error,
}

impl SessionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Recording => "recording",
            Self::Processing => "processing",
            Self::Complete => "complete",
            Self::Error => "error",
        }
    }

    pub fn parse(s: &str) -> AppResult<Self> {
        match s {
            "recording" => Ok(Self::Recording),
            "processing" => Ok(Self::Processing),
            "complete" => Ok(Self::Complete),
            "error" => Ok(Self::Error),
            other => Err(AppError::Other(format!(
                "invalid session status: {other:?}"
            ))),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Session {
    pub id: i64,
    pub uuid: String,
    pub mode_id: i64,
    pub hotkey_pressed: String,
    pub started_at: String,
    pub recording_ended_at: String,
    pub processing_completed_at: Option<String>,
    pub status: SessionStatus,
    pub error_message: Option<String>,
    pub foreground_app: Option<String>,
    pub foreground_window_title: Option<String>,
    pub audio_duration_ms: i64,
    pub audio_blob_path: Option<String>,
    pub prompt_id: Option<i64>,
    pub dictionary_snapshot_id: Option<i64>,
    pub example_set_id: Option<i64>,
    pub stt_latency_ms: Option<i64>,
    pub cleanup_latency_ms: Option<i64>,
    pub injection_latency_ms: Option<i64>,
}

#[derive(Debug, Clone, Default)]
pub struct ProcessingCompletion {
    pub completed_at: String,
    pub status: SessionStatus,
    pub stt_latency_ms: Option<i64>,
    pub cleanup_latency_ms: Option<i64>,
    pub injection_latency_ms: Option<i64>,
}

/// Insert a new session row. All provenance FKs must point at real
/// rows in their respective tables (mode_id is FK-enforced by SQL;
/// the others are NULLable in SQL but mandatory at this API layer).
pub fn insert(conn: &Connection, new: &NewSession) -> AppResult<i64> {
    conn.execute(
        "INSERT INTO sessions ( \
            uuid, mode_id, hotkey_pressed, started_at, recording_ended_at, \
            status, foreground_app, foreground_window_title, audio_duration_ms, \
            audio_blob_path, prompt_id, dictionary_snapshot_id, example_set_id \
         ) VALUES \
         (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            new.uuid,
            new.mode_id,
            new.hotkey_pressed,
            new.started_at,
            new.recording_ended_at,
            new.status.as_str(),
            new.foreground_app,
            new.foreground_window_title,
            new.audio_duration_ms,
            new.audio_blob_path,
            new.prompt_id,
            new.dictionary_snapshot_id,
            new.example_set_id,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn get_by_id(conn: &Connection, id: i64) -> AppResult<Option<Session>> {
    fetch_one(conn, "WHERE id = ?1", params![id])
}

pub fn get_by_uuid(conn: &Connection, uuid: &str) -> AppResult<Option<Session>> {
    fetch_one(conn, "WHERE uuid = ?1", params![uuid])
}

pub fn list_recent(conn: &Connection, limit: usize) -> AppResult<Vec<Session>> {
    let sql = format!("{} ORDER BY started_at DESC LIMIT ?1", SELECT_ALL);
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![limit as i64], row_to_session)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn update_processing_complete(
    conn: &Connection,
    id: i64,
    completion: &ProcessingCompletion,
) -> AppResult<()> {
    conn.execute(
        "UPDATE sessions SET \
            processing_completed_at = ?1, \
            status = ?2, \
            stt_latency_ms = ?3, \
            cleanup_latency_ms = ?4, \
            injection_latency_ms = ?5 \
         WHERE id = ?6",
        params![
            completion.completed_at,
            completion.status.as_str(),
            completion.stt_latency_ms,
            completion.cleanup_latency_ms,
            completion.injection_latency_ms,
            id,
        ],
    )?;
    Ok(())
}

pub fn update_status_error(conn: &Connection, id: i64, error_message: &str) -> AppResult<()> {
    conn.execute(
        "UPDATE sessions SET status = 'error', error_message = ?1 WHERE id = ?2",
        params![error_message, id],
    )?;
    Ok(())
}

const SELECT_ALL: &str =
    "SELECT id, uuid, mode_id, hotkey_pressed, started_at, recording_ended_at, \
            processing_completed_at, status, error_message, foreground_app, \
            foreground_window_title, audio_duration_ms, audio_blob_path, \
            prompt_id, dictionary_snapshot_id, example_set_id, \
            stt_latency_ms, cleanup_latency_ms, injection_latency_ms \
     FROM sessions";

fn fetch_one(
    conn: &Connection,
    where_clause: &str,
    params: impl rusqlite::Params,
) -> AppResult<Option<Session>> {
    let sql = format!("{SELECT_ALL} {where_clause}");
    let mut stmt = conn.prepare(&sql)?;
    let session = stmt.query_row(params, row_to_session).optional()?;
    Ok(session)
}

fn row_to_session(row: &rusqlite::Row<'_>) -> rusqlite::Result<Session> {
    let status_str: String = row.get(7)?;
    let status = SessionStatus::parse(&status_str).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            7,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                e.to_string(),
            )),
        )
    })?;
    Ok(Session {
        id: row.get(0)?,
        uuid: row.get(1)?,
        mode_id: row.get(2)?,
        hotkey_pressed: row.get(3)?,
        started_at: row.get(4)?,
        recording_ended_at: row.get(5)?,
        processing_completed_at: row.get(6)?,
        status,
        error_message: row.get(8)?,
        foreground_app: row.get(9)?,
        foreground_window_title: row.get(10)?,
        audio_duration_ms: row.get(11)?,
        audio_blob_path: row.get(12)?,
        prompt_id: row.get(13)?,
        dictionary_snapshot_id: row.get(14)?,
        example_set_id: row.get(15)?,
        stt_latency_ms: row.get(16)?,
        cleanup_latency_ms: row.get(17)?,
        injection_latency_ms: row.get(18)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    /// Build a NewSession with valid seeded mode_id=1 and freshly-inserted
    /// snapshot + example_set rows so FK constraints pass.
    fn fresh_new_session(conn: &Connection) -> NewSession {
        // Provenance prerequisites via raw INSERTs (we're testing
        // sessions.rs, not dictionary/examples).
        conn.execute(
            "INSERT INTO dictionary_snapshots (term_ids) VALUES ('[]')",
            [],
        )
        .unwrap();
        let snapshot_id = conn.last_insert_rowid();

        conn.execute(
            "INSERT INTO example_sets (mode_slug, example_ids) VALUES ('normal', '[]')",
            [],
        )
        .unwrap();
        let example_set_id = conn.last_insert_rowid();

        // prompt_id from seed (migration 003 inserted version-1 of each mode).
        let prompt_id: i64 = conn
            .query_row(
                "SELECT id FROM prompts WHERE mode_slug='normal' AND version=1",
                [],
                |r| r.get(0),
            )
            .unwrap();

        NewSession {
            uuid: uuid::Uuid::new_v4().to_string(),
            mode_id: 1,
            hotkey_pressed: "Ctrl+Win".into(),
            started_at: "2026-05-15T00:00:00Z".into(),
            recording_ended_at: "2026-05-15T00:00:05Z".into(),
            status: SessionStatus::Recording,
            foreground_app: Some("notepad.exe".into()),
            foreground_window_title: Some("Untitled - Notepad".into()),
            audio_duration_ms: 5000,
            audio_blob_path: None,
            prompt_id,
            dictionary_snapshot_id: snapshot_id,
            example_set_id,
        }
    }

    #[test]
    fn insert_and_round_trip() {
        let db = Database::open_in_memory().unwrap();
        let new = fresh_new_session(&db.conn);
        let uuid_in = new.uuid.clone();
        let id = insert(&db.conn, &new).unwrap();
        let got = get_by_id(&db.conn, id).unwrap().unwrap();
        assert_eq!(got.uuid, uuid_in);
        assert_eq!(got.status, SessionStatus::Recording);
        assert_eq!(got.foreground_app.as_deref(), Some("notepad.exe"));
    }

    #[test]
    fn duplicate_uuid_errors() {
        let db = Database::open_in_memory().unwrap();
        let new = fresh_new_session(&db.conn);
        insert(&db.conn, &new).unwrap();
        let err = insert(&db.conn, &new).unwrap_err();
        assert!(matches!(err, AppError::Sqlite(_)));
    }

    #[test]
    fn get_by_uuid_returns_none_for_missing() {
        let db = Database::open_in_memory().unwrap();
        assert!(get_by_uuid(&db.conn, "no-such-uuid").unwrap().is_none());
    }

    #[test]
    fn list_recent_orders_by_started_at_desc() {
        let db = Database::open_in_memory().unwrap();
        let mut new1 = fresh_new_session(&db.conn);
        new1.started_at = "2026-05-15T00:00:00Z".into();
        insert(&db.conn, &new1).unwrap();

        let mut new2 = fresh_new_session(&db.conn);
        new2.started_at = "2026-05-15T01:00:00Z".into();
        insert(&db.conn, &new2).unwrap();

        let list = list_recent(&db.conn, 10).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].started_at, "2026-05-15T01:00:00Z");
        assert_eq!(list[1].started_at, "2026-05-15T00:00:00Z");
    }

    #[test]
    fn update_processing_complete_sets_latencies() {
        let db = Database::open_in_memory().unwrap();
        let new = fresh_new_session(&db.conn);
        let id = insert(&db.conn, &new).unwrap();
        update_processing_complete(
            &db.conn,
            id,
            &ProcessingCompletion {
                completed_at: "2026-05-15T00:00:10Z".into(),
                status: SessionStatus::Complete,
                stt_latency_ms: Some(150),
                cleanup_latency_ms: Some(800),
                injection_latency_ms: Some(20),
            },
        )
        .unwrap();
        let got = get_by_id(&db.conn, id).unwrap().unwrap();
        assert_eq!(got.status, SessionStatus::Complete);
        assert_eq!(got.stt_latency_ms, Some(150));
        assert_eq!(got.cleanup_latency_ms, Some(800));
        assert_eq!(got.injection_latency_ms, Some(20));
    }

    #[test]
    fn update_status_error_sets_status_and_message() {
        let db = Database::open_in_memory().unwrap();
        let new = fresh_new_session(&db.conn);
        let id = insert(&db.conn, &new).unwrap();
        update_status_error(&db.conn, id, "stt failed: cuda OOM").unwrap();
        let got = get_by_id(&db.conn, id).unwrap().unwrap();
        assert_eq!(got.status, SessionStatus::Error);
        assert_eq!(got.error_message.as_deref(), Some("stt failed: cuda OOM"));
    }

    #[test]
    fn session_status_parse_round_trips() {
        for s in [
            SessionStatus::Recording,
            SessionStatus::Processing,
            SessionStatus::Complete,
            SessionStatus::Error,
        ] {
            assert_eq!(SessionStatus::parse(s.as_str()).unwrap(), s);
        }
        assert!(SessionStatus::parse("BOGUS").is_err());
    }

    #[test]
    fn insert_with_bad_mode_id_errors_via_fk() {
        let db = Database::open_in_memory().unwrap();
        let mut new = fresh_new_session(&db.conn);
        new.mode_id = 99_999;
        let err = insert(&db.conn, &new).unwrap_err();
        assert!(matches!(err, AppError::Sqlite(_)));
    }
}
