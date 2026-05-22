//! Activity-capture orchestrator.
//!
//! Owns:
//!
//! - The current [`LifecycleState`] (one session at a time).
//! - The active session id (None when `Idle`/`Stopped`).
//! - The platform [`Sampler`] (Windows polls; other platforms stub).
//! - A shared `rusqlite::Connection` for persist.
//!
//! Cheap to clone (Arc-backed). One instance is `app.manage()`'d at
//! boot via [`spawn`]. The Command Center calls `start()` / `stop()`
//! via its `dispatch_start(Activity)` / `dispatch_stop(Activity)`
//! paths; the IPC commands in [`crate::commands::activity`] expose
//! list/detail/delete for the UI.
//!
//! ## Concurrency model
//!
//! All public API methods acquire the orchestrator mutex briefly,
//! apply the FSM, and dispatch the effect (which may do IO outside
//! the lock). The sampler thread captures a sink closure that
//! routes events back through the orchestrator's `record_event`
//! method — that method also acquires the mutex briefly, but only
//! to read the current session id; the DB write is per-event and
//! drops the lock around the rusqlite call.
//!
//! ## Why no `tokio` / async
//!
//! Mirroring the meeting and dictation runtimes (and the rest of
//! Mockingbird): we use `std::thread` + channels + `Arc<Mutex<>>`.
//! Adding tokio just for two threads (sampler + a deferred WinEvent
//! hook in Wave 2) would balloon the runtime footprint without
//! changing the design.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use rusqlite::Connection;

use crate::error::{AppError, AppResult};

use super::audio::{self, AudioPipeline};
use super::block_audio_stitcher::TranscriptChannel;
use super::exclusion::ExclusionMatcher;
use super::lifecycle::{
    apply as lifecycle_apply, LifecycleEffect, LifecycleInput, LifecycleState, Transition,
};
use super::persist::{
    finalize_session, insert_event, insert_session_with_options, set_session_audio_provenance,
    SessionStatus,
};
use super::sampler::{make_default_sampler, Sampler, SamplerEvent};
use super::segments_persist::insert_segments_bulk;

/// Closure shape for the audio-pipeline factory. Production uses
/// [`audio::start_default`]; tests inject a stub builder.
pub type AudioFactory =
    Box<dyn Fn(&str, &Path) -> AppResult<Box<dyn AudioPipeline + Send>> + Send + Sync>;

/// Shared activity-capture handle. Clone-cheap.
#[derive(Clone)]
pub struct ActivityCaptureRuntime {
    inner: Arc<Inner>,
}

struct Inner {
    state: Mutex<RuntimeState>,
    conn: Arc<Mutex<Connection>>,
    sampler: Mutex<Box<dyn Sampler>>,
    /// Parent directory under which per-session audio chunk
    /// subdirectories are created. Mirrors the meetings runtime's
    /// `chunk_base_dir` shape. Empty `PathBuf` is a sentinel meaning
    /// "audio capture not wired" — the unit tests use this default.
    audio_chunk_base_dir: PathBuf,
    /// Factory closure that builds an audio pipeline per session.
    /// Production wires [`audio::start_default`]; tests override.
    audio_factory: AudioFactory,
    /// Phase 10 Wave 5 — exclusion matcher (ADR 0043). Consulted by
    /// [`record_event`] before INSERTing the row, so excluded events
    /// never touch disk. Reloadable via [`reload_exclusion_rules`]
    /// when the IPC layer edits a rule.
    exclusion_matcher: RwLock<ExclusionMatcher>,
}

/// All of the mutable orchestrator state that needs to move together
/// across transitions. One mutex guards the whole tuple, so the FSM
/// step is always atomic with the session-id read.
struct RuntimeState {
    lifecycle: LifecycleState,
    current_session: Option<String>,
    /// Set by [`ActivityCaptureRuntime::start_with_audio`] just before
    /// the FSM step that drives `OpenSession`. Read by
    /// `run_open_session`, then cleared. Carrying it on the state
    /// rather than as an arg lets us reuse the existing
    /// `LifecycleInput::Start` shape without adding a new variant
    /// (and without inventing a parallel `StartWithAudio` lifecycle
    /// path which would multiply the FSM transition matrix).
    pending_audio: bool,
    /// Active audio pipeline for the in-flight session. `None` when
    /// the session is non-audio OR when no session is in flight.
    audio_pipeline: Option<Box<dyn AudioPipeline + Send>>,
}

