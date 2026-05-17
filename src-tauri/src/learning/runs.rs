//! Repository for the `learning_runs` table.
//!
//! Records one row per `learn` binary invocation, regardless of
//! whether the run committed or rolled back. Powers the Settings →
//! Advanced → "View learning history" UI and the
//! `mb-learning-eval` judge.

use rusqlite::{params, Connection};

use crate::error::AppResult;

/// One row in `learning_runs`.
#[derive(Debug, Clone, PartialEq)]
pub struct LearningRun {
    /// PK.
    pub id: i64,
    /// ISO-8601 start timestamp.
    pub started_at: String,
    /// ISO-8601 end timestamp; populated by [`complete`].
    pub completed_at: Option<String>,
    /// Count of sessions inspected by the eval pass.
    pub sessions_analyzed: Option<i64>,
    /// Count of corrections successfully classified this run.
    pub corrections_classified: Option<i64>,
    /// Style examples inserted (after `style_change` classifications).
    pub examples_added: Option<i64>,
    /// Style examples disabled by per-mode pruning (lowest rank tail).
    pub examples_removed: Option<i64>,
    /// Dictionary terms inserted (after `new_vocab` classifications).
    pub dictionary_terms_added: Option<i64>,
    /// Pre-run correction-rate metric.
    pub eval_correction_rate_before: Option<f64>,
    /// Post-run correction-rate metric (same eval after changes).
    pub eval_correction_rate_after: Option<f64>,
    /// `true` iff this run reverted its changes via `audit::rollback_*`.
    pub rolled_back: bool,
    /// Free-form notes (errors caught, eval skipped reason, etc.).
    pub notes: Option<String>,
}

/// Insert a "run started" row; returns the new PK.
pub fn start(conn: &Connection) -> AppResult<i64> {
    conn.execute(
        "INSERT INTO learning_runs (started_at) VALUES (datetime('now'))",
        [],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Payload for [`complete`] — every metric is optional so the runner
/// can record partial state when an early-exit path fires.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Completion {
    /// Sessions the eval pass replayed.
    pub sessions_analyzed: Option<i64>,
    /// Corrections successfully classified.
    pub corrections_classified: Option<i64>,
    /// Style examples added.
    pub examples_added: Option<i64>,
    /// Style examples disabled (pruning).
    pub examples_removed: Option<i64>,
    /// Dictionary terms added.
    pub dictionary_terms_added: Option<i64>,
    /// Correction-rate before this run's changes.
    pub eval_correction_rate_before: Option<f64>,
    /// Correction-rate after this run's changes.
    pub eval_correction_rate_after: Option<f64>,
    /// Whether the runner rolled back.
    pub rolled_back: bool,
    /// Free-form note.
    pub notes: Option<String>,
}

/// Finalise the run row.
pub fn complete(conn: &Connection, id: i64, c: &Completion) -> AppResult<()> {
    conn.execute(
        "UPDATE learning_runs SET \
            completed_at = datetime('now'), \
            sessions_analyzed = ?1, \
            corrections_classified = ?2, \
            examples_added = ?3, \
            examples_removed = ?4, \
            dictionary_terms_added = ?5, \
            eval_correction_rate_before = ?6, \
            eval_correction_rate_after = ?7, \
            rolled_back = ?8, \
            notes = ?9 \
         WHERE id = ?10",
        params![
            c.sessions_analyzed,
            c.corrections_classified,
            c.examples_added,
            c.examples_removed,
            c.dictionary_terms_added,
            c.eval_correction_rate_before,
            c.eval_correction_rate_after,
            i64::from(c.rolled_back),
            c.notes,
            id,
        ],
    )?;
    Ok(())
}

/// Lookup by PK.
pub fn get_by_id(conn: &Connection, id: i64) -> AppResult<Option<LearningRun>> {
    let mut stmt = conn.prepare(
        "SELECT id, started_at, completed_at, sessions_analyzed, \
                corrections_classified, examples_added, examples_removed, \
                dictionary_terms_added, eval_correction_rate_before, \
                eval_correction_rate_after, rolled_back, notes \
         FROM learning_runs WHERE id = ?1",
    )?;
    let mut rows = stmt.query_map([id], row_to_run)?;
    match rows.next() {
        Some(Ok(r)) => Ok(Some(r)),
        Some(Err(e)) => Err(e.into()),
        None => Ok(None),
    }
}

/// Latest runs first.
pub fn list_recent(conn: &Connection, limit: usize) -> AppResult<Vec<LearningRun>> {
    let mut stmt = conn.prepare(
        "SELECT id, started_at, completed_at, sessions_analyzed, \
                corrections_classified, examples_added, examples_removed, \
                dictionary_terms_added, eval_correction_rate_before, \
                eval_correction_rate_after, rolled_back, notes \
         FROM learning_runs ORDER BY id DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map([limit as i64], row_to_run)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

fn row_to_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<LearningRun> {
    Ok(LearningRun {
        id: row.get(0)?,
        started_at: row.get(1)?,
        completed_at: row.get(2)?,
        sessions_analyzed: row.get(3)?,
        corrections_classified: row.get(4)?,
        examples_added: row.get(5)?,
        examples_removed: row.get(6)?,
        dictionary_terms_added: row.get(7)?,
        eval_correction_rate_before: row.get(8)?,
        eval_correction_rate_after: row.get(9)?,
        rolled_back: row.get::<_, i64>(10)? != 0,
        notes: row.get(11)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    #[test]
    fn start_then_complete_round_trips() {
        let db = Database::open_in_memory().unwrap();
        let id = start(&db.conn).unwrap();
        complete(
            &db.conn,
            id,
            &Completion {
                sessions_analyzed: Some(50),
                corrections_classified: Some(8),
                examples_added: Some(3),
                examples_removed: Some(1),
                dictionary_terms_added: Some(2),
                eval_correction_rate_before: Some(0.10),
                eval_correction_rate_after: Some(0.08),
                rolled_back: false,
                notes: Some("happy run".into()),
            },
        )
        .unwrap();
        let row = get_by_id(&db.conn, id).unwrap().unwrap();
        assert_eq!(row.sessions_analyzed, Some(50));
        assert_eq!(row.examples_added, Some(3));
        assert!(!row.rolled_back);
        assert_eq!(row.notes.as_deref(), Some("happy run"));
        assert!(row.completed_at.is_some());
    }

    #[test]
    fn rolled_back_run_persists_flag() {
        let db = Database::open_in_memory().unwrap();
        let id = start(&db.conn).unwrap();
        complete(
            &db.conn,
            id,
            &Completion {
                rolled_back: true,
                eval_correction_rate_before: Some(0.05),
                eval_correction_rate_after: Some(0.07),
                notes: Some("regression detected".into()),
                ..Completion::default()
            },
        )
        .unwrap();
        let row = get_by_id(&db.conn, id).unwrap().unwrap();
        assert!(row.rolled_back);
    }

    #[test]
    fn list_recent_orders_newest_first() {
        let db = Database::open_in_memory().unwrap();
        let a = start(&db.conn).unwrap();
        let b = start(&db.conn).unwrap();
        let runs = list_recent(&db.conn, 10).unwrap();
        assert_eq!(runs[0].id, b);
        assert_eq!(runs[1].id, a);
    }

    #[test]
    fn list_recent_respects_limit() {
        let db = Database::open_in_memory().unwrap();
        for _ in 0..5 {
            start(&db.conn).unwrap();
        }
        let runs = list_recent(&db.conn, 3).unwrap();
        assert_eq!(runs.len(), 3);
    }
}
