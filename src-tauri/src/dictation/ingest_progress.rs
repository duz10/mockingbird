//! Ingest-progress event bus (ADR 0046 Iter 4 / mb-q1xt).
//!
//! Mirror of [`super::events::SessionsEventBus`] for a different
//! concern: instead of "a session row landed", these events surface
//! "the import pipeline transitioned to stage X". Two producers feed
//! one event channel (`ingest_progress`):
//!
//! - [`crate::commands::dictation::dictation_import_file`] — desktop
//!   `+ Audio file` IPC.
//! - [`crate::inbox::courier::Courier`] — mobile inbox file arrival.
//!
//! ## Boundary
//!
//! `dictation::ingest` itself is SEALED per ADR §3 + §3.2 and emits
//! nothing. Both call sites bracket their calls into the orchestrator
//! with these emits, so the progress UI lights up without touching
//! the sealed module.
//!
//! ## Why a trait, not a free function
//!
//! Same rationale as [`super::events::SessionsEventBus`]: the courier
//! is unit-tested in pure-Rust mode (no live Tauri runtime), so it
//! needs a stub-able interface. The trait + [`NoopIngestProgressBus`]
//! keeps the test gate green.
//!
//! ## Best-effort by contract
//!
//! All emits MUST swallow transient failures and never block the
//! ingest pipeline. The progress overlay is convenience UX; a missing
//! emit must not stall whisper-rs or fail an import.

use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// Event name the React side listens to via
/// `listen("ingest_progress", …)`. Kept here as a const so the wire
/// label is owned by exactly one module.
pub const INGEST_PROGRESS_EVENT: &str = "ingest_progress";

/// Pipeline stages surfaced to the UI.
///
/// Wire-format stage labels. Strings (not an enum discriminator) so
/// adding a new stage on the Rust side does not silently reshape the
/// TypeScript contract.
pub mod stage {
    /// Audio decode (symphonia) in progress.
    pub const DECODING: &str = "decoding";
    /// Whisper STT (plus opaque cleanup pass) in progress.
    pub const TRANSCRIBING: &str = "transcribing";
    /// Reserved for future stage-splitting; currently the IPC + courier
    /// collapse cleanup into [`TRANSCRIBING`] because the orchestrator
    /// runs them opaquely behind the headless-ingest channel.
    pub const CLEANING: &str = "cleaning";
    /// Terminal — ingest succeeded; `session_id` is populated.
    pub const DONE: &str = "done";
    /// Terminal — ingest failed; `error` is populated.
    pub const FAILED: &str = "failed";
}

/// Origin of an ingest event.
///
/// Wire-format origin labels. Matches `sessions.source`. Lets the UI
/// distinguish desktop-pick imports from mobile-inbox arrivals
/// (different label, same overlay).
pub mod source {
    /// User clicked `+ Audio file` on the Dictations page.
    pub const DESKTOP_IMPORT: &str = "desktop-import";
    /// File arrived via the Iter 3 inbox watcher/courier.
    pub const MOBILE_INBOX: &str = "mobile-inbox";
}

