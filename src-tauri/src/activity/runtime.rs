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

use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use crate::error::{AppError, AppResult};

use super::lifecycle::{
    apply as lifecycle_apply, LifecycleEffect, LifecycleInput, LifecycleState, Transition,
};
use super::persist::{finalize_session, insert_event, insert_session, SessionStatus};
use super::sampler::{make_default_sampler, Sampler, SamplerEvent};

/// Shared activity-capture handle. Clone-cheap.
#[derive(Clone)]
pub struct ActivityCaptureRuntime {
    inner: Arc<Inner>,
}

struct Inner {
    state: Mutex<RuntimeState>,
    conn: Arc<Mutex<Connection>>,
    sampler: Mutex<Box<dyn Sampler>>,
}

/// All of the mutable orchestrator state that needs to move together
/// across transitions. One mutex guards the whole tuple, so the FSM
/// step is always atomic with the session-id read.
struct RuntimeState {
    lifecycle: LifecycleState,
    current_session: Option<String>,
}

impl ActivityCaptureRuntime {
    /// Spin up the runtime with the platform-default sampler.
    pub fn spawn(conn: Arc<Mutex<Connection>>) -> Self {
        Self::with_sampler(conn, make_default_sampler())
    }

    /// Spin up with a caller-provided sampler. Used by tests.
    pub fn with_sampler(conn: Arc<Mutex<Connection>>, sampler: Box<dyn Sampler>) -> Self {
        Self {
            inner: Arc::new(Inner {
                state: Mutex::new(RuntimeState {
                    lifecycle: LifecycleState::Idle,
                    current_session: None,
                }),
                conn,
                sampler: Mutex::new(sampler),
            }),
        }
    }

    /// User clicked "Activity" in the Command Center.
    pub fn start(&self) -> AppResult<()> {
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
        let id = {
            let conn = self.inner.conn.lock().map_err(|_| {
                AppError::Activity("db mutex poisoned during open_session".to_string())
            })?;
            insert_session(&conn, started_at)?
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

        let Some(sid) = session_id else {
            tracing::debug!(
                target: "activity",
                "close requested with no active session — nothing to do"
            );
            return Ok(());
        };
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
            SamplerEvent::ContextSnapshot { app, title, ts_ms } => {
                let payload = format!(
                    "{{\"app\":{},\"title\":{}}}",
                    json_escape_string(&app),
                    json_escape_string(&title)
                );
                insert_event(
                    &conn,
                    &sid,
                    ts_ms,
                    "context_snapshot",
                    Some(&app),
                    Some(&title),
                    Some(&payload),
                )?;
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
    fn context_snapshot_emits_json_payload_with_escaping() {
        let (rt, conn) = fresh_runtime();
        rt.start().unwrap();
        let sid = rt.current_session_id().unwrap();
        rt.record_event(SamplerEvent::ContextSnapshot {
            app: "notepad.exe".into(),
            title: r#"Untitled - "quoted" \ backslash"#.into(),
            ts_ms: 100,
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
        let json = snap.snapshot_json.as_ref().unwrap();
        // Round-trip via serde_json to assert it's actually valid JSON.
        let v: serde_json::Value = serde_json::from_str(json).expect("valid JSON");
        assert_eq!(v["app"], "notepad.exe");
        assert_eq!(v["title"], r#"Untitled - "quoted" \ backslash"#);
    }
}
