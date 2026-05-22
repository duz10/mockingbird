//! DB persistence for `activity_transcript_segments` (Phase 10 Wave 4).
//!
//! Layer-2 (audio) transcription writes one row per Whisper segment.
//! Reads land at two callers:
//!
//! 1. **The abstractor's audio-aware re-run path** in
//!    [`crate::activity::export`]. It loads all segments for a session
//!    and hands them to [`crate::activity::block_audio_stitcher::stitch`]
//!    along with the freshly-generated Blocks.
//! 2. **The IPC layer** — `activity_list_transcript_segments` lets
//!    the Activity detail page surface the raw transcript alongside
//!    the visual timeline.
//!
//! Schema (from migration 012, untouched by 014):
//!
//! ```text
//! id           TEXT PRIMARY KEY
//! session_id   TEXT NOT NULL REFERENCES activity_sessions(id) ON DELETE CASCADE
//! started_at   INTEGER NOT NULL                 -- ms, global meeting timeline
//! ended_at     INTEGER NOT NULL                 -- ms
//! text         TEXT NOT NULL
//! source       TEXT NOT NULL                    -- 'mic' | 'system'
//! created_at   INTEGER NOT NULL
//! ```
//!
//! There is **no UPDATE path** — segments are append-only, like raw
//! events. Re-running the audio pipeline on the same session would
//! produce duplicates; the orchestrator (`activity::audio`) only
//! writes segments once per session (on stop), so the "dup on re-run"
//! risk is theoretical until Wave 5+ adds a re-transcribe path.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::activity::block_audio_stitcher::{TranscriptChannel, TranscriptSegment};
use crate::activity::ids::new_event_id;
use crate::error::AppResult;

/// IPC-facing projection. Mirrors [`TranscriptSegment`] but with the
/// channel as the persisted string so JS doesn't have to learn a new
/// enum encoding.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ActivityTranscriptSegmentRow {
    /// `activity_transcript_segments.id`. ULID-ish, generated at insert.
    pub id: String,
    /// Owning session id (FK to `activity_sessions.id`).
    pub session_id: String,
    /// Global epoch-ms start of the segment.
    pub started_at: i64,
    /// Global epoch-ms end of the segment.
    pub ended_at: i64,
    /// Whisper-recognized text.
    pub text: String,
    /// `"mic"` | `"system"`.
    pub source: String,
    /// Epoch-ms when this row was INSERTed.
    pub created_at: i64,
}

impl ActivityTranscriptSegmentRow {
    /// Promote to the typed stitcher input. Returns `None` if `source`
    /// is an unrecognised value (the stitcher would drop it anyway).
    pub fn to_stitcher_input(&self) -> Option<TranscriptSegment> {
        let channel = TranscriptChannel::from_db_str(&self.source)?;
        Some(TranscriptSegment {
            id: self.id.clone(),
            started_at: self.started_at,
            ended_at: self.ended_at,
            text: self.text.clone(),
            channel,
        })
    }
}

/// Insert one transcript segment. Returns the generated id.
pub fn insert_segment(
    conn: &Connection,
    session_id: &str,
    started_at_ms: i64,
    ended_at_ms: i64,
    text: &str,
    channel: TranscriptChannel,
    now_ms: i64,
) -> AppResult<String> {
    let id = new_event_id();
    conn.execute(
        "INSERT INTO activity_transcript_segments \
         (id, session_id, started_at, ended_at, text, source, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            id,
            session_id,
            started_at_ms,
            ended_at_ms,
            text,
            channel.as_db_str(),
            now_ms,
        ],
    )?;
    Ok(id)
}

/// Bulk-insert helper used by `activity::audio` at session-stop. Wraps
/// the writes in a single transaction so a long-tail batch (10k+
/// segments for a 4-hour session) commits as one fsync.
pub fn insert_segments_bulk(
    conn: &mut Connection,
    session_id: &str,
    segments: &[(i64, i64, String, TranscriptChannel)],
    now_ms: i64,
) -> AppResult<Vec<String>> {
    let tx = conn.transaction()?;
    let mut ids = Vec::with_capacity(segments.len());
    for (started_at, ended_at, text, channel) in segments {
        let id = new_event_id();
        tx.execute(
            "INSERT INTO activity_transcript_segments \
             (id, session_id, started_at, ended_at, text, source, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                id,
                session_id,
                started_at,
                ended_at,
                text,
                channel.as_db_str(),
                now_ms,
            ],
        )?;
        ids.push(id);
    }
    tx.commit()?;
    Ok(ids)
}

/// List all transcript segments for a session in chronological order.
/// `ORDER BY started_at ASC, id ASC` matches the events query shape.
pub fn list_segments(
    conn: &Connection,
    session_id: &str,
) -> AppResult<Vec<ActivityTranscriptSegmentRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, session_id, started_at, ended_at, text, source, created_at \
         FROM activity_transcript_segments \
         WHERE session_id = ?1 \
         ORDER BY started_at ASC, id ASC",
    )?;
    let rows = stmt
        .query_map(params![session_id], row_to_segment)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Whether a session has any transcript segments. Used by the
