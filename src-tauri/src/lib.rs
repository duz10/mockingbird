//! Mockingbird — local-first voice dictation for Windows.
//!
//! Binary entry point is in `main.rs`; this library crate is what gets
//! linked into the Tauri shell. See `PLAN-mockingbird-v2.md` for the
//! full design and `docs/phases/phase1.md` for the current phase plan.

// `#![warn(missing_docs)]` is intentionally NOT set yet. Wave 5 polish
// will enable it once every public item has a doc comment; until then
// it would drown clippy in noise about self-explanatory struct fields.

pub mod db;
pub mod error;

use tauri::Manager;

/// Build and run the Tauri application.
///
/// Phase 1 progress:
///   - Wave 1 ✅ — skeleton + ADR 0004 + Cargo + tauri.conf.json
///   - Wave 2 ✅ — SQLite migrations 001-003 + runner wired into `.setup()`
///   - Wave 3 (next) — DB repository modules
///   - Wave 4 — typed settings, logging w/ PII scrub, tray, commands
///   - Wave 5 — docs, judges, seal `phase-1-complete`
///
/// Today calling `run()` boots a Tauri app that opens to a hidden main
/// window, applies all migrations on startup, and runs the integrity
/// check. No tray icon yet (Wave 4); no commands registered (Wave 4).
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            tracing::info!("Mockingbird starting (Phase 1 Wave 2)");

            // Resolve %APPDATA%/Mockingbird/, ensure it exists, open the DB.
            // In Wave 4 the Database moves into `app.manage()` so
            // `#[tauri::command]`s can inject it via `tauri::State`.
            let app_data = app
                .path()
                .app_data_dir()
                .map_err(|e| -> Box<dyn std::error::Error> { Box::new(e) })?;
            std::fs::create_dir_all(&app_data)?;
            let db_path = app_data.join("mockingbird.db");
            let _db = db::Database::open(&db_path)
                .map_err(|e| -> Box<dyn std::error::Error> { Box::new(e) })?;
            tracing::info!(?db_path, "database ready");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Tauri application");
}
