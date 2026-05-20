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
    /// Total over the input enum: every (state, event) pair has a
    /// defined transition per Section MC.1. The `AppResult` return
    /// type is preserved for forward-compatibility with future
    /// timing-window gestures (clock-skew warnings, etc.); Wave 2's
    /// implementation never returns `Err`.
    pub fn on_event(&mut self, event: ActivationEvent) -> AppResult<ActivationAction> {
        // PauseToggle wins everywhere: it forces IDLE and updates the
        // paused flag regardless of current state.
        if let ActivationEvent::PauseToggle { paused } = event {
            self.paused = paused;
            self.state = ActivationState::Idle;
            return Ok(ActivationAction::Noop);
        }

        // While paused, every key event + tick is a no-op. The state
        // machine stays parked at IDLE (set by the PauseToggle that
        // brought us here, or by `new()`).
        if self.paused {
            return Ok(ActivationAction::Noop);
        }

        // Tick is unused by the chord machine — input-enum symmetry only.
        if matches!(event, ActivationEvent::Tick { .. }) {
            return Ok(ActivationAction::Noop);
        }

        let action = match (self.state, event) {
            // IDLE: only ModifierDown advances state.
            (ActivationState::Idle, ActivationEvent::ModifierDown { .. }) => {
                self.state = ActivationState::ModHeld;
                ActivationAction::Noop
            }
            (ActivationState::Idle, _) => ActivationAction::Noop,

            // MOD_HELD: MainKeyDown fires the toggle (the chord!).
            (ActivationState::ModHeld, ActivationEvent::MainKeyDown { .. }) => {
                self.state = ActivationState::MainPressed;
                ActivationAction::MeetingToggle {
                    source: self.last_source,
                }
            }
            (ActivationState::ModHeld, ActivationEvent::ModifierUp { .. }) => {
                self.state = ActivationState::Idle;
                ActivationAction::Noop
            }
            (ActivationState::ModHeld, _) => ActivationAction::Noop,

            // MAIN_PRESSED: chord fully held. Suppress key-repeat
            // until MainKeyUp; ModifierUp aborts.
            (ActivationState::MainPressed, ActivationEvent::MainKeyUp { .. }) => {
                self.state = ActivationState::ModHeld;
                ActivationAction::Noop
            }
            (ActivationState::MainPressed, ActivationEvent::ModifierUp { .. }) => {
                self.state = ActivationState::Idle;
                ActivationAction::Noop
            }
            (ActivationState::MainPressed, _) => ActivationAction::Noop,
        };

        Ok(action)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- helpers ----------

    /// Drive a sequence of events through the state machine and
    /// collect the actions. Tests prefer this over hand-rolled
    /// match-on-Ok-then-push loops.
    fn drive(a: &mut Activation, events: &[ActivationEvent]) -> Vec<ActivationAction> {
        events
            .iter()
            .map(|e| a.on_event(*e).expect("Wave 2 on_event never errs"))
            .collect()
    }

    // The chord state machine ignores ts_ms; use a constant for
    // readability. The few tests that exercise time-ordering still
    // pass strictly-monotonic stamps to document the intent.
    const T: u64 = 0;

    fn mod_down() -> ActivationEvent {
        ActivationEvent::ModifierDown { ts_ms: T }
    }
    fn mod_up() -> ActivationEvent {
        ActivationEvent::ModifierUp { ts_ms: T }
    }
    fn main_down() -> ActivationEvent {
        ActivationEvent::MainKeyDown { ts_ms: T }
    }
    fn main_up() -> ActivationEvent {
        ActivationEvent::MainKeyUp { ts_ms: T }
    }
    fn tick() -> ActivationEvent {
        ActivationEvent::Tick { ts_ms: T }
    }
    fn pause(p: bool) -> ActivationEvent {
        ActivationEvent::PauseToggle { paused: p }
    }
    fn toggle(src: LastChosenSource) -> ActivationAction {
        ActivationAction::MeetingToggle { source: src }
    }
    const NOOP: ActivationAction = ActivationAction::Noop;

    // ---------- IDLE transitions ----------

    #[test]
    fn idle_modifier_down_goes_to_mod_held() {
        let mut a = Activation::new(LastChosenSource::Mic);
        assert_eq!(drive(&mut a, &[mod_down()]), vec![NOOP]);
        assert_eq!(a.state(), ActivationState::ModHeld);
    }

    #[test]
    fn idle_lone_main_keydown_is_noop() {
        let mut a = Activation::new(LastChosenSource::Mic);
        assert_eq!(drive(&mut a, &[main_down()]), vec![NOOP]);
        assert_eq!(a.state(), ActivationState::Idle);
    }

    #[test]
    fn idle_lone_main_keyup_is_noop() {
        let mut a = Activation::new(LastChosenSource::Mic);
        assert_eq!(drive(&mut a, &[main_up()]), vec![NOOP]);
        assert_eq!(a.state(), ActivationState::Idle);
    }

    #[test]
    fn idle_lone_modifier_up_is_noop() {
        let mut a = Activation::new(LastChosenSource::Mic);
        assert_eq!(drive(&mut a, &[mod_up()]), vec![NOOP]);
        assert_eq!(a.state(), ActivationState::Idle);
    }

    // ---------- MOD_HELD transitions ----------

    #[test]
    fn mod_held_main_down_fires_meeting_toggle() {
        let mut a = Activation::new(LastChosenSource::Mic);
        let acts = drive(&mut a, &[mod_down(), main_down()]);
        assert_eq!(acts, vec![NOOP, toggle(LastChosenSource::Mic)]);
        assert_eq!(a.state(), ActivationState::MainPressed);
    }

    #[test]
    fn mod_held_modifier_up_returns_to_idle() {
        let mut a = Activation::new(LastChosenSource::Mic);
        let acts = drive(&mut a, &[mod_down(), mod_up()]);
        assert_eq!(acts, vec![NOOP, NOOP]);
        assert_eq!(a.state(), ActivationState::Idle);
    }

    #[test]
    fn mod_held_main_up_without_main_down_is_noop() {
        let mut a = Activation::new(LastChosenSource::Mic);
        let acts = drive(&mut a, &[mod_down(), main_up()]);
        assert_eq!(acts, vec![NOOP, NOOP]);
        assert_eq!(a.state(), ActivationState::ModHeld);
    }

    #[test]
    fn mod_held_redundant_modifier_down_is_noop() {
        let mut a = Activation::new(LastChosenSource::Mic);
        let acts = drive(&mut a, &[mod_down(), mod_down()]);
        assert_eq!(acts, vec![NOOP, NOOP]);
        assert_eq!(a.state(), ActivationState::ModHeld);
    }

    // ---------- MAIN_PRESSED transitions ----------

    #[test]
    fn main_pressed_key_repeat_does_not_re_fire() {
        let mut a = Activation::new(LastChosenSource::Mic);
        let acts = drive(&mut a, &[mod_down(), main_down(), main_down(), main_down()]);
        assert_eq!(acts, vec![NOOP, toggle(LastChosenSource::Mic), NOOP, NOOP]);
        assert_eq!(a.state(), ActivationState::MainPressed);
    }

    #[test]
    fn main_pressed_main_up_returns_to_mod_held() {
        let mut a = Activation::new(LastChosenSource::Mic);
        let acts = drive(&mut a, &[mod_down(), main_down(), main_up()]);
        assert_eq!(acts, vec![NOOP, toggle(LastChosenSource::Mic), NOOP]);
        assert_eq!(a.state(), ActivationState::ModHeld);
    }

    #[test]
    fn main_pressed_modifier_up_returns_to_idle() {
        let mut a = Activation::new(LastChosenSource::Mic);
        let acts = drive(&mut a, &[mod_down(), main_down(), mod_up()]);
        assert_eq!(acts, vec![NOOP, toggle(LastChosenSource::Mic), NOOP]);
        assert_eq!(a.state(), ActivationState::Idle);
    }

    // ---------- Re-chord behaviour ----------

    #[test]
    fn release_and_re_press_fires_again() {
        let mut a = Activation::new(LastChosenSource::Mic);
        let acts = drive(&mut a, &[mod_down(), main_down(), main_up(), main_down()]);
        assert_eq!(
            acts,
            vec![
                NOOP,
                toggle(LastChosenSource::Mic),
                NOOP,
                toggle(LastChosenSource::Mic),
            ]
        );
        assert_eq!(a.state(), ActivationState::MainPressed);
    }

    #[test]
    fn release_modifier_then_re_chord_fires_again() {
        let mut a = Activation::new(LastChosenSource::Mic);
        let acts = drive(
            &mut a,
            &[
                mod_down(),
                main_down(),
                main_up(),
                mod_up(),
                mod_down(),
                main_down(),
            ],
        );
        assert_eq!(
            acts,
            vec![
                NOOP,
                toggle(LastChosenSource::Mic),
                NOOP,
                NOOP,
                NOOP,
                toggle(LastChosenSource::Mic),
            ]
        );
        assert_eq!(a.state(), ActivationState::MainPressed);
    }

    // ---------- Tick is always a no-op ----------

    #[test]
    fn tick_in_idle_is_noop() {
        let mut a = Activation::new(LastChosenSource::Mic);
        assert_eq!(drive(&mut a, &[tick()]), vec![NOOP]);
        assert_eq!(a.state(), ActivationState::Idle);
    }

    #[test]
    fn tick_in_mod_held_is_noop() {
        let mut a = Activation::new(LastChosenSource::Mic);
        let acts = drive(&mut a, &[mod_down(), tick()]);
        assert_eq!(acts, vec![NOOP, NOOP]);
        assert_eq!(a.state(), ActivationState::ModHeld);
    }

    #[test]
    fn tick_in_main_pressed_is_noop() {
        let mut a = Activation::new(LastChosenSource::Mic);
        let acts = drive(&mut a, &[mod_down(), main_down(), tick()]);
        assert_eq!(acts, vec![NOOP, toggle(LastChosenSource::Mic), NOOP]);
        assert_eq!(a.state(), ActivationState::MainPressed);
    }

    // ---------- Pause-toggle behaviour ----------

    #[test]
    fn pause_toggle_in_idle_resets_to_idle() {
        let mut a = Activation::new(LastChosenSource::Mic);
        assert_eq!(drive(&mut a, &[pause(true)]), vec![NOOP]);
        assert_eq!(a.state(), ActivationState::Idle);
        assert!(a.is_paused());
    }

    #[test]
    fn pause_toggle_in_main_pressed_resets_to_idle() {
        let mut a = Activation::new(LastChosenSource::Mic);
        let acts = drive(&mut a, &[mod_down(), main_down(), pause(true)]);
        assert_eq!(acts, vec![NOOP, toggle(LastChosenSource::Mic), NOOP]);
        assert_eq!(a.state(), ActivationState::Idle);
        assert!(a.is_paused());
    }

    #[test]
    fn paused_suppresses_chord() {
        let mut a = Activation::new(LastChosenSource::Mic);
        let acts = drive(&mut a, &[pause(true), mod_down(), main_down()]);
        assert_eq!(acts, vec![NOOP, NOOP, NOOP]);
        assert_eq!(a.state(), ActivationState::Idle);
        assert!(a.is_paused());
    }

    #[test]
    fn unpaused_chord_resumes() {
        let mut a = Activation::new(LastChosenSource::Mic);
        let acts = drive(
            &mut a,
            &[pause(true), pause(false), mod_down(), main_down()],
        );
        assert_eq!(acts, vec![NOOP, NOOP, NOOP, toggle(LastChosenSource::Mic)]);
        assert_eq!(a.state(), ActivationState::MainPressed);
        assert!(!a.is_paused());
    }

    // ---------- last_source carry-through ----------

    #[test]
    fn set_last_source_affects_next_toggle() {
        let mut a = Activation::new(LastChosenSource::Mic);
        a.set_last_source(LastChosenSource::Both);
        let acts = drive(&mut a, &[mod_down(), main_down()]);
        assert_eq!(acts, vec![NOOP, toggle(LastChosenSource::Both)]);
    }

    #[test]
    fn toggles_during_main_pressed_carry_current_src() {
        // First chord fires with `Mic`; then the runtime updates the
        // last-chosen source to `System` (e.g. user picked System in
        // the overlay); the next chord must carry the new source.
        let mut a = Activation::new(LastChosenSource::Mic);
        let first = drive(&mut a, &[mod_down(), main_down(), main_up()]);
        assert_eq!(first, vec![NOOP, toggle(LastChosenSource::Mic), NOOP]);

        a.set_last_source(LastChosenSource::System);
        let second = drive(&mut a, &[main_down()]);
        assert_eq!(second, vec![toggle(LastChosenSource::System)]);
        assert_eq!(a.state(), ActivationState::MainPressed);
    }

    // ---------- Wave-1 carry-forward smokes (kept; cheap insurance) ----------

    #[test]
    fn new_starts_idle_unpaused() {
        let a = Activation::new(LastChosenSource::Mic);
        assert_eq!(a.state(), ActivationState::Idle);
        assert!(!a.is_paused());
        assert_eq!(a.last_source(), LastChosenSource::Mic);
    }
}
