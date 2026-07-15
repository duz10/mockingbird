//! Meeting lifecycle methods on [`MeetingRuntimeShared`].
//!
//! Split out of `meetings/runtime.rs` to keep both files under the
//! 600-line cap. The struct definition + activation thread loop +
//! Drop impl live in `runtime.rs`; this file owns the
//! recording-to-canonical-transcript path:
//!   - [`MeetingRuntimeShared::start_meeting`]
//!   - [`MeetingRuntimeShared::stop_meeting`]
//!   - [`MeetingRuntimeShared::finalize_in_flight_as_interrupted`]
//!     (used by runtime's `Drop`)
//!
//! ## Critical-path invariant (binding — Wave 6 judge
//! `mc-no-llm-in-critical-path`)
//!
//! Every code path in this file is part of the
//! recording-to-canonical-transcript pipeline. **NO `OllamaProvider`,
//! `LlmCleaner`, or HTTP-to-LLM construction may appear here.** The
//! optional LLM pass is reached only via the
//! `meeting_run_llm_pass` IPC command, which writes into the
//! runtime's `llm_pass_cache` (an in-memory `HashMap<String, String>`
//! keyed by request id) and is rendered into the export markdown by
//! [`super::export::render_markdown`] when the user explicitly asks
//! for "Export with LLM pass". The DB never sees the LLM output.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use tauri::Emitter;

use crate::meetings::levels::LevelsState;

use crate::error::{AppError, AppResult};
use crate::meetings::capture::{MeetingSource, TwinStreamCapture};
use crate::meetings::chunker::ChunkerConfig;
use crate::meetings::filler_words::FILLERS;
use crate::meetings::formatter::{format, FormatOpts};
use crate::meetings::long_form_stt::{LongFormConfig, LongFormOutput, LongFormStt};
use crate::meetings::merge::{merge_two_channels, SpeakerLabels};
use crate::meetings::persist::{persist_meeting, MeetingPersistRequest, MeetingStatus};
use crate::meetings::runtime::{InFlightMeeting, MeetingRuntimeShared};

impl MeetingRuntimeShared {
    /// Idempotent start. Returns the in-flight uuid (new or existing).
    pub fn start_meeting(&self, source: MeetingSource) -> AppResult<String> {
        let mut guard = self
            .in_flight
            .lock()
            .map_err(|_| AppError::MeetingCapture("in_flight mutex poisoned".into()))?;
        if let Some(existing) = guard.as_ref() {
            self.emit_state("warn-already-running", Some(&existing.uuid), None);
            return Ok(existing.uuid.clone());
        }

        let uuid = uuid_v4_simple();
        let started_at_iso = now_iso();
        let started_at_instant = Instant::now();
        let chunk_dir = self.config.chunk_base_dir.join(&uuid);
        std::fs::create_dir_all(&chunk_dir).map_err(|e| {
            AppError::MeetingCapture(format!("create chunk dir {chunk_dir:?}: {e}"))
        })?;

        let chunker_cfg = ChunkerConfig::default();
        let mut capture =
            TwinStreamCapture::start(uuid.clone(), source, chunk_dir.clone(), chunker_cfg)?;
        let chunk_rx = capture.take_chunk_rx().ok_or_else(|| {
            AppError::MeetingCapture("TwinStreamCapture chunk_rx already taken".into())
        })?;

        // Long-form STT worker owns the chunk receiver + a fresh STT
        // instance. The worker survives until the receiver
        // disconnects, which happens when `capture.stop()` drops the
        // underlying capture threads.
        let app_handle = self.app_handle.clone();
        let uuid_for_thread = uuid.clone();
        let long_form_thread = thread::Builder::new()
            .name(format!("mockingbird-long-form-{uuid_for_thread}"))
            .spawn(move || -> AppResult<LongFormOutput> {
                let mut stt = crate::stt::make_default_stt()?;
                let progress_uuid = uuid_for_thread.clone();
                let progress_handle = app_handle.clone();
                let driver = LongFormStt::new(
                    stt.as_mut(),
                    chunk_rx,
                    move |p| {
                        // Per-progress overlay update — best-effort emit.
                        let _ = progress_handle.emit(
                            "meeting:progress",
                            serde_json::json!({
                                "uuid": progress_uuid,
                                "channel": format!("{:?}", p.channel),
                                "chunkSeq": p.chunk_seq,
                                "chunksDone": p.chunks_done,
                            }),
                        );
                    },
                    LongFormConfig::default(),
                );
                driver.run()
            })
            .map_err(|e| AppError::MeetingCapture(format!("spawn long-form thread: {e}")))?;

        // ADR 0032 / mb-nig: per-meeting tick thread. Reads
        // capture-side dBFS levels every 250ms and emits
        // `meeting:tick` to the overlay window. Pure I/O — no LLM,
        // no critical-path mutation.
        let tick_running = Arc::new(AtomicBool::new(true));
        let tick_thread = spawn_tick_emitter(
            self.app_handle.clone(),
            uuid.clone(),
            started_at_instant,
            capture.levels_handle(),
            Arc::clone(&tick_running),
        );

        let in_flight = InFlightMeeting {
            uuid: uuid.clone(),
            started_at_iso,
            started_at_instant,
            source,
            capture,
            long_form_thread,
            chunk_dir,
            tick_running,
            tick_thread: Some(tick_thread),
        };
        *guard = Some(in_flight);
        drop(guard);

        self.emit_state("started", Some(&uuid), Some(source));
        Ok(uuid)
    }

