//! Foreground-window sampler.
//!
//! Wave 1B scope: titles-only. We poll the platform's foreground
//! window at ~1 Hz, dedupe by (app, title), and emit:
//!
//! - `app_switch` whenever the `(app_name, window_title)` tuple
//!   changes since the previous sample (or there was no previous
//!   sample);
//! - `context_snapshot` periodically with the same payload — Wave 1B
//!   emits one snapshot per app_switch (so the timeline view has a
//!   stable "current context" marker even when nothing's changing).
//!   Wave 2 will adjust this when richer UIA payloads land.
//!
//! Wave 2 will add a `SetWinEventHook(EVENT_SYSTEM_FOREGROUND, …)`
//! fast path so we react to focus changes immediately rather than
//! waiting up to a second. The 1Hz polling stays as the steady-state
//! fallback when the hook is silent.
//!
//! ## Cross-platform discipline
//!
//! Per Principle 5 (cross-platform from day one): the [`Sampler`]
//! trait is target-agnostic; the Windows implementation lives below
//! a `#[cfg(target_os = "windows")]` block. macOS / Linux stubs in
//! `sampler_macos.rs` + `sampler_linux.rs` return `Err` immediately
//! so the runtime can downgrade to "session row but no events"
//! gracefully (Phase 9 will fill the bodies).

#![allow(missing_docs)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::error::AppResult;
use crate::window_context::{make_default_context, WindowContext};

/// What the sampler emits per tick. Translated by [`super::runtime`]
/// into one or more rows in `activity_events`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SamplerEvent {
    /// `(app_name, window_title)` differs from the previous sample
    /// (or is the first sample).
    AppSwitch {
        app: String,
        title: String,
        /// Unix epoch ms.
        ts_ms: i64,
    },
    /// Snapshot of the current window — Wave 1B emits one per
    /// `AppSwitch`; Wave 2 may emit more.
    ContextSnapshot {
        app: String,
        title: String,
        ts_ms: i64,
    },
    /// Best-effort layer error — the sampler could not read the
    /// foreground window this tick. The orchestrator logs and
    /// persists a `layer_error` row so the gap shows up in the
    /// timeline UI.
    LayerError { message: String, ts_ms: i64 },
}

/// Receiver for sampler events. The runtime owns the consumer side.
pub type SamplerSink = Box<dyn FnMut(SamplerEvent) + Send + 'static>;

/// Trait the runtime uses to control the sampler. The real
/// implementation polls the OS; tests use a hand-rolled fake that
/// fires `SamplerEvent`s on a channel.
pub trait Sampler: Send {
    /// Start sampling on a dedicated thread. The returned handle is
    /// joined by [`stop`]. Idempotent — calling twice is an error
    /// (the runtime never does this, but we surface it loudly).
    fn start(&mut self, sink: SamplerSink) -> AppResult<()>;
    /// Suspend event emission. The sampler thread continues running
    /// (cheap; no platform handles to release) but `sink` is not
    /// called. The runtime flips this when the lifecycle FSM enters
    /// `Paused`.
    fn set_paused(&self, paused: bool);
    /// Stop the sampler thread. Idempotent.
    fn stop(&mut self);
}

/// Construct the platform-default sampler. Windows ships a real impl;
/// other platforms return [`StubSampler`] (writes a single
/// `layer_error` event and noops thereafter).
pub fn make_default_sampler() -> Box<dyn Sampler> {
    #[cfg(target_os = "windows")]
    {
        Box::new(WindowsSampler::new())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Box::new(StubSampler::new(
            "activity sampler not implemented on this platform (Phase 9)",
        ))
    }
}

// ----------------------------------------------------------------------------
// Windows implementation
// ----------------------------------------------------------------------------

#[cfg(target_os = "windows")]
pub struct WindowsSampler {
    paused: Arc<AtomicBool>,
    stop_signal: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

#[cfg(target_os = "windows")]
impl WindowsSampler {
    pub fn new() -> Self {
        Self {
            paused: Arc::new(AtomicBool::new(false)),
            stop_signal: Arc::new(AtomicBool::new(false)),
            thread: None,
        }
    }
}

#[cfg(target_os = "windows")]
impl Default for WindowsSampler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "windows")]
impl Sampler for WindowsSampler {
    fn start(&mut self, mut sink: SamplerSink) -> AppResult<()> {
        if self.thread.is_some() {
            tracing::warn!(target: "activity", "sampler already running; ignoring second start");
            return Ok(());
        }
        let paused = self.paused.clone();
        let stop = self.stop_signal.clone();
        let ctx = make_default_context()?;
        let handle = thread::Builder::new()
            .name("activity-sampler".into())
            .spawn(move || sampler_loop(ctx, paused, stop, &mut sink))
            .map_err(|e| {
                crate::error::AppError::ActivitySampler(format!(
                    "failed to spawn sampler thread: {e}"
                ))
            })?;
        self.thread = Some(handle);
        Ok(())
    }

    fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
    }

    fn stop(&mut self) {
        self.stop_signal.store(true, Ordering::Relaxed);
        if let Some(h) = self.thread.take() {
            let _ = h.join();
        }
    }
}

#[cfg(target_os = "windows")]
impl Drop for WindowsSampler {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(target_os = "windows")]
fn sampler_loop(
    ctx: Box<dyn WindowContext>,
    paused: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    sink: &mut SamplerSink,
) {
    let mut last_seen: Option<(String, String)> = None;
    while !stop.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(1000));
        if stop.load(Ordering::Relaxed) {
            break;
        }
        if paused.load(Ordering::Relaxed) {
            continue;
        }
        let now_ms = now_ms();
        match ctx.foreground() {
            Ok(fg) => {
                let key = (fg.process_name.clone(), fg.title.clone());
                if last_seen.as_ref() != Some(&key) {
                    last_seen = Some(key.clone());
                    sink(SamplerEvent::AppSwitch {
                        app: fg.process_name.clone(),
                        title: fg.title.clone(),
                        ts_ms: now_ms,
                    });
                    sink(SamplerEvent::ContextSnapshot {
                        app: fg.process_name,
                        title: fg.title,
                        ts_ms: now_ms,
                    });
                }
            }
            Err(e) => {
                sink(SamplerEvent::LayerError {
                    message: format!("foreground read failed: {e}"),
                    ts_ms: now_ms,
                });
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// ----------------------------------------------------------------------------
// Non-Windows stub
// ----------------------------------------------------------------------------

/// A sampler that emits ONE `LayerError` describing why no events
/// will follow, then sits idle until [`stop`].
///
/// Used on non-Windows hosts in Wave 1B. The trait is satisfied
/// (start/stop/set_paused all behave) but no foreground polling
/// happens. Phase 9's macOS sampler will replace this on macOS via
/// a `#[cfg]` swap in [`make_default_sampler`].
#[allow(dead_code)] // Constructed in non-windows make_default_sampler path; the cfg gate
                    // hides this from the windows build, hence dead_code here in mock tests.
pub struct StubSampler {
    explanation: String,
    paused: Arc<AtomicBool>,
}

#[allow(dead_code)]
impl StubSampler {
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            explanation: reason.into(),
            paused: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl Sampler for StubSampler {
    fn start(&mut self, mut sink: SamplerSink) -> AppResult<()> {
        // Fire one event so the timeline shows the gap explicitly.
        sink(SamplerEvent::LayerError {
            message: self.explanation.clone(),
            ts_ms: 0,
        });
        Ok(())
    }
    fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
    }
    fn stop(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_sampler_emits_layer_error_on_start() {
        // The stub exists so non-Windows hosts get a single honest
        // LayerError row instead of a silently-empty session. Verify
        // the contract holds.
        let mut s = StubSampler::new("test stub");
        let mut got: Vec<SamplerEvent> = vec![];
        // The sink is called synchronously inside start() — no thread.
        s.start(Box::new(move |e| {
            got.push(e);
        }))
        .unwrap();
        // We can't borrow `got` back through the Box; instead
        // verify by constructing a second start with a channel.
        let (tx, rx) = std::sync::mpsc::channel();
        let mut s2 = StubSampler::new("test stub 2");
        s2.start(Box::new(move |e| tx.send(e).unwrap())).unwrap();
        let ev = rx
            .recv_timeout(std::time::Duration::from_millis(50))
            .unwrap();
        match ev {
            SamplerEvent::LayerError { message, .. } => {
                assert!(message.contains("test stub 2"));
            }
            other => panic!("expected LayerError, got {other:?}"),
        }
    }

    #[test]
    fn make_default_sampler_returns_something() {
        // Smoke test — we just want to know the boxed trait object
        // constructs without panicking on whatever platform CI is.
        let _ = make_default_sampler();
    }
}
