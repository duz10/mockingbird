//! Tauri command surface for activity capture (Phase 10 Wave 1B).
//!
//! Commands here are the **only** sanctioned way the UI reaches the
//! activity subsystem. The Command Center invokes start/stop via
//! its in-process Rust path (`command_center::dispatch_start`); the
//! list/detail/delete commands serve the Activity + ActivityDetail
//! pages.
//!
//! `meeting:state` / `dictation:state`-style event emission for the
//! activity subsystem is deferred to Wave 2 — Wave 1B's UI polls
//! `activity_list_sessions` on focus instead. Cheaper than wiring a
//! whole event channel for a skeleton.

use std::path::PathBuf;
use std::sync::Arc;

use tauri::State;

use crate::activity::{
    blocks_persist::{delete_block, list_blocks, rename_block, rewrite_abstract, ActivityBlockRow},
    export::{copy_to_clipboard, export_to_file, regenerate_summary, render_work_report},
    persist::{
        delete_session, get_session_detail, list_sessions, ActivitySessionDetail,
        ActivitySessionRow,
    },
    runtime::ActivityCaptureRuntime,
};

use super::{into_err, lock_db, AppStateHandle};

/// Best-effort wall-clock for the persistence layer. Unit ms.
fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Hard cap on the list-API page size. Matches the existing dictation
/// `list_sessions` ceiling — keeps the UI from accidentally hauling
/// 50_000 rows over IPC.
const LIST_LIMIT_MAX: i64 = 500;

/// Start an activity-capture session.
///
/// Idempotent: calling this from a non-Idle state is a no-op (the
/// FSM enforces). Returns the session id on success — that's either
/// the newly-created session OR the already-running session.
#[tauri::command]
pub fn activity_start(
    runtime: State<'_, ActivityCaptureRuntime>,
) -> Result<Option<String>, String> {
    runtime.start().map_err(into_err)?;
    Ok(runtime.current_session_id())
}

/// Pause the active session.
#[tauri::command]
pub fn activity_pause(runtime: State<'_, ActivityCaptureRuntime>) -> Result<(), String> {
    runtime.pause().map_err(into_err)
}

/// Resume the paused session.
#[tauri::command]
pub fn activity_resume(runtime: State<'_, ActivityCaptureRuntime>) -> Result<(), String> {
    runtime.resume().map_err(into_err)
}

/// Stop the active session. Idempotent.
#[tauri::command]
pub fn activity_stop(runtime: State<'_, ActivityCaptureRuntime>) -> Result<(), String> {
    runtime.stop().map_err(into_err)
}

/// Snapshot — returns the in-memory lifecycle state + session id.
/// The UI uses this when CommandCenter.tsx mounts mid-session.
#[tauri::command]
pub fn activity_runtime_snapshot(
    runtime: State<'_, ActivityCaptureRuntime>,
) -> Result<ActivityRuntimeSnapshot, String> {
    Ok(ActivityRuntimeSnapshot {
        lifecycle: runtime.lifecycle_state().to_string(),
        current_session_id: runtime.current_session_id(),
    })
}

/// Shape of the runtime snapshot returned to JS.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityRuntimeSnapshot {
    /// One of `"idle" | "active" | "paused" | "stopped"`.
    pub lifecycle: String,
    /// Present when `lifecycle` is `active` or `paused`.
    pub current_session_id: Option<String>,
}

/// List recent activity sessions, newest first.
///
/// `limit` is clamped to [`LIST_LIMIT_MAX`]. The UI's Activity page
/// renders only a few dozen rows at a time; larger requests are
/// programmer error and we don't want to be a vector for "fetch
/// 100k rows" misuse.
#[tauri::command]
pub fn activity_list_sessions(
    state: State<'_, AppStateHandle>,
    limit: i64,
) -> Result<Vec<ActivitySessionRow>, String> {
    let limit = limit.clamp(1, LIST_LIMIT_MAX);
    let conn = lock_db(&state)?;
    list_sessions(&conn, limit).map_err(into_err)
}

/// Load one session's detail view: session row + all events
/// chronologically.
#[tauri::command]
pub fn activity_get_session_detail(
    state: State<'_, AppStateHandle>,
    session_id: String,
) -> Result<Option<ActivitySessionDetail>, String> {
    let conn = lock_db(&state)?;
    get_session_detail(&conn, &session_id).map_err(into_err)
}

/// Delete one session (cascades to its events via the FK in
/// migration 012).
#[tauri::command]
pub fn activity_delete_session(
    state: State<'_, AppStateHandle>,
    session_id: String,
) -> Result<(), String> {
    let conn = lock_db(&state)?;
    delete_session(&conn, &session_id).map_err(into_err)
}