    /// Stop the in-flight meeting if its uuid matches. Persists with
    /// `MeetingStatus::Complete`.
    pub fn stop_meeting(&self, uuid: &str) -> AppResult<()> {
        let in_flight = self.take_in_flight_or_error(uuid)?;
        let session_rowid = self.finalize_meeting(in_flight, MeetingStatus::Complete, None)?;
        let _ = self.app_handle.emit(
            "meetings:session-saved",
            serde_json::json!({
                "uuid": uuid,
                "sessionRowid": session_rowid,
            }),
        );
        // ADR 0046 Iter 2 / mb-lvzw — fire the vault trigger after
        // the Complete persist + UI event. Same fire-and-forget
        // pattern dictation uses; export job runs in its own
        // worker thread.
        self.vault.trigger(Arc::clone(&self.shared_conn));
        Ok(())
    }

    /// User-initiated cancel. Stops capture, joins the long-form
    /// thread (discards its output), and **deletes the on-disk chunk
    /// directory**. Does NOT persist anything to the meetings DB —
    /// the user explicitly told us to throw this away.
    ///
    /// Mirrors the early teardown steps of [`Self::finalize_meeting`]
    /// (stop tick emitter → stop capture → join long-form) but skips
    /// the format/merge/persist tail. Emits `meeting:state=cancelled`.
    ///
    /// Raw-data immutability rule (PLAN principle #1) is not
    /// violated: nothing in `transcripts(stage='raw')` has been
    /// written yet at the point this runs. We're cleaning up the
    /// pre-DB scratch space (chunk WAVs) that the user discarded.
    pub fn cancel_meeting(&self, uuid: &str) -> AppResult<()> {
        let in_flight = self.take_in_flight_or_error(uuid)?;
        let InFlightMeeting {
            uuid: ifl_uuid,
            source,
            mut capture,
            long_form_thread,
            chunk_dir,
            tick_running,
            mut tick_thread,
            ..
        } = in_flight;

        // 0. Stop tick emitter first (matches finalize_meeting order).
        tick_running.store(false, Ordering::Relaxed);
        if let Some(h) = tick_thread.take() {
            if let Err(e) = h.join() {
                tracing::warn!(target: "meetings", error = ?e, "cancel: tick thread join failed");
            }
        }

        // 1. Stop capture — closes WAV file handles in the chunker.
        if let Err(e) = capture.stop() {
            tracing::warn!(target: "meetings", error = %e, "cancel: capture.stop() reported errors");
        }

        // 2. Join the long-form worker. Output is discarded.
        match long_form_thread.join() {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => {
                tracing::warn!(target: "meetings", error = %e, "cancel: long-form thread errored")
            }
            Err(e) => {
                tracing::warn!(target: "meetings", error = ?e, "cancel: long-form thread panicked")
            }
        }

        // 3. Best-effort remove of chunk_dir. Failure here is OK —
        //    the directory will be GC'd by app startup eventually,
        //    or the user can delete manually. Logging only.
        if let Err(e) = std::fs::remove_dir_all(&chunk_dir) {
            tracing::warn!(
                target: "meetings",
                error = ?e,
                path = %chunk_dir.display(),
                "cancel: chunk_dir cleanup failed"
            );
        }

        // 4. Emit the cancelled state. React side flips to a
        //    "cancelled, not saved" mode for ~3s, then back to choose.
        self.emit_state("cancelled", Some(&ifl_uuid), Some(source));
        Ok(())
    }