/// Payload shape mirrored by the React `ImportProgressOverlay`.
///
/// `stage` and `source` are `&'static str` because they only ever
/// hold one of the constants above — the type system makes accidental
/// "decoidng" typos impossible by routing all emits through helpers
/// that pin the literal at the call site.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IngestProgressEvent {
    /// One of [`stage`]'s constants.
    pub stage: &'static str,
    /// One of [`source`]'s constants.
    pub source: &'static str,
    /// Filename the user (or iOS Shortcut) gave the source audio.
    /// Quoted in the overlay so the user knows which import is
    /// in flight when several stack up.
    pub original_filename: String,
    /// Populated only on the terminal `done` emit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<i64>,
    /// Populated only on the terminal `failed` emit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl IngestProgressEvent {
    /// Helper for the common non-terminal stages (decoding,
    /// transcribing, cleaning) that have neither a session_id nor
    /// an error to carry.
    pub fn staged(
        stage: &'static str,
        source: &'static str,
        original_filename: impl Into<String>,
    ) -> Self {
        Self {
            stage,
            source,
            original_filename: original_filename.into(),
            session_id: None,
            error: None,
        }
    }

    /// Helper for the terminal `done` emit.
    pub fn done(
        source: &'static str,
        original_filename: impl Into<String>,
        session_id: i64,
    ) -> Self {
        Self {
            stage: stage::DONE,
            source,
            original_filename: original_filename.into(),
            session_id: Some(session_id),
            error: None,
        }
    }

    /// Helper for the terminal `failed` emit.
    pub fn failed(
        source: &'static str,
        original_filename: impl Into<String>,
        error: impl Into<String>,
    ) -> Self {
        Self {
            stage: stage::FAILED,
            source,
            original_filename: original_filename.into(),
            session_id: None,
            error: Some(error.into()),
        }
    }
}

/// Trait every progress producer accepts (`&dyn IngestProgressBus`).
///
/// Mirrors the [`super::events::SessionsEventBus`] shape: best-effort,
/// never blocks the pipeline. Trait existence is what lets the courier
/// be unit-tested against a recording double rather than requiring
/// a live Tauri app handle.
pub trait IngestProgressBus: Send + Sync {
    /// Fire one progress event. Best-effort: implementations MUST
    /// swallow transient failures.
    fn emit(&self, event: IngestProgressEvent);
}

/// Production impl. Cheaply clonable — the `AppHandle` slot is an
/// `Arc<Mutex<Option<_>>>` so multiple consumers (the courier owns
/// one clone, the IPC layer constructs ad-hoc clones) share one
/// underlying handle.
///
/// The `Option` lets the app boot before the handle exists (Tauri's
/// `setup` callback runs AFTER managed-state construction in our
/// pipeline) and lets tests construct without a runtime — same
/// pattern as [`crate::recording_window::RecordingWindow`].
#[derive(Default, Clone)]
pub struct AppIngestProgressBus {
    app: Arc<Mutex<Option<AppHandle>>>,
}

impl AppIngestProgressBus {
    /// Empty bus — emits become no-ops until [`Self::set_app_handle`]
    /// plugs in a real handle. Used for the "construct early, wire
    /// late" Tauri setup pattern.
    pub fn new() -> Self {
        Self::default()
    }

    /// Plug the Tauri handle in. Idempotent — last writer wins.
    pub fn set_app_handle(&self, app: AppHandle) {
        if let Ok(mut g) = self.app.lock() {
            *g = Some(app);
        }
    }
}

impl IngestProgressBus for AppIngestProgressBus {
    fn emit(&self, event: IngestProgressEvent) {
        let Ok(g) = self.app.lock() else { return };
        let Some(app) = g.as_ref() else {
            tracing::trace!(
                stage = event.stage,
                "skip ingest_progress emit: no app handle (tests / not booted)"
            );
            return;
        };
        if let Err(e) = app.emit(INGEST_PROGRESS_EVENT, &event) {
            tracing::debug!(
                error = ?e,
                stage = event.stage,
                "ingest_progress emit failed"
            );
        }
    }
}

/// No-op impl for tests + the "progress not wired" fallback.
///
/// Production code paths that own an `Arc<dyn IngestProgressBus>`
/// (the courier, the inbox runtime) default to this when the
/// Tauri side hasn't wired a real bus yet — keeps the rest of the
/// pipeline trait-object-clean without forcing `Option<Arc<dyn …>>`
/// branches at every emit site.
pub struct NoopIngestProgressBus;

impl IngestProgressBus for NoopIngestProgressBus {
    fn emit(&self, _event: IngestProgressEvent) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct CapturingBus {
        count: AtomicUsize,
        last_stage: Mutex<Option<&'static str>>,
        last_source: Mutex<Option<&'static str>>,
        last_session_id: Mutex<Option<i64>>,
    }

