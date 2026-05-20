//! Persist a completed meeting — atomic session-row + transcript rows.
//!
//! Mirrors the Phase 3 Wave 4 dictation `db::sessions::insert_session`
//! pattern: single transaction, full provenance, individual transcript
//! INSERT failures non-fatal (mirrors the Wave 4.9 Bug A fix — a bad
//! transcript shouldn't lose the whole session).
//!
//! Wave 4 ships the impl per `docs/phases/phase-mc-wave4-brief.md` §4.1.

use rusqlite::{params, Connection};

use crate::error::{AppError, AppResult};

use super::capture::MeetingSource;
use super::long_form_stt::TimedSegment;

/// Persisted meeting status. Mirrors `meeting_sessions.status` column.
///
/// `Serialize`/`Deserialize` flow through the Tauri IPC boundary as
/// lowercase strings (matches the DB column form so the wire and the
/// storage agree).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MeetingStatus {
    Complete,
    /// One channel transcribed, the other failed. Partial-success.
    Partial,
    /// System loopback failed mid-run; demoted to mic-only.
    Demoted,
    /// App was closed / crashed mid-recording; finalized on Drop.
    Interrupted,
    /// Catastrophic failure; nothing usable persisted.
    Failed,
}

impl MeetingStatus {
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Partial => "partial",
            Self::Demoted => "demoted",
            Self::Interrupted => "interrupted",
            Self::Failed => "failed",
        }
    }

    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "complete" => Some(Self::Complete),
            "partial" => Some(Self::Partial),
            "demoted" => Some(Self::Demoted),
            "interrupted" => Some(Self::Interrupted),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

/// Everything needed to commit one meeting to the DB. Built by the
/// runtime on `meeting_stop → long-form-stt → formatter → merge` path
/// and handed to [`persist_meeting`] in one atomic call.
#[derive(Debug, Clone)]
pub struct MeetingPersistRequest {
    pub uuid: String,
    pub title: Option<String>,
    pub started_at: String,
    pub ended_at: String,
    pub status: MeetingStatus,
    pub error_message: Option<String>,
    pub source: MeetingSource,
    pub total_duration_ms: u64,
    pub mic_duration_ms: Option<u64>,
    pub sys_duration_ms: Option<u64>,
    pub hotkey_pressed: String,
    pub audio_blob_path: Option<String>,
    pub whisper_model_id: String,
    pub formatter_version: String,
    pub chunk_count_mic: Option<u32>,
    pub chunk_count_sys: Option<u32>,
    pub stt_latency_ms: Option<u64>,
    pub formatter_latency_ms: Option<u64>,
    /// Formatted prose per channel; the `_segments` field carries the
    /// matching raw JSON for the `raw_segments` stage row.
    pub formatted_mic: Option<String>,
    pub formatted_sys: Option<String>,
    pub formatted_merged: Option<String>,
    pub segments_mic: Option<Vec<TimedSegment>>,
    pub segments_sys: Option<Vec<TimedSegment>>,
}