impl ActivityCaptureRuntime {
    /// Spin up the runtime with the platform-default sampler.
    /// `audio_chunk_base_dir` is the parent directory under which
    /// per-session audio chunk subdirs land; pass an empty PathBuf
    /// to disable audio entirely (the toggle still works, the
    /// pipeline just fails fast).
    pub fn spawn(conn: Arc<Mutex<Connection>>, audio_chunk_base_dir: PathBuf) -> Self {
        Self::with_components(
            conn,
            make_default_sampler(),
            audio_chunk_base_dir,
            default_audio_factory(),
        )
    }

    /// Spin up with a caller-provided sampler. Tests pass
    /// [`InertSampler`]; production goes through [`Self::spawn`].
    /// Audio defaults to the production factory + an empty chunk dir.
    pub fn with_sampler(conn: Arc<Mutex<Connection>>, sampler: Box<dyn Sampler>) -> Self {
        Self::with_components(conn, sampler, PathBuf::new(), default_audio_factory())
    }

    /// Full constructor — tests use this when they need a custom
    /// audio factory in addition to a custom sampler. Production
    /// path is [`Self::spawn`].
    pub fn with_components(
        conn: Arc<Mutex<Connection>>,
        sampler: Box<dyn Sampler>,
        audio_chunk_base_dir: PathBuf,
        audio_factory: AudioFactory,
    ) -> Self {
        // Best-effort initial matcher load. If the DB read fails
        // (shouldn't, but rusqlite errors are recoverable), we start
        // with an empty matcher and let the IPC layer trigger a
        // reload via `reload_exclusion_rules`. Privacy-by-default
        // posture would be "refuse to capture" but that's worse UX
        // for the more likely "first-launch, table just got seeded"
        // path — and the built-in rules are pre-seeded by migration
        // 015 so the empty case is genuinely rare.
        let initial_matcher = match conn.lock() {
            Ok(c) => ExclusionMatcher::load(&c).unwrap_or_else(|e| {
                tracing::warn!(
                    target: "activity::exclusion",
                    error = %e,
                    "failed to load exclusion matcher at spawn; starting empty"
                );
                ExclusionMatcher::empty()
            }),
            Err(_) => ExclusionMatcher::empty(),
        };
        Self {
            inner: Arc::new(Inner {
                state: Mutex::new(RuntimeState {
                    lifecycle: LifecycleState::Idle,
                    current_session: None,
                    pending_audio: false,
                    audio_pipeline: None,
                }),
                conn,
                sampler: Mutex::new(sampler),
                audio_chunk_base_dir,
                audio_factory,
                exclusion_matcher: RwLock::new(initial_matcher),
            }),
        }
    }

    /// Reload the exclusion matcher from the DB. The IPC layer calls
    /// this after editing a rule (create/update/delete/toggle).
    pub fn reload_exclusion_rules(&self) -> AppResult<()> {
        let conn = self.inner.conn.lock().map_err(|_| {
            AppError::Activity("db mutex poisoned during exclusion reload".to_string())
        })?;
        let new_matcher = ExclusionMatcher::load(&conn)?;
        if let Ok(mut g) = self.inner.exclusion_matcher.write() {
            tracing::debug!(
                target: "activity::exclusion",
                count = new_matcher.len(),
                "exclusion matcher reloaded"
            );
            *g = new_matcher;
        }
        Ok(())
    }

    /// User clicked "Activity" in the Command Center. Audio is OFF
    /// for sessions opened this way.
    pub fn start(&self) -> AppResult<()> {
        self.start_with_audio(false)
    }

    /// Start a session with the Wave-4 audio toggle in the requested
    /// state. The Command Center + the IPC layer read the
    /// `activity_audio_enabled` setting and pass the result here.
    /// FSM-equivalent to [`Self::start`] otherwise.
    pub fn start_with_audio(&self, audio_enabled: bool) -> AppResult<()> {
        // Stash the pending flag before driving the FSM. The OpenSession
        // effect reads it (and clears it) inside the locked critical
        // section of `run_open_session`.
        {
            let mut g =
                self.inner.state.lock().map_err(|_| {
                    AppError::Activity("activity runtime mutex poisoned".to_string())
                })?;
            g.pending_audio = audio_enabled;
        }
        self.drive(LifecycleInput::Start)
    }

    /// User clicked Pause in the Command Center SessionCard.
    pub fn pause(&self) -> AppResult<()> {
        self.drive(LifecycleInput::Pause)
    }

    /// User clicked Resume.
    pub fn resume(&self) -> AppResult<()> {
        self.drive(LifecycleInput::Resume)
    }

    /// User clicked Stop.
    pub fn stop(&self) -> AppResult<()> {
        self.drive(LifecycleInput::Stop)
    }

