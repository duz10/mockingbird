//! IPC commands for the meeting-capture subsystem (Section MC.6).
//!
//! The 10 `#[tauri::command]` fns exposed here are the only surface
//! the UI uses to drive meetings. They split into three groups:
//!
//! ## Lifecycle (drives [`MeetingRuntimeShared`])
//!   - `meeting_probe_sources` — what's capturable on this box
//!   - `meeting_start { source }` — begin capture (idempotent)
//!   - `meeting_stop  { uuid }`  — stop + persist + emit done
//!
//! ## History (reads via [`meetings::repo`])
//!   - `list_meetings { limit?, offset? }`
//!   - `get_meeting_detail { uuid }`
//!   - `delete_meeting { uuid }`
//!   - `search_meeting_transcripts { query }`
//!
//! ## Export (read + render via [`meetings::export`] + [`meetings::clipboard`])
//!   - `meeting_export_markdown   { uuid, dest_path?, include_llm_pass? }`
//!   - `meeting_copy_to_clipboard { uuid, include_llm_pass? }`
//!   - `meeting_run_llm_pass      { uuid, prompt, model_id? }`
//!     — IN-MEMORY cache only; **never persisted**
//!     (judge `mc-no-llm-in-critical-path`).
//!
//! ## Wave 4 deviation from the plan
//!
//! Section MC.6 says `meeting_export_markdown` "writes a markdown file
//! to a user-chosen path via `tauri::api::dialog`". That API moved to
//! `tauri-plugin-dialog` in Tauri 2, and pulling in the dialog plugin
//! is a non-trivial dep + capabilities change for a single command.
//!
//! Wave 4 punts the picker to the UI: the command accepts an optional
//! `dest_path` parameter; the UI uses an HTML `<input type=file>` (or
//! the Wave 5 dialog plugin) to obtain it. When `dest_path` is
//! `None`, the command writes to a default sticky location at
//! `<app_data>/meetings/exports/<uuid>.md`. Either way the response
//! is `{ path: string }` — same contract as the plan.
//!
//! Wave 5 will revisit if we add the dialog plugin globally.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::commands::{into_err, lock_db, AppStateHandle};
use crate::meetings::capture::{probe_sources, MeetingSource, MeetingSourceProbe};
use crate::meetings::clipboard::copy_text_one_shot;
use crate::meetings::export::render_markdown;
use crate::meetings::llm_pass::{run_llm_pass, LlmPassPrompt, LlmPassRequest};
use crate::meetings::repo::{
    delete_meeting as repo_delete_meeting, list_meetings as repo_list_meetings,
    load_meeting_detail, search_meetings, MeetingDetail, MeetingMatch, MeetingSummary,
};
use crate::meetings::runtime::MeetingRuntimeShared;

// --------------------------------------------------------------------
// Wire types — `serde(rename_all = "camelCase")` matches the rest of
// the IPC surface (see `commands::types::*`).
// --------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingStartResult {
    pub uuid: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingSourceProbeDto {
    pub mic_available: bool,
    pub system_available: bool,
}

