//! Foreground-window sampler.
//!
//! ## Wave 1B (titles-only) → Wave 2 (deep snapshots)
//!
//! Wave 1B emitted `app_switch` + `context_snapshot` events carrying
//! just `(app_name, window_title)`. Wave 2 keeps the same event
//! shapes but enriches `context_snapshot` with a full **UIA payload**
//! (focused field, visible text fragments, control-summary, monitor
//! attribution, password-field-active flag) via the [`super::uia::Probe`]
//! trait, AND adds idle-state surfacing via [`super::activity_level::IdleTracker`].
//!
//! ## Sampling cadence
//!
//! ~1 Hz polling stays for Wave 2 — the same coarse tick that
//! drove Wave 1B. The Wave 2 spec sketches a future
//! `SetWinEventHook(EVENT_SYSTEM_FOREGROUND, ...)` fast-path, but
//! the 1 Hz cadence is empirically fine for the activity-capture
//! use case (we are summarizing the session AFTER it ends; sub-
//! second granularity isn't payoff). YAGNI; revisit in Wave 5
//! polish if smoke testing surfaces a UX gap.
//!
//! ## Per-tick effects
//!
//! 1. Read foreground window (cheap, ~50 µs).
//! 2. If `(app, title)` changed since the previous tick:
//!    - Emit `AppSwitch`.
//!    - Run the UIA probe and emit `ContextSnapshot { snapshot_json }`.
//! 3. Tick the [`IdleTracker`] with `GetLastInputInfo` age; emit
//!    `IdleStart` / `IdleEnd` on transitions.
//!
//! ## Cross-platform discipline (Principle 5)
//!
//! The Windows impl below is gated on `cfg(target_os = "windows")`.
//! Non-Windows hosts get [`StubSampler`] which emits one `LayerError`
//! and idles.

#![allow(missing_docs)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::error::AppResult;
use crate::window_context::{make_default_context, WindowContext};

use super::activity_level::{read_last_input_age_ms, IdleTracker, IdleTransition};
use super::uia::{make_default_probe, to_payload_json, Probe};

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
    /// Snapshot of the current window — Wave 2 carries the rich UIA
    /// payload as a pre-serialized JSON string in `snapshot_json`.
    /// The sampler builds the JSON (it knows the platform-specific
    /// probe details); the runtime just persists it.
    ContextSnapshot {
        app: String,
        title: String,
        ts_ms: i64,
        snapshot_json: String,
    },
    /// User transitioned to idle (no input in ≥ idle-threshold).
    IdleStart { ts_ms: i64 },
    /// User transitioned back from idle to active.
    IdleEnd { ts_ms: i64 },
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
    /// Hot-swappable for tests; we use [`make_default_probe`] in
    /// production. Wrapped in `Mutex` only so we can `take()` it
    /// when the sampler thread is spawned without `Box<dyn Probe>`
    /// needing to be `Clone`.
    probe_slot: Arc<Mutex<Option<Box<dyn Probe>>>>,
}

#[cfg(target_os = "windows")]
impl WindowsSampler {
    pub fn new() -> Self {
        Self::with_probe(make_default_probe())
    }

    /// Inject a custom probe (test seam — the COM probe needs a real
    /// desktop to do anything; tests use a fake).
    pub fn with_probe(probe: Box<dyn Probe>) -> Self {
        Self {
            paused: Arc::new(AtomicBool::new(false)),
            stop_signal: Arc::new(AtomicBool::new(false)),
            thread: None,
            probe_slot: Arc::new(Mutex::new(Some(probe))),
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
        // Move the probe onto the sampler thread — COM is bound to
        // the thread that calls CoInitializeEx, so the probe MUST
        // live on the sampler thread end-to-end.
        let probe = self
            .probe_slot
            .lock()
            .map_err(|_| crate::error::AppError::ActivitySampler("probe slot poisoned".into()))?
            .take()
            .ok_or_else(|| {
                crate::error::AppError::ActivitySampler(
                    "sampler already consumed its probe (second start)".into(),
                )
            })?;
        let handle = thread::Builder::new()
            .name("activity-sampler".into())
            .spawn(move || sampler_loop(ctx, probe, paused, stop, &mut sink))
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
    mut probe: Box<dyn Probe>,
    paused: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    sink: &mut SamplerSink,
) {
    let mut last_seen: Option<(String, String)> = None;
    let mut idle = IdleTracker::default();
    while !stop.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(1000));
        if stop.load(Ordering::Relaxed) {
            break;
        }
        if paused.load(Ordering::Relaxed) {
            continue;
        }
        let now_ms = now_ms();

        // 1) Foreground window sweep.
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
                    let probe_result = probe.snapshot(fg.hwnd, &fg.process_name, &fg.title);
                    let (snapshot_json, _truncated) = to_payload_json(&probe_result);
                    sink(SamplerEvent::ContextSnapshot {
                        app: fg.process_name,
                        title: fg.title,
                        ts_ms: now_ms,
                        snapshot_json,
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

        // 2) Idle-state tick. Silent on failure (lock screen / Phase 9
        // platform), no LayerError spam — the runtime knows what to
        // do with a tracker that never transitions.
        if let Ok(age) = read_last_input_age_ms() {
            match idle.tick(age, now_ms) {
                Some(IdleTransition::Started { ts_ms }) => {
                    sink(SamplerEvent::IdleStart { ts_ms });
                }
                Some(IdleTransition::Ended { ts_ms }) => {
                    sink(SamplerEvent::IdleEnd { ts_ms });
                }
                None => {}
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
#[allow(dead_code)]
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
        let (tx, rx) = std::sync::mpsc::channel();
        let mut s = StubSampler::new("test stub");
        s.start(Box::new(move |e| tx.send(e).unwrap())).unwrap();
        let ev = rx
            .recv_timeout(std::time::Duration::from_millis(50))
            .unwrap();
        match ev {
            SamplerEvent::LayerError { message, .. } => {
                assert!(message.contains("test stub"));
            }
            other => panic!("expected LayerError, got {other:?}"),
        }
    }

    #[test]
    fn make_default_sampler_returns_something() {
        let _ = make_default_sampler();
    }

    #[test]
    fn context_snapshot_includes_snapshot_json_variant_fields() {
        // The variant shape is part of the IPC contract — adding /
        // removing fields ripples through `runtime.rs` and the UI.
        // Lock the shape with a construction-only test.
        let ev = SamplerEvent::ContextSnapshot {
            app: "a.exe".into(),
            title: "T".into(),
            ts_ms: 1,
            snapshot_json: r#"{"schema":"v2"}"#.into(),
        };
        match ev {
            SamplerEvent::ContextSnapshot { snapshot_json, .. } => {
                assert!(snapshot_json.contains("\"schema\":\"v2\""));
            }
            _ => panic!("variant mismatch"),
        }
    }
}
