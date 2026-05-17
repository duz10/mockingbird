//! Learning loop: list past runs + trigger a manual run.
//!
//! The manual trigger runs the loop synchronously on the calling
//! thread. That's OK for a Settings → "Run now" button — the typical
//! batch is small. If it ever blocks the UI noticeably, promote to
//! a `tokio::task::spawn_blocking` with a `tauri::Window::emit`
//! progress channel.

use tauri::State;

use crate::commands::types::LearningRunDto;
use crate::commands::{into_err, lock_db, AppStateHandle};
use crate::learning;

fn to_dto(r: learning::runs::LearningRun) -> LearningRunDto {
    LearningRunDto {
        id: r.id,
        started_at: r.started_at,
        completed_at: r.completed_at,
        sessions_analyzed: r.sessions_analyzed,
        corrections_classified: r.corrections_classified,
        examples_added: r.examples_added,
        examples_removed: r.examples_removed,
        dictionary_terms_added: r.dictionary_terms_added,
        rolled_back: r.rolled_back,
        notes: r.notes,
    }
}

#[tauri::command]
pub fn list_learning_runs(
    db: State<'_, AppStateHandle>,
    limit: usize,
) -> Result<Vec<LearningRunDto>, String> {
    let conn = lock_db(&db)?;
    let rows = learning::runs::list_recent(&conn, limit).map_err(into_err)?;
    Ok(rows.into_iter().map(to_dto).collect())
}

#[tauri::command]
pub fn trigger_learning_run(db: State<'_, AppStateHandle>) -> Result<i64, String> {
    let mut clf = learning::classifier::HeuristicClassifier;
    let mut eval = learning::eval::DefaultEvalProvider;
    let cfg = learning::runner::RunnerConfig::default();
    let outcome = learning::runner::run_once(&db.db, &mut clf, &mut eval, &cfg)
        .map_err(into_err)?;
    Ok(match outcome {
        learning::runner::RunOutcome::NoOp => 0,
        learning::runner::RunOutcome::Committed { run_id, .. } => run_id,
        learning::runner::RunOutcome::RolledBack { run_id, .. } => run_id,
    })
}
