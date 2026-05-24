//! Dictation IPC commands.
//!
//! Per ADR 0045 the Dictation kind supports two start modes:
//! push-to-talk via the OS keyboard hook (UNCHANGED) and programmatic
//! start/stop via `dictation_start` / `dictation_stop`. Both modes
//! share one `HotkeyStateMachine` — the synthetic events these
//! commands inject are indistinguishable from real key events from
//! the FSM's POV.
//!
//! ADR 0046 §3.2 / mb-7vyz adds a THIRD entry point — `dictation_
//! import_file` — which bypasses the FSM entirely and feeds a
//! decoded audio file straight into the orchestrator via the sibling
//! `HeadlessIngestSender` channel. The orchestrator reuses its
//! existing VAD/STT/Cleaner deps, so a file import costs zero
//! additional model loads.
//!
//! See `dictation::runtime::DictationRuntime::start` /
//! `DictationRuntime::stop` for the synthetic-event mechanism, and
//! `dictation::ingest_channel` for the headless-ingest channel.

use std::path::PathBuf;

use tauri::{AppHandle, Runtime, State};

use crate::audio::decode::decode_to_pcm16_mono_16k;
use crate::dictation::ingest::IngestProvenance;
use crate::dictation::ingest_channel::{HeadlessIngestRequest, HeadlessIngestSender};
use crate::dictation::ingest_progress::{self, IngestProgressBus, IngestProgressEvent};

/// Start a dictation session programmatically.
///
/// Equivalent to pressing the configured push-to-talk key. The state
/// machine takes 80 ms to confirm the "hold" (matches PTT semantics)
/// before the orchestrator gets a `StartCapture` action — UI should
/// expect `dictation:state` to flip to `listening` shortly after this
/// IPC returns, not synchronously.
///
/// Returns `Ok(())` if the synthetic event was queued; the actual
/// recording start is observed via the `dictation:state` event stream.
#[tauri::command]
pub fn dictation_start<R: Runtime>(
    runtime: State<'_, crate::dictation::runtime::DictationRuntime>,
    _app: AppHandle<R>,
) -> Result<(), String> {
    runtime.start().map_err(|e| e.to_string())
}

/// Stop a dictation session programmatically.
///
/// Equivalent to releasing the PTT key. Idempotent: if no session is
/// active, this is a state-machine no-op (the FSM ignores `KeyUp` in
/// `Idle` / `Processing`). Safe to call defensively from a UI Stop
/// button.
///
/// Returns `Ok(())` if the synthetic event was queued; the actual
/// pipeline completion is observed via the `dictation:state` event
/// stream (the orchestrator finalizes the session asynchronously).
#[tauri::command]
pub fn dictation_stop<R: Runtime>(
    runtime: State<'_, crate::dictation::runtime::DictationRuntime>,
    _app: AppHandle<R>,
) -> Result<(), String> {
    runtime.stop().map_err(|e| e.to_string())
}

/// Successful-import response shape — fed to the toast and the
/// Dictations refetch trigger.
///
/// The `session_id` matches the new `sessions.id` row the orchestrator
/// just persisted; the React side uses it to optionally scroll-to-row.
#[derive(serde::Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SessionImportSummary {
    /// Inserted `sessions.id`.
    pub session_id: i64,
    /// DB string for the persisted `sessions.source` — currently
    /// always `"desktop-import"` for this IPC. The Iter 3 inbox
    /// watcher persists `"mobile-inbox"` via the same channel.
    pub source: String,
    /// First ~120 chars of the cleaned (or raw, on fallback)
    /// transcript so the toast can quote what was actually
    /// transcribed instead of an opaque "imported ✓".
    pub transcript_preview: String,
}