    impl IngestProgressBus for CapturingBus {
        fn emit(&self, event: IngestProgressEvent) {
            self.count.fetch_add(1, Ordering::SeqCst);
            *self.last_stage.lock().unwrap() = Some(event.stage);
            *self.last_source.lock().unwrap() = Some(event.source);
            *self.last_session_id.lock().unwrap() = event.session_id;
        }
    }

    #[test]
    fn staged_helper_omits_session_id_and_error() {
        let e = IngestProgressEvent::staged(stage::DECODING, source::DESKTOP_IMPORT, "memo.m4a");
        assert_eq!(e.stage, "decoding");
        assert_eq!(e.source, "desktop-import");
        assert_eq!(e.original_filename, "memo.m4a");
        assert!(e.session_id.is_none());
        assert!(e.error.is_none());
    }

    #[test]
    fn done_helper_carries_session_id() {
        let e = IngestProgressEvent::done(source::MOBILE_INBOX, "memo.m4a", 42);
        assert_eq!(e.stage, "done");
        assert_eq!(e.session_id, Some(42));
        assert!(e.error.is_none());
    }

    #[test]
    fn failed_helper_carries_error() {
        let e = IngestProgressEvent::failed(source::DESKTOP_IMPORT, "x.wav", "decode busted");
        assert_eq!(e.stage, "failed");
        assert_eq!(e.error.as_deref(), Some("decode busted"));
        assert!(e.session_id.is_none());
    }

    #[test]
    fn noop_bus_swallows_emits() {
        let bus = NoopIngestProgressBus;
        let dyn_bus: &dyn IngestProgressBus = &bus;
        dyn_bus.emit(IngestProgressEvent::staged(
            stage::DECODING,
            source::DESKTOP_IMPORT,
            "x.wav",
        ));
        // No panic, no observable state -- the test is that this
        // compiles and runs.
    }

    #[test]
    fn capturing_bus_records_in_order() {
        let bus = CapturingBus::default();
        let dyn_bus: &dyn IngestProgressBus = &bus;
        dyn_bus.emit(IngestProgressEvent::staged(
            stage::DECODING,
            source::DESKTOP_IMPORT,
            "x.wav",
        ));
        dyn_bus.emit(IngestProgressEvent::staged(
            stage::TRANSCRIBING,
            source::DESKTOP_IMPORT,
            "x.wav",
        ));
        dyn_bus.emit(IngestProgressEvent::done(
            source::DESKTOP_IMPORT,
            "x.wav",
            7,
        ));
        assert_eq!(bus.count.load(Ordering::SeqCst), 3);
        assert_eq!(*bus.last_stage.lock().unwrap(), Some("done"));
        assert_eq!(*bus.last_session_id.lock().unwrap(), Some(7));
    }

    #[test]
    fn serialized_payload_is_camel_case() {
        let e = IngestProgressEvent::done(source::DESKTOP_IMPORT, "memo.m4a", 7);
        let j = serde_json::to_string(&e).unwrap();
        assert!(j.contains("\"stage\":\"done\""), "got {j}");
        assert!(j.contains("\"source\":\"desktop-import\""), "got {j}");
        assert!(j.contains("\"originalFilename\":\"memo.m4a\""), "got {j}");
        assert!(j.contains("\"sessionId\":7"), "got {j}");
        assert!(!j.contains("\"error\""), "got {j}");
    }

    #[test]
    fn app_bus_without_handle_is_a_noop() {
        // No app handle plugged in => emit is best-effort silent.
        // We don't assert anything observable; the test is that
        // calling emit does not panic.
        let bus = AppIngestProgressBus::new();
        bus.emit(IngestProgressEvent::staged(
            stage::DECODING,
            source::DESKTOP_IMPORT,
            "x.wav",
        ));
    }
}
