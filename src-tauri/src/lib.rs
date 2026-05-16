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
pub mod cleanup;
pub mod commands;
pub mod db;
pub mod dictation;
pub mod error;
pub mod hotkey;
pub mod injection;
pub mod logging;
pub mod recording_window;
pub mod settings;
pub mod stt;
pub mod tray;
pub mod window_context;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tauri::Manager;

use commands::AppState;
use dictation::runtime::{default_normal_config, DictationRuntime};

/// Build and run the Tauri application.
///
/// Phase 3 Wave 4.5 progress:
///   - DB + logging + tray (Phase 1) ✅
///   - Audio + STT + VAD (Phase 2) ✅
///   - Hotkey + injection + dictation orchestrator wired (Wave 4.5) ✅
///
/// Calling `run()` boots a Tauri app that: initializes daily-rotated
/// tracing with PII scrubbing, opens the DB at
/// `%APPDATA%/Mockingbird/mockingbird.db`, registers the system tray,
/// installs the WH_KEYBOARD_LL hotkey hook, and spawns the dictation
/// orchestrator thread. Holding RightAlt triggers the full pipeline.
pub fn run() {
    tauri::Builder::default()
        .setup(|app| -> Result<(), Box<dyn std::error::Error>> {
            let app_data = app.path().app_data_dir().map_err(box_err)?;
            std::fs::create_dir_all(&app_data)?;

            // Initialize logging FIRST so DB-open errors get captured.
            // WorkerGuard MUST outlive the Tauri runtime; leaking via
            // mem::forget is the cleanest pattern for a
            // process-lifetime singleton.
            let guard = logging::init(&app_data).map_err(box_err)?;
            std::mem::forget(guard);

            tracing::info!(?app_data, "Mockingbird starting (Phase 3 Wave 4.5)");

            let db_path = app_data.join("mockingbird.db");
            let database = db::Database::open(&db_path).map_err(box_err)?;
            tracing::info!(?db_path, "database ready");

            // Build the orchestrator config BEFORE moving the
            // connection into the shared Arc<Mutex<>>. The bootstrap
            // creates default provenance rows if missing.
            let orchestrator_config = default_normal_config(&database.conn).map_err(box_err)?;
            tracing::info!(
                mode = %orchestrator_config.mode_slug,
                prompt_id = orchestrator_config.prompt_id,
                dict_id = orchestrator_config.dictionary_snapshot_id,
                example_id = orchestrator_config.example_set_id,
                "orchestrator config resolved"
            );

            // Share the connection between IPC handlers + dictation thread.
            // WAL mode (set in Database::open) makes parallel access safe.
            let shared_conn = Arc::new(Mutex::new(database.conn));
            app.manage(AppState::new(shared_conn.clone()));
            tray::register(app).map_err(box_err)?;

            // Spawn the full dictation pipeline. Drop-on-AppState-drop
            // tears down the hook + threads cleanly.
            #[cfg(target_os = "windows")]
            {
                match DictationRuntime::spawn(shared_conn, orchestrator_config, HashMap::new()) {
                    Ok(runtime) => {
                        tracing::info!("dictation runtime started; hold RightAlt to dictate");
                        app.manage(runtime);
                    }
                    Err(e) => {
                        // Non-fatal: the Tauri shell + IPC still work.
                        // The user gets a clear log line about what's
                        // missing (typically: ONNX DLL, Whisper model,
                        // or microphone permission).
                        tracing::error!(
                            error = ?e,
                            "dictation runtime failed to start; app continues without dictation"
                        );
                    }
                }
            }
            #[cfg(not(target_os = "windows"))]
            {
                let _ = orchestrator_config;
                let _ = &shared_conn;
                tracing::warn!("dictation runtime is Windows-only; skipping (Phase 9)");
            }

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
