//! Activity Capture Layer-2: audio capture + chunked Whisper
//! transcription (Phase 10 Wave 4).
//!
//! This module is the orchestrator that ties:
//!
//! 1. **Twin-stream WASAPI capture** from `meetings::capture` (mic +
//!    system loopback, both required per ADR 0041 §Decision item 2).
//! 2. **Chunked Whisper** from `meetings::long_form_stt` (rolling
//!    chunks transcribed live as they arrive).
//!
//! together for an Activity Capture session. We deliberately reuse
//! the Meeting Capture primitives wholesale — duplicating WASAPI
//! capture or chunked Whisper here would be a second source of
//! truth + a second place to fix bugs (ADR 0036 sibling-subsystem
//! principle: share leaf infrastructure, not orchestration).
//!
//! ## Lifecycle
//!
//! ```text
//!   run_open_session                run_close_session
//!         │                                │
//!         │  audio_enabled? yes            │  stop pipeline
//!         ▼                                ▼
//!   start TwinStreamCapture        capture.stop() ─► chunk_rx disconnects
//!         │                                │
//!         │  take chunk_rx                 │  long-form thread sees disconnect
//!         ▼                                ▼
//!   spawn long-form worker         worker returns LongFormOutput
//!     thread (owns Whisper +              │
//!     drives LongFormStt::run)            ▼
//!         │                        flatten to RawSegments
//!         ▼                                │
//!   pipeline holds capture +               ▼
//!     worker JoinHandle             persist segments + provenance
//! ```
//!
//! ## Cross-platform (Principle 5)
//!
//! Audio capture is Windows-only in v1 (WASAPI). Non-Windows builds
//! get a stub impl whose `start` returns
//! `AppError::Activity("activity audio capture requires Windows")`
//! and whose `stop` returns an empty segment list. The non-Windows
//! Activity timeline continues to work; audio just isn't captured.
//!
//! ## Trait-based seam
//!
//! [`AudioPipeline`] is the trait. The runtime takes
//! `Box<dyn AudioPipeline + Send>` so tests can inject a
//! [`StubAudioPipeline`] and prove the start/stop wiring without
//! touching WASAPI or Whisper.

use std::path::{Path, PathBuf};

use crate::activity::block_audio_stitcher::TranscriptChannel;
use crate::error::AppResult;

/// One Whisper segment in a form the activity persist layer can
/// directly INSERT. Constructed by the audio pipeline's `stop` and
/// fanned into `segments_persist::insert_segments_bulk`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawSegment {
    /// Global session timeline, ms. Already offset by chunk position
    /// per `meetings::long_form_stt::LongFormStt`'s stitch invariants.
    pub started_at_ms: i64,
    /// Global session timeline, ms. `>= started_at_ms`.
    pub ended_at_ms: i64,
    /// Whisper-recognized text for this segment. May be empty for
    /// silence-only segments.
    pub text: String,
    /// Which capture channel produced this segment.
    pub channel: TranscriptChannel,
}

/// Per-session audio-pipeline provenance, surfaced to
/// `activity_sessions.audio_whisper_model` + `audio_chunk_window_ms`.
/// ADR 0041 §Decision item 4 + Principle 2 (provenance is total).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioProvenance {
    /// Whisper GGUF label active for this session. Goes to
    /// `activity_sessions.audio_whisper_model`.
    pub whisper_model: String,
    /// Long-form chunk window in ms. Goes to
    /// `activity_sessions.audio_chunk_window_ms`.
    pub chunk_window_ms: i64,
}

/// What every audio pipeline (real + stub) implements. The runtime
/// owns one of these per session when `audio_enabled = true`.
pub trait AudioPipeline: Send {
    /// Stop the pipeline, drain the long-form worker, return all
    /// transcribed segments. Idempotent: a second call returns
    /// `Ok(empty)`. Errors here are non-fatal at the runtime — the
    /// session still gets a non-audio summary, the error is logged.
    fn stop(self: Box<Self>) -> AppResult<AudioPipelineOutput>;
}