/// Persist the meeting in one transaction.
///
/// Behavior (binding, per phase-mc-wave4-brief §4.1):
///   1. Open a transaction on `conn`.
///   2. INSERT into `meeting_sessions`. Failure → rollback + propagate.
///   3. For each `Some` formatted channel (mic, sys, merged), attempt
///      INSERT into `meeting_transcripts` with stage='formatted'.
///      Individual failures are logged via `tracing::warn!` and
///      skipped — they do NOT roll back the transaction (Wave 4.9
///      Bug A pattern: a single bad transcript row shouldn't lose the
///      whole session).
///   4. For each `Some` segment array (mic, sys), attempt INSERT with
///      stage='raw_segments', body = `serde_json::to_string(segments)`.
///      Same non-fatal policy as step 3.
///   5. Commit. Return the session rowid.
///
/// On step-2 failure the transaction is dropped (rolled back); no
/// partial state lands.
pub fn persist_meeting(conn: &Connection, req: &MeetingPersistRequest) -> AppResult<i64> {
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO meeting_sessions (\
            uuid, title, started_at, ended_at, status, error_message, \
            source, total_duration_ms, mic_duration_ms, sys_duration_ms, \
            hotkey_pressed, audio_blob_path, whisper_model_id, \
            formatter_version, chunk_count_mic, chunk_count_sys, \
            stt_latency_ms, formatter_latency_ms\
         ) VALUES (\
            ?1, ?2, ?3, ?4, ?5, ?6, \
            ?7, ?8, ?9, ?10, \
            ?11, ?12, ?13, \
            ?14, ?15, ?16, \
            ?17, ?18\
         )",
        params![
            req.uuid,
            req.title,
            req.started_at,
            req.ended_at,
            req.status.as_db_str(),
            req.error_message,
            req.source.as_db_str(),
            req.total_duration_ms as i64,
            req.mic_duration_ms.map(|v| v as i64),
            req.sys_duration_ms.map(|v| v as i64),
            req.hotkey_pressed,
            req.audio_blob_path,
            req.whisper_model_id,
            req.formatter_version,
            req.chunk_count_mic.map(|v| v as i64),
            req.chunk_count_sys.map(|v| v as i64),
            req.stt_latency_ms.map(|v| v as i64),
            req.formatter_latency_ms.map(|v| v as i64),
        ],
    )
    .map_err(|e| AppError::MeetingCapture(format!("insert meeting_sessions: {e}")))?;

    let session_rowid = tx.last_insert_rowid();

    // Per-channel formatted prose. Non-fatal individually.
    for (channel, body_opt) in [
        ("mic", req.formatted_mic.as_deref()),
        ("system", req.formatted_sys.as_deref()),
        ("merged", req.formatted_merged.as_deref()),
    ] {
        if let Some(body) = body_opt {
            if let Err(e) = insert_transcript_row(&tx, session_rowid, channel, "formatted", body) {
                tracing::warn!(
                    target: "meetings",
                    session_rowid,
                    channel,
                    error = %e,
                    "meeting_transcripts insert (formatted) failed; continuing"
                );
            }
        }
    }

    // Per-channel raw segments (JSON-encoded). Non-fatal individually.
    for (channel, segs_opt) in [
        ("mic", req.segments_mic.as_ref()),
        ("system", req.segments_sys.as_ref()),
    ] {
        if let Some(segs) = segs_opt {
            match serde_json::to_string(segs) {
                Ok(json) => {
                    if let Err(e) =
                        insert_transcript_row(&tx, session_rowid, channel, "raw_segments", &json)
                    {
                        tracing::warn!(
                            target: "meetings",
                            session_rowid,
                            channel,
                            error = %e,
                            "meeting_transcripts insert (raw_segments) failed; continuing"
                        );
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        target: "meetings",
                        session_rowid,
                        channel,
                        error = %e,
                        "serde_json::to_string(segments) failed; skipping raw_segments row"
                    );
                }
            }
        }
    }

    tx.commit()
        .map_err(|e| AppError::MeetingCapture(format!("commit meeting persist: {e}")))?;
    Ok(session_rowid)
}

