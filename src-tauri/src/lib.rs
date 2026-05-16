//! Mockingbird — local-first voice dictation for Windows.
//!
//! Binary entry point is in `main.rs`; this library crate is what gets
//! linked into the Tauri shell. See `PLAN-mockingbird-v2.md` for the
//! full design and `docs/phases/phase1.md` for the current phase plan.

#![warn(missing_docs)]

//! Mockingbird — local-first voice dictation for Windows.
//!
//! Library crate. Binary entry point lives in `main.rs`. See
//! `PLAN-mockingbird-v2.md` for the design and `docs/phases/` for
//! per-phase implementation plans.

pub mod audio;
pub mod commands;
pub mod db;
pub mod error;
pub mod hotkey;
pub mod injection;
pub mod logging;
pub mod settings;
pub mod stt;
pub mod tray;
pub mod window_context;

use tauri::Manager;

use commands::AppState;

/// Build and run the Tauri application.
///
/// Phase 1 progress:
///   - Wave 1 ✅ — skeleton + ADR 0004 + Cargo + tauri.conf.json
///   - Wave 2 ✅ — SQLite migrations 001-003 + runner wired
///   - Wave 3 ✅ — 7 DB repository modules + integration tests
///   - Wave 4 ✅ — typed settings, logging w/ PII scrub, tray, commands
///   - Wave 5 — docs, judges, seal `phase-1-complete`
///
/// Calling `run()` boots a Tauri app that: opens to a hidden main
/// window, initializes daily-rotated tracing with PII scrubbing, opens
/// the DB at `%APPDATA%/Mockingbird/mockingbird.db`, registers the
/// system tray, and exposes the typed command surface (`get_setting`,
/// `set_setting`, `fts_smoke_test`).
pub fn run() {
    tauri::Builder::default()
        .setup(|app| -> Result<(), Box<dyn std::error::Error>> {
            let app_data = app.path().app_data_dir().map_err(box_err)?;
            std::fs::create_dir_all(&app_data)?;

            // Initialize logging FIRST so DB-open errors get captured.
            // The WorkerGuard MUST outlive the Tauri runtime; leaking
            // it via mem::forget is the cleanest pattern for a
            // process-lifetime singleton. Thread is reclaimed on exit.
            let guard = logging::init(&app_data).map_err(box_err)?;
            std::mem::forget(guard);

            tracing::info!(?app_data, "Mockingbird starting (Phase 1 Wave 4)");

            let db_path = app_data.join("mockingbird.db");
            let database = db::Database::open(&db_path).map_err(box_err)?;
            tracing::info!(?db_path, "database ready");

            app.manage(AppState::new(database));
            tray::register(app).map_err(box_err)?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_setting,
            commands::set_setting,
            commands::fts_smoke_test,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Tauri application");
}

fn box_err<E>(e: E) -> Box<dyn std::error::Error>
where
    E: Into<Box<dyn std::error::Error>>,
{
    e.into()
}
