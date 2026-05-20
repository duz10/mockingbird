//! Persist a completed meeting — atomic session-row + transcript rows.
//!
//! Mirrors the Phase 3 Wave 4 dictation `db::sessions::insert_session`
//! pattern: single transaction, full provenance, individual transcript
//! INSERT failures non-fatal (mirrors the Wave 4.9 Bug A fix — a bad
//! transcript shouldn't lose the whole session).
//!
//! Wave 1 scaffold — types + `todo!()` stub. Wave 4 ships the impl.

use rusqlite::Connection;

use crate::error::AppResult;

use super::capture::MeetingSource;
use super::long_form_stt::TimedSegment;

/// Persisted meeting status. Mirrors `meeting_sessions.status` column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
/// Wave 1: `todo!()` — Wave 4 ships the implementation. Individual
/// transcript-row failures are logged + skipped (mirrors Wave 4.9
/// Bug A fix); a session row that fails to INSERT propagates.
pub fn persist_meeting(_conn: &Connection, _req: &MeetingPersistRequest) -> AppResult<i64> {
    todo!("Wave 4: implement persist_meeting per Section MC.4 schema")
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