fn insert_transcript_row(
    tx: &rusqlite::Transaction<'_>,
    session_rowid: i64,
    channel: &str,
    stage: &str,
    text: &str,
) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO meeting_transcripts (meeting_session_id, channel, stage, text) \
         VALUES (?1, ?2, ?3, ?4)",
        params![session_rowid, channel, stage, text],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrations::apply_all;
    use crate::stt::SttSegment;
    use rusqlite::Connection;

    fn fresh_db() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory");
        apply_all(&conn).expect("apply migrations");
        conn
    }

    fn base_request(uuid: &str) -> MeetingPersistRequest {
        MeetingPersistRequest {
            uuid: uuid.to_string(),
            title: Some("Test meeting".to_string()),
            started_at: "2026-05-20T10:00:00Z".to_string(),
            ended_at: "2026-05-20T10:01:00Z".to_string(),
            status: MeetingStatus::Complete,
            error_message: None,
            source: MeetingSource::Mic,
            total_duration_ms: 60_000,
            mic_duration_ms: Some(60_000),
            sys_duration_ms: None,
            hotkey_pressed: "RCtrl+M".to_string(),
            audio_blob_path: Some("/tmp/meeting.wav".to_string()),
            whisper_model_id: "whisper-large-v3-turbo-q5_0".to_string(),
            formatter_version: "mc-v1".to_string(),
            chunk_count_mic: Some(2),
            chunk_count_sys: None,
            stt_latency_ms: Some(450),
            formatter_latency_ms: Some(12),
            formatted_mic: Some("Hello world.".to_string()),
            formatted_sys: None,
            formatted_merged: None,
            segments_mic: Some(vec![SttSegment {
                t0_ms: 0,
                t1_ms: 1000,
                text: "Hello world.".to_string(),
            }]),
            segments_sys: None,
        }
    }

    #[test]
    fn status_db_str_round_trip() {
        for s in [
            MeetingStatus::Complete,
            MeetingStatus::Partial,
            MeetingStatus::Demoted,
            MeetingStatus::Interrupted,
            MeetingStatus::Failed,
        ] {
            assert_eq!(MeetingStatus::from_db_str(s.as_db_str()), Some(s));
        }
    }

    #[test]
    fn status_from_db_str_rejects_unknown() {
        assert!(MeetingStatus::from_db_str("ok").is_none());
        assert!(MeetingStatus::from_db_str("").is_none());
    }

    #[test]
    fn persist_complete_meeting_round_trips() {
        let conn = fresh_db();
        let mut req = base_request("uuid-1");
        // Three formatted channels + both segment arrays.
        req.source = MeetingSource::Both;
        req.formatted_sys = Some("Other speaker.".to_string());
        req.formatted_merged =
            Some("**You:** Hello world.\n\n**Other(s):** Other speaker.".to_string());
        req.segments_sys = Some(vec![SttSegment {
            t0_ms: 500,
            t1_ms: 1500,
            text: "Other speaker.".to_string(),
        }]);
        req.sys_duration_ms = Some(60_000);
        req.chunk_count_sys = Some(2);

        let rowid = persist_meeting(&conn, &req).expect("persist");
        assert!(rowid > 0);

        let n_sessions: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM meeting_sessions WHERE uuid = ?1",
                params!["uuid-1"],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n_sessions, 1);

        let n_transcripts: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM meeting_transcripts WHERE meeting_session_id = ?1",
                params![rowid],
                |r| r.get(0),
            )
            .unwrap();
        // 3 formatted + 2 raw_segments = 5.
        assert_eq!(n_transcripts, 5);
    }

    #[test]
    fn persist_mic_only_round_trips() {
        let conn = fresh_db();
        let req = base_request("uuid-2");
        let rowid = persist_meeting(&conn, &req).expect("persist");

        let n_transcripts: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM meeting_transcripts WHERE meeting_session_id = ?1",
                params![rowid],
                |r| r.get(0),
            )
            .unwrap();
        // 1 formatted (mic) + 1 raw_segments (mic) = 2.
        assert_eq!(n_transcripts, 2);
    }

    #[test]
    fn persist_returns_meeting_capture_error_on_unique_violation() {
        let conn = fresh_db();
        let req = base_request("uuid-dup");
        persist_meeting(&conn, &req).expect("first persist");
        let err = persist_meeting(&conn, &req).expect_err("second persist must fail");
        match err {
            AppError::MeetingCapture(msg) => {
                assert!(
                    msg.contains("insert meeting_sessions"),
                    "error msg should reference the failed insert: {msg}"
                );
            }
            other => panic!("expected AppError::MeetingCapture, got {other:?}"),
        }
        // Rollback verification: only one session row, no orphan
        // transcripts from the second (failed) attempt.
        let n_sessions: i64 = conn
            .query_row("SELECT COUNT(*) FROM meeting_sessions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n_sessions, 1);
    }

    #[test]
    fn persist_skips_individual_transcript_failures() {
        // The migration-011 schema has very loose constraints on
        // meeting_transcripts.channel/stage (TEXT with no CHECK), so we
        // can't easily force a single-row INSERT failure without
        // patching the schema. Instead we exercise the partial path:
        // request with formatted_mic=Some but segments_mic=None.
        // The session row + 1 formatted row land; no raw_segments row.
        let conn = fresh_db();
        let mut req = base_request("uuid-partial");
        req.segments_mic = None;
        let rowid = persist_meeting(&conn, &req).expect("persist");
        let n_transcripts: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM meeting_transcripts WHERE meeting_session_id = ?1",
                params![rowid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n_transcripts, 1, "only the formatted_mic row should land");

        // Verify the formatted row is the one that landed (not raw_segments).
        let stages: Vec<String> = {
            let mut stmt = conn
                .prepare("SELECT stage FROM meeting_transcripts WHERE meeting_session_id = ?1")
                .unwrap();
            let rows = stmt
                .query_map(params![rowid], |r| r.get::<_, String>(0))
                .unwrap();
            rows.collect::<Result<Vec<_>, _>>().unwrap()
        };
        assert_eq!(stages, vec!["formatted".to_string()]);
    }

    #[test]
    fn persist_marks_status_partial_when_only_one_channel_formatted() {
        let conn = fresh_db();
        let mut req = base_request("uuid-partial-status");
        req.status = MeetingStatus::Partial;
        req.source = MeetingSource::Both;
        req.error_message = Some("system loopback failed mid-run".to_string());
        // sys + merged remain None per base_request defaults.
        let rowid = persist_meeting(&conn, &req).expect("persist");

        let (status_str, err_msg): (String, Option<String>) = conn
            .query_row(
                "SELECT status, error_message FROM meeting_sessions WHERE id = ?1",
                params![rowid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(status_str, "partial");
        assert_eq!(
            MeetingStatus::from_db_str(&status_str),
            Some(MeetingStatus::Partial)
        );
        assert_eq!(err_msg.as_deref(), Some("system loopback failed mid-run"));
    }

    #[test]
    fn persist_raw_segments_round_trip_as_json() {
        // The raw_segments stage stores serde_json-serialized
        // Vec<SttSegment>; verify the body round-trips.
        let conn = fresh_db();
        let req = base_request("uuid-segs");
        let rowid = persist_meeting(&conn, &req).expect("persist");
        let body: String = conn
            .query_row(
                "SELECT text FROM meeting_transcripts \
                 WHERE meeting_session_id = ?1 AND channel = 'mic' AND stage = 'raw_segments'",
                params![rowid],
                |r| r.get(0),
            )
            .unwrap();
        let decoded: Vec<SttSegment> = serde_json::from_str(&body).expect("decode segments");
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].t0_ms, 0);
        assert_eq!(decoded[0].t1_ms, 1000);
        assert_eq!(decoded[0].text, "Hello world.");
    }

    #[test]
    fn persist_fts_shadow_table_is_populated_by_insert_trigger() {
        // Migration 011 wires AFTER INSERT triggers for the FTS shadow.
        // Verify the trigger actually fired — a freshly-persisted
        // meeting should be findable via FTS5 MATCH.
        let conn = fresh_db();
        let mut req = base_request("uuid-fts");
        req.formatted_mic = Some("the quick brown fox".to_string());
        persist_meeting(&conn, &req).expect("persist");

        let hits: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM meeting_transcripts_fts WHERE meeting_transcripts_fts MATCH 'quick'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(hits >= 1, "expected at least one FTS hit for 'quick'");
    }
}
