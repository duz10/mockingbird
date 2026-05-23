//! Sessions-side UI event bus -- the post-persist refetch signal.
//!
//! Extracted from a direct `RecordingWindow` method per ADR 0046
//! Section 3.1 so the headless dictation ingest path can fire the
//! same `history:session-saved` event without taking a dependency
//! on the recording-overlay window subsystem. Both producers -- PTT
//! (via the `DictationOrchestrator`'s `RecordingWindow` field) and
//! headless (via the `dictation_import_file` IPC and the future
//! Iter 3 inbox courier) -- converge on a single emit point so the
//! React Dictations page's refetch trigger is identical regardless
//! of origin.
//!
//! ## Why a trait, not a function
//!
//! The PTT path already has a [`RecordingWindow`] handle for
//! everything else it does (show/hide pill, set state, flash done).
//! Adding a parallel emit-only object would be redundant. The trait
//! lets the existing handle keep doing its job while letting the
//! headless path satisfy the same contract with a tiny adapter
//! that does not need any of the overlay machinery.
//!
//! ## File placement (ADR 0046 Section 3.1 nailed this down)
//!
//! Lives under `dictation/` rather than under `recording_window.rs`
//! because the trait belongs to the dictation domain -- it
//! describes "a session row just landed", not "the recording overlay
//! did something". Keeping it here lets `headless_ingest` depend on
//! a strictly smaller surface than the recording-window module.

use crate::recording_window::RecordingWindow;

/// Fired by the persistence layer after a new `sessions` row lands
/// in the DB so the React Dictations page (and anything else
/// listening for `history:session-saved`) can refetch.
///
/// Best-effort by contract: impls MUST swallow transient emit
/// failures and never block the dictation pipeline on a UI hiccup.
/// The original `RecordingWindow::emit_session_saved` documents
/// this invariant; the trait inherits it.
pub trait SessionsEventBus: Send + Sync {
    /// Fire the refetch signal with the new row id. Implementations
    /// log failures internally -- there is no recovery path the
    /// caller can take, and the dictation pipeline must not block.
    fn emit_session_saved(&self, session_id: i64);
}

/// `RecordingWindow` is the PTT path's `SessionsEventBus`. Thin
/// delegation to the existing inherent method (which holds the
/// `AppHandle` and knows the canonical event name); no logic
/// moves.
///
/// The `&dyn SessionsEventBus` view of a `RecordingWindow` is
/// exactly what `headless_ingest` accepts via `IngestDeps`, so the
/// PTT-path `RecordingWindow` doubles as the bus for any caller
/// that already has one in hand (mostly the orchestrator + the
/// import-file IPC, which both reach it through Tauri state).
impl SessionsEventBus for RecordingWindow {
    fn emit_session_saved(&self, session_id: i64) {
        RecordingWindow::emit_session_saved(self, session_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicI64, Ordering};

    /// Test double capturing the last emitted session id. The real
    /// `RecordingWindow` impl is exercised by Tauri-level smoke;
    /// here we just verify the trait shape compiles and that
    /// headless ingest can drive a non-RecordingWindow impl (the
    /// property the trait was extracted for in the first place).
    #[derive(Default)]
    pub struct CapturingBus {
        pub last: AtomicI64,
    }

    impl SessionsEventBus for CapturingBus {
        fn emit_session_saved(&self, session_id: i64) {
            self.last.store(session_id, Ordering::SeqCst);
        }
    }

    #[test]
    fn capturing_bus_records_the_emit() {
        let bus = CapturingBus::default();
        let dyn_bus: &dyn SessionsEventBus = &bus;
        dyn_bus.emit_session_saved(42);
        assert_eq!(bus.last.load(Ordering::SeqCst), 42);
    }
}
