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
// ADR 0046 Iter 3 — inbound mobile-inbox subsystem. Sibling of
// `vault::` (which owns the outbound projection). Wave 3.1 ships the
// file-watcher + stability state machine; Waves 3.2 + 3.3 add the
// courier processor and the runtime wiring respectively.
pub mod inbox;
pub mod injection;
// Phase 1A KG (ADR 0049 / mb-2mc9) — schema-driven structured-entry
// pipeline. Wave 1 (this commit) is a scaffold; Chunks 2+3 graduate
// the library subset from experimental/kg-validation/ + land the
// parity probe. Wired in alphabetical position between injection and
// learning so future additions slot in naturally.
pub mod kg;
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
// ADR 0046 Iter 2 — outbound Obsidian projection. Lives alongside
// the other top-level subsystems (dictation, meetings, activity).
pub mod vault;
pub mod window_context;

// macOS port: HashMap is consumed only by the Windows-gated DictationRuntime
// spawn block below; gate the import to match (crate-root, so no module-wide
// allow). Wired cross-platform in Phase 3/4.
#[cfg(target_os = "windows")]
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tauri::Manager;

use activity::ActivityCaptureRuntime;
use commands::AppState;
use dictation::runtime::default_normal_config;
// macOS port: DictationRuntime is only spawned in the Windows-gated block in
// `run()`; gate the import to match. Cross-platform spawn lands in Phase 3/4.
#[cfg(target_os = "windows")]
use dictation::runtime::DictationRuntime;
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
    // mb-1z0m Round 4 — bootstrap the DB + AppState BEFORE the Tauri
    // Builder so AppState lands on the builder-level StateManager.
    // Round 3 (commit bd5a1f7) probes showed that calling
    // `app.manage(AppState::new(..))` inside `.setup()` registered
    // the value (post_manage_probe + end_setup_probe both `true`)
    // but the IPC `State<AppStateHandle>` extractor at webview-
    // dispatch time still saw "state not managed for field db on
    // command insights_snapshot". The canonical Tauri-2 fix for
    // post-setup state-extractor failures is to register on the
    // Builder PRE-setup: the builder-level manager is provably
    // visible to every later webview, plugin, and command handler.
    //
    // Mechanically: moving bootstrap out of setup means we resolve
    // %APPDATA% directly (matching what `app.path().app_data_dir()`
    // would return for the configured identifier — see
    // `tauri.conf.json::identifier` + the `APP_IDENTIFIER` const +
    // its drift-protection test at the bottom of this file).
    let app_data = resolve_app_data_dir().expect("resolve app data dir");
    std::fs::create_dir_all(&app_data).expect("create app data dir");

    // Initialize logging FIRST so any later panic / error during
    // bootstrap is captured. WorkerGuard MUST outlive the Tauri
    // runtime; leaking via mem::forget is the cleanest pattern for
    // a process-lifetime singleton.
    let guard = logging::init(&app_data).expect("init logging");
    std::mem::forget(guard);

    tracing::info!(?app_data, "Mockingbird starting");

    let db_path = app_data.join("mockingbird.db");
    let database = db::Database::open(&db_path).expect("open database");
    tracing::info!(?db_path, "database ready");

    // Build the orchestrator config BEFORE moving the connection
    // into the shared Arc<Mutex<>>. The bootstrap creates default
    // provenance rows if missing.
    let orchestrator_config =
        default_normal_config(&database.conn).expect("orchestrator config resolved");
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

    // Path A — Builder-level AppState registration. The pre-Round-4
    // pattern was `.setup(|app| { app.manage(AppState::new(...)); })`
    // which Round 3 probes showed registered but did NOT propagate
    // reliably to webview IPC handlers (the visible symptom was
    // `"state not managed for field db on command insights_snapshot"`
    // rendered in the Insights body). Builder::manage on the builder
    // chain BEFORE .setup() puts the value in the runtime-level
    // StateManager that's visible to every later webview, plugin,
    // and command.
    let app_state = AppState::new(Arc::clone(&shared_conn));

    // LR.0.B / mb-hiar (ADR 0055) — DPAPI-backed secret store for
    // user-entered API keys (Claude + Unsplash). Registered on the
    // Builder PRE-`.setup()` for the same reason as AppState: the
    // `State<'_, SecretStoreHandle>` extractor at IPC dispatch time
    // must see the value, and `.setup()`-time `.manage()` calls do
    // not propagate reliably to webview handlers (mb-1z0m Round 3
    // post-mortem; LESSONS PINNED P16). Failure to construct the
    // store is fatal — on Windows this means LOCALAPPDATA is missing
    // or unwritable, which would brick every other write path too.
    let secret_store: secrets::SecretStoreHandle =
        secrets::make_default_store().expect("build secret store");
    tracing::info!(backend = secret_store.backend_name(), "secret store ready");

    // Clones for the move-into-setup closure. Tauri 2's setup
    // closure has 'static lifetime requirements on captured values,
    // hence the clones rather than borrows.
    let shared_conn_for_setup = Arc::clone(&shared_conn);
    let orchestrator_config_for_setup = orchestrator_config.clone();
    let app_data_for_setup = app_data.clone();

    let builder = tauri::Builder::default()
        // mb-1z0m Round 4 — Builder-level state registration. See
        // the long comment above `let app_state` for why this lives
        // here and not in `.setup()`.
        .manage(app_state)
        // LR.0.B / mb-hiar — Builder-level SecretStore registration.
        // Same propagation reasoning as AppState above.
        .manage(secret_store)
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
        .setup(move |app| -> Result<(), Box<dyn std::error::Error>> {
            // mb-1z0m Round 4 — receive the pre-Builder bootstrap
            // (db handle + orchestrator config + app data dir) via
            // move-capture. AppState itself is already registered on
            // the Builder; only the pieces setup-time code needs to
            // wire up other runtimes (dictation, meetings, activity)
            // are threaded through here.
            let shared_conn = shared_conn_for_setup;
            let orchestrator_config = orchestrator_config_for_setup;
            let app_data = app_data_for_setup;

            // mb-1z0m Round 3 + Round 4 — setup-entry probe. AppState
            // was registered on the Builder BEFORE this closure ran;
            // this probe confirms the builder-level manager is
            // visible from the App handle the setup closure receives.
            // If this is `false`, Tauri 2's builder-state propagation
            // is broken (very unlikely; it's the canonical pattern).
            // If this is `true` but IPC handlers still see "state not
            // managed", the bug is downstream of state registration
            // (Webview state isolation, plugin ordering, etc.) and
            // the next round investigates Tauri 2 internals.
            let post_manage_probe = app.try_state::<AppState>().is_some();
            tracing::info!(
                state = "AppState",
                site = "lib.rs:setup_entry",
                post_manage_probe,
                "managed-state visible (builder-registered)"
            );

            // mb-0gt6 / Wave 1D.3 — publish the orchestrator config
            // as managed state so the KG text-note IPC
            // (`kg_ingest_text_note`) can read the same dictionary /
            // example / mode fallbacks the dictation pipeline uses.
            // Cloned here BEFORE the spawn-time move into
            // `DictationRuntime::spawn`. Provenance binding is
            // strictly read-only after boot; the dictation tail does
            // its OWN per-session `resolve_active_mode_from_db`
            // lookup so the user's mode-picker selection still wins
            // for both paths.
            let kg_text_note_config = std::sync::Arc::new(orchestrator_config.clone());
            app.manage(std::sync::Arc::clone(&kg_text_note_config));

            // ADR 0046 Iter 2 / mb-lvzw — vault export-job runtime.
            // Construct BEFORE the dictation + meeting runtimes spawn so
            // both can grab a clone for their post-commit triggers.
            // Disabled-by-default per `MobileSyncEnabled` settings
            // default; `trigger()` short-circuits while disabled.
            let vault_runtime = match crate::vault::export_job::VaultRuntime::new(&shared_conn) {
                Ok(v) => Arc::new(v),
                Err(e) => {
                    tracing::error!(error = ?e, "vault runtime init failed; mobile sync disabled this session");
                    // Fall back to a default-config runtime so the
                    // dictation + meeting wiring still gets a handle
                    // (trigger() is a no-op when disabled).
                    Arc::new(
                        crate::vault::export_job::VaultRuntime::new(&shared_conn)
                            .unwrap_or_else(|_| panic!("vault runtime fallback also failed")),
                    )
                }
            };
            app.manage(Arc::clone(&vault_runtime));
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
                    Arc::clone(&vault_runtime),
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
                        // ADR 0046 §3.2 / mb-7vyz — publish the
                        // headless-ingest sender as its own managed-
                        // state entry so IPC handlers (and the future
                        // Iter 3 inbox watcher) can grab a clone via
                        // `State<'_, HeadlessIngestSender>` without
                        // reaching through DictationRuntime.
                        let headless_ingest_tx = runtime.headless_ingest_sender();
                        app.manage(headless_ingest_tx.clone());
                        tracing::info!("🐦 dictation runtime started; hold RightAlt to dictate");
                        app.manage(runtime);

                        // ADR 0046 Iter 4 / mb-q1xt — single shared
                        // ingest-progress bus. Both `dictation_import_file`
                        // (via `State<Arc<AppIngestProgressBus>>`) and the
                        // inbox courier (via the constructor below) emit
                        // through this one instance, so the React overlay
                        // sees a single coherent event stream regardless
                        // of origin.
                        let progress_bus = std::sync::Arc::new(
                            crate::dictation::ingest_progress::AppIngestProgressBus::new(),
                        );
                        progress_bus.set_app_handle(app.handle().clone());
                        app.manage(std::sync::Arc::clone(&progress_bus));
                        // Type-erased clone so the inbox + kg-inbox
                        // runtimes can share one bus instance via
                        // `Arc<dyn IngestProgressBus>` (LESSONS
                        // 2026-06-08: cloning the concrete
                        // `Arc<AppIngestProgressBus>` does NOT
                        // coerce to `dyn` automatically -- coercion
                        // only fires at the function-boundary move,
                        // so multi-consumer wiring needs the
                        // type-erased binding up front).
                        let progress_bus_dyn: std::sync::Arc<
                            dyn crate::dictation::ingest_progress::IngestProgressBus,
                        > = progress_bus;

                        // ADR 0046 Iter 3 / mb-3ivf — inbox runtime
                        // (Wave 3.3). Mirrors the vault export-job
                        // runtime's shape; gated by the SAME
                        // MobileSyncEnabled + VaultPath settings.
                        // Disabled-by-default so a stock install sees
                        // zero watcher overhead until the user opts in.
                        let inbox_runtime = Arc::new(
                            crate::inbox::runtime::InboxRuntime::new_with_progress(
                                headless_ingest_tx.clone(),
                                Arc::clone(&progress_bus_dyn),
                            ),
                        );
                        if let Err(e) = inbox_runtime.refresh_config(&shared_conn) {
                            tracing::error!(
                                error = ?e,
                                "inbox runtime: initial refresh_config failed; mobile inbox disabled this session"
                            );
                        }
                        app.manage(Arc::clone(&inbox_runtime));

                        // Phase 1E Wave 1E.6 (`mb-i46v`, ADR 0053
                        // Section "KG-Inbox courier") -- the KG-Inbox
                        // runtime. Sibling of the ADR 0046 inbox
                        // above; gated by `KgGraphEnabled` +
                        // `VaultPath` (NOT `MobileSyncEnabled` --
                        // the KG can be used with desktop-only
                        // drag-and-drop too). Same off-by-default
                        // ergonomics.
                        let kg_inbox_runtime = Arc::new(
                            crate::vault::kg_inbox_runtime::KgInboxRuntime::new_with_progress(
                                headless_ingest_tx,
                                Arc::clone(&shared_conn),
                                progress_bus_dyn,
                            ),
                        );
                        if let Err(e) = kg_inbox_runtime.refresh_config(&shared_conn) {
                            tracing::error!(
                                error = ?e,
                                "kg-inbox runtime: initial refresh_config failed; KG-Inbox disabled this session"
                            );
                        }
                        app.manage(Arc::clone(&kg_inbox_runtime));
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
                    Arc::clone(&vault_runtime),
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

            // Phase 1B Chunk 3 (`mb-eke8`, ADR 0050) — KG filing worker.
            // Phase 1C Wave 1C.1 (`mb-ucmx` / `mb-7w5f`, ADR 0051):
            // boot-vs-poll promotion. The worker now ALWAYS spawns;
            // the `KgGraphEnabled` gate moved inside the worker's
            // drain loop so flipping the setting from the Settings
            // KG tab takes effect within one IDLE_SLEEP tick (≤1s)
            // without an app restart. When the setting is `false`
            // (the default) the worker sleeps without dequeuing, so
            // there is no observable difference from the prior
            // boot-gated behaviour for opted-out users.
            //
            // Wrapped in cfg(target_os = "windows") to match the rest
            // of the v1 surface; the Settings table + queue tables
            // exist cross-platform, but the dictation hook that fills
            // the queue is Windows-only in v1 (ADR 0050 §1).
            #[cfg(target_os = "windows")]
            {
                let kg_runtime =
                    crate::kg::worker::KgFilingRuntime::spawn(shared_conn.clone());
                // Managed so Tauri's drop-on-shutdown fires the
                // worker's Drop (flips the shutdown AtomicBool).
                app.manage(kg_runtime);
                tracing::info!(
                    "\u{1f9e0} KG filing worker spawned (per-tick KgGraphEnabled poll)"
                );

                // Phase 1E Wave 1E.5 (`mb-qwfy`, ADR 0053 §D5) —
                // reverse-watcher. Mirrors the KG filing worker's
                // "always-spawn, settings-gated internally" shape:
                // the manager thread polls KgGraphEnabled +
                // VaultPath every 3s and only constructs the inner
                // FS-watcher once both are live. Disabled-by-default
                // installs pay one bounded sleep loop and zero I/O.
                let reverse_watcher =
                    crate::vault::watcher::ReverseWatcherRuntime::spawn(
                        shared_conn.clone(),
                    );
                app.manage(reverse_watcher);
                tracing::info!(
                    "\u{1f441}\u{fe0f}  KG reverse-watcher manager spawned (gated on KgGraphEnabled + VaultPath)"
                );
            }

            // Phase 1E Wave 1E.1 (`mb-e16d`, ADR 0053 §D1) — KG
            // vault subtree bootstrap. Mirrors the toggle-on IPC
            // (`kg_subtree_bootstrap` in `commands::kg`) for the
            // case where the user already had `KgGraphEnabled=true`
            // when the app booted. Fully idempotent; safe to call
            // on every boot. Best-effort: a failure here is logged
            // and swallowed — the toggle-on IPC will re-attempt the
            // bootstrap on the next user-driven flip, and the
            // graph-off invariant means no KG writes hit the disk
            // until both the toggle AND the bootstrap have
            // succeeded.
            //
            // Gated on BOTH `KgGraphEnabled` AND `VaultPath` being
            // set — opted-out / unconfigured users pay zero I/O
            // here. Mirrors the ADR 0046 inbox runtime's gate
            // shape (LESSONS P5: don't materialize vault state
            // until the user explicitly opted in).
            {
                let bootstrap_result = {
                    let conn_guard = shared_conn.lock();
                    match conn_guard {
                        Ok(conn) => {
                            let s = crate::settings::Settings::new(&conn);
                            let kg_on = s
                                .get::<bool>(crate::settings::model::SettingKey::KgGraphEnabled)
                                .unwrap_or(false);
                            let vault_path = s
                                .get::<Option<String>>(
                                    crate::settings::model::SettingKey::VaultPath,
                                )
                                .ok()
                                .flatten()
                                .filter(|p| !p.trim().is_empty());
                            match (kg_on, vault_path) {
                                (true, Some(p)) => Some(p),
                                _ => None,
                            }
                        }
                        Err(_) => None,
                    }
                };
                if let Some(path) = bootstrap_result {
                    match crate::vault::kg_layout::bootstrap_kg_subtree(
                        &std::path::PathBuf::from(&path),
                    ) {
                        Ok(report) => {
                            tracing::info!(
                                target: "kg::vault_bootstrap",
                                vault = %path,
                                ?report,
                                "\u{1f9e0} KG vault subtree bootstrap (boot-fire) complete"
                            );
                        }
                        Err(e) => {
                            // Non-fatal: the worker still spawns;
                            // entries can persist to the DB; the
                            // user can retry by toggling off/on or
                            // by fixing the vault path.
                            tracing::error!(
                                target: "kg::vault_bootstrap",
                                vault = %path,
                                error = ?e,
                                "KG vault subtree bootstrap (boot-fire) failed; app continues"
                            );
                        }
                    }
                }
            }

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

            // mb-1z0m (Round 3) — end-of-setup probe. If `post_manage`
            // succeeded but THIS one fails, something between line 137
            // and here nuked AppState (a duplicate manage of a different
            // type masking the slot, a replaced StateManager, etc.). If
            // BOTH succeed but UI-time IPCs still see "state not managed",
            // the failure is post-setup (IPC dispatcher / window bound
            // state isolation) — the next round investigates Tauri 2
            // internals, not setup-time wiring.
            let end_setup_probe = app.try_state::<AppState>().is_some();
            tracing::info!(
                state = "AppState",
                site = "lib.rs:end_of_setup",
                end_setup_probe,
                "managed-state end-of-setup probe"
            );

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

/// Tauri app identifier — MUST stay in sync with
/// `src-tauri/tauri.conf.json::identifier`. Used by
/// [`resolve_app_data_dir`] so the pre-Builder bootstrap path
/// (mb-1z0m Round 4) resolves to the SAME directory Tauri's runtime
/// `app.path().app_data_dir()` would return.
///
/// Drift protection: [`tests::identifier_matches_tauri_conf`] asserts
/// this constant matches the JSON config at compile-time.
const APP_IDENTIFIER: &str = "com.dustin.mockingbird";

/// Resolve `<platform app-data root>/<APP_IDENTIFIER>`, mirroring what
/// Tauri 2's `PathResolver::app_data_dir()` produces for the configured
/// identifier on each OS. Called from [`run`] BEFORE the Tauri Builder
/// is constructed so we can `Builder::manage(AppState)` before the
/// .setup() closure runs (Path A fix for mb-1z0m).
///
/// Per-OS roots (must stay in lock-step with Tauri's own resolver, or
/// the pre-Builder bootstrap lands the DB in the wrong place again):
/// - Windows: `%APPDATA%` (= `dirs::data_dir()`, Roaming).
/// - macOS:   `$HOME/Library/Application Support` (mb-mac-v1.2.4).
/// - other unix (CI dev only): `$HOME/.local/share` (XDG default).
///
/// Returns an error rather than panicking so the caller can decide
/// how to surface the failure (run() expects it; tests can probe).
fn resolve_app_data_dir() -> Result<PathBuf, Box<dyn std::error::Error>> {
    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var("APPDATA")
            .map_err(|_| "APPDATA env var not set; cannot resolve app data dir")?;
        Ok(PathBuf::from(appdata).join(APP_IDENTIFIER))
    }
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME")
            .map_err(|_| "HOME env var not set; cannot resolve app data dir")?;
        Ok(PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join(APP_IDENTIFIER))
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let home = std::env::var("HOME")
            .map_err(|_| "HOME env var not set; cannot resolve app data dir")?;
        Ok(PathBuf::from(home)
            .join(".local")
            .join("share")
            .join(APP_IDENTIFIER))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// mb-1z0m Round 4 — drift protection. If someone edits the
    /// `identifier` in `tauri.conf.json` without updating
    /// [`APP_IDENTIFIER`], every IPC will rejected with
    /// "state not managed" again because the pre-Builder bootstrap
    /// will land the DB at the wrong directory while the Tauri
    /// runtime still resolves the config-driven identifier. This
    /// test catches the mismatch before it ships.
    #[test]
    fn identifier_matches_tauri_conf() {
        let conf = include_str!("../tauri.conf.json");
        // Cheap substring match — avoids dragging serde_json into
        // the test surface for a one-line check.
        let needle = format!("\"identifier\": \"{APP_IDENTIFIER}\"");
        assert!(
            conf.contains(&needle),
            "APP_IDENTIFIER {APP_IDENTIFIER:?} not found in tauri.conf.json; \
             update one or the other (mb-1z0m Round 4 fix relies on the match)."
        );
    }

    /// mb-1z0m Round 4 — primary regression gate for the bug class.
    ///
    /// Round 3 instrumentation (`bd5a1f7`) confirmed AppState was
    /// being registered via `app.manage()` inside `.setup()` (both
    /// `post_manage_probe` AND `end_setup_probe` returned `true`),
    /// but the IPC `State<'_, AppStateHandle>` extractor at webview
    /// dispatch time STILL produced `"state not managed for field
    /// db on command insights_snapshot"`. Path A's fix (this commit)
    /// is to register AppState on the Tauri `Builder` BEFORE
    /// `.setup()` runs — the canonical Tauri 2 pattern that
    /// guarantees post-setup IPC handler visibility.
    ///
    /// This test asserts the property directly: a `Builder::manage`
    /// of `AppState` MUST be visible via `try_state::<AppStateHandle>`
    /// on the built App. If a future refactor reintroduces the
    /// pre-Round-4 bug (moves `.manage()` back inside `.setup()` or
    /// otherwise breaks builder-state propagation), this gate trips.
    ///
    /// Uses `tauri::test::mock_builder` (dev-dep `tauri/test`
    /// feature) so no live webview / message pump is required. The
    /// `try_state` call IS what `State<T>` extractors call under
    /// the hood — exercising it asserts the IPC-side property
    /// without needing to dispatch an actual IPC invocation.
    #[test]
    fn state_extractor_sees_builder_managed_app_state() {
        use crate::commands::{AppState, AppStateHandle};
        use rusqlite::Connection;
        use std::sync::{Arc, Mutex};
        use tauri::Manager;

        // Mirror the production bootstrap shape from `run()`:
        // open an in-memory sqlite (the only piece we substitute
        // for the on-disk DB), wrap in Arc<Mutex<>>, and hand to
        // AppState::new. Same constructor `run()` calls.
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        let shared = Arc::new(Mutex::new(conn));
        let app_state = AppState::new(Arc::clone(&shared));

        // Path A pattern: `.manage` on the Builder BEFORE any
        // `.setup()`. This is exactly what `run()` does at the top
        // of the builder chain.
        let app = tauri::test::mock_builder()
            .manage(app_state)
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app builds");

        // The smoking gun. `State<'_, AppStateHandle>` IPC
        // extractors resolve to `app.try_state::<AppStateHandle>()`
        // at dispatch time; if THIS returns None on a built app,
        // every state-using IPC will return
        // "state not managed for field ...". That's the bug we
        // chased for 4 rounds. With the Round 4 fix this MUST be
        // Some(_).
        assert!(
            app.try_state::<AppStateHandle>().is_some(),
            "Builder::manage(AppState) MUST be visible to \
             State<AppStateHandle> extractor at IPC dispatch time. \
             If this assertion fails, the pre-Round-4 bug has \
             been reintroduced — `app.manage()` inside `.setup()` \
             does NOT propagate reliably to post-setup IPC handlers \
             in Tauri 2.x; use `Builder::manage()` BEFORE `.setup()` \
             instead. See mb-1z0m + LESSONS 2026-06-04."
        );
    }
}
