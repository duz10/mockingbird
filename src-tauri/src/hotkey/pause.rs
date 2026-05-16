//! Tray pause-toggle wiring.
//!
//! Wave 3 sets up the atomic + channel-send primitive that the tray
//! menu's "Pause dictation" item flips. Wave 4 wires the actual Tauri
//! command + tray menu state.
//!
//! ## Why this lives in `hotkey/` (not `tray/`)
//!
//! The state machine is the consumer — it interprets the pause flag
//! per PLAN §6.1 ("key-down events no-op until cleared"). The tray
//! menu is just the UI surface that flips it. Putting the primitive
//! here keeps the §6.1 semantics in one place; the tray module (Wave
//! 4) calls into this with one line:
//!
//! ```ignore
//! pause_handle.set(true)?; // user clicked "Pause dictation"
//! ```

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;

use super::HotkeyEvent;
use crate::error::{AppError, AppResult};

/// Pause-state handle shared between the tray UI and the state-machine
/// driver.
///
/// Cloneable + `Send + Sync` so the orchestrator can hand the same
/// instance to the tray + the driver without lifetime gymnastics.
#[derive(Clone)]
pub struct PauseHandle {
    paused: Arc<AtomicBool>,
    events: Sender<HotkeyEvent>,
}

impl PauseHandle {
    /// Wire the handle to the hotkey-event channel that's also fed by
    /// the OS hook. `set()` will inject [`HotkeyEvent::PauseToggle`]
    /// onto this channel so the state machine sees the change as
    /// part of its normal event stream (no out-of-band state mutation).
    pub fn new(events: Sender<HotkeyEvent>) -> Self {
        Self {
            paused: Arc::new(AtomicBool::new(false)),
            events,
        }
    }

    /// Current paused flag — cheap atomic read.
    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    /// Flip the paused state and inform the state machine.
    ///
    /// Idempotent at the atomic level: setting to the value it already
    /// holds is a no-op (but a `PauseToggle` event IS still emitted
    /// so the state machine logs the user gesture). Wave 5 may add a
    /// branch here to suppress duplicates if that turns out to spam
    /// the audit log.
    pub fn set(&self, paused: bool) -> AppResult<()> {
        self.paused.store(paused, Ordering::SeqCst);
        self.events
            .send(HotkeyEvent::PauseToggle { paused })
            .map_err(|e| AppError::Hotkey(format!("PauseHandle::set: event channel closed: {e}")))
    }

    /// Toggle: returns the new value.
    pub fn toggle(&self) -> AppResult<bool> {
        let next = !self.is_paused();
        self.set(next)?;
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn handle() -> (PauseHandle, mpsc::Receiver<HotkeyEvent>) {
        let (tx, rx) = mpsc::channel();
        (PauseHandle::new(tx), rx)
    }

    #[test]
    fn new_handle_is_unpaused() {
        let (h, _rx) = handle();
        assert!(!h.is_paused());
    }

    #[test]
    fn set_true_persists_and_emits_event() {
        let (h, rx) = handle();
        h.set(true).unwrap();
        assert!(h.is_paused());
        match rx.recv().unwrap() {
            HotkeyEvent::PauseToggle { paused } => assert!(paused),
            other => panic!("expected PauseToggle, got {other:?}"),
        }
    }

    #[test]
    fn set_false_persists_and_emits_event() {
        let (h, rx) = handle();
        h.set(true).unwrap();
        let _ = rx.recv().unwrap();
        h.set(false).unwrap();
        assert!(!h.is_paused());
        match rx.recv().unwrap() {
            HotkeyEvent::PauseToggle { paused } => assert!(!paused),
            other => panic!("expected PauseToggle, got {other:?}"),
        }
    }

    #[test]
    fn toggle_flips_and_returns_new_value() {
        let (h, rx) = handle();
        assert!(h.toggle().unwrap(), "first toggle should return true");
        assert!(h.is_paused());
        let _ = rx.recv().unwrap();
        assert!(!h.toggle().unwrap(), "second toggle should return false");
        assert!(!h.is_paused());
        let _ = rx.recv().unwrap();
    }

    #[test]
    fn closed_channel_yields_hotkey_error() {
        let (tx, rx) = mpsc::channel();
        let h = PauseHandle::new(tx);
        drop(rx);
        let err = h.set(true).unwrap_err();
        match err {
            AppError::Hotkey(msg) => {
                assert!(msg.contains("channel closed"));
            }
            other => panic!("expected Hotkey, got {other:?}"),
        }
    }

    #[test]
    fn clones_share_state() {
        // The tray and the driver each hold a clone; they MUST see
        // the same paused flag.
        let (h, rx) = handle();
        let h2 = h.clone();
        h.set(true).unwrap();
        assert!(h2.is_paused());
        let _ = rx.recv().unwrap();
    }
}
