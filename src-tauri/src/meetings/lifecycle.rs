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
use std::thread;
use std::time::Instant;

use tauri::Emitter;

use crate::error::{AppError, AppResult};
use crate::meetings::capture::{MeetingSource, TwinStreamCapture};
use crate::meetings::chunker::ChunkerConfig;
use crate::meetings::filler_words::FILLERS;
use crate::meetings::formatter::{format, FormatOpts};
use crate::meetings::long_form_stt::{LongFormConfig, LongFormOutput, LongFormStt};
use crate::meetings::merge::merge_two_channels;
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

        let in_flight = InFlightMeeting {
            uuid: uuid.clone(),
            started_at_iso,
            started_at_instant,
            source,
            capture,
            long_form_thread,
            chunk_dir,
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
        } = in_flight;
        if let Err(e) = capture.stop() {
            tracing::warn!(target: "meetings", error = %e, "capture.stop() reported errors");
        }

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
            Some(merge_two_channels(
                &output.mic_segments,
                &output.sys_segments,
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
        MeetingPersistRequest {
            uuid: uuid.to_string(),
            title: None,
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
        let _ = self.app_handle.emit(
            "meeting:state",
            serde_json::json!({
                "state": state,
                "uuid": uuid,
                "source": source.map(|s| s.as_db_str()),
            }),
        );
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
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut h = std::collections::hash_map::DefaultHasher::new();
    nanos.hash(&mut h);
    std::thread::current().id().hash(&mut h);
    let lo = h.finish();
    let mut h2 = std::collections::hash_map::DefaultHasher::new();
    lo.hash(&mut h2);
    nanos.wrapping_mul(0x9E37_79B9_7F4A_7C15).hash(&mut h2);
    let hi = h2.finish();
    format!("{hi:016x}{lo:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

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
