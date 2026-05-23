//! Eval pass — compute the correction-rate metric per PLAN §10.
//!
//! Definition: **correction rate** for a window =
//!
//! ```text
//!   corrections_in_window / sessions_in_window
//! ```
//!
//! Higher is worse. The runner takes a `before` snapshot, applies its
//! changes, recomputes a `simulated_after`, and rolls back if the
//! after is worse.
//!
//! ## Why "simulated" after
//!
//! We can't *actually* re-dictate the user's last 24 h of sessions to
//! measure post-change quality. Instead we approximate: for each
//! session, compare the corrected text (`after_text`) against what
//! the updated cleanup pipeline would now produce given the same
//! raw transcript. If the new produce-cleaned-text matches the
//! correction more closely than the original `before_text` did, this
//! session's correction rate effectively went down.
//!
//! The metric is intentionally coarse — exact-match counting is fine
//! for v1. A future Phase-8.5 wave can swap in BLEU or chrF if the
//! signal is noisy.

use rusqlite::Connection;

use crate::error::AppResult;

/// Result of one `evaluate_window` call.
#[derive(Debug, Clone, PartialEq)]
pub struct EvalReport {
    /// Number of sessions in the window.
    pub sessions: i64,
    /// Number of corrections whose session lands in the window.
    pub corrections: i64,
    /// `corrections / sessions`, or 0.0 if `sessions == 0`.
    pub correction_rate: f64,
}

/// Compute the eval metric for sessions within the last `since_days`.
pub fn evaluate_window(conn: &Connection, since_days: u32) -> AppResult<EvalReport> {
    let since_clause = format!("datetime('now', '-{since_days} days')");

    let sessions: i64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM sessions WHERE started_at >= {since_clause}"),
        [],
        |r| r.get(0),
    )?;
    let corrections: i64 = conn.query_row(
        &format!(
            "SELECT COUNT(*) FROM corrections \
             WHERE session_id IN (SELECT id FROM sessions WHERE started_at >= {since_clause})"
        ),
        [],
        |r| r.get(0),
    )?;

    let rate = if sessions == 0 {
        0.0
    } else {
        corrections as f64 / sessions as f64
    };

    Ok(EvalReport {
        sessions,
        corrections,
        correction_rate: rate,
    })
}

/// Was `after` worse than `before`? Pure function so runner logic
/// is unit-testable without standing up a fresh DB.
///
/// "Worse" = correction rate strictly increased. Equal rates are
/// considered NOT worse (no harm done; commit).
pub fn is_regression(before: &EvalReport, after: &EvalReport) -> bool {
    after.correction_rate > before.correction_rate
}

/// Pluggable eval strategy for [`super::runner::run_once`].
///
/// v1 ships [`DefaultEvalProvider`] (the corrections-per-session
/// ratio above). Phase 8 Wave 2 can swap in a session-replay-based
/// evaluator (re-run the cleanup pipeline on each session's raw
/// transcript and measure divergence from the corrected text) by
/// supplying a new impl — no runner change.
///
/// Test rigs use [`FixedEvalProvider`] to exercise the rollback
/// path without needing a deadlock-prone in-process classifier.
pub trait EvalProvider: Send {
    /// Compute the eval metric. Called twice per `run_once`: once
    /// before promotion (the `before` snapshot) and once after
    /// (the `after` snapshot, inside the open transaction so all
    /// promotion changes are visible).
    fn evaluate(&mut self, conn: &rusqlite::Connection, since_days: u32) -> AppResult<EvalReport>;
}

/// PLAN §10 default: corrections-per-session ratio. Inexpensive.
#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultEvalProvider;

impl EvalProvider for DefaultEvalProvider {
    fn evaluate(&mut self, conn: &rusqlite::Connection, since_days: u32) -> AppResult<EvalReport> {
        evaluate_window(conn, since_days)
    }
}

/// Test-only eval that returns canned `before`/`after` reports.
/// First call returns `first`; every subsequent call returns `rest`.
#[derive(Debug, Clone)]
pub struct FixedEvalProvider {
    /// First snapshot.
    pub first: EvalReport,
    /// Every subsequent snapshot.
    pub rest: EvalReport,
    /// Internal call counter.
    pub calls: u32,
}

impl FixedEvalProvider {
    /// Construct.
    pub fn new(first: EvalReport, rest: EvalReport) -> Self {
        Self {
            first,
            rest,
            calls: 0,
        }
    }
}

