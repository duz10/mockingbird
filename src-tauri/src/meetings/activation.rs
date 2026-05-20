//! Chord activation state machine for meeting capture (ADR 0027).
//!
//! Pure Rust. NO Windows API surface in this file — the second
//! `WH_KEYBOARD_LL` hook lives in [`super::runtime`] (Wave 3) and
//! feeds [`ActivationEvent`]s into this state machine via an `mpsc`.
//!
//! The chord is configurable (settings:
//! `MeetingHotkeyModifier` + `MeetingHotkeyKey`); default is
//! `VK_RCONTROL + VK_M`. The three-state machine (`IDLE`, `MOD_HELD`,
//! `MAIN_PRESSED`) fires `MeetingToggle` once per chord activation and
//! suppresses Windows key-repeat for the duration of the main-key hold.
//!
//! Section MC.1 of the phase plan is the binding spec. Test density
//! target: ≥20 unit tests (Wave 2).
//!
//! Wave 1 scaffold — types + struct skeleton only. Wave 2 implements
//! [`Activation::on_event`] and lands the ≥20 unit tests.

use crate::error::AppResult;

/// Modifier-key family. The conflict probe (Wave 3) clamps the
/// configured `MeetingHotkeyModifier` setting to this allowed set
/// before installing the hook.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModifierKey {
    RCtrl,
    LCtrl,
    RAlt,
    LAlt,
    RShift,
    LShift,
    RWin,
    LWin,
}

/// Meeting-source the activation last selected. Mirrors
/// `SettingKey::MeetingLastSelectedSource`; the runtime echoes it back
/// on every toggle so the overlay preselects the last-used source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LastChosenSource {
    Mic,
    System,
    Both,
}

/// Inputs to the [`Activation`] state machine.
///
/// All timestamps are monotonic-clock milliseconds since some fixed
/// epoch (typically process start). The chord state machine itself
/// doesn't use the timestamps — they're stored on the events for
/// observability (logging, future timing-window gestures, and parity
/// with `dictation::HotkeyEvent`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivationEvent {
    ModifierDown {
        ts_ms: u64,
    },
    ModifierUp {
        ts_ms: u64,
    },
    MainKeyDown {
        ts_ms: u64,
    },
    MainKeyUp {
        ts_ms: u64,
    },
    /// Periodic clock tick. Unused by the chord state machine (kept
    /// for input-enum symmetry with future timing-based gestures).
    Tick {
        ts_ms: u64,
    },
    /// Tray-menu "Pause meeting hotkey" toggle. When `paused`, the
    /// state machine emits [`ActivationAction::Noop`] for all key
    /// events and resets internal state to `IDLE`.
    PauseToggle {
        paused: bool,
    },
}

/// Outputs from the [`Activation`] state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivationAction {
    /// Fire a meeting-toggle to the runtime. The runtime interprets a
    /// toggle during idle as "start"; a toggle during a live meeting
    /// as "stop". The `source` is the last-chosen source from the
    /// settings facade, echoed for overlay preselect.
    MeetingToggle { source: LastChosenSource },
    /// No-op (event dispatched but no chord action). Returned for the
    /// vast majority of `Tick` events, redundant key edges, and any
    /// event while paused.
    Noop,
}

/// Internal state of the chord state machine. Public for tests; the
/// runtime treats [`Activation`] as opaque.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivationState {
    /// No modifier observed. Lone main-key edges are ignored
    /// (chord-broken; the user pressed M without holding RCtrl).
    Idle,
    /// Modifier held, main-key not yet pressed.
    ModHeld,
    /// Modifier + main-key both down. Suppresses Windows key-repeat
    /// for the duration of the main-key hold (released by `MainKeyUp`).
    MainPressed,
}

/// The chord state machine. See module docs.
///
/// Wave 1 ships the struct + constructor. Wave 2 implements
/// `on_event` and ≥20 unit tests covering every state transition,
/// every event in every state, plus the edge cases enumerated in
/// Section MC.1.
#[derive(Debug)]
pub struct Activation {
    state: ActivationState,
    last_source: LastChosenSource,
    paused: bool,
}

impl Activation {
    /// Build a fresh state machine, idle, unpaused, with the given
    /// last-chosen source (typically loaded from
    /// `SettingKey::MeetingLastSelectedSource` at runtime startup).
    pub fn new(last_source: LastChosenSource) -> Self {
        Self {
            state: ActivationState::Idle,
            last_source,
            paused: false,
        }
    }

    /// Observable accessors for tests + the runtime's structured logs.
    pub fn state(&self) -> ActivationState {
        self.state
    }
    pub fn is_paused(&self) -> bool {
        self.paused
    }
    pub fn last_source(&self) -> LastChosenSource {
        self.last_source
    }

    /// Update last-chosen source. Called by the runtime after every
    /// meeting-toggle's overlay confirmation so the next chord
    /// preselects the same source.
    pub fn set_last_source(&mut self, src: LastChosenSource) {
        self.last_source = src;
    }

    /// Apply one input. Returns the resulting action.
    ///
    /// Wave 1: `todo!()` — Wave 2 ships the implementation + ≥20 tests.
    pub fn on_event(&mut self, _event: ActivationEvent) -> AppResult<ActivationAction> {
        todo!("Wave 2: implement chord state machine per Section MC.1")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Smoke: construct + read back. Wave 2's full test set replaces
    /// this with the ≥20 transition tests; for now this keeps the
    /// scaffold honest by exercising the public surface that exists.
    #[test]
    fn new_starts_idle_unpaused() {
        let a = Activation::new(LastChosenSource::Mic);
        assert_eq!(a.state(), ActivationState::Idle);
        assert!(!a.is_paused());
        assert_eq!(a.last_source(), LastChosenSource::Mic);
    }

    #[test]
    fn set_last_source_updates() {
        let mut a = Activation::new(LastChosenSource::Mic);
        a.set_last_source(LastChosenSource::Both);
        assert_eq!(a.last_source(), LastChosenSource::Both);
    }
}