    /// Process shutdown hook.
    pub fn shutdown(&self) -> AppResult<()> {
        self.drive(LifecycleInput::ShutdownRequested)
    }

    /// Snapshot of the current lifecycle state (test + IPC).
    pub fn lifecycle_state(&self) -> LifecycleState {
        self.inner
            .state
            .lock()
            .map(|g| g.lifecycle)
            .unwrap_or(LifecycleState::Idle)
    }

    /// Snapshot of the current session id (None when not Active/Paused).
    pub fn current_session_id(&self) -> Option<String> {
        self.inner
            .state
            .lock()
            .ok()
            .and_then(|g| g.current_session.clone())
    }

    // ----------------------------------------------------------------
    // Internal: FSM drive + effect dispatch
    // ----------------------------------------------------------------

    fn drive(&self, input: LifecycleInput) -> AppResult<()> {
        let (Transition { next, effect }, current_session) = {
            let mut g =
                self.inner.state.lock().map_err(|_| {
                    AppError::Activity("activity runtime mutex poisoned".to_string())
                })?;
            let t = lifecycle_apply(g.lifecycle, input);
            g.lifecycle = t.next;
            // We may need to mutate current_session after running the
            // effect; clone what we have right now to pass into the
            // effect runner without holding the lock.
            (t, g.current_session.clone())
        };

        tracing::debug!(
            target: "activity",
            ?input,
            ?next,
            ?effect,
            ?current_session,
            "activity fsm step"
        );

        match effect {
            LifecycleEffect::None => {}
            LifecycleEffect::OpenSession => self.run_open_session()?,
            LifecycleEffect::EmitPausedEvent => {
                self.run_emit_control_event(current_session.as_deref(), "paused")?;
                if let Ok(s) = self.inner.sampler.lock() {
                    s.set_paused(true);
                }
            }
            LifecycleEffect::EmitResumedEvent => {
                self.run_emit_control_event(current_session.as_deref(), "resumed")?;
                if let Ok(s) = self.inner.sampler.lock() {
                    s.set_paused(false);
                }
            }
            LifecycleEffect::CloseSession => {
                self.run_close_session(current_session.as_deref(), SessionStatus::Completed)?;
            }
            LifecycleEffect::CloseSessionForShutdown => {
                self.run_close_session(current_session.as_deref(), SessionStatus::Partial)?;
            }
        }

        Ok(())
    }

    fn run_open_session(&self) -> AppResult<()> {
        let started_at = now_ms();

        // Take the pending-audio flag (set by start_with_audio).
        // Clearing it here means a future Start call must re-set it,
        // so a stale flag from a previous session can never bleed in.
        let audio_enabled = {
            let mut g =
                self.inner.state.lock().map_err(|_| {
                    AppError::Activity("activity runtime mutex poisoned".to_string())
                })?;
            std::mem::replace(&mut g.pending_audio, false)
        };

        let id = {
            let conn = self.inner.conn.lock().map_err(|_| {
                AppError::Activity("db mutex poisoned during open_session".to_string())
            })?;
            insert_session_with_options(&conn, started_at, audio_enabled)?
        };

        // Stash the new id BEFORE we start the sampler — the
        // sampler's first event might race the orchestrator's
        // session-id assignment otherwise.
        {
            let mut g =
                self.inner.state.lock().map_err(|_| {
                    AppError::Activity("activity runtime mutex poisoned".to_string())
                })?;
            g.current_session = Some(id.clone());
        }

        // Spawn the audio pipeline if the user requested audio for
        // this session. Failure is non-fatal: the visual timeline
        // still runs; we just emit a `layer_error` event and continue.
        // This matches the sampler's degradation contract below.
        if audio_enabled {
            match (self.inner.audio_factory)(&id, &self.inner.audio_chunk_base_dir) {
                Ok(pipeline) => {
                    if let Ok(mut g) = self.inner.state.lock() {
                        g.audio_pipeline = Some(pipeline);
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        target: "activity",
                        error = %e,
                        %id,
                        "audio pipeline failed to start; session continues without audio"
                    );
                    let _ = self.emit_layer_error(
                        &id,
                        &format!("audio pipeline failed: {e}"),
                        started_at,
                    );
                }
            }
        }

        // Spawn the sampler. Failure here is non-fatal: we still
        // have an `in_progress` session row, the user can stop it
        // explicitly via the Command Center, and the timeline will
        // be empty (Wave 2 may upgrade this to a `layer_error` row).
        let me = self.clone();
        let sink = Box::new(move |ev: SamplerEvent| {
            if let Err(e) = me.record_event(ev) {
                tracing::warn!(target: "activity", error = ?e, "record_event failed");
            }
        });
        if let Ok(mut s) = self.inner.sampler.lock() {
            if let Err(e) = s.start(sink) {
                tracing::warn!(
                    target: "activity",
                    error = ?e,
                    %id,
                    "sampler failed to start; session continues without events"
                );
            }
        }
        tracing::info!(
            target: "activity",
            session_id = %id,
            "activity session opened"
        );
        Ok(())
    }