/// abstractor to short-circuit the audio-aware path when audio was
/// disabled (or produced zero output).
pub fn session_has_segments(conn: &Connection, session_id: &str) -> AppResult<bool> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM activity_transcript_segments WHERE session_id = ?1",
        params![session_id],
        |row| row.get(0),
    )?;
    Ok(n > 0)
}

fn row_to_segment(row: &rusqlite::Row<'_>) -> rusqlite::Result<ActivityTranscriptSegmentRow> {
    Ok(ActivityTranscriptSegmentRow {
        id: row.get(0)?,
        session_id: row.get(1)?,
        started_at: row.get(2)?,
        ended_at: row.get(3)?,
        text: row.get(4)?,
        source: row.get(5)?,
        created_at: row.get(6)?,
    })
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
    fn insert_and_list_round_trip() {
        let c = fresh_db();
        let sid = insert_session(&c, 1_000).unwrap();
        let id = insert_segment(
            &c,
            &sid,
            2_000,
            3_500,
            "hello world",
            TranscriptChannel::Mic,
            2_000,
        )
        .unwrap();
        let rows = list_segments(&c, &sid).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, id);
        assert_eq!(rows[0].started_at, 2_000);
        assert_eq!(rows[0].ended_at, 3_500);
        assert_eq!(rows[0].text, "hello world");
        assert_eq!(rows[0].source, "mic");
    }

    #[test]
    fn list_is_chronological() {
        let c = fresh_db();
        let sid = insert_session(&c, 1_000).unwrap();
        insert_segment(
            &c,
            &sid,
            5_000,
            6_000,
            "third",
            TranscriptChannel::Mic,
            5_000,
        )
        .unwrap();
        insert_segment(
            &c,
            &sid,
            1_000,
            2_000,
            "first",
            TranscriptChannel::Mic,
            1_000,
        )
        .unwrap();
        insert_segment(
            &c,
            &sid,
            3_000,
            4_000,
            "second",
            TranscriptChannel::System,
            3_000,
        )
        .unwrap();
        let rows = list_segments(&c, &sid).unwrap();
        let texts: Vec<&str> = rows.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(texts, vec!["first", "second", "third"]);
    }

    #[test]
    fn system_channel_is_accepted() {
        let c = fresh_db();
        let sid = insert_session(&c, 1_000).unwrap();
        insert_segment(
            &c,
            &sid,
            1_000,
            2_000,
            "they spoke",
            TranscriptChannel::System,
            1_000,
        )
        .unwrap();
        let rows = list_segments(&c, &sid).unwrap();
        assert_eq!(rows[0].source, "system");
        let promoted = rows[0].to_stitcher_input().unwrap();
        assert_eq!(promoted.channel, TranscriptChannel::System);
    }

    #[test]
    fn unknown_source_promotes_to_none() {
        let row = ActivityTranscriptSegmentRow {
            id: "x".into(),
            session_id: "s".into(),
            started_at: 0,
            ended_at: 1,
            text: "?".into(),
            source: "bluetooth".into(),
            created_at: 0,
        };
        assert!(row.to_stitcher_input().is_none());
    }

    #[test]
    fn bulk_insert_writes_in_one_transaction() {
        let mut c = fresh_db();
        let sid = insert_session(&c, 1_000).unwrap();
        let batch = vec![
            (1_000, 2_000, "alpha".to_string(), TranscriptChannel::Mic),
            (2_500, 3_500, "beta".to_string(), TranscriptChannel::System),
            (4_000, 5_000, "gamma".to_string(), TranscriptChannel::Mic),
        ];
        let ids = insert_segments_bulk(&mut c, &sid, &batch, 1_000).unwrap();
        assert_eq!(ids.len(), 3);
        let rows = list_segments(&c, &sid).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].text, "alpha");
        assert_eq!(rows[1].source, "system");
    }

    #[test]
    fn session_has_segments_reports_truthfully() {
        let c = fresh_db();
        let sid = insert_session(&c, 1_000).unwrap();
        assert!(!session_has_segments(&c, &sid).unwrap());
        insert_segment(&c, &sid, 1_000, 2_000, "x", TranscriptChannel::Mic, 1_000).unwrap();
        assert!(session_has_segments(&c, &sid).unwrap());
    }

    #[test]
    fn cascade_delete_on_session_clears_segments() {
        let c = fresh_db();
        let sid = insert_session(&c, 1_000).unwrap();
        insert_segment(&c, &sid, 1_000, 2_000, "x", TranscriptChannel::Mic, 1_000).unwrap();
        c.execute("DELETE FROM activity_sessions WHERE id = ?1", params![&sid])
            .unwrap();
        let rows = list_segments(&c, &sid).unwrap();
        assert!(rows.is_empty());
    }
}
