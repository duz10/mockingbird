//! Repository for the `corrections` table.
//!
//! Schema (migration 001):
//!
//! ```sql
//! CREATE TABLE corrections (
//!   id               INTEGER PRIMARY KEY,
//!   session_id       INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
//!   before_text      TEXT NOT NULL,
//!   after_text       TEXT NOT NULL,
//!   detection_method TEXT NOT NULL,
//!   classification   TEXT,
//!   classified_at    TEXT,
//!   created_at       TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
//! );
//! ```
//!
//! `before_text` is what Mockingbird typed; `after_text` is what the
//! user changed it to. `detection_method` records HOW we noticed the
//! correction — for v1 the only producer is `"manual"` (a Tauri
//! command from the History UI's "this was wrong" right-click). A
//! future `"clipboard_undo"` detection method will land when the
//! clipboard-monitor watcher is implemented (Phase 8 Wave 2).

use rusqlite::{params, Connection};

use crate::error::AppResult;

/// A correction event.
#[derive(Debug, Clone)]
pub struct Correction {
    /// PK.
    pub id: i64,
    /// FK to the session this correction applies to.
    pub session_id: i64,
    /// Text Mockingbird injected.
    pub before_text: String,
    /// Text the user replaced it with.
    pub after_text: String,
    /// How we noticed: `"manual"`, `"clipboard_undo"` (future), etc.
    pub detection_method: String,
    /// Classifier output (`new_vocab` | `style_change` | `mistranscription` | `noise`).
    /// `None` until the nightly runner classifies it.
    pub classification: Option<String>,
    /// ISO-8601 timestamp set when `mark_classified` succeeds.
    pub classified_at: Option<String>,
    /// ISO-8601 insert timestamp.
    pub created_at: String,
}

/// Payload for [`insert`].
#[derive(Debug, Clone)]
pub struct NewCorrection {
    /// Session this correction applies to (must exist; FK enforced).
    pub session_id: i64,
    /// What Mockingbird typed.
    pub before_text: String,
    /// What the user wrote instead.
    pub after_text: String,
    /// `"manual"` for v1.
    pub detection_method: String,
}