    /// Drop-time best-effort finalizer. Persists any still-in-flight
    /// meeting as `MeetingStatus::Interrupted`.
    pub(crate) fn finalize_in_flight_as_interrupted(&self) -> AppResult<()> {
        let Some(in_flight) = self
            .in_flight
            .lock()
            .map_err(|_| AppError::MeetingCapture("in_flight mutex poisoned (drop)".into()))?
            .take()
        else {
            return Ok(());
        };
        self.finalize_meeting(
            in_flight,
            MeetingStatus::Interrupted,
            Some("runtime dropped mid-recording".into()),
        )
        .map(|_| ())
    }

    /// Take the in-flight cell, or error if uuid doesn't match.
    /// Puts the cell back if the uuid mismatches so the caller's
    /// mistake doesn't lose the live meeting.
    fn take_in_flight_or_error(&self, uuid: &str) -> AppResult<InFlightMeeting> {
        let in_flight = self
            .in_flight
            .lock()
            .map_err(|_| AppError::MeetingCapture("in_flight mutex poisoned".into()))?
            .take()
            .ok_or_else(|| AppError::MeetingCapture("stop_meeting: no in-flight meeting".into()))?;
        if in_flight.uuid == uuid {
            return Ok(in_flight);
        }
        let conflict = in_flight.uuid.clone();
        *self.in_flight.lock().expect("re-lock in_flight") = Some(in_flight);
        Err(AppError::MeetingCapture(format!(
            "stop_meeting: uuid mismatch (live={conflict}, requested={uuid})"
        )))
    }