// ===========================================================================
// Phase 10 Wave 3 — summarization + Block CRUD + export.
// ===========================================================================

/// Run the full Wave-3 pipeline against an existing session: normalize
/// events → group into Blocks → abstract each Block (LLM or template)
/// → assemble Markdown → write to `summary_markdown`. Returns the
/// resulting Markdown body.
///
/// User-edited Block rows are preserved across re-runs (see
/// `export::abstract_blocks_respecting_user_edits`).
#[tauri::command]
pub fn activity_regenerate_summary(
    state: State<'_, AppStateHandle>,
    session_id: String,
) -> Result<String, String> {
    let db: Arc<_> = state.db.clone();
    let mut conn = db
        .lock()
        .map_err(|_| "database mutex poisoned \u{2014} restart the app".to_string())?;
    regenerate_summary(&mut conn, &session_id).map_err(into_err)
}

/// List the Blocks for one session, chronologically.
#[tauri::command]
pub fn activity_list_blocks(
    state: State<'_, AppStateHandle>,
    session_id: String,
) -> Result<Vec<ActivityBlockRow>, String> {
    let conn = lock_db(&state)?;
    list_blocks(&conn, &session_id).map_err(into_err)
}

/// Set a Block's user-facing label.
#[tauri::command]
pub fn activity_block_rename(
    state: State<'_, AppStateHandle>,
    block_id: String,
    new_label: Option<String>,
) -> Result<(), String> {
    let conn = lock_db(&state)?;
    rename_block(&conn, &block_id, new_label.as_deref(), now_ms()).map_err(into_err)
}

/// Overwrite a Block's generated_abstract with user text.
#[tauri::command]
pub fn activity_block_rewrite_abstract(
    state: State<'_, AppStateHandle>,
    block_id: String,
    text: String,
) -> Result<(), String> {
    let conn = lock_db(&state)?;
    rewrite_abstract(&conn, &block_id, &text, now_ms()).map_err(into_err)
}

/// Delete a single Block. The session's stored summary becomes
/// stale; caller is expected to re-run regenerate_summary if they
/// want the Markdown refreshed.
#[tauri::command]
pub fn activity_block_delete(
    state: State<'_, AppStateHandle>,
    block_id: String,
) -> Result<(), String> {
    let conn = lock_db(&state)?;
    delete_block(&conn, &block_id).map_err(into_err)
}

/// Merge `source_ids` into `target_id`. The target absorbs the
/// sources' time range + provenance; the sources are deleted.
#[tauri::command]
pub fn activity_block_merge(
    state: State<'_, AppStateHandle>,
    target_id: String,
    source_ids: Vec<String>,
) -> Result<(), String> {
    let db: Arc<_> = state.db.clone();
    let mut conn = db
        .lock()
        .map_err(|_| "database mutex poisoned \u{2014} restart the app".to_string())?;
    crate::activity::blocks_persist::merge_blocks(&mut conn, &target_id, &source_ids, now_ms())
        .map_err(into_err)
}

/// Split a Block at `split_at_ms`. Returns the new (right-half)
/// Block id.
#[tauri::command]
pub fn activity_block_split(
    state: State<'_, AppStateHandle>,
    block_id: String,
    split_at_ms: i64,
) -> Result<String, String> {
    let db: Arc<_> = state.db.clone();
    let mut conn = db
        .lock()
        .map_err(|_| "database mutex poisoned \u{2014} restart the app".to_string())?;
    crate::activity::blocks_persist::split_block(&mut conn, &block_id, split_at_ms, now_ms())
        .map_err(into_err)
}

/// Write the stored summary_markdown to a destination path.
#[tauri::command]
pub fn activity_export_markdown(
    state: State<'_, AppStateHandle>,
    session_id: String,
    dest_path: String,
) -> Result<(), String> {
    let conn = lock_db(&state)?;
    export_to_file(&conn, &session_id, &PathBuf::from(dest_path)).map_err(into_err)
}

/// Copy the stored summary_markdown to the system clipboard.
#[tauri::command]
pub fn activity_copy_to_clipboard(
    state: State<'_, AppStateHandle>,
    session_id: String,
) -> Result<(), String> {
    let conn = lock_db(&state)?;
    copy_to_clipboard(&conn, &session_id).map_err(into_err)
}

/// Render the work-report Markdown variant (bullets of Block summaries
/// only) on demand. Does NOT re-run the LLM or write to the DB.
#[tauri::command]
pub fn activity_render_work_report(
    state: State<'_, AppStateHandle>,
    session_id: String,
) -> Result<String, String> {
    let conn = lock_db(&state)?;
    render_work_report(&conn, &session_id).map_err(into_err)
}