/// Insert a correction row. Returns the new row's PK.
pub fn insert(conn: &Connection, new: &NewCorrection) -> AppResult<i64> {
    conn.execute(
        "INSERT INTO corrections (session_id, before_text, after_text, detection_method) \
         VALUES (?1, ?2, ?3, ?4)",
        params![
            new.session_id,
            new.before_text,
            new.after_text,
            new.detection_method
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Count rows currently lacking a classification, optionally limited
/// to the last `since_days` days. `since_days = 0` means "all".
pub fn count_unclassified(conn: &Connection, since_days: u32) -> AppResult<i64> {
    let count: i64 = if since_days == 0 {
        conn.query_row(
            "SELECT COUNT(*) FROM corrections WHERE classification IS NULL",
            [],
            |r| r.get(0),
        )?
    } else {
        conn.query_row(
            "SELECT COUNT(*) FROM corrections \
             WHERE classification IS NULL AND created_at >= datetime('now', ?1)",
            [format!("-{since_days} days")],
            |r| r.get(0),
        )?
    };
    Ok(count)
}

/// Pull every unclassified correction newer than `since_days`. Used
/// by [`super::runner`].
pub fn list_unclassified_within(conn: &Connection, since_days: u32) -> AppResult<Vec<Correction>> {
    let sql = if since_days == 0 {
        "SELECT id, session_id, before_text, after_text, detection_method, \
                classification, classified_at, created_at \
         FROM corrections WHERE classification IS NULL ORDER BY id ASC"
            .to_string()
    } else {
        format!(
            "SELECT id, session_id, before_text, after_text, detection_method, \
                    classification, classified_at, created_at \
             FROM corrections \
             WHERE classification IS NULL AND created_at >= datetime('now', '-{since_days} days') \
             ORDER BY id ASC"
        )
    };
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], row_to_correction)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Record the classifier's verdict on a correction.
pub fn mark_classified(conn: &Connection, id: i64, classification: &str) -> AppResult<()> {
    conn.execute(
        "UPDATE corrections SET classification = ?1, classified_at = datetime('now') \
         WHERE id = ?2",
        params![classification, id],
    )?;
    Ok(())
}

/// Lookup by PK (used in tests + the History detail pane future query).
pub fn get_by_id(conn: &Connection, id: i64) -> AppResult<Option<Correction>> {
    let mut stmt = conn.prepare(
        "SELECT id, session_id, before_text, after_text, detection_method, \
                classification, classified_at, created_at \
         FROM corrections WHERE id = ?1",
    )?;
    let mut rows = stmt.query_map([id], row_to_correction)?;
    match rows.next() {
        Some(Ok(c)) => Ok(Some(c)),
        Some(Err(e)) => Err(e.into()),
        None => Ok(None),
    }
}

fn row_to_correction(row: &rusqlite::Row<'_>) -> rusqlite::Result<Correction> {
    Ok(Correction {
        id: row.get(0)?,
        session_id: row.get(1)?,
        before_text: row.get(2)?,
        after_text: row.get(3)?,
        detection_method: row.get(4)?,
        classification: row.get(5)?,
        classified_at: row.get(6)?,
        created_at: row.get(7)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::sessions::{
        self, CaptureKind, NewSession, SessionSource, SessionStatus, StartMode,
    };
    use crate::db::Database;

    fn seed_session(conn: &Connection) -> i64 {
        // Build a minimal session row that satisfies the FK +
        // not-null constraints in the schema. Reuses bootstrap rows
        // for the dict / example FKs.
        crate::dictation::runtime::bootstrap_provenance_rows(conn).unwrap();
        let prompt_id: i64 = conn
            .query_row("SELECT prompt_id FROM modes WHERE slug='normal'", [], |r| {
                r.get(0)
            })
            .unwrap();
        let dict_id: i64 = conn
            .query_row(
                "SELECT id FROM dictionary_snapshots ORDER BY id ASC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let example_id: i64 = conn
            .query_row(
                "SELECT id FROM example_sets ORDER BY id ASC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        sessions::insert(
            conn,
            &NewSession {
                uuid: uuid::Uuid::new_v4().to_string(),
                mode_id: 1,
                hotkey_pressed: "RightAlt".into(),
                started_at: "2026-05-18T00:00:00Z".into(),
                recording_ended_at: "2026-05-18T00:00:01Z".into(),
                status: SessionStatus::Recording,
                foreground_app: Some("notepad.exe".into()),
                foreground_window_title: Some("Untitled - Notepad".into()),
                audio_duration_ms: 1000,
                audio_blob_path: None,
                prompt_id,
                dictionary_snapshot_id: dict_id,
                example_set_id: example_id,
                start_mode: StartMode::Ptt,
                source: SessionSource::Desktop,
                capture_kind: CaptureKind::Dictation,
            },
        )
        .unwrap()
    }

    fn fresh() -> Connection {
        Database::open_in_memory().unwrap().conn
    }

    #[test]
    fn insert_round_trips() {
        let conn = fresh();
        let sid = seed_session(&conn);
        let id = insert(
            &conn,
            &NewCorrection {
                session_id: sid,
                before_text: "kubectl".into(),
                after_text: "kubeCTL".into(),
                detection_method: "manual".into(),
            },
        )
        .unwrap();
        let row = get_by_id(&conn, id).unwrap().unwrap();
        assert_eq!(row.before_text, "kubectl");
        assert_eq!(row.after_text, "kubeCTL");
        assert_eq!(row.detection_method, "manual");
        assert!(row.classification.is_none());
    }

    #[test]
    fn count_unclassified_respects_since_days() {
        let conn = fresh();
        let sid = seed_session(&conn);
        // Insert one current row + one row backdated 30 days.
        insert(
            &conn,
            &NewCorrection {
                session_id: sid,
                before_text: "a".into(),
                after_text: "A".into(),
                detection_method: "manual".into(),
            },
        )
        .unwrap();
        conn.execute(
            "INSERT INTO corrections (session_id, before_text, after_text, detection_method, created_at) \
             VALUES (?1, 'b', 'B', 'manual', datetime('now', '-30 days'))",
            [sid],
        )
        .unwrap();
        assert_eq!(count_unclassified(&conn, 0).unwrap(), 2);
        assert_eq!(count_unclassified(&conn, 7).unwrap(), 1);
        assert_eq!(count_unclassified(&conn, 60).unwrap(), 2);
    }

    #[test]
    fn list_unclassified_within_skips_classified() {
        let conn = fresh();
        let sid = seed_session(&conn);
        let a = insert(&conn, &nc(sid, "a", "A")).unwrap();
        let _b = insert(&conn, &nc(sid, "b", "B")).unwrap();
        mark_classified(&conn, a, "new_vocab").unwrap();
        let pending = list_unclassified_within(&conn, 7).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].before_text, "b");
    }

    #[test]
    fn mark_classified_sets_field_and_timestamp() {
        let conn = fresh();
        let sid = seed_session(&conn);
        let id = insert(&conn, &nc(sid, "before", "after")).unwrap();
        mark_classified(&conn, id, "style_change").unwrap();
        let row = get_by_id(&conn, id).unwrap().unwrap();
        assert_eq!(row.classification.as_deref(), Some("style_change"));
        assert!(row.classified_at.is_some());
    }

    fn nc(sid: i64, b: &str, a: &str) -> NewCorrection {
        NewCorrection {
            session_id: sid,
            before_text: b.into(),
            after_text: a.into(),
            detection_method: "manual".into(),
        }
    }
}
