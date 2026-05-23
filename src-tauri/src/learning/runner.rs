//! End-to-end orchestrator: pull → classify → promote → eval →
//! commit-or-rollback. Single public entry point [`run_once`].
//!
//! Transactional model: every change happens inside a single SQLite
//! `BEGIN ... COMMIT/ROLLBACK` so partial-state DBs are impossible.
//! If the eval pass sees a regression, the whole batch reverts via
//! `ROLLBACK` and the `learning_runs` row is inserted with
//! `rolled_back = 1` AFTER the rollback (in a separate transaction
//! so the meta-row survives).
//!
//! ## Why we don't use [`audit::rollback_table_to_timestamp`]
//!
//! That function reverts schema-history rows on a per-table basis,
//! but is awkward for cross-table atomicity (dictionary + style_examples
//! both touched). A plain SQLite transaction is the right primitive
//! for "all of this batch, or none" — the audit-rollback path is for
//! point-in-time recovery (Phase 8 Wave 2 UI button).

use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use crate::error::{AppError, AppResult};

use super::classifier::Classifier;
use super::corrections;
use super::eval::{is_regression, EvalProvider};
use super::promoter::{promote_one, prune_mode_examples, DEFAULT_EXAMPLES_PER_MODE};
use super::runs::{self, Completion};

/// Tunables. Defaults match PLAN §10.
#[derive(Debug, Clone)]
pub struct RunnerConfig {
    /// Pull corrections newer than this many days.
    pub since_days: u32,
    /// Eval window — replay this many days of sessions.
    pub eval_window_days: u32,
    /// Cap on enabled examples per mode after pruning.
    pub examples_cap_per_mode: usize,
    /// Default mode-slug to attribute style examples to when the
    /// session's mode lookup fails (rare; keeps the runner robust).
    pub fallback_mode_slug: String,
}

impl Default for RunnerConfig {
    fn default() -> Self {
        Self {
            since_days: 7,
            eval_window_days: 1,
            examples_cap_per_mode: DEFAULT_EXAMPLES_PER_MODE,
            fallback_mode_slug: "normal".into(),
        }
    }
}

/// Outcome of [`run_once`].
#[derive(Debug, Clone, PartialEq)]
pub enum RunOutcome {
    /// No work to do — zero pending corrections in window.
    NoOp,
    /// Batch committed.
    Committed {
        /// New `learning_runs.id`.
        run_id: i64,
        /// Stats for the dashboard.
        completion: Completion,
    },
    /// Batch rolled back due to regression.
    RolledBack {
        /// New `learning_runs.id`.
        run_id: i64,
        /// Stats for the dashboard.
        completion: Completion,
    },
}

/// Run one iteration of the nightly learning loop.
///
/// `eval` is the pluggable evaluator (see [`super::eval::EvalProvider`]).
/// Production callers pass [`super::eval::DefaultEvalProvider`].
pub fn run_once(
    db: &Arc<Mutex<Connection>>,
    classifier: &mut dyn Classifier,
    eval: &mut dyn EvalProvider,
    config: &RunnerConfig,
) -> AppResult<RunOutcome> {
    let mut conn = db
        .lock()
        .map_err(|_| AppError::Other("learning runner: db mutex poisoned".into()))?;

    // 0. Insert the run row up-front. Even if classification errors,
    //    we want a record of the attempt.
    let run_id = runs::start(&conn)?;

    // 1. Pull unclassified corrections in window.
    let pending = corrections::list_unclassified_within(&conn, config.since_days)?;
    if pending.is_empty() {
        runs::complete(
            &conn,
            run_id,
            &Completion {
                sessions_analyzed: Some(0),
                corrections_classified: Some(0),
                examples_added: Some(0),
                examples_removed: Some(0),
                dictionary_terms_added: Some(0),
                rolled_back: false,
                notes: Some("no pending corrections".into()),
                ..Completion::default()
            },
        )?;
        return Ok(RunOutcome::NoOp);
    }

    // 2. Snapshot pre-eval. Compute BEFORE we change anything.
    let before = eval.evaluate(&conn, config.eval_window_days)?;

    // 3. BEGIN transaction. Classify + promote everything inside.
    let tx = conn.transaction()?;

    let mut classified_count: i64 = 0;
    let mut examples_added: i64 = 0;
    let mut dict_added: i64 = 0;
    for c in &pending {
        let classification = classifier.classify(c)?;
        // Look up the session's mode slug for style-example attribution.
        let mode_slug = mode_slug_for_session(&tx, c.session_id)
            .unwrap_or_else(|| config.fallback_mode_slug.clone());
        let stats = promote_one(&tx, c, classification, &mode_slug)?;
        examples_added += stats.examples_added;
        dict_added += stats.dictionary_terms_added;
        // Mark classified inside the txn — if we rollback, this
        // mark is also reverted, so a future run can retry.
        corrections::mark_classified(&tx, c.id, classification.as_str())?;
        classified_count += 1;
    }

    // 4. Prune each mode's examples.
    let mut examples_removed: i64 = 0;
    for slug in distinct_mode_slugs(&tx)? {
        examples_removed += prune_mode_examples(&tx, &slug, config.examples_cap_per_mode)?;
    }

    // 5. Post-eval (inside the open transaction so the changes are
    //    visible to the eval queries).
    let after = eval.evaluate(&tx, config.eval_window_days)?;

    let regressed = is_regression(&before, &after);
    let completion = Completion {
        sessions_analyzed: Some(before.sessions),
        corrections_classified: Some(classified_count),
        examples_added: Some(examples_added),
        examples_removed: Some(examples_removed),
        dictionary_terms_added: Some(dict_added),
        eval_correction_rate_before: Some(before.correction_rate),
        eval_correction_rate_after: Some(after.correction_rate),
        rolled_back: regressed,
        notes: Some(if regressed {
            format!(
                "regression: before={:.4} after={:.4}; rolled back",
                before.correction_rate, after.correction_rate
            )
        } else {
            "committed".into()
        }),
    };

    if regressed {
        tx.rollback()?;
        // Run-row completion in a fresh implicit txn so the rollback
        // doesn't take the meta-row with it.
        runs::complete(&conn, run_id, &completion)?;
        Ok(RunOutcome::RolledBack { run_id, completion })
    } else {
        tx.commit()?;
        runs::complete(&conn, run_id, &completion)?;
        Ok(RunOutcome::Committed { run_id, completion })
    }
}

