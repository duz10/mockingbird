//! Mockingbird — local-first voice dictation for Windows.
//!
//! Binary entry point is in `main.rs`; this library crate is what gets
//! linked into the Tauri shell. See `PLAN-mockingbird-v2.md` for the
//! full design and `docs/phases/` for per-phase implementation plans.

#![warn(missing_docs)]

pub mod audio;
pub mod cleanup;
// `commands` module is a thin shim over typed DTOs that mirror the
// TypeScript types in `ui/src/lib/types.ts`. Documenting every field
// twice (here + on the TS side) is busywork; the JS types are the
// contract surface end users care about.
#[allow(missing_docs)]
pub mod commands;
pub mod db;
pub mod dictation;
pub mod error;
pub mod hotkey;
pub mod injection;
pub mod learning;
pub mod logging;
pub mod recording_window;
pub mod secrets;
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
/// Boots a Tauri app that: initializes daily-rotated tracing with PII
/// scrubbing, opens the DB at `%APPDATA%/Mockingbird/mockingbird.db`,
/// registers the system tray, installs the WH_KEYBOARD_LL hotkey
/// hook, spawns the dictation orchestrator thread, and registers
/// every Tauri command the UI uses (see `commands::register`).
pub fn run() {
    let builder =
        tauri::Builder::default().setup(|app| -> Result<(), Box<dyn std::error::Error>> {
            let app_data = app.path().app_data_dir().map_err(box_err)?;
            std::fs::create_dir_all(&app_data)?;

            // Initialize logging FIRST so DB-open errors get captured.
            // WorkerGuard MUST outlive the Tauri runtime; leaking via
            // mem::forget is the cleanest pattern for a
            // process-lifetime singleton.
            let guard = logging::init(&app_data).map_err(box_err)?;
            std::mem::forget(guard);

            tracing::info!(?app_data, "Mockingbird starting");

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
                        // Plug the Tauri AppHandle into the recording
                        // window so it can show/hide the real webview
                        // and emit `dictation:state` events to the
                        // overlay. Done AFTER spawn so the orchestrator
                        // thread already has its clone; the
                        // Arc<Mutex<Option<_>>> inside is shared with
                        // every clone, so this writes once and propagates.
                        runtime
                            .recording_window
                            .set_app_handle(app.handle().clone());
                        tracing::info!("🐦 dictation runtime started; hold RightAlt to dictate");
                        app.manage(runtime);
                    }
                    Err(e) => {
                        // Non-fatal: the Tauri shell + IPC still work.
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
                tracing::warn!("dictation runtime is Windows-only; skipping");
            }

            Ok(())
        });

    commands::register(builder)
        .run(tauri::generate_context!())
        .expect("error while running Tauri application");
}

fn box_err<E>(e: E) -> Box<dyn std::error::Error>
where
    E: Into<Box<dyn std::error::Error>>,
{
    e.into()
}