impl From<MeetingSourceProbe> for MeetingSourceProbeDto {
    fn from(p: MeetingSourceProbe) -> Self {
        Self {
            mic_available: p.mic_available,
            system_available: p.system_available,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingExportResult {
    pub path: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IncludeLlmPass {
    pub id: String,
}

/// Wire shape for the `meeting_run_llm_pass.prompt_id` argument.
/// Matches the plan's Section MC.6 contract:
///   `prompt_id: string | { custom: string }`.
/// `#[serde(untagged)]` lets serde distinguish the two cases by JSON
/// shape — a bare string is a built-in name, an object with a
/// `custom` field is a verbatim body.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum LlmPassPromptArg {
    /// Built-in prompt by short name (`"summary"`, `"action_items"`,
    /// `"cleaner_punctuation"`). Mirrors `LlmPassPrompt::BuiltIn`.
    BuiltIn(String),
    /// User-supplied body, used verbatim.
    Custom { custom: String },
}

impl From<LlmPassPromptArg> for LlmPassPrompt {
    fn from(arg: LlmPassPromptArg) -> Self {
        match arg {
            LlmPassPromptArg::BuiltIn(name) => {
                // `LlmPassPrompt::BuiltIn` wants `&'static str` — we
                // canonicalize on the three known names here so the
                // resolver in `llm_pass::resolve_prompt_body` can match.
                let canon: &'static str = match name.as_str() {
                    "summary" => "summary",
                    "action_items" => "action_items",
                    "cleaner_punctuation" => "cleaner_punctuation",
                    _ => "__unknown__",
                };
                LlmPassPrompt::BuiltIn(canon)
            }
            LlmPassPromptArg::Custom { custom } => LlmPassPrompt::Custom(custom),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmPassResultDto {
    pub id: String,
    pub text: String,
    pub latency_ms: u64,
}

// --------------------------------------------------------------------
// Lifecycle commands
//
// LLM-pass cache lives inside `MeetingRuntimeShared::llm_pass_cache`
// (an `Arc<Mutex<HashMap<String, String>>>`). Outside the DB by
// design (judge `mc-no-llm-in-critical-path`): filled by
// `meeting_run_llm_pass`, drained by export/copy via
// `IncludeLlmPass { id }`, evicted on app shutdown. Single source of
// truth keeps the boundary clean — we don't register a separate
// cache state.
// --------------------------------------------------------------------

/// What sources are usable on this machine right now. Pure probe —
/// doesn't open any streams, doesn't touch state. Safe to call on
/// every overlay-open.
#[tauri::command]
pub fn meeting_probe_sources() -> Result<MeetingSourceProbeDto, String> {
    probe_sources()
        .map(MeetingSourceProbeDto::from)
        .map_err(into_err)
}

/// Begin a meeting capture. Idempotent: if a meeting is already in
/// flight, returns its uuid + emits `meeting:state=warn-already-
/// running` on the overlay channel.
#[tauri::command]
pub fn meeting_start(
    rt: State<'_, MeetingRuntimeShared>,
    source: String,
) -> Result<MeetingStartResult, String> {
    let src = MeetingSource::from_db_str(&source)
        .ok_or_else(|| format!("unknown meeting source: {source:?} (want mic|system|both)"))?;
    let uuid = rt.start_meeting(src).map_err(into_err)?;
    Ok(MeetingStartResult { uuid })
}

/// Stop a meeting capture. Drives the full
/// capture→long-form-stt→formatter→merge→persist pipeline on the
/// caller's thread (Tauri's IPC worker), then emits `meeting:state=
/// done` + `meetings:session-saved` on success.
///
/// Errors if `uuid` doesn't match the live meeting (caller passed a
/// stale handle); the live meeting stays in flight in that case so a
/// retry with the correct uuid still works.
#[tauri::command]
pub fn meeting_stop(rt: State<'_, MeetingRuntimeShared>, uuid: String) -> Result<(), String> {
    rt.stop_meeting(&uuid).map_err(into_err)
}

// --------------------------------------------------------------------
// History commands
// --------------------------------------------------------------------

const LIST_DEFAULT_LIMIT: i64 = 200;
const LIST_MAX_LIMIT: i64 = 1_000;
const SEARCH_LIMIT: i64 = 50;

#[tauri::command]
pub fn list_meetings(
    db: State<'_, AppStateHandle>,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<Vec<MeetingSummary>, String> {
    let limit = limit.unwrap_or(LIST_DEFAULT_LIMIT).clamp(1, LIST_MAX_LIMIT);
    let offset = offset.unwrap_or(0).max(0);
    let conn = lock_db(&db)?;
    repo_list_meetings(&conn, limit, offset).map_err(into_err)
}

#[tauri::command]
pub fn get_meeting_detail(
    db: State<'_, AppStateHandle>,
    uuid: String,
) -> Result<MeetingDetail, String> {
    let conn = lock_db(&db)?;
    let detail = load_meeting_detail(&conn, &uuid)
        .map_err(into_err)?
        .ok_or_else(|| format!("meeting not found: {uuid}"))?;
    Ok(detail)
}

#[tauri::command]
pub fn delete_meeting(db: State<'_, AppStateHandle>, uuid: String) -> Result<(), String> {
    let conn = lock_db(&db)?;
    let _existed = repo_delete_meeting(&conn, &uuid).map_err(into_err)?;
    // Idempotent per repo contract — a stale "Delete" click on a row
    // that already vanished is not an error.
    Ok(())
}

#[tauri::command]
pub fn search_meeting_transcripts(
    db: State<'_, AppStateHandle>,
    query: String,
) -> Result<Vec<MeetingMatch>, String> {
    let conn = lock_db(&db)?;
    search_meetings(&conn, &query, SEARCH_LIMIT).map_err(into_err)
}

// --------------------------------------------------------------------
// Export commands
// --------------------------------------------------------------------

/// Render the meeting to Markdown and write it to disk.
///
/// `dest_path` is optional; when `None` the file lands at
/// `<app_data>/Mockingbird/meetings/exports/<uuid>.md`. See the
/// module-level "Wave 4 deviation" note for why the picker dialog is
/// UI-side.
///
/// `include_llm_pass` looks up the cached LLM-pass text by id; if
/// the id isn't in the cache we error rather than silently producing
/// a "no LLM pass" markdown (the user explicitly asked for it).
#[tauri::command]
pub fn meeting_export_markdown(
    db: State<'_, AppStateHandle>,
    rt: State<'_, MeetingRuntimeShared>,
    uuid: String,
    dest_path: Option<String>,
    include_llm_pass: Option<IncludeLlmPass>,
) -> Result<MeetingExportResult, String> {
    let body = render_for_export(&db, &rt, &uuid, include_llm_pass.as_ref())?;
    let path = resolve_export_path(dest_path.as_deref(), &uuid)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create export dir {parent:?}: {e}"))?;
    }
    std::fs::write(&path, body.as_bytes()).map_err(|e| format!("write export {path:?}: {e}"))?;
    Ok(MeetingExportResult {
        path: path.display().to_string(),
    })
}

/// Render the meeting to Markdown and copy it to the clipboard (one-
/// shot — clipboard ownership is left to the OS after the call).
/// Same `include_llm_pass` semantics as the export command.
#[tauri::command]
pub fn meeting_copy_to_clipboard(
    db: State<'_, AppStateHandle>,
    rt: State<'_, MeetingRuntimeShared>,
    uuid: String,
    include_llm_pass: Option<IncludeLlmPass>,
) -> Result<(), String> {
    let body = render_for_export(&db, &rt, &uuid, include_llm_pass.as_ref())?;
    copy_text_one_shot(&body).map_err(into_err)
}

/// Run one optional LLM-pass against a persisted meeting's formatted
/// transcript. Result is cached in-memory under a fresh uuid; the UI
/// then passes that id to the export commands via
/// `IncludeLlmPass { id }`.
///
/// **NOT persisted to the DB** — the cache is process-local and dies
/// on app shutdown. This is the binding contract for judge
/// `mc-no-llm-in-critical-path`.
#[tauri::command]
pub fn meeting_run_llm_pass(
    db: State<'_, AppStateHandle>,
    rt: State<'_, MeetingRuntimeShared>,
    uuid: String,
    prompt_id: LlmPassPromptArg,
    model_id: Option<String>,
) -> Result<LlmPassResultDto, String> {
    let transcript_text = {
        let conn = lock_db(&db)?;
        let detail = load_meeting_detail(&conn, &uuid)
            .map_err(into_err)?
            .ok_or_else(|| format!("meeting not found: {uuid}"))?;
        transcript_for_llm_pass(&detail)
    };
    let req = LlmPassRequest {
        meeting_uuid: uuid,
        prompt: prompt_id.into(),
        model_id,
    };
    let result = run_llm_pass(&req, &transcript_text).map_err(into_err)?;
    // Cache for later export. Drop the lock immediately.
    {
        let mut guard = rt
            .llm_pass_cache
            .lock()
            .map_err(|_| "llm-pass cache mutex poisoned".to_string())?;
        guard.insert(result.id.clone(), result.text.clone());
    }
    Ok(LlmPassResultDto {
        id: result.id,
        text: result.text,
        latency_ms: result.latency_ms,
    })
}

// --------------------------------------------------------------------
// Internal helpers
// --------------------------------------------------------------------

/// Shared between export + copy-to-clipboard: load the detail, look
/// up the optional LLM-pass text, and render the markdown body.
fn render_for_export(
    db: &State<'_, AppStateHandle>,
    rt: &State<'_, MeetingRuntimeShared>,
    uuid: &str,
    include_llm_pass: Option<&IncludeLlmPass>,
) -> Result<String, String> {
    let detail = {
        let conn = lock_db(db)?;
        load_meeting_detail(&conn, uuid)
            .map_err(into_err)?
            .ok_or_else(|| format!("meeting not found: {uuid}"))?
    };
    let llm_text = match include_llm_pass {
        None => None,
        Some(req) => {
            let guard = rt
                .llm_pass_cache
                .lock()
                .map_err(|_| "llm-pass cache mutex poisoned".to_string())?;
            Some(guard.get(&req.id).cloned().ok_or_else(|| {
                format!(
                    "llm-pass id {:?} not in cache (re-run meeting_run_llm_pass)",
                    req.id
                )
            })?)
        }
    };
    render_markdown(&detail, llm_text.as_deref()).map_err(into_err)
}

/// Prefer the merged channel (best for LLM passes), fall back to mic
/// then system. Empty transcript is allowed — the LLM will just say
/// "nothing to summarize" and that's fine.
fn transcript_for_llm_pass(detail: &MeetingDetail) -> String {
    detail
        .formatted_merged
        .as_deref()
        .or(detail.formatted_mic.as_deref())
        .or(detail.formatted_sys.as_deref())
        .unwrap_or("")
        .to_string()
}

fn resolve_export_path(dest_path: Option<&str>, uuid: &str) -> Result<PathBuf, String> {
    if let Some(p) = dest_path {
        return Ok(PathBuf::from(p));
    }
    let appdata = std::env::var("APPDATA").map_err(|_| "APPDATA env var not set".to_string())?;
    Ok(PathBuf::from(appdata)
        .join("Mockingbird")
        .join("meetings")
        .join("exports")
        .join(format!("{uuid}.md")))
}

// --------------------------------------------------------------------
// Tests
// --------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn detail_with(mic: Option<&str>, sys: Option<&str>, merged: Option<&str>) -> MeetingDetail {
        use crate::meetings::persist::MeetingStatus;
        MeetingDetail {
            uuid: "u".into(),
            title: None,
            started_at: "2026-05-20T00:00:00Z".into(),
            ended_at: "2026-05-20T00:01:00Z".into(),
            status: MeetingStatus::Complete,
            error_message: None,
            source: MeetingSource::Both,
            total_duration_ms: 60_000,
            mic_duration_ms: Some(60_000),
            sys_duration_ms: Some(60_000),
            formatter_version: "mc-v1".into(),
            whisper_model_id: "test".into(),
            formatted_mic: mic.map(String::from),
            formatted_sys: sys.map(String::from),
            formatted_merged: merged.map(String::from),
        }
    }

    #[test]
    fn transcript_for_llm_pass_prefers_merged() {
        let d = detail_with(Some("mic"), Some("sys"), Some("merged"));
        assert_eq!(transcript_for_llm_pass(&d), "merged");
    }

    #[test]
    fn transcript_for_llm_pass_falls_back_to_mic_when_no_merged() {
        let d = detail_with(Some("mic-only"), None, None);
        assert_eq!(transcript_for_llm_pass(&d), "mic-only");
    }

    #[test]
    fn transcript_for_llm_pass_falls_back_to_sys_when_no_mic_or_merged() {
        let d = detail_with(None, Some("sys-only"), None);
        assert_eq!(transcript_for_llm_pass(&d), "sys-only");
    }

    #[test]
    fn transcript_for_llm_pass_returns_empty_string_when_all_none() {
        let d = detail_with(None, None, None);
        assert_eq!(transcript_for_llm_pass(&d), "");
    }

    #[test]
    fn resolve_export_path_uses_dest_when_given() {
        let p = resolve_export_path(Some("C:/tmp/x.md"), "abc").unwrap();
        assert_eq!(p, PathBuf::from("C:/tmp/x.md"));
    }

    #[test]
    fn resolve_export_path_default_lands_under_appdata_meetings_exports() {
        // Set APPDATA explicitly so the test is hermetic.
        // SAFETY: tests run sequentially per `#[cfg(test)]` default; if
        // future parallelism breaks this, gate with a Mutex.
        // SAFETY: pre-Rust 2024 std::env::set_var was safe; from 2024
        // it's unsafe due to multi-threaded reads. This test crate is
        // edition 2021, so the call is still safe.
        std::env::set_var("APPDATA", "C:/Users/test/AppData/Roaming");
        let p = resolve_export_path(None, "fake-uuid").unwrap();
        let s = p.display().to_string().replace('\\', "/");
        assert!(
            s.ends_with("Mockingbird/meetings/exports/fake-uuid.md"),
            "unexpected default path: {s}"
        );
    }

    #[test]
    fn llm_pass_prompt_arg_canonicalizes_builtin_names() {
        let p: LlmPassPrompt = LlmPassPromptArg::BuiltIn("summary".into()).into();
        match p {
            LlmPassPrompt::BuiltIn(n) => assert_eq!(n, "summary"),
            _ => panic!("expected BuiltIn"),
        }
        let p2: LlmPassPrompt = LlmPassPromptArg::BuiltIn("action_items".into()).into();
        match p2 {
            LlmPassPrompt::BuiltIn(n) => assert_eq!(n, "action_items"),
            _ => panic!("expected BuiltIn"),
        }
    }

    #[test]
    fn llm_pass_prompt_arg_unknown_builtin_maps_to_sentinel() {
        let p: LlmPassPrompt = LlmPassPromptArg::BuiltIn("made-up-name".into()).into();
        match p {
            // `resolve_prompt_body` will reject the sentinel — that's
            // intentional: an unknown built-in shouldn't silently
            // succeed with the "summary" body. Wire-level test of the
            // resolver lives in `llm_pass::tests`.
            LlmPassPrompt::BuiltIn(n) => assert_eq!(n, "__unknown__"),
            _ => panic!("expected BuiltIn"),
        }
    }

    #[test]
    fn llm_pass_prompt_arg_custom_passes_through_body() {
        let body = "do the thing".to_string();
        let p: LlmPassPrompt = LlmPassPromptArg::Custom {
            custom: body.clone(),
        }
        .into();
        match p {
            LlmPassPrompt::Custom(b) => assert_eq!(b, body),
            _ => panic!("expected Custom"),
        }
    }

    /// End-to-end wire parse: the JSON shape `"summary"` (bare string)
    /// deserializes into the BuiltIn variant.
    #[test]
    fn llm_pass_prompt_arg_deserializes_bare_string_as_builtin() {
        let parsed: LlmPassPromptArg = serde_json::from_str("\"summary\"").unwrap();
        match parsed {
            LlmPassPromptArg::BuiltIn(s) => assert_eq!(s, "summary"),
            _ => panic!("expected BuiltIn from bare string"),
        }
    }

    /// End-to-end wire parse: the JSON shape `{"custom": "..."}`
    /// deserializes into the Custom variant. Matches the plan's
    /// `prompt_id: string | { custom: string }` contract.
    #[test]
    fn llm_pass_prompt_arg_deserializes_object_as_custom() {
        let parsed: LlmPassPromptArg = serde_json::from_str(r#"{"custom":"do thing"}"#).unwrap();
        match parsed {
            LlmPassPromptArg::Custom { custom } => assert_eq!(custom, "do thing"),
            _ => panic!("expected Custom from object shape"),
        }
    }

    #[test]
    fn meeting_source_probe_dto_round_trips_from_typed_probe() {
        let probe = MeetingSourceProbe {
            mic_available: true,
            system_available: false,
        };
        let dto: MeetingSourceProbeDto = probe.into();
        assert!(dto.mic_available);
        assert!(!dto.system_available);
    }
}