    fn run_emit_control_event(&self, session_id: Option<&str>, kind: &str) -> AppResult<()> {
        let Some(sid) = session_id else {
            tracing::debug!(
                target: "activity",
                kind = %kind,
                "control event with no active session — dropping"
            );
            return Ok(());
        };
        let conn = self.inner.conn.lock().map_err(|_| {
            AppError::Activity("db mutex poisoned during control event".to_string())
        })?;
        insert_event(&conn, sid, now_ms(), kind, None, None, None)?;
        Ok(())
    }

    fn run_close_session(&self, session_id: Option<&str>, status: SessionStatus) -> AppResult<()> {
        // Stop the sampler regardless of whether we have a session
        // id (cheap; the sampler's stop is idempotent).
        if let Ok(mut s) = self.inner.sampler.lock() {
            s.stop();
        }

        // Take the audio pipeline (if any) OUT of the mutex before
        // calling .stop() — stop() can block for seconds joining the
        // long-form worker thread, and we don't want to hold the
        // runtime lock for that long.
        let pipeline = if let Ok(mut g) = self.inner.state.lock() {
            g.audio_pipeline.take()
        } else {
            None
        };

        let Some(sid) = session_id else {
            // No session in flight; drop the pipeline so we don't
            // leak any thread.
            if let Some(p) = pipeline {
                let _ = p.stop();
            }
            tracing::debug!(
                target: "activity",
                "close requested with no active session — nothing to do"
            );
            return Ok(());
        };

        // Persist audio results FIRST (before finalize_session changes
        // the session row's `status`) so the row's audit + provenance
        // make sense in any partial-failure case.
        if let Some(p) = pipeline {
            self.persist_audio_results(sid, p);
        }

        {
            let conn =
                self.inner.conn.lock().map_err(|_| {
                    AppError::Activity("db mutex poisoned during close".to_string())
                })?;
            finalize_session(&conn, sid, now_ms(), status)?;
        }
        // Clear the in-memory session id so the next Start can take.
        if let Ok(mut g) = self.inner.state.lock() {
            g.current_session = None;
            // Reset terminal Stopped → Idle so the user can start
            // another session without an orchestrator restart.
            g.lifecycle = LifecycleState::Idle;
        }
        tracing::info!(
            target: "activity",
            session_id = %sid,
            ?status,
            "activity session closed"
        );
        Ok(())
    }

    /// Drain the audio pipeline, persist segments + provenance.
    /// Errors are logged + swallowed because a session that produced
    /// no audio (Whisper failed, mic mute) should still finalize.
    fn persist_audio_results(&self, session_id: &str, pipeline: Box<dyn AudioPipeline + Send>) {
        let output = match pipeline.stop() {
            Ok(o) => o,
            Err(e) => {
                tracing::warn!(
                    target: "activity::audio",
                    error = %e,
                    %session_id,
                    "audio pipeline stop failed; no segments persisted"
                );
                return;
            }
        };
        let now = now_ms();

        // Look up the session's started_at so we can shift the
        // capture-relative (0-based) Whisper timestamps into the
        // epoch-ms coordinate system the blocker / stitcher use.
        // Without this, stitching would always fail (segments would
        // sit in [0, duration] while Blocks live near now()).
        let session_started_at_ms: i64 = match self.inner.conn.lock() {
            Ok(conn) => conn
                .query_row(
                    "SELECT started_at FROM activity_sessions WHERE id = ?1",
                    rusqlite::params![session_id],
                    |r| r.get(0),
                )
                .unwrap_or(0),
            Err(_) => 0,
        };

        // Convert RawSegments to the persist tuple shape (now epoch-ms).
        // We do this outside the DB lock to keep the critical section short.
        let rows: Vec<(i64, i64, String, TranscriptChannel)> = output
            .segments
            .into_iter()
            .map(|s| {
                (
                    session_started_at_ms.saturating_add(s.started_at_ms),
                    session_started_at_ms.saturating_add(s.ended_at_ms),
                    s.text,
                    s.channel,
                )
            })
            .collect();
        let segment_count = rows.len();

        let res = (|| -> AppResult<()> {
            let mut conn = self.inner.conn.lock().map_err(|_| {
                AppError::Activity("db mutex poisoned during audio persist".to_string())
            })?;
            if !rows.is_empty() {
                insert_segments_bulk(&mut conn, session_id, &rows, now)?;
            }
            if let Some(prov) = &output.provenance {
                set_session_audio_provenance(
                    &conn,
                    session_id,
                    &prov.whisper_model,
                    prov.chunk_window_ms,
                    now,
                )?;
            }
            Ok(())
        })();
        match res {
            Ok(()) => tracing::info!(
                target: "activity::audio",
                %session_id,
                segment_count,
                "persisted activity audio segments"
            ),
            Err(e) => tracing::warn!(
                target: "activity::audio",
                error = %e,
                %session_id,
                "failed to persist audio segments / provenance"
            ),
        }
    }