/// The bundle a pipeline hands back on stop.
#[derive(Debug, Clone, Default)]
pub struct AudioPipelineOutput {
    /// Per-channel segments, time-ordered across both channels.
    pub segments: Vec<RawSegment>,
    /// Per-session provenance — `None` if the pipeline produced
    /// nothing (worker panicked, no audio captured, etc).
    pub provenance: Option<AudioProvenance>,
}

/// Start the platform-default audio pipeline for one session.
///
/// `audio_chunk_base_dir` is the parent dir under which per-session
/// chunk subdirs live (mirroring meetings' `chunk_base_dir`). The
/// runtime computes this once at spawn from the app data dir.
///
/// `session_id` is used to namespace the on-disk chunk dir so two
/// concurrent sessions (theoretically impossible per the FSM, but
/// belt-and-braces) can't collide.
pub fn start_default(
    session_id: &str,
    audio_chunk_base_dir: &Path,
) -> AppResult<Box<dyn AudioPipeline + Send>> {
    #[cfg(target_os = "windows")]
    {
        windows_impl::WindowsAudioPipeline::start(session_id, audio_chunk_base_dir)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (session_id, audio_chunk_base_dir);
        Err(crate::error::AppError::Activity(
            "activity audio capture requires Windows".into(),
        ))
    }
}

// ===========================================================================
// Windows implementation
// ===========================================================================

#[cfg(target_os = "windows")]
mod windows_impl {
    use super::*;
    use std::path::PathBuf;
    use std::thread::{self, JoinHandle};

    use crate::error::{AppError, AppResult};
    use crate::meetings::capture::{MeetingSource, TwinStreamCapture};
    use crate::meetings::chunker::ChunkerConfig;
    use crate::meetings::long_form_stt::{LongFormConfig, LongFormOutput, LongFormStt};

    /// Best-effort label for the active GGUF. Matches the default the
    /// stt-layer ships (LESSONS-pinned + `models/` README). When a
    /// `MOCKINGBIRD_WHISPER_MODEL` env override is set, this won't
    /// match the real file — Wave 5+ can refine. For now it's the
    /// honest default-case label.
    const DEFAULT_WHISPER_MODEL_LABEL: &str = "whisper-large-v3-turbo-q5_0";

    /// The Windows audio pipeline. Owns the capture + worker thread
    /// until `stop()` is called.
    pub struct WindowsAudioPipeline {
        capture: TwinStreamCapture,
        long_form_thread: JoinHandle<AppResult<LongFormOutput>>,
        chunk_dir: PathBuf,
        chunker_cfg: ChunkerConfig,
    }

