//! Mockingbird — local-first voice dictation for Windows.
//!
//! Binary entry point is in `main.rs`; this library crate is what gets
//! linked into the Tauri shell. See `PLAN-mockingbird-v2.md` for the
//! full design and `docs/phases/` for per-phase implementation plans.

#![warn(missing_docs)]

// Phase 10 Wave 1B — sibling subsystem to dictation + meeting capture
// (ADR 0036). Activity-log skeleton: titles-only foreground sampler,
// sessions/events tables (migration 012), Command-Center-invoked
// lifecycle. Layers 2+/abstractor/encryption land in later waves.
pub mod activity;
pub mod audio;
pub mod cleanup;
// `commands` module is a thin shim over typed DTOs that mirror the
// TypeScript types in `ui/src/lib/types.ts`. Documenting every field
// twice (here + on the TS side) is busywork; the JS types are the
// contract surface end users care about.
#[allow(missing_docs)]
pub mod commands;
// Phase 10 Wave 1A — Unified Recording Command Center (ADR 0037).
pub mod command_center;
pub mod db;
pub mod dictation;
pub mod error;
pub mod hotkey;
pub mod injection;
pub mod learning;
pub mod logging;
// Phase MC — sibling subsystem to dictation (ADR 0026). Wave 1 ships
// scaffolds + types + trait shapes; Waves 2-6 fill the bodies.
pub mod meetings;
// Phase 10 Wave 1A — shared bottom-center overlay conventions.
pub mod overlay_conventions;
pub mod recording_window;
pub mod secrets;
pub mod settings;
pub mod stt;
pub mod tray;
pub mod window_context;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tauri::Manager;

use activity::ActivityCaptureRuntime;
use commands::AppState;
use dictation::runtime::{default_normal_config, DictationRuntime};
#[cfg(target_os = "windows")]
use meetings::runtime::{MeetingCaptureRuntime, MeetingRuntimeConfig};

