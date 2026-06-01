//! Small pure-SELECT helpers for the `transcripts` table.
//!
//! Two flavours of transcript fetch are needed across the worker
//! submodules:
//!
//! - [`load_dictation_text`]: first-available-wins cascade
//!   (`final` → `cleaned` → `raw`); used by `filing::process_one`
//!   and `projection::maybe_commit_to_vault` for the user-visible
//!   body. ADR 0050 § D6 wires the dictation hook *after* the
//!   session is finalized, so `stage='final'` should always exist;
//!   the fallbacks defend against partial-cleanup states.
//! - [`load_transcript_stage`]: single-stage fetch; used by
//!   `archive::maybe_archive_history`, which wants both `raw` AND
//!   `cleaned` independently rather than a first-available cascade.
//!
//! Both are read-only — Principle 1 (raw is immutable) holds because
//! these are SELECTs not UPDATEs.
//!
//! Split out of `worker.rs` during Wave 1E.7 Part 2 (`mb-5lla`).

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::AppResult;

/// Read the dictation text for a session by trying transcript stages
/// in priority order (`final` → `cleaned` → `raw`). Returns `None`
/// only when no row exists at all for `session_id`.
pub(super) fn load_dictation_text(conn: &Connection, session_id: i64) -> AppResult<Option<String>> {
    for stage in ["final", "cleaned", "raw"] {
        let text: Option<String> = conn
            .query_row(
                "SELECT text FROM transcripts WHERE session_id = ?1 AND stage = ?2",
                params![session_id, stage],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(t) = text {
            return Ok(Some(t));
        }
    }
    Ok(None)
}

/// Single-stage transcript fetch. Like [`load_dictation_text`] but
/// returns just one stage's text — the history archive wants both
/// raw + cleaned independently, not the first-available cascade.
pub(super) fn load_transcript_stage(
    conn: &Connection,
    session_id: i64,
    stage: &str,
) -> AppResult<Option<String>> {
    conn.query_row(
        "SELECT text FROM transcripts WHERE session_id = ?1 AND stage = ?2",
        params![session_id, stage],
        |r| r.get::<_, String>(0),
    )
    .optional()
    .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE transcripts (
               id INTEGER PRIMARY KEY,
               session_id INTEGER NOT NULL,
               stage TEXT NOT NULL,
               text TEXT NOT NULL,
               UNIQUE(session_id, stage)
             );
             INSERT INTO transcripts (session_id, stage, text) VALUES
               (1, 'raw', 'raw-text'),
               (1, 'cleaned', 'cleaned-text'),
               (1, 'final', 'final-text'),
               (2, 'cleaned', 'cleaned-text-2'),
               (3, 'raw', 'raw-text-3');",
        )
        .unwrap();
        conn
    }

    #[test]
    fn load_dictation_text_prefers_final_over_cleaned() {
        let conn = fixture_conn();
        assert_eq!(
            load_dictation_text(&conn, 1).unwrap().as_deref(),
            Some("final-text")
        );
        assert_eq!(
            load_dictation_text(&conn, 2).unwrap().as_deref(),
            Some("cleaned-text-2")
        );
        assert_eq!(
            load_dictation_text(&conn, 3).unwrap().as_deref(),
            Some("raw-text-3")
        );
        assert!(load_dictation_text(&conn, 999).unwrap().is_none());
    }

    #[test]
    fn load_transcript_stage_returns_exact_match_or_none() {
        let conn = fixture_conn();
        assert_eq!(
            load_transcript_stage(&conn, 1, "raw").unwrap().as_deref(),
            Some("raw-text")
        );
        assert_eq!(
            load_transcript_stage(&conn, 2, "final").unwrap(),
            None,
            "stage that doesn't exist must return None"
        );
        assert_eq!(
            load_transcript_stage(&conn, 999, "raw").unwrap(),
            None,
            "session that doesn't exist must return None"
        );
    }
}