    /// Shared finalize path used by both `stop_meeting` (status =
    /// Complete) and `finalize_in_flight_as_interrupted` (status =
    /// Interrupted). Owns the in-flight value, so each caller path
    /// takes it once and hands it in.
    fn finalize_meeting(
        &self,
        in_flight: InFlightMeeting,
        status: MeetingStatus,
        error_message: Option<String>,
    ) -> AppResult<i64> {
        // 1. Stop the capture threads — drops streams, flushes
        //    trailing chunks. The long-form worker's chunk_rx will
        //    disconnect, ending its loop on the next recv.
        let InFlightMeeting {
            uuid,
            started_at_iso,
            started_at_instant,
            source,
            mut capture,
            long_form_thread,
            chunk_dir,
            tick_running,
            mut tick_thread,
        } = in_flight;

        // 0. Stop the tick emitter FIRST so the overlay stops
        //    receiving live levels before we tear down capture.
        tick_running.store(false, Ordering::Relaxed);
        if let Some(h) = tick_thread.take() {
            // Tick thread sleeps in 250ms increments; join is bounded.
            if let Err(e) = h.join() {
                tracing::warn!(target: "meetings", error = ?e, "tick thread join failed");
            }
        }

        // mb-z5y wave-5: track capture-stop errors so we can flag
        // meetings that never produced audio. Previously errors here
        // were swallowed as warnings and the meeting persisted as
        // Complete with an empty transcript — confusing for users
        // ("why is my system-audio recording blank?").
        let capture_stop_error: Option<String> = match capture.stop() {
            Ok(_) => None,
            Err(e) => {
                tracing::warn!(
                    target: "meetings",
                    error = %e,
                    "capture.stop() reported errors"
                );
                Some(e.to_string())
            }
        };

        // 2. Join the long-form worker.
        let output = long_form_thread
            .join()
            .map_err(|e| AppError::MeetingCapture(format!("long-form thread panicked: {e:?}")))?
            .unwrap_or_else(|e| {
                tracing::warn!(
                    target: "meetings",
                    error = %e,
                    "long-form returned error; persisting empty output"
                );
                LongFormOutput::default()
            });

        // mb-z5y wave-5: detect silent capture failure. If the user
        // asked for a channel that produced ZERO segments AND we hit
        // a capture error, the recording is effectively a no-op.
        // Downgrade status + attach a user-facing error message so
        // the persisted row reflects reality and the UI can surface
        // a meaningful failure. (Real silence with a working stream
        // is fine — only flagged when stop also errored.)
        let (status, error_message) = {
            let mic_dead = source.needs_mic() && output.mic_segments.is_empty();
            let sys_dead = source.needs_system() && output.sys_segments.is_empty();
            let all_required_dead = match source {
                MeetingSource::Mic => mic_dead,
                MeetingSource::System => sys_dead,
                MeetingSource::Both => mic_dead && sys_dead,
            };
            if all_required_dead && capture_stop_error.is_some() {
                let msg = format!(
                    "capture produced no audio for source '{}': {}",
                    source.as_db_str(),
                    capture_stop_error.as_deref().unwrap_or("(no detail)")
                );
                tracing::error!(target: "meetings", error = %msg, "meeting failed: silent capture");
                (MeetingStatus::Failed, Some(msg))
            } else {
                (status, error_message)
            }
        };

        // 3. Format + merge.
        let opts = FormatOpts::default();
        let formatted_mic = if source.needs_mic() {
            Some(format(&output.mic_segments, &FILLERS, &opts)?)
        } else {
            None
        };
        let formatted_sys = if source.needs_system() {
            Some(format(&output.sys_segments, &FILLERS, &opts)?)
        } else {
            None
        };
        let formatted_merged = if source == MeetingSource::Both {
            // Pull the user's speaker-name labels from settings at
            // merge time so the persisted markdown reflects whatever
            // the user has currently set. Read errors degrade to
            // defaults (`You`/`Other(s)`) — never abort a meeting on
            // a label-lookup glitch. See ADR 0028 §4.
            let labels = {
                let conn_guard = self
                    .shared_conn
                    .lock()
                    .map_err(|_| AppError::MeetingCapture("shared_conn mutex poisoned".into()))?;
                SpeakerLabels::load(&conn_guard)
            };
            Some(merge_two_channels(
                &output.mic_segments,
                &output.sys_segments,
                &labels,
            ))
        } else {
            None
        };

        // 4. Build the persist request + commit.
        let total_duration_ms = started_at_instant.elapsed().as_millis() as u64;
        let req = self.build_persist_request(BuildPersistArgs {
            uuid: &uuid,
            started_at_iso: &started_at_iso,
            source,
            status,
            error_message,
            total_duration_ms,
            chunk_dir: &chunk_dir,
            output: &output,
            formatted_mic,
            formatted_sys,
            formatted_merged,
        });

        let session_rowid = {
            let conn = self
                .shared_conn
                .lock()
                .map_err(|_| AppError::MeetingCapture("db mutex poisoned".into()))?;
            persist_meeting(&conn, &req)?
        };

        let state_label = match status {
            MeetingStatus::Complete => "done",
            MeetingStatus::Interrupted => "interrupted",
            MeetingStatus::Partial => "partial",
            MeetingStatus::Demoted => "demoted",
            MeetingStatus::Failed => "failed",
        };
        self.emit_state(state_label, Some(&uuid), Some(source));

        // mb-z5y wave-3 belt-and-suspenders: on a clean stop the React
        // side shows the "Saved to history" confirmation for 5s then
        // calls `getCurrentWindow().hide()`. In wave-2 we confirmed JS
        // listeners fire (capabilities fix landed), but Dustin still
        // saw the pill stuck visible after stop in one live-fire run.
        // Schedule a Rust-side fallback hide 5.5s after `done` — by
        // then the React side has either succeeded (no-op, window
        // already hidden) or failed (we rescue the UX).
        //
        // Only fires for the clean `done` path. error/interrupted/
        // failed deliberately leave the overlay visible so the user
        // sees what went wrong.
        if matches!(status, MeetingStatus::Complete) {
            let app = self.app_handle.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(5_500));
                crate::meetings::overlay::hide_overlay(&app);
                tracing::info!(
                    target: "mb_listener_ping",
                    "rust fallback hide_overlay() called 5.5s post-done"
                );
            });
        }

        Ok(session_rowid)
    }

    /// Owned-fields wrapper around `MeetingPersistRequest` so we can
    /// build it without owning + cloning `output.{mic,sys}_segments`
    /// twice. Internal to this module.
    fn build_persist_request(&self, args: BuildPersistArgs<'_>) -> MeetingPersistRequest {
        let BuildPersistArgs {
            uuid,
            started_at_iso,
            source,
            status,
            error_message,
            total_duration_ms,
            chunk_dir,
            output,
            formatted_mic,
            formatted_sys,
            formatted_merged,
        } = args;
        // Auto-derive a short title from the formatted transcripts.
        // Pure heuristic — see `meetings::title`. Returns None when
        // every channel is silent; UI then falls back to the
        // localized "Untitled meeting" string. Users can rename via
        // the `meeting_rename` command.
        let title = crate::meetings::title::derive_meeting_title(
            formatted_merged.as_deref(),
            formatted_mic.as_deref(),
            formatted_sys.as_deref(),
        );
        MeetingPersistRequest {
            uuid: uuid.to_string(),
            title,
            started_at: started_at_iso.to_string(),
            ended_at: now_iso(),
            status,
            error_message,
            source,
            total_duration_ms,
            mic_duration_ms: source.needs_mic().then_some(total_duration_ms),
            sys_duration_ms: source.needs_system().then_some(total_duration_ms),
            hotkey_pressed: self.config.hotkey_label.clone(),
            audio_blob_path: Some(chunk_dir.display().to_string()),
            whisper_model_id: self.config.whisper_model_id.clone(),
            formatter_version: self.config.formatter_version.clone(),
            chunk_count_mic: source
                .needs_mic()
                .then_some(output.mic_segments.len() as u32),
            chunk_count_sys: source
                .needs_system()
                .then_some(output.sys_segments.len() as u32),
            stt_latency_ms: None,
            formatter_latency_ms: None,
            formatted_mic,
            formatted_sys,
            formatted_merged,
            segments_mic: source.needs_mic().then_some(output.mic_segments.clone()),
            segments_sys: source.needs_system().then_some(output.sys_segments.clone()),
        }
    }

    pub(crate) fn emit_state(
        &self,
        state: &str,
        uuid: Option<&str>,
        source: Option<MeetingSource>,
    ) {
        // mb-z5y: emit failures used to be swallowed by `let _ =` —
        // that's how the cross-window event-delivery bug went
        // un-diagnosed until live-fire. Trace both outcomes so a
        // future regression here shows up in logs immediately
        // (cf. dictation `recording_window.rs::emit_state`, which
        // already logs on Err).
        let payload = serde_json::json!({
            "state": state,
            "uuid": uuid,
            "source": source.map(|s| s.as_db_str()),
        });
        match self.app_handle.emit("meeting:state", &payload) {
            Ok(()) => tracing::debug!(
                state = state,
                uuid = uuid.unwrap_or(""),
                "meeting:state broadcast"
            ),
            Err(e) => tracing::warn!(
                error = ?e,
                state = state,
                uuid = uuid.unwrap_or(""),
                "meeting:state emit failed"
            ),
        }
    }
}