/// Resolve `sessions.id → modes.slug`. Returns `None` if the session
/// or its mode is missing (very rare; suggests data corruption).
fn mode_slug_for_session(conn: &Connection, session_id: i64) -> Option<String> {
    conn.query_row(
        "SELECT m.slug FROM sessions s JOIN modes m ON m.id = s.mode_id \
         WHERE s.id = ?1",
        [session_id],
        |r| r.get::<_, String>(0),
    )
    .ok()
}

/// Every distinct mode slug currently present in the `modes` table.
/// (Not just the ones with style examples — that's what `prune`
/// checks anyway.)
fn distinct_mode_slugs(conn: &Connection) -> AppResult<Vec<String>> {
    let mut stmt = conn.prepare("SELECT slug FROM modes")?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::sessions::{self, NewSession, SessionStatus, StartMode};
    use crate::db::Database;
    use crate::dictation::runtime::bootstrap_provenance_rows;
    use crate::learning::classifier::{Classification, HeuristicClassifier};
    use crate::learning::corrections::{insert as insert_correction, Correction, NewCorrection};
    use crate::learning::eval::{DefaultEvalProvider, EvalReport, FixedEvalProvider};

    fn fresh_db() -> Arc<Mutex<Connection>> {
        let conn = Database::open_in_memory().unwrap().conn;
        bootstrap_provenance_rows(&conn).unwrap();
        Arc::new(Mutex::new(conn))
    }

    fn add_recent_session(conn: &Connection) -> i64 {
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
        let id = sessions::insert(
            conn,
            &NewSession {
                uuid: uuid::Uuid::new_v4().to_string(),
                mode_id: 1,
                hotkey_pressed: "RightAlt".into(),
                started_at: "ph".into(),
                recording_ended_at: "ph".into(),
                status: SessionStatus::Recording,
                foreground_app: Some("notepad.exe".into()),
                foreground_window_title: None,
                audio_duration_ms: 0,
                audio_blob_path: None,
                prompt_id,
                dictionary_snapshot_id: dict_id,
                example_set_id: example_id,
                start_mode: StartMode::Ptt,
            },
        )
        .unwrap();
        // Patch to "now" so eval window catches it.
        conn.execute(
            "UPDATE sessions SET started_at = datetime('now', '-6 hours') WHERE id = ?1",
            [id],
        )
        .unwrap();
        id
    }

    #[test]
    fn noop_when_nothing_pending() {
        let db = fresh_db();
        let mut clf = HeuristicClassifier;
        let mut eval = DefaultEvalProvider;
        let r = run_once(&db, &mut clf, &mut eval, &RunnerConfig::default()).unwrap();
        assert_eq!(r, RunOutcome::NoOp);
        // A run row was still inserted.
        let conn = db.lock().unwrap();
        let recent = runs::list_recent(&conn, 10).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].notes.as_deref(), Some("no pending corrections"));
    }

    #[test]
    fn happy_run_promotes_and_commits() {
        let db = fresh_db();
        let sid = {
            let conn = db.lock().unwrap();
            let sid = add_recent_session(&conn);
            // Two corrections: a new-vocab + a style-change.
            insert_correction(
                &conn,
                &NewCorrection {
                    session_id: sid,
                    before_text: "mockingbird".into(),
                    after_text: "Mockingbird".into(),
                    detection_method: "manual".into(),
                },
            )
            .unwrap();
            insert_correction(
                &conn,
                &NewCorrection {
                    session_id: sid,
                    before_text: "hi".into(),
                    after_text: "Hello there my dear friend!".into(),
                    detection_method: "manual".into(),
                },
            )
            .unwrap();
            sid
        };
        let mut clf = HeuristicClassifier;
        let mut eval = DefaultEvalProvider;
        let r = run_once(&db, &mut clf, &mut eval, &RunnerConfig::default()).unwrap();
        match r {
            RunOutcome::Committed { completion, .. } => {
                assert_eq!(completion.corrections_classified, Some(2));
                assert_eq!(completion.dictionary_terms_added, Some(1));
                assert_eq!(completion.examples_added, Some(1));
                assert!(!completion.rolled_back);
            }
            other => panic!("expected Committed, got {other:?}"),
        }

        // The dictionary entry should be there.
        let conn = db.lock().unwrap();
        let entry = crate::db::dictionary::find_by_term(&conn, "Mockingbird", None)
            .unwrap()
            .unwrap();
        assert_eq!(entry.source, "learned");
        // And the style example.
        let cands = crate::cleanup::few_shot::select_candidates(&conn, "normal", None).unwrap();
        assert_eq!(cands.len(), 1);
        // And the corrections are marked classified.
        let pending = corrections::list_unclassified_within(&conn, 7).unwrap();
        assert!(pending.is_empty());
        let _ = sid;
    }

    #[test]
    fn regression_path_rolls_back_and_records_run() {
        // Cleanly tested via a FixedEvalProvider that returns a
        // worse `after` than `before` — no deadlock-prone classifier
        // tricks needed.
        let db = fresh_db();
        let sid = {
            let conn = db.lock().unwrap();
            let sid = add_recent_session(&conn);
            insert_correction(
                &conn,
                &NewCorrection {
                    session_id: sid,
                    before_text: "x".into(),
                    after_text: "yes a much longer reword".into(),
                    detection_method: "manual".into(),
                },
            )
            .unwrap();
            sid
        };

        let mut clf = HeuristicClassifier;
        let mut eval = FixedEvalProvider::new(
            EvalReport {
                sessions: 10,
                corrections: 1,
                correction_rate: 0.1,
            },
            EvalReport {
                sessions: 10,
                corrections: 5,
                correction_rate: 0.5,
            },
        );
        let r = run_once(&db, &mut clf, &mut eval, &RunnerConfig::default()).unwrap();
        match r {
            RunOutcome::RolledBack { completion, .. } => {
                assert!(completion.rolled_back);
                let notes = completion.notes.as_deref().unwrap_or("");
                assert!(notes.contains("regression"), "got notes: {notes}");
                assert_eq!(completion.eval_correction_rate_before, Some(0.1));
                assert_eq!(completion.eval_correction_rate_after, Some(0.5));
            }
            other => panic!("expected RolledBack, got {other:?}"),
        }

        // Style example should NOT exist — rolled back.
        let conn = db.lock().unwrap();
        let cands = crate::cleanup::few_shot::select_candidates(&conn, "normal", None).unwrap();
        assert!(
            cands.is_empty(),
            "style example should have been rolled back"
        );
        // The original correction should still be unclassified
        // (mark_classified happened inside the rolled-back txn).
        let pending = corrections::list_unclassified_within(&conn, 7).unwrap();
        assert!(
            pending.iter().any(|c| c.before_text == "x"),
            "original correction should not be marked classified after rollback"
        );
        let _ = sid;
    }

    /// Helper: ensure we don't accidentally let the `Correction` import
    /// go stale — used by the regression test rig and by `Classifier` impls.
    #[allow(dead_code)]
    fn _correction_type_lives() -> Correction {
        Correction {
            id: 0,
            session_id: 0,
            before_text: String::new(),
            after_text: String::new(),
            detection_method: String::new(),
            classification: None,
            classified_at: None,
            created_at: String::new(),
        }
    }

    /// Stand-in to ensure `Classification` is reachable for any future
    /// test additions (keeps the lint quiet without `#[allow]`).
    #[allow(dead_code)]
    fn _classification_lives() -> Classification {
        Classification::Noise
    }

    #[test]
    fn simulated_50_corrections_dataset_completes_within_eval_window() {
        let db = fresh_db();
        {
            let conn = db.lock().unwrap();
            let sid = add_recent_session(&conn);
            for i in 0..50 {
                let kind = i % 3;
                let (b, a) = match kind {
                    0 => ("mockingbird".to_string(), format!("Mockingbird{i}")),
                    1 => (
                        "hi".to_string(),
                        format!("Hello there friend #{i} this is way longer"),
                    ),
                    _ => ("x".to_string(), "y".to_string()),
                };
                insert_correction(
                    &conn,
                    &NewCorrection {
                        session_id: sid,
                        before_text: b,
                        after_text: a,
                        detection_method: "manual".into(),
                    },
                )
                .unwrap();
            }
        }
        let mut clf = HeuristicClassifier;
        let mut eval = DefaultEvalProvider;
        let r = run_once(&db, &mut clf, &mut eval, &RunnerConfig::default()).unwrap();
        match r {
            RunOutcome::Committed { completion, .. } => {
                assert_eq!(completion.corrections_classified, Some(50));
                // ~17 of each classification.
                assert!(completion.dictionary_terms_added.unwrap_or(0) >= 10);
                assert!(completion.examples_added.unwrap_or(0) >= 10);
            }
            other => panic!("expected Committed for 50-row dataset, got {other:?}"),
        }
    }
}
