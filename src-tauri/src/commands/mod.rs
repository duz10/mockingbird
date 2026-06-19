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
pub mod dictation;
pub mod dictionary;
pub mod insights;
pub mod kg;
pub mod learning;
pub mod legacy;
pub mod meetings;
pub mod modes;
// mb-mac-v1.4.6 (ADR 0061) — macOS permissions onboarding IPC.
pub mod permissions;
pub mod sessions;
pub mod settings;
pub mod system;
pub mod types;
// LR.0.B / mb-hiar (ADR 0055) — Unsplash key DPAPI surface.
pub mod unsplash;
pub mod vault;

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
        sessions::dictation_mark_edit_observed,
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
        // Phase 1C Wave 1C.1 — typed KG-settings IPC (ADR 0051).
        kg::kg_settings_get_all,
        kg::kg_settings_set,
        // Phase 1C Wave 1C.2 — failed-filings UX + manual retry (ADR 0051).
        kg::kg_list_failed_filings,
        kg::kg_requeue_failed,
        kg::kg_queue_status,
        // Phase 1C Wave 1C.3 — Dictations retrieval UX (ADR 0051).
        kg::kg_search_entries,
        kg::kg_list_entities,
        kg::kg_list_tags,
        kg::kg_entries_summary,
        // Phase 1C Wave 1C.4 — concept-modal drill-down (ADR 0051).
        kg::kg_entity_detail,
        kg::kg_tag_detail,
        // Phase 1D Wave 1D.2 — KG dashboard payload (ADR 0052).
        kg::kg_dashboard_snapshot,
        // Phase 1D Wave 1D.5 — Settings KG tab vocabularies + Obsidian
        // launcher (ADR 0052).
        kg::kg_vocabularies_get,
        kg::kg_launch_obsidian,
        // Learning
        learning::list_learning_runs,
        learning::trigger_learning_run,
        // System
        system::open_path,
        system::app_paths,
        system::list_installed_models,
        system::host_os,
        // macOS permissions onboarding (mb-mac-v1.4.6 / ADR 0061).
        permissions::mac_permission_statuses,
        permissions::mac_open_settings_pane,
        // mb-1z0m (Round 3) — JS→Rust IPC-outcome mirror.
        system::report_ipc_status,
        // mb-1z0m (Round 4) — React mount beacon (no state, no args).
        system::react_mounted,
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
        // ADR 0045 — programmatic dictation start/stop (mb-ddfx).
        dictation::dictation_start,
        dictation::dictation_stop,
        // ADR 0046 §3.2 — desktop audio-file import (mb-7vyz).
        dictation::dictation_import_file,
        // Phase 1D Wave 1D.3 (mb-0gt6) — KG capture surface (ADR 0052).
        dictation::dictation_start_kg_note,
        kg::kg_ingest_text_note,
        // Phase 1E Wave 1E.1 (mb-e16d) — KG vault subtree bootstrap (ADR 0053 D1).
        kg::kg_subtree_bootstrap,
        // Phase 1E hotfix (mb-43xw + new sibling for 1E.4) — on-demand
        // reconcile of `<vault>/Knowledge Graph/{Entries,History}` against
        // `sessions`. KG-toggle + vault-configured gated.
        kg::kg_reconcile_vault,
        kg::kg_reconcile_history,
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
        // Phase 10 Wave 3 — Summarization + Block CRUD + export (ADR 0040).
        activity::activity_regenerate_summary,
        activity::activity_list_blocks,
        activity::activity_block_rename,
        activity::activity_block_rewrite_abstract,
        activity::activity_block_delete,
        activity::activity_block_merge,
        activity::activity_block_split,
        activity::activity_export_markdown,
        activity::activity_copy_to_clipboard,
        activity::activity_render_work_report,
        // Phase 10 Wave 4 — Activity audio (ADR 0041).
        activity::activity_list_transcript_segments,
        // Phase 10 Wave 5 — Hardening (ADR 0042 + 0043 + 0044).
        activity::activity_exclusion_list,
        activity::activity_exclusion_validate,
        activity::activity_exclusion_upsert,
        activity::activity_exclusion_set_enabled,
        activity::activity_exclusion_delete,
        activity::activity_retention_get,
        activity::activity_retention_set,
        activity::activity_retention_sweep_now,
        activity::activity_export_pdf,
        // ADR 0046 Iter 2 / mb-lvzw + mb-vg3p — vault IPC.
        vault::vault_settings_get,
        vault::vault_settings_set,
        vault::vault_export_now,
        vault::vault_pick_directory,
        // ADR 0046 Iter 4 / mb-vg3p — runtime health card.
        vault::vault_runtime_status,
        vault::inbox_runtime_status,
        // ADR 0046 Iter 4 / mb-3xww — nested-vault guided dialog.
        vault::vault_check_path,
        vault::vault_ensure_dir,
        // LR.0.B / mb-hiar (ADR 0055) — Unsplash key DPAPI surface.
        // Three commands matching the existing SecretStore put/get/
        // delete trio so the JS side never has to know DPAPI exists.
        unsplash::unsplash_set_api_key,
        unsplash::unsplash_get_api_key,
        unsplash::unsplash_clear_api_key,
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
