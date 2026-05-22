//! DB read + write for activity capture.
//!
//! Tables owned (migration 012):
//!
//! - `activity_sessions` — one row per `Start..Stop` session.
//! - `activity_events` — RAW, IMMUTABLE timeline events.
//!
//! Wave 1B writes a narrow slice of these columns. Optional columns
//! (`label`, `project_id`, `project_label`, `summary_markdown`,
//! `prompt_set_sha`, `snapshot_json` for non-trivial UIA payloads)
//! land in later waves; the v1 inserts leave them `NULL`.
//!
//! ## Principle 1 (raw is immutable)
//!
//! The `activity_events` table is enforced by SQL triggers
//! (migration 012). This module never issues UPDATE against it.
//! `update_session_status` only touches the mutable
//! `activity_sessions` row.

#![allow(missing_docs)]

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

use super::ids::{new_event_id, new_session_id};

/// Persisted session status. Mirrors the DB-side `TEXT` enum on
/// `activity_sessions.status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    /// Open session — sampler is (or has been) running.
    InProgress,
    /// Clean stop via the orchestrator's `Stop` input.
    Completed,
    /// Process is shutting down; session row was finalized but the
    /// last few events may not have made it to disk. Wave 2's
    /// crash-recovery pass converts `in_progress` rows older than
    /// the grace window into `crashed_recovered`.
    Partial,
    /// Wave 2-+: a previously-`in_progress` row finalized by the
    /// recovery pass.
    CrashedRecovered,
}

impl SessionStatus {
    /// String the DB stores. Matches the CHECK constraints implicit
    /// in migration 012's prose (no SQL-level check enum; the
    /// strings are the wire format).
    pub const fn as_db_str(self) -> &'static str {
        match self {
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
            Self::Partial => "partial",
            Self::CrashedRecovered => "crashed_recovered",
        }
    }
}

/// One persisted event row. Read-only DTO; events are immutable.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityEventRow {
    pub id: String,
    pub session_id: String,
    /// Unix epoch ms.
    pub ts: i64,
    pub kind: String,
    pub app_name: Option<String>,
    pub window_title: Option<String>,
    /// Wave 1B writes `null` or a minimal `{"app":..,"title":..}`
    /// payload; Wave 2 expands.
    pub snapshot_json: Option<String>,
    pub created_at: i64,
}

/// One persisted session row, projection used by the IPC list/detail.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivitySessionRow {
    pub id: String,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub status: SessionStatus,
    pub audio_enabled: bool,
    pub screenshot_enabled: bool,
    pub label: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Detail view: session row plus its events in chronological order.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivitySessionDetail {
    pub session: ActivitySessionRow,
    pub events: Vec<ActivityEventRow>,
}

/// Insert a new `in_progress` session row. Returns the assigned id.
pub fn insert_session(conn: &Connection, started_at_ms: i64) -> AppResult<String> {
    let id = new_session_id();
    let now = started_at_ms;
    conn.execute(
        "INSERT INTO activity_sessions \
         (id, started_at, status, audio_enabled, screenshot_enabled, created_at, updated_at) \
         VALUES (?1, ?2, 'in_progress', 0, 0, ?3, ?3)",
        params![id, started_at_ms, now],
    )?;
    Ok(id)
}

/// Mark a session as terminated. The orchestrator should pass
/// `SessionStatus::Completed` for a user stop and
/// `SessionStatus::Partial` for a shutdown-triggered close.
pub fn finalize_session(
    conn: &Connection,
    session_id: &str,
    ended_at_ms: i64,
    status: SessionStatus,
) -> AppResult<()> {
    let n = conn.execute(
        "UPDATE activity_sessions SET status = ?1, ended_at = ?2, updated_at = ?2 \
         WHERE id = ?3 AND status = 'in_progress'",
        params![status.as_db_str(), ended_at_ms, session_id],
    )?;
    if n == 0 {
        return Err(AppError::ActivityPersist(format!(
            "no in_progress session to finalize: {session_id}"
        )));
    }
    Ok(())
}