    /// Helper that emits a `layer_error` event under the current
    /// session id. Used when a sub-layer (audio, sampler) fails at
    /// startup so the timeline records the degradation rather than
    /// silently losing context.
    fn emit_layer_error(&self, session_id: &str, message: &str, ts_ms: i64) -> AppResult<()> {
        let conn = self.inner.conn.lock().map_err(|_| {
            AppError::Activity("db mutex poisoned during layer_error emit".to_string())
        })?;
        insert_event(
            &conn,
            session_id,
            ts_ms,
            "layer_error",
            None,
            None,
            Some(&format!("{{\"message\":{}}}", json_escape_string(message))),
        )?;
        Ok(())
    }

    /// Called by the sampler thread (via its sink closure) per event.
    /// Persists the event under the current session id; silently
    /// drops if no session is active (the sampler may emit one more
    /// event after `stop()` returns, depending on thread scheduling).
    pub fn record_event(&self, ev: SamplerEvent) -> AppResult<()> {
        let sid = self.current_session_id();
        let Some(sid) = sid else {
            tracing::trace!(
                target: "activity",
                ?ev,
                "sampler event with no active session — dropping"
            );
            return Ok(());
        };
        let conn =
            self.inner.conn.lock().map_err(|_| {
                AppError::Activity("db mutex poisoned during record_event".to_string())
            })?;
        match ev {
            SamplerEvent::AppSwitch { app, title, ts_ms } => {
                // ADR 0043 — exclusion check BEFORE insert. AppSwitch
                // has no snapshot_json so password_field_active = false.
                if let Some(hit) = self.check_excluded(Some(&app), Some(&title), false) {
                    tracing::debug!(
                        target: "activity::exclusion",
                        rule_id = hit,
                        %app, %title,
                        "app_switch dropped by exclusion rule"
                    );
                    return Ok(());
                }
                insert_event(
                    &conn,
                    &sid,
                    ts_ms,
                    "app_switch",
                    Some(&app),
                    Some(&title),
                    None,
                )?;
            }
            SamplerEvent::ContextSnapshot {
                app,
                title,
                ts_ms,
                snapshot_json,
            } => {
                // ADR 0043 — exclusion check BEFORE insert. Parse out
                // the password_field_active bit from snapshot_json so
                // the system:password_field_active rule kind works.
                let pwd_field = extract_password_field_active(&snapshot_json);
                if let Some(rule_id) = self.check_excluded(Some(&app), Some(&title), pwd_field) {
                    tracing::debug!(
                        target: "activity::exclusion",
                        rule_id = rule_id,
                        %app, %title, pwd_field,
                        "context_snapshot dropped by exclusion rule"
                    );
                    return Ok(());
                }
                // Wave 2: the sampler builds the JSON payload (it owns
                // the platform-specific UIA probe). We just persist it.
                insert_event(
                    &conn,
                    &sid,
                    ts_ms,
                    "context_snapshot",
                    Some(&app),
                    Some(&title),
                    Some(&snapshot_json),
                )?;
            }
            SamplerEvent::IdleStart { ts_ms } => {
                insert_event(&conn, &sid, ts_ms, "idle_start", None, None, None)?;
            }
            SamplerEvent::IdleEnd { ts_ms } => {
                insert_event(&conn, &sid, ts_ms, "idle_end", None, None, None)?;
            }
            SamplerEvent::LayerError { message, ts_ms } => {
                insert_event(
                    &conn,
                    &sid,
                    ts_ms,
                    "layer_error",
                    None,
                    None,
                    Some(&format!("{{\"message\":{}}}", json_escape_string(&message))),
                )?;
            }
        }
        Ok(())
    }
}