    impl WindowsAudioPipeline {
        pub fn start(
            session_id: &str,
            audio_chunk_base_dir: &std::path::Path,
        ) -> AppResult<Box<dyn AudioPipeline + Send>> {
            let chunk_dir = audio_chunk_base_dir.join(session_id);
            std::fs::create_dir_all(&chunk_dir).map_err(|e| {
                AppError::Activity(format!(
                    "create activity audio chunk dir {chunk_dir:?}: {e}"
                ))
            })?;

            // Hard-coded `MeetingSource::Both` — Activity audio always
            // captures both mic + system (ADR 0041 §Decision item 2).
            // If a future preference lets users pick mic-only or sys-
            // only, surface via a new field on AudioProvenance.
            let chunker_cfg = ChunkerConfig::default();
            let mut capture = TwinStreamCapture::start(
                session_id.to_string(),
                MeetingSource::Both,
                chunk_dir.clone(),
                chunker_cfg.clone(),
            )?;
            let chunk_rx = capture.take_chunk_rx().ok_or_else(|| {
                AppError::Activity("TwinStreamCapture chunk_rx already taken".into())
            })?;

            let session_id_owned = session_id.to_string();
            let long_form_thread = thread::Builder::new()
                .name(format!("activity-long-form-{session_id_owned}"))
                .spawn(move || -> AppResult<LongFormOutput> {
                    let mut stt = crate::stt::make_default_stt()?;
                    let sid = session_id_owned.clone();
                    let driver = LongFormStt::new(
                        stt.as_mut(),
                        chunk_rx,
                        move |p| {
                            // Activity audio currently has no live UI
                            // surface; we just log progress for the
                            // operator. Wave 5+ may wire to a status
                            // bar mic glyph.
                            tracing::debug!(
                                target: "activity::audio",
                                session_id = %sid,
                                channel = ?p.channel,
                                chunk_seq = p.chunk_seq,
                                chunks_done = p.chunks_done,
                                "activity long-form progress"
                            );
                        },
                        LongFormConfig::default(),
                    );
                    driver.run()
                })
                .map_err(|e| AppError::Activity(format!("spawn activity long-form thread: {e}")))?;

            tracing::info!(
                target: "activity::audio",
                session_id = %session_id,
                chunk_dir = %chunk_dir.display(),
                "activity audio pipeline started"
            );

            Ok(Box::new(Self {
                capture,
                long_form_thread,
                chunk_dir,
                chunker_cfg,
            }))
        }
    }

    impl AudioPipeline for WindowsAudioPipeline {
        fn stop(self: Box<Self>) -> AppResult<AudioPipelineOutput> {
            let WindowsAudioPipeline {
                mut capture,
                long_form_thread,
                chunk_dir,
                chunker_cfg,
            } = *self;

            // 1. Stop capture — disconnects the chunk channel and
            //    causes the long-form worker's `recv()` to return Err
            //    on its next iteration, ending `driver.run()`.
            if let Err(e) = capture.stop() {
                tracing::warn!(
                    target: "activity::audio",
                    error = %e,
                    "capture.stop() reported errors; continuing"
                );
            }

            // 2. Join the worker.
            let output = match long_form_thread.join() {
                Ok(Ok(o)) => Some(o),
                Ok(Err(e)) => {
                    tracing::warn!(
                        target: "activity::audio",
                        error = %e,
                        "long-form thread errored; segments lost"
                    );
                    None
                }
                Err(e) => {
                    tracing::warn!(
                        target: "activity::audio",
                        error = ?e,
                        "long-form thread panicked; segments lost"
                    );
                    None
                }
            };

            // 3. Best-effort chunk-dir cleanup. Activity-audio chunks
            //    are not needed once transcription is done — unlike
            //    meetings, we don't keep the raw audio for re-runs in
            //    v1 (retention policy lands in Wave 5).
            if let Err(e) = std::fs::remove_dir_all(&chunk_dir) {
                tracing::debug!(
                    target: "activity::audio",
                    error = %e,
                    path = %chunk_dir.display(),
                    "activity audio chunk_dir cleanup failed (non-fatal)"
                );
            }

            let provenance = AudioProvenance {
                whisper_model: DEFAULT_WHISPER_MODEL_LABEL.to_string(),
                chunk_window_ms: chunker_window_ms(&chunker_cfg),
            };

            let segments = output.map(flatten_long_form).unwrap_or_default();
            tracing::info!(
                target: "activity::audio",
                segment_count = segments.len(),
                "activity audio pipeline stopped"
            );

            Ok(AudioPipelineOutput {
                segments,
                provenance: Some(provenance),
            })
        }
    }

    fn chunker_window_ms(cfg: &ChunkerConfig) -> i64 {
        // ChunkerConfig holds chunk_samples + sample_rate. Convert to
        // ms; saturate on the (impossible) 0-rate path so the column
        // gets a real number rather than panicking.
        if cfg.sample_rate == 0 {
            return 0;
        }
        (cfg.chunk_samples as i64 * 1000) / cfg.sample_rate as i64
    }

