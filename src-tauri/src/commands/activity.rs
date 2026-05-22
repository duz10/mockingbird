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

use tauri::State;

use crate::activity::{
    persist::{
        delete_session, get_session_detail, list_sessions, ActivitySessionDetail,
        ActivitySessionRow,
    },
    runtime::ActivityCaptureRuntime,
};

use super::{into_err, lock_db, AppStateHandle};

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