/// ADR 0046 §3.2 / mb-7vyz — desktop audio-file import.
///
/// User clicks the "+ Audio file" button on the Dictations page → we
/// open a native file picker → decode the selected file off this
/// thread → queue a `HeadlessIngestRequest` on the orchestrator's
/// sibling channel → await the reply → return a typed summary the UI
/// can toast.
///
/// Cancel semantics: if the user dismisses the file picker we return
/// `Err("cancelled".into())` so the UI can suppress the failure toast.
/// All other error paths (decode, ingest) propagate with a descriptive
/// string the toast renders verbatim.
#[tauri::command]
pub async fn dictation_import_file<R: Runtime>(
    app: AppHandle<R>,
    headless_tx: State<'_, HeadlessIngestSender>,
    progress: State<'_, std::sync::Arc<crate::dictation::ingest_progress::AppIngestProgressBus>>,
    db: State<'_, crate::commands::AppStateHandle>,
) -> Result<SessionImportSummary, String> {
    // Snapshot the sender BEFORE any await — `State<'_, T>` is not
    // Send across .await points, but a cloned `HeadlessIngestSender`
    // (crossbeam `Sender` is `Send + Clone`) is fine to carry.
    let headless_tx: HeadlessIngestSender = headless_tx.inner().clone();
    let progress: std::sync::Arc<crate::dictation::ingest_progress::AppIngestProgressBus> =
        std::sync::Arc::clone(progress.inner());
    let db = std::sync::Arc::clone(&db.inner().db);

    // 1. File picker. `tauri-plugin-dialog` is already loaded in
    //    `lib.rs::run`. The Save-As wrapper in `commands/meetings.rs`
    //    uses the same plugin's `blocking_save_file`; we mirror that
    //    with `blocking_pick_file` here.
    let picked = pick_audio_file(&app)?;
    let Some(path) = picked else {
        // Cancel is NOT a failure -- no progress emit, the UI never
        // saw "decoding" so there's nothing to clear.
        return Err("cancelled".into());
    };
    let original_filename = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();
    tracing::info!(?path, "dictation_import_file: picked");

    // Helper closures keep the bracket emits readable -- the
    // ingest pipeline below is the protagonist.
    let emit = |event: IngestProgressEvent| progress.emit(event);
    let on_err = |original: &str, err: String| -> String {
        emit(IngestProgressEvent::failed(
            ingest_progress::source::DESKTOP_IMPORT,
            original,
            err.clone(),
        ));
        err
    };

    // 2. Decode off-thread (CPU-heavy symphonia pass). `spawn_blocking`
    //    keeps the Tauri async runtime responsive while ffmpeg-grade
    //    AAC decode runs.
    emit(IngestProgressEvent::staged(
        ingest_progress::stage::DECODING,
        ingest_progress::source::DESKTOP_IMPORT,
        &original_filename,
    ));
    let path_for_decode = path.clone();
    let samples =
        match tokio::task::spawn_blocking(move || decode_to_pcm16_mono_16k(&path_for_decode))
            .await
            .map_err(|e| format!("decode task panicked: {e}"))
            .and_then(|r| r.map_err(|e| format!("decode failed: {e}")))
        {
            Ok(s) => s,
            Err(e) => return Err(on_err(&original_filename, e)),
        };
    tracing::info!(
        samples = samples.len(),
        approx_seconds = samples.len() as f64 / 16_000.0,
        "dictation_import_file: decoded"
    );

    // 3. Queue the headless ingest request. Bounded(1) reply channel
    //    so a buggy double-send from the orchestrator would surface
    //    rather than buffer silently. We emit `transcribing` BEFORE
    //    the send -- the orchestrator opaquely runs both whisper +
    //    cleanup before replying, so this single label covers the
    //    whole crunch from the UI's POV (see kickoff: "collapse
    //    cleaning into transcribing if staging is hard").
    emit(IngestProgressEvent::staged(
        ingest_progress::stage::TRANSCRIBING,
        ingest_progress::source::DESKTOP_IMPORT,
        &original_filename,
    ));
    let (reply_tx, reply_rx) = crossbeam_channel::bounded(1);
    let provenance = IngestProvenance::desktop_import(original_filename.clone(), now_iso_utc());
    if let Err(_e) = headless_tx.send(HeadlessIngestRequest {
        samples,
        provenance,
        reply_tx,
    }) {
        return Err(on_err(
            &original_filename,
            "orchestrator unavailable (dictation runtime not started)".to_string(),
        ));
    }

    // 4. Block this async task on the reply. The orchestrator runs
    //    on its own thread; `tokio::task::spawn_blocking` ensures we
    //    don't park the async executor while whisper-rs crunches.
    let session_id = match tokio::task::spawn_blocking(move || reply_rx.recv())
        .await
        .map_err(|e| format!("reply task panicked: {e}"))
        .and_then(|r| r.map_err(|_| "orchestrator dropped reply channel".to_string()))
        .and_then(|r| r.map_err(|e| format!("ingest failed: {e}")))
    {
        Ok(id) => id,
        Err(e) => return Err(on_err(&original_filename, e)),
    };
    tracing::info!(session_id, "dictation_import_file: ingest complete");
    emit(IngestProgressEvent::done(
        ingest_progress::source::DESKTOP_IMPORT,
        &original_filename,
        session_id,
    ));

    // 5. Read a short preview for the toast. Falls back across stages
    //    so a partial pipeline (cleanup failed → cleaned == raw)
    //    still surfaces something.
    let preview = preview_transcript(&db, session_id);

    Ok(SessionImportSummary {
        session_id,
        source: "desktop-import".into(),
        transcript_preview: preview,
    })
}