/// Parse `passwordFieldActive` out of a context-snapshot JSON
/// payload. Used by the exclusion matcher to honor the
/// `system:password_field_active` rule kind. Returns `false` on any
/// parse failure (safe default — we don't claim a password field is
/// active when we can't tell).
fn extract_password_field_active(snapshot_json: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(snapshot_json)
        .ok()
        .and_then(|v| {
            v.get("passwordFieldActive")
                .or_else(|| v.get("password_field_active"))
                .and_then(|b| b.as_bool())
        })
        .unwrap_or(false)
}

impl ActivityCaptureRuntime {
    /// Helper: consult the matcher under a read lock and return the
    /// matched rule id (for logging) if any rule fires.
    fn check_excluded(
        &self,
        app: Option<&str>,
        title: Option<&str>,
        password_field_active: bool,
    ) -> Option<String> {
        let g = self.inner.exclusion_matcher.read().ok()?;
        g.matches(app, title, password_field_active)
            .map(|hit| hit.rule_id.to_string())
    }
}

/// Minimal JSON-string escaper for the snapshot payload. We don't
/// pull `serde_json` for a two-field object that's well within the
/// "DIY is cheaper than the dep" boundary (and `serde_json::to_string`
/// would need its own error path). Backslash + double-quote + control
/// chars get the standard `\uXXXX` treatment; everything else passes.
fn json_escape_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The production audio factory — delegates to
/// [`audio::start_default`]. Tests substitute their own via
/// [`ActivityCaptureRuntime::with_components`].
fn default_audio_factory() -> AudioFactory {
    Box::new(
        |session_id: &str, base_dir: &Path| -> AppResult<Box<dyn AudioPipeline + Send>> {
            audio::start_default(session_id, base_dir)
        },
    )
}

fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity::persist::get_session_detail;
    use crate::activity::sampler::SamplerSink;
    use crate::db::migrations;

    /// A pure in-memory sampler so the runtime tests don't actually
    /// poll the OS. start() stashes the sink so the test can drive
    /// it explicitly.
    struct InertSampler {
        paused: std::sync::atomic::AtomicBool,
    }
    impl InertSampler {
        fn new() -> Self {
            Self {
                paused: std::sync::atomic::AtomicBool::new(false),
            }
        }
    }
    impl Sampler for InertSampler {
        fn start(&mut self, _sink: SamplerSink) -> AppResult<()> {
            Ok(())
        }
        fn set_paused(&self, paused: bool) {
            self.paused
                .store(paused, std::sync::atomic::Ordering::Relaxed);
        }
        fn stop(&mut self) {}
    }

    fn fresh_runtime() -> (ActivityCaptureRuntime, Arc<Mutex<Connection>>) {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        migrations::apply_all(&conn).unwrap();
        let conn_arc = Arc::new(Mutex::new(conn));
        let rt =
            ActivityCaptureRuntime::with_sampler(conn_arc.clone(), Box::new(InertSampler::new()));
        (rt, conn_arc)
    }

    #[test]
    fn start_sets_active_and_creates_session_row() {
        let (rt, conn) = fresh_runtime();
        rt.start().unwrap();
        assert_eq!(rt.lifecycle_state(), LifecycleState::Active);
        assert!(rt.current_session_id().is_some());
        let n: i64 = conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM activity_sessions WHERE status='in_progress'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn pause_emits_paused_event_then_resume_emits_resumed() {
        let (rt, conn) = fresh_runtime();
        rt.start().unwrap();
        rt.pause().unwrap();
        assert_eq!(rt.lifecycle_state(), LifecycleState::Paused);
        rt.resume().unwrap();
        assert_eq!(rt.lifecycle_state(), LifecycleState::Active);

        let n: i64 = conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM activity_events WHERE kind IN ('paused','resumed')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 2);
    }

    #[test]
    fn stop_finalizes_session_and_resets_to_idle() {
        let (rt, _conn) = fresh_runtime();
        rt.start().unwrap();
        let sid = rt.current_session_id().unwrap();
        rt.stop().unwrap();
        assert_eq!(rt.lifecycle_state(), LifecycleState::Idle);
        assert!(rt.current_session_id().is_none());

        // The row should now be 'completed'.
        let (rt2, conn) = (rt, _conn);
        let detail = get_session_detail(&conn.lock().unwrap(), &sid)
            .unwrap()
            .unwrap();
        assert_eq!(detail.session.status, SessionStatus::Completed);
        let _ = rt2;
    }

    #[test]
    fn shutdown_finalizes_with_partial_status() {
        let (rt, conn) = fresh_runtime();
        rt.start().unwrap();
        let sid = rt.current_session_id().unwrap();
        rt.shutdown().unwrap();
        let detail = get_session_detail(&conn.lock().unwrap(), &sid)
            .unwrap()
            .unwrap();
        assert_eq!(detail.session.status, SessionStatus::Partial);
    }

    #[test]
    fn double_start_is_noop_and_keeps_same_session() {
        let (rt, _) = fresh_runtime();
        rt.start().unwrap();
        let sid1 = rt.current_session_id().unwrap();
        rt.start().unwrap(); // idempotent per FSM
        let sid2 = rt.current_session_id().unwrap();
        assert_eq!(sid1, sid2);
    }

    #[test]
    fn record_event_persists_app_switch_with_payload() {
        let (rt, conn) = fresh_runtime();
        rt.start().unwrap();
        let sid = rt.current_session_id().unwrap();
        rt.record_event(SamplerEvent::AppSwitch {
            app: "chrome.exe".into(),
            title: "Tabs".into(),
            ts_ms: 1234,
        })
        .unwrap();
        let detail = get_session_detail(&conn.lock().unwrap(), &sid)
            .unwrap()
            .unwrap();
        assert!(detail
            .events
            .iter()
            .any(|e| e.kind == "app_switch" && e.app_name.as_deref() == Some("chrome.exe")));
    }

    #[test]
    fn record_event_with_no_active_session_is_dropped() {
        let (rt, conn) = fresh_runtime();
        rt.record_event(SamplerEvent::AppSwitch {
            app: "a.exe".into(),
            title: "b".into(),
            ts_ms: 1,
        })
        .unwrap();
        let n: i64 = conn
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM activity_events", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn context_snapshot_persists_sampler_built_payload_verbatim() {
        // Wave 2: the sampler hands the runtime a pre-built JSON
        // payload (it owns the platform-specific UIA probe). The
        // runtime's only job is to write it to the column unchanged.
        let (rt, conn) = fresh_runtime();
        rt.start().unwrap();
        let sid = rt.current_session_id().unwrap();
        let payload = r#"{"schema":"v2","app":"notepad.exe","title":"T","status":{"kind":"ok"}}"#;
        rt.record_event(SamplerEvent::ContextSnapshot {
            app: "notepad.exe".into(),
            title: "T".into(),
            ts_ms: 100,
            snapshot_json: payload.to_string(),
        })
        .unwrap();
        let detail = get_session_detail(&conn.lock().unwrap(), &sid)
            .unwrap()
            .unwrap();
        let snap = detail
            .events
            .iter()
            .find(|e| e.kind == "context_snapshot")
            .unwrap();
        assert_eq!(snap.snapshot_json.as_deref(), Some(payload));
        // Should still be valid JSON for the UI to parse.
        let v: serde_json::Value =
            serde_json::from_str(snap.snapshot_json.as_ref().unwrap()).expect("valid JSON");
        assert_eq!(v["schema"], "v2");
    }

    #[test]
    fn record_event_handles_idle_start_and_idle_end() {
        let (rt, conn) = fresh_runtime();
        rt.start().unwrap();
        rt.record_event(SamplerEvent::IdleStart { ts_ms: 500 })
            .unwrap();
        rt.record_event(SamplerEvent::IdleEnd { ts_ms: 700 })
            .unwrap();
        let n: i64 = conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM activity_events WHERE kind IN ('idle_start','idle_end')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 2);
    }

    // ----------------------------------------------------------
    // Phase 10 Wave 6.B — exclusion-is-total judge fixtures (C3, C4).
    // ADR 0043 (capture-time enforcement + hot reload).
    // ----------------------------------------------------------

    /// Replace whatever exclusion rules `apply_all` seeded with the
    /// caller-supplied list. Returns after the runtime's matcher has
    /// been reloaded from the new state.
    fn replace_exclusion_rules(
        rt: &ActivityCaptureRuntime,
        conn: &Arc<Mutex<Connection>>,
        rules: &[(&str, &str, &str)], // (id, kind, pattern)
    ) {
        {
            let c = conn.lock().unwrap();
            c.execute("DELETE FROM activity_exclusion_rules", [])
                .unwrap();
            for (id, kind, pattern) in rules {
                c.execute(
                    "INSERT INTO activity_exclusion_rules \
                     (id, kind, pattern, enabled, is_builtin, note, created_at, updated_at) \
                     VALUES (?1, ?2, ?3, 1, 0, NULL, 0, 0)",
                    rusqlite::params![id, kind, pattern],
                )
                .unwrap();
            }
        }
        rt.reload_exclusion_rules().unwrap();
    }

    #[test]
    fn record_event_drops_matched_rows() {
        let (rt, conn) = fresh_runtime();
        // The runtime construction already loaded the migration-015
        // built-ins (8 rules, all enabled). That's the realistic
        // posture for this judge.
        rt.start().unwrap();
        let sid = rt.current_session_id().unwrap();

        // (a) Plain app event — nothing about it matches a built-in.
        rt.record_event(SamplerEvent::AppSwitch {
            app: "Notepad.exe".into(),
            title: "x".into(),
            ts_ms: 1_000,
        })
        .unwrap();
        // (b) 1Password — must be dropped by `builtin-1password`.
        rt.record_event(SamplerEvent::AppSwitch {
            app: "1Password 7".into(),
            title: "Vault".into(),
            ts_ms: 1_100,
        })
        .unwrap();
        // (c) Password-field-active snapshot — must be dropped by
        //     `builtin-secure-input` regardless of app/title.
        let snapshot_json =
            r#"{"schema":"v2","app":"chrome.exe","title":"ok","passwordFieldActive":true}"#;
        rt.record_event(SamplerEvent::ContextSnapshot {
            app: "chrome.exe".into(),
            title: "ok".into(),
            ts_ms: 1_200,
            snapshot_json: snapshot_json.to_string(),
        })
        .unwrap();

        let c = conn.lock().unwrap();
        // 1Password / consent.exe / LogonUI.exe rows: must be zero.
        let pwd_apps: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM activity_events \
                 WHERE app_name LIKE '1Password%' \
                    OR app_name = 'consent.exe' \
                    OR app_name = 'LogonUI.exe'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            pwd_apps, 0,
            "password-manager / UAC events must be excluded"
        );

        // Password-field-active snapshots: must be zero.
        let pwd_field: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM activity_events \
                 WHERE snapshot_json LIKE '%\"passwordFieldActive\":true%'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            pwd_field, 0,
            "password-field-active snapshots must be excluded"
        );

        // Positive control — the matcher is not a sledgehammer.
        let kept: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM activity_events \
                 WHERE session_id = ?1 AND app_name = 'Notepad.exe'",
                rusqlite::params![&sid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(kept, 1, "Notepad event should have been persisted");
    }

    #[test]
    fn reload_exclusion_rules_no_leak_across_window() {
        let (rt, conn) = fresh_runtime();

        // Window 1: ONLY "Foo*" enabled — wipe migration seed + insert.
        replace_exclusion_rules(&rt, &conn, &[("r-foo", "app_glob", "Foo*")]);

        rt.start().unwrap();
        let sid = rt.current_session_id().unwrap();

        // "Bar" persists (no Bar* rule yet).
        rt.record_event(SamplerEvent::AppSwitch {
            app: "Bar".into(),
            title: "t".into(),
            ts_ms: 1_000,
        })
        .unwrap();
        // "Foo 1" dropped.
        rt.record_event(SamplerEvent::AppSwitch {
            app: "Foo 1".into(),
            title: "t".into(),
            ts_ms: 1_100,
        })
        .unwrap();

        // Window 2: add Bar* rule + reload. "Bar" should now drop.
        {
            let c = conn.lock().unwrap();
            c.execute(
                "INSERT INTO activity_exclusion_rules \
                 (id, kind, pattern, enabled, is_builtin, note, created_at, updated_at) \
                 VALUES ('r-bar', 'app_glob', 'Bar*', 1, 0, NULL, 1, 1)",
                [],
            )
            .unwrap();
        }
        rt.reload_exclusion_rules().unwrap();
        rt.record_event(SamplerEvent::AppSwitch {
            app: "Bar".into(),
            title: "t".into(),
            ts_ms: 1_200,
        })
        .unwrap();

        // Window 3: disable Bar* + reload. "Bar" should persist again.
        {
            let c = conn.lock().unwrap();
            c.execute(
                "UPDATE activity_exclusion_rules SET enabled = 0 WHERE id = 'r-bar'",
                [],
            )
            .unwrap();
        }
        rt.reload_exclusion_rules().unwrap();
        rt.record_event(SamplerEvent::AppSwitch {
            app: "Bar".into(),
            title: "t".into(),
            ts_ms: 1_300,
        })
        .unwrap();

        // Tally: exactly TWO "Bar" rows (windows 1 + 3), zero "Foo" rows ever.
        let c = conn.lock().unwrap();
        let bar: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM activity_events \
                 WHERE session_id = ?1 AND app_name = 'Bar'",
                rusqlite::params![&sid],
                |r| r.get(0),
            )
            .unwrap();
        let foo: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM activity_events \
                 WHERE session_id = ?1 AND app_name LIKE 'Foo%'",
                rusqlite::params![&sid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(bar, 2, "Bar rows from windows 1+3 should survive");
        assert_eq!(foo, 0, "Foo* events should never persist");
    }
}