/// Build and run the Tauri application.
///
/// Boots a Tauri app that: initializes daily-rotated tracing with PII
/// scrubbing, opens the DB at `%APPDATA%/Mockingbird/mockingbird.db`,
/// registers the system tray, installs the WH_KEYBOARD_LL hotkey
/// hook, spawns the dictation orchestrator thread, and registers
/// every Tauri command the UI uses (see `commands::register`).
pub fn run() {
    let builder = tauri::Builder::default()
        // Phase MC Wave 5 — tauri-plugin-dialog enables the native
        // Save As… picker for `meeting_export_markdown`. Tauri 2
        // moved dialogs out of core; this is the in-org replacement.
        .plugin(tauri_plugin_dialog::init())
        // Close-to-tray for the main window: clicking X should hide
        // the window, not destroy it + exit the app. The tray icon
        // (and tray menu "Open History") re-shows it. The recording
        // overlay window keeps the default destroy-on-close behavior
        // — it's a transient overlay, not a primary surface.
        //
        // Without this handler, X-close kills the whole process, the
        // tray icon zombies until Windows GCs it, and the user has to
        // re-launch the exe to use the app again. Reported as a UX
        // bug post phase-mc kickoff (2026-05-18).
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    tracing::info!(
                        window = %window.label(),
                        "close requested on main window — hiding to tray"
                    );
                    api.prevent_close();
                    if let Err(e) = window.hide() {
                        tracing::warn!(error = ?e, "failed to hide main window on close");
                    }
                }
            }
        })
        .setup(|app| -> Result<(), Box<dyn std::error::Error>> {
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
            // Tray registration moved BELOW the runtime spawns so the
            // tray menu can read `MeetingCaptureRuntime::
            // is_meeting_hotkey_paused()` when building the initial
            // "Pause Meeting Hotkey" check item. Phase MC Wave 5.

            // Spawn the full dictation pipeline. Drop-on-AppState-drop
            // tears down the hook + threads cleanly.
            #[cfg(target_os = "windows")]
            {
                match DictationRuntime::spawn(
                    shared_conn.clone(),
                    orchestrator_config,
                    HashMap::new(),
                ) {
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

            // Phase MC Wave 4 — Meeting capture runtime. Non-fatal
            // failure mirrors the dictation runtime: if the second
            // WH_KEYBOARD_LL hook can't install, the app still works,
            // the user just can't capture meetings. Both the runtime
            // (to keep it alive) AND its shared bag (for IPC) are
            // registered as managed state.
            #[cfg(target_os = "windows")]
            {
                let chunk_base_dir = app_data.join("meetings");
                if let Err(e) = std::fs::create_dir_all(&chunk_base_dir) {
                    tracing::warn!(
                        error = ?e,
                        path = ?chunk_base_dir,
                        "failed to create meetings chunk base dir"
                    );
                }
                // mb-fc1: hydrate the chord + max-duration + default
                // source from the settings DB rather than baking the
                // defaults at startup. The pre-hotfix code path called
                // `defaults_with` unconditionally, which made the
                // Settings → Meetings tab chord picker a no-op.
                let mc_config = {
                    // The shared_conn mutex is fresh at this point in
                    // startup — no other thread has touched it yet —
                    // so a poisoned lock here means a previous panic
                    // before us, which we can't recover from anyway.
                    let guard = shared_conn
                        .lock()
                        .expect("shared_conn must be unpoisoned at startup");
                    MeetingRuntimeConfig::from_settings(&guard, chunk_base_dir)
                };
                let chord_label = mc_config.hotkey_label.clone();
                match MeetingCaptureRuntime::spawn(
                    shared_conn.clone(),
                    mc_config,
                    app.handle().clone(),
                ) {
                    Ok(mc_runtime) => {
                        // Publish the cheaply-clonable shared bag for
                        // commands::meetings::* to take via State<>.
                        app.manage(mc_runtime.shared());
                        // Hold the outer runtime alive for the
                        // process lifetime. Tauri drops managed state
                        // on shutdown, which fires the runtime's Drop
                        // (stops the hotkey + persists any in-flight
                        // meeting as Interrupted).
                        app.manage(mc_runtime);
                        tracing::info!(
                            chord = %chord_label,
                            "🎙️ meeting capture runtime started"
                        );
                    }
                    Err(e) => {
                        tracing::error!(
                            error = ?e,
                            "meeting capture runtime failed to start; app continues without it"
                        );
                    }
                }
            }

            // Phase 10 Wave 1B — activity-capture runtime. Sibling
            // subsystem (ADR 0036). Always spawns (no chord, no hook
            // install) — the worst case is an empty timeline if the
            // platform sampler can't poll, which is the right UX
            // (Activity sessions exist; the user can stop them via
            // the CC). Registered BEFORE the Command Center so the
            // CC can dispatch into it via managed state immediately.
            //
            // Phase 10 Wave 4: pass an `activity_audio` chunk base
            // dir sibling to the meetings one. The directory exists
            // even when audio is disabled; per-session subdirs are
            // created + GC'd by the orchestrator (ADR 0041).
            let activity_audio_dir = app_data.join("activity_audio");
            if let Err(e) = std::fs::create_dir_all(&activity_audio_dir) {
                tracing::warn!(
                    error = ?e,
                    path = ?activity_audio_dir,
                    "failed to create activity audio chunk dir; audio will fail if enabled"
                );
            }
            // Phase 10 Wave 5 — Crash recovery sweep BEFORE the
            // activity runtime spawns. Promotes orphan `in_progress`
            // sessions to `crashed_recovered` and deletes orphan
            // chunk_dir subdirs (audio for sessions that no longer
            // exist). Best-effort; logs + swallows individual
            // errors so a failed recovery can't block boot.
            {
                let conn_guard = shared_conn.lock();
                if let Ok(conn) = conn_guard {
                    let report =
                        crate::activity::crash_recovery::recover_all(&conn, &activity_audio_dir);
                    tracing::info!(
                        target: "activity::crash_recovery",
                        sessions_recovered = report.sessions_recovered,
                        orphan_dirs_deleted = report.orphan_dirs_deleted,
                        orphan_dirs_kept = report.orphan_dirs_kept,
                        "\u{1f527} activity crash-recovery pass complete"
                    );
                }
            }

            let activity_runtime =
                ActivityCaptureRuntime::spawn(shared_conn.clone(), activity_audio_dir);
            app.manage(activity_runtime);
            tracing::info!("\u{1f4ca} activity-capture runtime registered");

            // Phase 10 Wave 5 — Retention sweep daemon. Throttled
            // boot sweep + daily cadence. Fire-and-forget thread.
            // No-op while all TTLs are `0 = forever` (the default).
            crate::activity::retention::spawn_daemon(shared_conn.clone());
            tracing::info!("\u{1f9f9} activity retention daemon started");

            // Phase 10 Wave 1A — spin up the Command Center after the
            // dictation + meeting runtimes are registered, so the
            // chord-fire → dispatch path can resolve them via managed
            // state. First-run auto-open happens inside `spawn()` if
            // `command_center_seen_v1 == false`.
            let cc = command_center::CommandCenter::spawn(app.handle().clone(), shared_conn.clone());
            // Attach the meeting runtime to the CC if it managed to
            // boot. Late-binding keeps the CC alive even when meeting
            // capture failed to install — the user still sees the
            // mode picker, the Meeting tile just dispatches to a
            // missing runtime and gets a graceful failure toast.
            #[cfg(target_os = "windows")]
            {
                if let Some(mc_shared) = app.try_state::<crate::meetings::runtime::MeetingRuntimeShared>() {
                    cc.attach_meeting_runtime(mc_shared.inner().clone());
                }
            }
            app.manage(cc);

            // Register the tray now that both runtimes + the Command
            // Center are in the managed-state registry. The tray menu-
            // build path reads `MeetingCaptureRuntime::
            // is_meeting_hotkey_paused()` to set the initial checkmark
            // on "Pause Meeting Hotkey".
            tray::register(app).map_err(box_err)?;

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
