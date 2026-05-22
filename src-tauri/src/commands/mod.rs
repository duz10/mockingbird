//! Tauri command surface — the IPC API the UI calls.
//!
//! Every UI page calls into one or more of these. The naming
//! convention is `noun_verb` (snake_case) and matches the JS-side
//! names exactly. Adding a new command means:
//!
//!   1. Add a `#[tauri::command]` fn in the appropriate sub-module.
//!   2. Register it in [`register`] below.
//!   3. Add the typed wrapper + fixture in `ui/src/lib/tauri.ts`.
//!   4. Add the response shape in `ui/src/lib/types.ts`.
//!
//! ## Error model
//!
//! Commands return `Result<T, String>` because Tauri serializes the
//! error to JS as a string. We funnel through `AppError::to_string()`
//! at the boundary so the JS side gets a friendly message.

pub mod active_mode;
pub mod activity;
pub mod dictionary;
pub mod insights;
pub mod learning;
pub mod legacy;
pub mod meetings;
pub mod modes;
pub mod sessions;
pub mod settings;
pub mod system;
pub mod types;

use std::sync::{Arc, Mutex};

use rusqlite::Connection;
use tauri::Builder;

pub use self::state::{AppState, AppStateHandle};

/// Managed state held by Tauri. Wraps a shared `Arc<Mutex<Connection>>`
/// because `Connection` is `Send` but not `Sync`. The same Arc is
/// shared with the dictation runtime so IPC handlers and the
/// orchestrator hit the same WAL-mode DB.
mod state {
    use super::{Arc, Connection, Mutex};

    /// Tauri-managed state.
    pub struct AppState {
        /// Shared, locked connection. `Arc` clone is held by the
        /// dictation runtime (Phase 3 Wave 4.5).
        pub db: Arc<Mutex<Connection>>,
    }

    /// Short alias for the Tauri-state generic the command modules use.
    pub type AppStateHandle = AppState;

    impl AppState {
        /// Build with a pre-shared connection handle.
        pub fn new(db: Arc<Mutex<Connection>>) -> Self {
            Self { db }
        }
    }
}

/// Register every command with the Tauri builder. Call this once
/// from `lib.rs::run()`.
pub fn register<R: tauri::Runtime>(builder: Builder<R>) -> Builder<R> {
    builder.invoke_handler(tauri::generate_handler![
        // Legacy (Phase 1) typed-setting bridge.
        legacy::get_setting,
        legacy::set_setting,
        legacy::fts_smoke_test,
        // Insights
        insights::insights_snapshot,
        // Sessions
        sessions::list_sessions,
        sessions::get_session_detail,
        sessions::search_transcripts,
        sessions::delete_session,
        sessions::mark_session_as_example,
        sessions::report_correction,
        sessions::dictation_run_llm_pass,
        // Dictionary
        dictionary::list_dictionary,
        dictionary::upsert_dictionary_entry,
        dictionary::delete_dictionary_entry,
        // Modes
        modes::list_modes,
        modes::update_mode,
        active_mode::get_active_mode,
        active_mode::set_active_mode,
        // Settings (UI panel)
        settings::get_settings,
        settings::update_setting,
        // Phase MC Wave 5 — typed meeting-settings IPC.
        settings::meeting_settings_get_all,
        settings::meeting_settings_set,
        // Learning
        learning::list_learning_runs,
        learning::trigger_learning_run,
        // System
        system::open_path,
        system::app_paths,
        system::list_installed_models,
        // Meetings (Phase MC Wave 4 — 10 commands per Section MC.6;
        // Phase MC Wave 5 — +2 for the tray pause-toggle wiring).
        meetings::meeting_probe_sources,
        meetings::meeting_start,
        meetings::meeting_stop,
        meetings::meeting_cancel,
        meetings::meeting_overlay_hide,
        meetings::meeting_debug_listener_ping,
        meetings::list_meetings,
        meetings::get_meeting_detail,
        meetings::delete_meeting,
        meetings::meeting_rename,
        meetings::search_meeting_transcripts,
        meetings::meeting_export_markdown,
        meetings::meeting_copy_to_clipboard,
        meetings::meeting_run_llm_pass,
        meetings::meeting_set_paused,
        meetings::meeting_is_paused,
        // Phase 10 Wave 1A — Command Center IPC (ADR 0037).
        crate::command_center::ipc::cc_open_via_tray,
        crate::command_center::ipc::cc_dismiss,
        crate::command_center::ipc::cc_pick_mode,
        crate::command_center::ipc::cc_stop_active_session,
        crate::command_center::ipc::cc_update_session,
        crate::command_center::ipc::cc_get_state,
        // Phase 10 Wave 1B — Activity capture IPC (ADR 0036).
        activity::activity_start,
        activity::activity_pause,
        activity::activity_resume,
        activity::activity_stop,
        activity::activity_runtime_snapshot,
        activity::activity_list_sessions,
        activity::activity_get_session_detail,
        activity::activity_delete_session,
    ])
}

/// Helper: lock the Tauri-managed DB. Returns a UI-friendly string
/// error when the lock is poisoned (which should only happen if a
/// Rust-side panic crossed a thread boundary).
pub(crate) fn lock_db<'a>(
    state: &'a tauri::State<'_, AppStateHandle>,
) -> Result<std::sync::MutexGuard<'a, Connection>, String> {
    state
        .db
        .lock()
        .map_err(|_| "database mutex poisoned — restart the app".to_string())
}

/// Funnel rusqlite + AppError into the String shape Tauri serializes
/// to JS. Use as `.map_err(into_err)`.
pub(crate) fn into_err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}