/// Insert one event row. Pure write; no UPDATE path (immutability).
///
/// `snapshot_json` is a free-form JSON string the caller controls.
/// Wave 1B passes `None` for control events (paused/resumed) and a
/// minimal `{"app":...,"title":...}` payload for context snapshots.
pub fn insert_event(
    conn: &Connection,
    session_id: &str,
    ts_ms: i64,
    kind: &str,
    app_name: Option<&str>,
    window_title: Option<&str>,
    snapshot_json: Option<&str>,
) -> AppResult<String> {
    let id = new_event_id();
    conn.execute(
        "INSERT INTO activity_events \
         (id, session_id, ts, kind, app_name, window_title, snapshot_json, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?3)",
        params![
            id,
            session_id,
            ts_ms,
            kind,
            app_name,
            window_title,
            snapshot_json
        ],
    )?;
    Ok(id)
}

/// Read the most recent N sessions, newest first.
pub fn list_sessions(conn: &Connection, limit: i64) -> AppResult<Vec<ActivitySessionRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, started_at, ended_at, status, audio_enabled, \
                screenshot_enabled, label, created_at, updated_at \
         FROM activity_sessions \
         ORDER BY started_at DESC \
         LIMIT ?1",
    )?;
    let rows = stmt
        .query_map(params![limit], row_to_session)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Load a session + all its events, chronologically.
