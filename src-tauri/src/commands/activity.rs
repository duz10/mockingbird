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
    exclusion::{
        self as exclusion_repo, list_all as list_exclusion_rules, set_enabled, upsert_user_rule,
        validate, ExclusionRule, RuleKind,
    },
    export::{copy_to_clipboard, export_to_file, regenerate_summary, render_work_report},
    pdf_export::{render_session_pdf, PdfMode},
    persist::{
        delete_session, get_session_detail, list_sessions, ActivitySessionDetail,
        ActivitySessionRow,
    },
    retention::{self, RetentionPolicy, SweepResult},
    runtime::ActivityCaptureRuntime,
    segments_persist::{list_segments, ActivityTranscriptSegmentRow},
};
use crate::settings::{model::SettingKey, Settings};

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
///
/// `with_audio` controls the Wave-4 audio toggle for THIS session.
/// When `None`, the function reads the `activity_audio_enabled`
/// setting (default `false`). UI surfaces (Command Center, Settings)
/// pass an explicit boolean when they know what the user picked;
/// keyboard-shortcut / programmatic callers leave it `None` to honor
/// the persisted default.
#[tauri::command]
pub fn activity_start(
    state: State<'_, AppStateHandle>,
    runtime: State<'_, ActivityCaptureRuntime>,
    with_audio: Option<bool>,
) -> Result<Option<String>, String> {
    let resolved = match with_audio {
        Some(v) => v,
        None => {
            // Fall back to the persisted setting. Read in its own
            // short critical section so we don't hold the DB lock
            // across the FSM step.
            let conn = lock_db(&state)?;
            Settings::new(&conn)
                .get::<bool>(SettingKey::ActivityAudioEnabled)
                .map_err(into_err)?
        }
    };
    runtime.start_with_audio(resolved).map_err(into_err)?;
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

// ===========================================================================
// Phase 10 Wave 4 — audio transcript surface.
// ===========================================================================

/// List all transcript segments for one session, chronologically.
/// Audio-disabled sessions return an empty Vec. The Activity detail
/// page renders these as a side-by-side timeline next to the visual
/// Blocks; an empty list means "no audio captured".
#[tauri::command]
pub fn activity_list_transcript_segments(
    state: State<'_, AppStateHandle>,
    session_id: String,
) -> Result<Vec<ActivityTranscriptSegmentRow>, String> {
    let conn = lock_db(&state)?;
    list_segments(&conn, &session_id).map_err(into_err)
}

// ===========================================================================
// Phase 10 Wave 5 — Hardening IPC (ADR 0042 + 0043 + 0044).
// ===========================================================================

/// List ALL exclusion rules (built-ins + user-created, enabled or
/// not). The Settings UI renders the full set with toggles + delete
/// affordances per ADR 0043 §UI surface.
#[tauri::command]
pub fn activity_exclusion_list(
    state: State<'_, AppStateHandle>,
) -> Result<Vec<ExclusionRule>, String> {
    let conn = lock_db(&state)?;
    list_exclusion_rules(&conn).map_err(into_err)
}

/// Validate a `(kind, pattern)` pair without persisting. Used by the
/// Settings UI to pre-flight a save (catches invalid regex before
/// the round-trip).
#[tauri::command]
pub fn activity_exclusion_validate(kind: String, pattern: String) -> Result<(), String> {
    let kind = RuleKind::from_db_str(&kind)
        .ok_or_else(|| format!("unknown exclusion rule kind: {kind:?}"))?;
    validate(kind, &pattern).map_err(into_err)
}

/// Upsert a user-created rule. Pass `id = None` to INSERT; pass an
/// existing user-rule id to UPDATE. Built-in rules cannot be modified
/// via this surface — use [`activity_exclusion_set_enabled`].
#[tauri::command]
pub fn activity_exclusion_upsert(
    state: State<'_, AppStateHandle>,
    runtime: State<'_, ActivityCaptureRuntime>,
    id: Option<String>,
    kind: String,
    pattern: String,
    enabled: bool,
    note: Option<String>,
) -> Result<String, String> {
    let kind = RuleKind::from_db_str(&kind)
        .ok_or_else(|| format!("unknown exclusion rule kind: {kind:?}"))?;
    validate(kind, &pattern).map_err(into_err)?;
    let new_id = {
        let conn = lock_db(&state)?;
        upsert_user_rule(
            &conn,
            id.as_deref(),
            kind,
            &pattern,
            enabled,
            note.as_deref(),
        )
        .map_err(into_err)?
    };
    runtime.reload_exclusion_rules().map_err(into_err)?;
    Ok(new_id)
}

/// Flip the `enabled` flag on any rule (built-in or user). The only
/// surface that can act on built-ins.
#[tauri::command]
pub fn activity_exclusion_set_enabled(
    state: State<'_, AppStateHandle>,
    runtime: State<'_, ActivityCaptureRuntime>,
    id: String,
    enabled: bool,
) -> Result<(), String> {
    {
        let conn = lock_db(&state)?;
        set_enabled(&conn, &id, enabled).map_err(into_err)?;
    }
    runtime.reload_exclusion_rules().map_err(into_err)?;
    Ok(())
}

/// Delete a user-created rule. Built-ins return an error.
#[tauri::command]
pub fn activity_exclusion_delete(
    state: State<'_, AppStateHandle>,
    runtime: State<'_, ActivityCaptureRuntime>,
    id: String,
) -> Result<(), String> {
    {
        let conn = lock_db(&state)?;
        exclusion_repo::delete_user_rule(&conn, &id).map_err(into_err)?;
    }
    runtime.reload_exclusion_rules().map_err(into_err)?;
    Ok(())
}

/// Read the current retention policy (TTLs in days + last sweep ts).
#[tauri::command]
pub fn activity_retention_get(state: State<'_, AppStateHandle>) -> Result<RetentionPolicy, String> {
    let conn = lock_db(&state)?;
    retention::load(&conn).map_err(into_err)
}

/// Persist the user-tunable TTL fields. Does NOT sweep — call
/// [`activity_retention_sweep_now`] to trigger an immediate sweep.
#[tauri::command]
pub fn activity_retention_set(
    state: State<'_, AppStateHandle>,
    events_days: i64,
    segments_days: i64,
    blocks_days: i64,
) -> Result<(), String> {
    let conn = lock_db(&state)?;
    retention::save_user_policy(&conn, events_days, segments_days, blocks_days).map_err(into_err)
}

/// Run the retention sweep once on demand. Returns row-count summary.
#[tauri::command]
pub fn activity_retention_sweep_now(
    state: State<'_, AppStateHandle>,
) -> Result<SweepResult, String> {
    let mut conn = lock_db(&state)?;
    retention::sweep_once(&mut conn, now_ms()).map_err(into_err)
}

/// Render the per-session PDF and write to `dest_path`.
/// `mode` is `"full"` or `"work_report"`.
#[tauri::command]
pub fn activity_export_pdf(
    state: State<'_, AppStateHandle>,
    session_id: String,
    dest_path: String,
    mode: String,
) -> Result<(), String> {
    let mode = PdfMode::parse(&mode).map_err(into_err)?;
    let bytes = {
        let mut conn = lock_db(&state)?;
        render_session_pdf(&mut conn, &session_id, mode).map_err(into_err)?
    };
    std::fs::write(PathBuf::from(&dest_path), &bytes)
        .map_err(|e| format!("write pdf to {dest_path}: {e}"))?;
    Ok(())
}