struct BuildPersistArgs<'a> {
    uuid: &'a str,
    started_at_iso: &'a str,
    source: MeetingSource,
    status: MeetingStatus,
    error_message: Option<String>,
    total_duration_ms: u64,
    chunk_dir: &'a PathBuf,
    output: &'a LongFormOutput,
    formatted_mic: Option<String>,
    formatted_sys: Option<String>,
    formatted_merged: Option<String>,
}

// --------------------------------------------------------------------
// Small helpers — co-located here because they're called from both
// the start and stop paths; keeping them in `runtime.rs` would
// force pub(super) just to satisfy the cap split.
// --------------------------------------------------------------------

/// ISO-8601 UTC timestamp without bringing in chrono just for this.
/// Mirrors `dictation::now_iso` pattern (LESSONS 2026-05-17 — kept
/// the dep surface minimal).
pub(crate) fn now_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    crate::dictation::format_secs_as_iso(secs)
}

/// Lightweight UUID-v4-ish identifier (32 hex chars). Avoids
/// pulling in the `uuid` crate when we already accept opaque strings
/// on the wire and in the DB.
pub(crate) fn uuid_v4_simple() -> String {
    use std::hash::{Hash, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    // Process-local monotonic sequence mixed into the hash so two
    // calls that land in the SAME clock tick still differ. On Apple
    // Silicon release builds `SystemTime::now()` in a tight loop can
    // return identical `as_nanos()` (the effective clock resolution
    // is coarser than 1 ns), which without this counter produced
    // duplicate ids (mb-mac-v1.5.1: the `uuid_v4_simple_unique_and_hex`
    // flake surfaced deterministically on M3 / --release).
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut h = std::collections::hash_map::DefaultHasher::new();
    nanos.hash(&mut h);
    seq.hash(&mut h);
    std::thread::current().id().hash(&mut h);
    let lo = h.finish();
    let mut h2 = std::collections::hash_map::DefaultHasher::new();
    lo.hash(&mut h2);
    seq.hash(&mut h2);
    nanos.wrapping_mul(0x9E37_79B9_7F4A_7C15).hash(&mut h2);
    let hi = h2.finish();
    format!("{hi:016x}{lo:016x}")
}

// --------------------------------------------------------------------
// Tick emitter (ADR 0032 / mb-nig)
// --------------------------------------------------------------------

/// Cadence for the `meeting:tick` event. 250ms is the sweet spot:
/// fast enough for a VU bar to feel responsive (4 Hz update rate is
/// well above visual fusion), slow enough that the per-tick Tauri
/// IPC roundtrip is negligible (~0.1% CPU at idle).
const TICK_INTERVAL: Duration = Duration::from_millis(250);

/// Build the `meeting:tick` JSON payload. Pure for testability —
/// the live emitter wraps this and pushes through `AppHandle::emit`.
pub(crate) fn build_tick_payload(
    uuid: &str,
    elapsed_ms: u128,
    levels: (Option<f32>, Option<f32>),
) -> serde_json::Value {
    // mb-x1d: `None` (no data yet) serializes to JSON `null`, unambiguous
    // against a real full-scale `Some(0.0)` reading. The overlay treats
    // `null` as "flat bar".
    let (mic_db, sys_db) = levels;
    serde_json::json!({
        "uuid": uuid,
        "elapsedMs": elapsed_ms as u64,
        "micDb": mic_db,
        "sysDb": sys_db,
    })
}

/// Spawn the per-meeting tick thread. The thread loops on
/// `running.load(Relaxed)`, sleeping [`TICK_INTERVAL`] between
/// emissions. `finalize_meeting` clears `running` and joins the
/// handle (the join takes ≤ one tick interval).
fn spawn_tick_emitter(
    app_handle: tauri::AppHandle,
    uuid: String,
    started_at: Instant,
    levels: Arc<LevelsState>,
    running: Arc<AtomicBool>,
) -> std::thread::JoinHandle<()> {
    let thread_name = format!("mockingbird-meeting-tick-{uuid}");
    std::thread::Builder::new()
        .name(thread_name)
        .spawn(move || {
            // Emit a first tick immediately so the overlay shows
            // a baseline state without waiting 250ms.
            let payload = build_tick_payload(&uuid, 0, (None, None));
            let _ = app_handle.emit("meeting:tick", payload);

            while running.load(Ordering::Relaxed) {
                std::thread::sleep(TICK_INTERVAL);
                if !running.load(Ordering::Relaxed) {
                    break;
                }
                let elapsed_ms = started_at.elapsed().as_millis();
                let snap = levels.snapshot();
                let payload = build_tick_payload(&uuid, elapsed_ms, snap);
                // Best-effort emit — the overlay may have closed.
                let _ = app_handle.emit("meeting:tick", payload);
            }
        })
        .expect("spawn meeting tick thread")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_tick_payload_shape() {
        // ADR 0032 / mb-nig: pin the JSON shape so the UI's typed
        // MeetingTickEvent stays in sync with the Rust emitter.
        let v = build_tick_payload("abc123", 1500, (Some(-6.0), Some(-100.0)));
        assert_eq!(v["uuid"], "abc123");
        assert_eq!(v["elapsedMs"], 1500u64);
        assert!((v["micDb"].as_f64().unwrap() - (-6.0)).abs() < 1e-3);
        assert!((v["sysDb"].as_f64().unwrap() - (-100.0)).abs() < 1e-3);
    }

    #[test]
    fn build_tick_payload_initial_sentinel() {
        // mb-x1d: a fresh meeting that hasn't drained yet serializes the
        // no-data state as JSON `null` (not `0.0`) so the UI can render
        // flat bars without confusing it with a real full-scale reading.
        let v = build_tick_payload("u", 0, (None, None));
        assert_eq!(v["elapsedMs"], 0u64);
        assert!(v["micDb"].is_null(), "no-data mic serializes to null");
        assert!(v["sysDb"].is_null(), "no-data sys serializes to null");
    }

    #[test]
    fn uuid_v4_simple_unique_and_hex() {
        let mut seen = std::collections::HashSet::new();
        for _ in 0..256 {
            let u = uuid_v4_simple();
            assert_eq!(u.len(), 32);
            assert!(u.chars().all(|c| c.is_ascii_hexdigit()));
            assert!(seen.insert(u));
        }
    }

    #[test]
    fn now_iso_returns_z_suffixed_string() {
        let s = now_iso();
        // Pattern: 2026-05-17T03:14:15Z
        assert!(s.ends_with('Z'), "now_iso must end with Z: {s}");
        assert!(s.contains('T'), "now_iso must contain T: {s}");
    }
}