pub fn get_session_detail(
    conn: &Connection,
    session_id: &str,
) -> AppResult<Option<ActivitySessionDetail>> {
    let session = {
        let mut stmt = conn.prepare(
            "SELECT id, started_at, ended_at, status, audio_enabled, \
                    screenshot_enabled, label, created_at, updated_at \
             FROM activity_sessions WHERE id = ?1",
        )?;
        match stmt.query_row(params![session_id], row_to_session) {
            Ok(r) => r,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
            Err(e) => return Err(e.into()),
        }
    };

    let mut stmt = conn.prepare(
        "SELECT id, session_id, ts, kind, app_name, window_title, \
                snapshot_json, created_at \
         FROM activity_events WHERE session_id = ?1 \
         ORDER BY ts ASC, id ASC",
    )?;
    let events = stmt
        .query_map(params![session_id], |row| {
            Ok(ActivityEventRow {
                id: row.get(0)?,
                session_id: row.get(1)?,
                ts: row.get(2)?,
                kind: row.get(3)?,
                app_name: row.get(4)?,
                window_title: row.get(5)?,
                snapshot_json: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Some(ActivitySessionDetail { session, events }))
}

/// Delete one session (CASCADE clears events through the FK).
pub fn delete_session(conn: &Connection, session_id: &str) -> AppResult<()> {
    let n = conn.execute(
        "DELETE FROM activity_sessions WHERE id = ?1",
        params![session_id],
    )?;
    if n == 0 {
        return Err(AppError::ActivityPersist(format!(
            "no such activity session: {session_id}"
        )));
    }
    Ok(())
}

fn row_to_session(row: &rusqlite::Row<'_>) -> rusqlite::Result<ActivitySessionRow> {
    let status_str: String = row.get(3)?;
    let status = match status_str.as_str() {
        "in_progress" => SessionStatus::InProgress,
        "completed" => SessionStatus::Completed,
        "partial" => SessionStatus::Partial,
        "crashed_recovered" => SessionStatus::CrashedRecovered,
        other => {
            return Err(rusqlite::Error::FromSqlConversionFailure(
                3,
                rusqlite::types::Type::Text,
                format!("unknown activity session status: {other}").into(),
            ))
        }
    };
    Ok(ActivitySessionRow {
        id: row.get(0)?,
        started_at: row.get(1)?,
        ended_at: row.get(2)?,
        status,
        audio_enabled: row.get::<_, i64>(4)? != 0,
        screenshot_enabled: row.get::<_, i64>(5)? != 0,
        label: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrations;

    fn fresh_db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        migrations::apply_all(&c).unwrap();
        c
    }

    #[test]
    fn insert_and_list_session_round_trip() {
        let c = fresh_db();
        let id = insert_session(&c, 1_000).unwrap();
        assert!(!id.is_empty());

        let rows = list_sessions(&c, 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, id);
        assert_eq!(rows[0].status, SessionStatus::InProgress);
        assert_eq!(rows[0].started_at, 1_000);
        assert!(rows[0].ended_at.is_none());
    }

    #[test]
    fn finalize_session_flips_to_completed() {
        let c = fresh_db();
        let id = insert_session(&c, 1_000).unwrap();
        finalize_session(&c, &id, 2_000, SessionStatus::Completed).unwrap();

        let detail = get_session_detail(&c, &id).unwrap().unwrap();
        assert_eq!(detail.session.status, SessionStatus::Completed);
        assert_eq!(detail.session.ended_at, Some(2_000));
    }

    #[test]
    fn finalize_session_twice_errors() {
        let c = fresh_db();
        let id = insert_session(&c, 1_000).unwrap();
        finalize_session(&c, &id, 2_000, SessionStatus::Completed).unwrap();
        let err = finalize_session(&c, &id, 3_000, SessionStatus::Completed).unwrap_err();
        assert!(matches!(err, AppError::ActivityPersist(_)));
    }

    #[test]
    fn insert_event_appears_in_detail() {
        let c = fresh_db();
        let id = insert_session(&c, 1_000).unwrap();
        insert_event(
            &c,
            &id,
            1_100,
            "app_switch",
            Some("chrome.exe"),
            Some("Tabs"),
            None,
        )
        .unwrap();
        insert_event(
            &c,
            &id,
            1_200,
            "context_snapshot",
            Some("chrome.exe"),
            Some("Tabs"),
            Some(r#"{"app":"chrome.exe","title":"Tabs"}"#),
        )
        .unwrap();

        let detail = get_session_detail(&c, &id).unwrap().unwrap();
        assert_eq!(detail.events.len(), 2);
        assert_eq!(detail.events[0].kind, "app_switch");
        assert_eq!(
            detail.events[1].snapshot_json.as_deref(),
            Some(r#"{"app":"chrome.exe","title":"Tabs"}"#)
        );
    }

    #[test]
    fn raw_immutability_trigger_fires_via_persist_api() {
        // Direct SQL UPDATE against an event row must fail — Principle 1.
        // We surface this here so a future "patch event" helper can't
        // be added without staring at this test failing first.
        let c = fresh_db();
        let id = insert_session(&c, 1_000).unwrap();
        insert_event(&c, &id, 1_100, "app_switch", Some("a.exe"), Some("t"), None).unwrap();
        let upd = c.execute(
            "UPDATE activity_events SET window_title = 'no' WHERE session_id = ?1",
            params![id],
        );
        assert!(upd.is_err(), "Principle 1: events are immutable");
    }

    #[test]
    fn delete_session_cascades_events() {
        let c = fresh_db();
        let id = insert_session(&c, 1_000).unwrap();
        insert_event(&c, &id, 1_100, "app_switch", None, None, None).unwrap();
        // Must finalize first — otherwise the no_delete trigger only
        // gates non-in_progress sessions; here delete on in_progress
        // is allowed but we still want to verify the CASCADE.
        finalize_session(&c, &id, 2_000, SessionStatus::Completed).unwrap();
        delete_session(&c, &id).unwrap();
        let detail = get_session_detail(&c, &id).unwrap();
        assert!(detail.is_none());
    }

    #[test]
    fn list_sessions_orders_by_started_at_desc() {
        let c = fresh_db();
        let _a = insert_session(&c, 1_000).unwrap();
        let _b = insert_session(&c, 3_000).unwrap();
        let _c = insert_session(&c, 2_000).unwrap();
        let rows = list_sessions(&c, 10).unwrap();
        assert_eq!(rows[0].started_at, 3_000);
        assert_eq!(rows[1].started_at, 2_000);
        assert_eq!(rows[2].started_at, 1_000);
    }
}