/// Open the native audio-file picker. Returns `Ok(None)` on cancel,
/// `Ok(Some(path))` on confirm, `Err(...)` on dialog failure.
fn pick_audio_file<R: Runtime>(app: &AppHandle<R>) -> Result<Option<PathBuf>, String> {
    use tauri_plugin_dialog::DialogExt;
    let picked = app
        .dialog()
        .file()
        .add_filter("Audio", &["m4a", "mp3", "wav", "ogg", "aac", "flac"])
        .blocking_pick_file();
    match picked {
        None => Ok(None),
        Some(fp) => fp
            .into_path()
            .map(Some)
            .map_err(|e| format!("file picker returned non-path FilePath: {e}")),
    }
}

/// Best-effort transcript preview. Falls back `cleaned` → `raw` so
/// even a degraded pipeline (cleanup failed → cleaned text falls back
/// to raw text inside `headless_ingest`) still surfaces SOMETHING.
/// Never errors — a missing preview just yields "(no transcript)".
fn preview_transcript(
    db: &std::sync::Arc<std::sync::Mutex<rusqlite::Connection>>,
    id: i64,
) -> String {
    let Ok(conn) = db.lock() else {
        return "(db unavailable)".into();
    };
    for stage in [
        crate::db::transcripts::Stage::Cleaned,
        crate::db::transcripts::Stage::Raw,
    ] {
        if let Ok(Some(t)) = crate::db::transcripts::get_stage(&conn, id, stage) {
            let trimmed = t.text.trim();
            if !trimmed.is_empty() {
                return truncate(trimmed, 120);
            }
        }
    }
    "(no transcript)".into()
}

/// Truncate a string to at most `max_chars` Unicode scalar values,
/// appending `...` when truncation actually occurred. Kept here as a
/// trivial inline helper rather than dragging in a dep or threading
/// through `lib/format.ts`-style code.
fn truncate(s: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i >= max_chars {
            out.push_str("...");
            return out;
        }
        out.push(ch);
    }
    out
}

/// Mirror of `dictation::ingest::now_iso_utc` so this module doesn't
/// have a pub-of-private dep on a sibling helper. ISO-8601 UTC, ms
/// precision — same shape the orchestrator uses for `sessions.
/// started_at` so file-import rows sort cleanly alongside PTT rows.
fn now_iso_utc() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_short_string_unchanged() {
        assert_eq!(truncate("hello", 120), "hello");
    }

    #[test]
    fn truncate_long_string_gets_ellipsis() {
        let long = "x".repeat(200);
        let out = truncate(&long, 120);
        assert_eq!(out.chars().count(), 120 + 3);
        assert!(out.ends_with("..."));
    }

    #[test]
    fn truncate_respects_unicode_scalar_boundaries() {
        // 5 emoji = 5 chars, well under 120 — unchanged.
        let s = "😀😁😂🤣😄";
        assert_eq!(truncate(s, 120), s);
        // Truncated at 3 chars + ellipsis.
        assert_eq!(truncate(s, 3), "😀😁😂...");
    }

    #[test]
    fn session_import_summary_serializes_camel_case() {
        let s = SessionImportSummary {
            session_id: 7,
            source: "desktop-import".into(),
            transcript_preview: "hi".into(),
        };
        let j = serde_json::to_string(&s).unwrap();
        assert!(j.contains("\"sessionId\":7"));
        assert!(j.contains("\"transcriptPreview\":\"hi\""));
    }
}