    fn flatten_long_form(output: LongFormOutput) -> Vec<RawSegment> {
        let mut out = Vec::with_capacity(output.mic_segments.len() + output.sys_segments.len());
        for s in output.mic_segments {
            out.push(RawSegment {
                started_at_ms: s.t0_ms as i64,
                ended_at_ms: s.t1_ms as i64,
                text: s.text,
                channel: TranscriptChannel::Mic,
            });
        }
        for s in output.sys_segments {
            out.push(RawSegment {
                started_at_ms: s.t0_ms as i64,
                ended_at_ms: s.t1_ms as i64,
                text: s.text,
                channel: TranscriptChannel::System,
            });
        }
        // Time-order across channels — `segments_persist::insert_segments_bulk`
        // doesn't require it, but a chronologically-sorted batch
        // produces a tidier `created_at` ordering and matches what
        // `list_segments` returns on read.
        out.sort_by_key(|s| s.started_at_ms);
        out
    }
}

// ===========================================================================
// Stub pipeline (always available; tests + non-Windows builds)
// ===========================================================================

/// In-memory pipeline for tests. Configurable canned output.
pub struct StubAudioPipeline {
    /// Canned output — returned verbatim from [`AudioPipeline::stop`].
    pub output: AudioPipelineOutput,
}

impl StubAudioPipeline {
    /// Build a stub whose `stop()` returns an empty output bundle.
    /// Useful when the runtime test only cares about the start path.
    pub fn empty() -> Box<dyn AudioPipeline + Send> {
        Box::new(Self {
            output: AudioPipelineOutput::default(),
        })
    }

    /// Build a stub whose `stop()` returns the given segments + a
    /// fixed `"stub"` provenance. Used by the runtime audio-persist
    /// tests.
    pub fn with_segments(segments: Vec<RawSegment>) -> Box<dyn AudioPipeline + Send> {
        Box::new(Self {
            output: AudioPipelineOutput {
                segments,
                provenance: Some(AudioProvenance {
                    whisper_model: "stub".into(),
                    chunk_window_ms: 30_000,
                }),
            },
        })
    }
}

impl AudioPipeline for StubAudioPipeline {
    fn stop(self: Box<Self>) -> AppResult<AudioPipelineOutput> {
        Ok(self.output)
    }
}

/// Sanity-suppress unused-path warning on non-Windows: the
/// `start_default` body references the `Path` type only behind
/// `#[cfg(target_os = "windows")]`. `PathBuf` re-export keeps
/// the public surface the same on all platforms.
#[doc(hidden)]
#[allow(dead_code)]
pub(crate) fn _silence_unused_path() -> Option<PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_returns_canned_segments() {
        let segs = vec![
            RawSegment {
                started_at_ms: 1_000,
                ended_at_ms: 2_000,
                text: "hi".into(),
                channel: TranscriptChannel::Mic,
            },
            RawSegment {
                started_at_ms: 2_500,
                ended_at_ms: 3_500,
                text: "yo".into(),
                channel: TranscriptChannel::System,
            },
        ];
        let pipeline = StubAudioPipeline::with_segments(segs.clone());
        let out = pipeline.stop().unwrap();
        assert_eq!(out.segments, segs);
        assert!(out.provenance.is_some());
    }

    #[test]
    fn empty_stub_yields_no_segments_no_provenance() {
        let pipeline = StubAudioPipeline::empty();
        let out = pipeline.stop().unwrap();
        assert!(out.segments.is_empty());
        assert!(out.provenance.is_none());
    }

    /// Non-Windows: `start_default` returns the documented error so
    /// the runtime can fall back to a no-audio session without
    /// crashing.
    #[cfg(not(target_os = "windows"))]
    #[test]
    fn start_default_errors_on_non_windows() {
        let tmp = std::env::temp_dir().join("activity_audio_stub_test");
        let r = start_default("test-session", &tmp);
        assert!(r.is_err());
    }
}
