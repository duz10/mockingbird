//! Phase 8 — nightly learning-loop entry point.
//!
//! Invoked by Windows Task Scheduler at 02:00 daily (set up by
//! `mockingbird::learning::scheduler::WinTaskScheduler::install`).
//! Runs one [`learning::runner::run_once`] iteration against the
//! user's database, then exits.
//!
//! Exit codes:
//!
//! - `0` — Committed (or NoOp).
//! - `2` — RolledBack (regression detected; check `learning_runs` for notes).
//! - `1` — Runner error (DB open, classifier failure, etc.).
//!
//! ## Why a separate binary
//!
//! Task Scheduler invokes commands, not in-process callbacks. A
//! separate binary that boots the DB + runs the loop + exits is the
//! cleanest fit. Shares `mockingbird_lib`, so all the production
//! Phase-4 cleanup providers + Phase-8 learning logic are reusable.
//!
//! ## Classifier choice
//!
//! Uses `LlmClassifier` wired to an `OllamaProvider` by default,
//! falling back to `HeuristicClassifier` if Ollama is unreachable.
//! Both routes are tested. The fallback means the nightly loop still
//! makes some progress (heuristic-only) on a box where Ollama is
//! down, rather than failing entirely.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use mockingbird_lib::cleanup::OllamaProvider;
use mockingbird_lib::db::Database;
use mockingbird_lib::learning::classifier::{Classifier, HeuristicClassifier, LlmClassifier};
use mockingbird_lib::learning::eval::DefaultEvalProvider;
use mockingbird_lib::learning::runner::{run_once, RunOutcome, RunnerConfig};

fn main() {
    // Initialize a minimal tracing subscriber that logs to stderr.
    // The full daily-rotated file logging is overkill for a one-shot
    // CLI invocation.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    let exit_code = match run() {
        Ok(0) => 0,
        Ok(n) => n,
        Err(e) => {
            tracing::error!(error = %e, "learn run failed");
            1
        }
    };
    std::process::exit(exit_code);
}

fn run() -> Result<i32, Box<dyn std::error::Error>> {
    let db_path = db_path()?;
    tracing::info!(?db_path, "learn: opening DB");
    let db = Database::open(&db_path)?;
    let db_arc: Arc<Mutex<rusqlite::Connection>> = Arc::new(Mutex::new(db.conn));

    let mut classifier = build_classifier();
    let mut eval = DefaultEvalProvider;
    let config = RunnerConfig::default();

    let outcome = run_once(&db_arc, classifier.as_mut(), &mut eval, &config)?;
    match outcome {
        RunOutcome::NoOp => {
            tracing::info!("learn: no pending corrections; exiting");
            Ok(0)
        }
        RunOutcome::Committed { run_id, completion } => {
            tracing::info!(
                run_id,
                classified = completion.corrections_classified,
                examples_added = completion.examples_added,
                dict_added = completion.dictionary_terms_added,
                before_rate = completion.eval_correction_rate_before,
                after_rate = completion.eval_correction_rate_after,
                "learn: committed"
            );
            Ok(0)
        }
        RunOutcome::RolledBack { run_id, completion } => {
            tracing::warn!(
                run_id,
                before_rate = completion.eval_correction_rate_before,
                after_rate = completion.eval_correction_rate_after,
                notes = ?completion.notes,
                "learn: rolled back due to regression"
            );
            Ok(2)
        }
    }
}

/// Mirror the path used by the main app: `%APPDATA%\Mockingbird\mockingbird.db`.
/// Falls back to the env var `MOCKINGBIRD_DB` for test rigs.
fn db_path() -> Result<PathBuf, Box<dyn std::error::Error>> {
    if let Ok(p) = std::env::var("MOCKINGBIRD_DB") {
        return Ok(PathBuf::from(p));
    }
    let appdata =
        std::env::var("APPDATA").map_err(|_| "APPDATA not set (set MOCKINGBIRD_DB to override)")?;
    Ok(PathBuf::from(appdata)
        .join("Mockingbird")
        .join("mockingbird.db"))
}

/// Build the classifier, preferring LLM but falling back to heuristic
/// if Ollama isn't reachable. Logs the choice.
fn build_classifier() -> Box<dyn Classifier> {
    let provider = OllamaProvider::new();
    match provider.health_check() {
        Ok(()) => {
            tracing::info!("learn: Ollama reachable; using LlmClassifier");
            Box::new(LlmClassifier::new(
                Box::new(provider),
                "qwen2.5:3b-instruct-q4_K_M".into(),
            ))
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                "learn: Ollama unreachable; falling back to HeuristicClassifier"
            );
            Box::new(HeuristicClassifier)
        }
    }
}