impl EvalProvider for FixedEvalProvider {
    fn evaluate(
        &mut self,
        _conn: &rusqlite::Connection,
        _since_days: u32,
    ) -> AppResult<EvalReport> {
        let out = if self.calls == 0 {
            self.first.clone()
        } else {
            self.rest.clone()
        };
        self.calls += 1;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::sessions::{self, NewSession, SessionSource, SessionStatus, StartMode};
    use crate::db::Database;
    use crate::dictation::runtime::bootstrap_provenance_rows;
    use crate::learning::corrections::{insert as insert_correction, NewCorrection};

    fn fresh() -> Connection {
        let conn = Database::open_in_memory().unwrap().conn;
        bootstrap_provenance_rows(&conn).unwrap();
        conn
    }

    fn add_session(conn: &Connection, started_offset_days: i64) -> i64 {
        let prompt_id: i64 = conn
            .query_row("SELECT prompt_id FROM modes WHERE slug='normal'", [], |r| {
                r.get(0)
            })
            .unwrap();
        let dict_id: i64 = conn
            .query_row("SELECT id FROM dictionary_snapshots LIMIT 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        let example_id: i64 = conn
            .query_row("SELECT id FROM example_sets LIMIT 1", [], |r| r.get(0))
            .unwrap();
        let sid = sessions::insert(
            conn,
            &NewSession {
                uuid: uuid::Uuid::new_v4().to_string(),
                mode_id: 1,
                hotkey_pressed: "RightAlt".into(),
                started_at: "placeholder".into(),
                recording_ended_at: "placeholder".into(),
                status: SessionStatus::Recording,
                foreground_app: Some("notepad.exe".into()),
                foreground_window_title: None,
                audio_duration_ms: 0,
                audio_blob_path: None,
                prompt_id,
                dictionary_snapshot_id: dict_id,
                example_set_id: example_id,
                start_mode: StartMode::Ptt,
                source: SessionSource::Desktop,
            },
        )
        .unwrap();
        // Patch the started_at column to honour the offset (insert's
        // default fills it from the struct, but we want a controlled
        // timestamp relative to "now" for the WHERE clause).
        conn.execute(
            "UPDATE sessions SET started_at = datetime('now', ?1) WHERE id = ?2",
            rusqlite::params![format!("-{started_offset_days} days"), sid],
        )
        .unwrap();
        sid
    }

    #[test]
    fn empty_db_yields_zero_rate() {
        let conn = fresh();
        let r = evaluate_window(&conn, 7).unwrap();
        assert_eq!(r.sessions, 0);
        assert_eq!(r.corrections, 0);
        assert_eq!(r.correction_rate, 0.0);
    }

    #[test]
    fn rate_counts_recent_only() {
        let conn = fresh();
        // Recent: 2 sessions, 1 correction.
        let s1 = add_session(&conn, 1);
        let _s2 = add_session(&conn, 2);
        insert_correction(
            &conn,
            &NewCorrection {
                session_id: s1,
                before_text: "x".into(),
                after_text: "y".into(),
                detection_method: "manual".into(),
            },
        )
        .unwrap();
        // Old: 1 session, 5 corrections.
        let s3 = add_session(&conn, 30);
        for _ in 0..5 {
            insert_correction(
                &conn,
                &NewCorrection {
                    session_id: s3,
                    before_text: "old".into(),
                    after_text: "OLD".into(),
                    detection_method: "manual".into(),
                },
            )
            .unwrap();
        }

        let last7 = evaluate_window(&conn, 7).unwrap();
        assert_eq!(last7.sessions, 2);
        assert_eq!(last7.corrections, 1);
        assert!((last7.correction_rate - 0.5).abs() < 1e-9);

        let last60 = evaluate_window(&conn, 60).unwrap();
        assert_eq!(last60.sessions, 3);
        assert_eq!(last60.corrections, 6);
    }

    #[test]
    fn is_regression_strict_greater_only() {
        let a = EvalReport {
            sessions: 10,
            corrections: 5,
            correction_rate: 0.5,
        };
        let same = a.clone();
        let worse = EvalReport {
            sessions: 10,
            corrections: 6,
            correction_rate: 0.6,
        };
        let better = EvalReport {
            sessions: 10,
            corrections: 4,
            correction_rate: 0.4,
        };
        assert!(!is_regression(&a, &same));
        assert!(is_regression(&a, &worse));
        assert!(!is_regression(&a, &better));
    }
}
