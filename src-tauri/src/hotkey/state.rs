//! State machine for the hotkey pipeline (PLAN §6.1).
//!
//! ## Scope
//!
//! Pure Rust. No OS dependencies. Driven by [`HotkeyEvent`]s from
//! [`super::HotkeyListener`] and the state-machine driver thread (the
//! latter supplies periodic [`HotkeyEvent::Tick`] events so we can
//! advance time-bound transitions deterministically without a wall
//! clock inside this module).
//!
//! ## §6.1 transitions (verbatim)
//!
//! ```text
//! IDLE
//!   └─ on key_down(any mode hotkey, held > 80 ms) → RECORDING(mode)
//!        (taps < 80 ms ignored — passes through to OS for native shortcuts)
//!
//! RECORDING(mode)
//!   ├─ on key_up                    → PROCESSING(mode, audio_buffer)
//!   ├─ on Escape (< 30 s recorded)  → CANCELLED (discard audio)
//!   ├─ on Escape (≥ 30 s recorded)  → CONFIRM_CANCEL toast (3 s timeout = continue)
//!   └─ on duration > 300 s          → STOPPED (auto-stop, treat as key_up)
//!
//! PROCESSING(mode, audio)
//!   ├─ VAD trim
//!   ├─ Whisper transcribe        → raw_transcript (immutable on write)
//!   ├─ Cleanup LLM call(mode)    → cleaned_text
//!   ├─ Secure-input check        → ABORT if secure field focused
//!   ├─ Text inject               → final_text (clipboard saved+restored)
//!   └─ Persist to DB (atomic)    → IDLE
//!   └─ on error                  → ERROR_STATE → IDLE
//! ```
//!
//! ## Special cases (also §6.1)
//!
//! - **Two mode hotkeys held simultaneously:** first wins; second ignored.
//! - **Same hotkey re-pressed during PROCESSING:** ignored.
//! - **Pause-dictation tray toggle:** key-down events no-op until cleared.
//!
//! ## Error state
//!
//! The PLAN's `ERROR_STATE → IDLE` is handled by the orchestrator, not
//! this machine — the orchestrator fires `complete_processing(Err(_))`
//! after persisting the error tombstone, and the machine just returns
//! to [`HotkeyState::Idle`]. UI flashes are an orchestrator concern.

use std::time::{Duration, Instant};

use super::HotkeyEvent;

/// Recording mode requested at the moment of key-down.
///
/// Wave 2 wires only [`HotkeyMode::Normal`]. The Shift/Ctrl modifier
/// detection that selects Fragment / Verbose lands with the actual OS
/// hook in Wave 3 — keeping the variants here lets downstream code
/// pattern-match exhaustively from day one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyMode {
    /// Default mode — long-press, full-sentence cleanup.
    Normal,
    /// Shift+hotkey — short fragment cleanup (no trailing punctuation
    /// added). Not detected in Wave 2; reserved for Phase 5.
    Fragment,
    /// Ctrl+hotkey — verbose cleanup (preserve all hedges + filler).
    /// Not detected in Wave 2; reserved for Phase 5.
    Verbose,
}

/// State of the hotkey pipeline.
///
/// Carries `Instant` markers where the §6.1 transitions need to reason
/// about elapsed time. All timestamps come from incoming
/// [`HotkeyEvent`]s — the state machine never reads the clock itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyState {
    /// No active session. Idle is the only state that responds to
    /// `KeyDown` by entering `PendingHold`.
    Idle,
    /// Key is currently held but we have not yet crossed the
    /// `hold_threshold`. A `KeyUp` here is a *tap* and returns to
    /// [`Idle`] without firing any pipeline action; a `Tick` past the
    /// threshold promotes to [`Recording`].
    PendingHold {
        vk: u32,
        since: Instant,
        mode: HotkeyMode,
    },
    /// Audio capture is live.
    Recording {
        vk: u32,
        mode: HotkeyMode,
        since: Instant,
    },
    /// Pipeline is running (VAD → Whisper → cleanup → inject → DB
    /// commit). New `KeyDown` events are ignored until the orchestrator
    /// calls [`HotkeyStateMachine::complete_processing`].
    Processing { mode: HotkeyMode },
    /// `Escape` was pressed at ≥ `cancel_threshold` into a recording;
    /// a second `Escape` inside `confirm_timeout` confirms the cancel,
    /// otherwise we revert to [`Recording`].
    ConfirmingCancel {
        vk: u32,
        mode: HotkeyMode,
        recording_since: Instant,
        prompt_at: Instant,
    },
}

impl HotkeyState {
    /// Convenience accessor for the orchestrator.
    pub fn is_recording(&self) -> bool {
        matches!(
            self,
            HotkeyState::Recording { .. } | HotkeyState::ConfirmingCancel { .. }
        )
    }
    /// Convenience accessor for the orchestrator.
    pub fn is_processing(&self) -> bool {
        matches!(self, HotkeyState::Processing { .. })
    }
}

/// Side-effect signal emitted by [`HotkeyStateMachine::handle`].
///
/// The state machine itself has no I/O. The orchestrator interprets
/// these actions, drives audio capture / pipeline / UI, and feeds the
/// machine new events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateAction {
    /// Begin audio capture in the given mode.
    StartCapture(HotkeyMode),
    /// Stop audio capture and enter the processing pipeline.
    StopCapture,
    /// Stop capture AND discard the audio buffer (cancel path).
    DiscardAudio,
    /// Show the "Press Escape again to cancel" toast UI.
    ShowConfirmCancel,
    /// Hide the confirm-cancel toast (3 s elapsed or recording exited
    /// via a different path).
    HideConfirmCancel,
    /// Nothing to do for this event. (Returned for ignored / no-op
    /// transitions so callers can pattern-match exhaustively rather
    /// than relying on an `Option`.)
    None,
}

/// Tuning parameters for the state machine. Defaults match §6.1.
#[derive(Debug, Clone, Copy)]
pub struct StateConfig {
    /// Duration a key must be held before transitioning from
    /// `PendingHold` to `Recording`. Default 80 ms; clamped to
    /// [40 ms, 250 ms] in [`StateConfig::clamped`].
    pub hold_threshold: Duration,
    /// Maximum recording duration before auto-stop. Default 300 s.
    pub max_session: Duration,
    /// Recorded duration above which an Escape triggers the
    /// confirm-cancel toast instead of an immediate discard.
    /// Default 30 s.
    pub cancel_threshold: Duration,
    /// Time allowed for the user to confirm cancellation. Default 3 s;
    /// after this, `Tick` reverts to `Recording` and emits
    /// `HideConfirmCancel`.
    pub confirm_timeout: Duration,
}

impl StateConfig {
    /// PLAN §6.1 defaults (80 ms / 300 s / 30 s / 3 s).
    pub const fn default_const() -> Self {
        Self {
            hold_threshold: Duration::from_millis(80),
            max_session: Duration::from_secs(300),
            cancel_threshold: Duration::from_secs(30),
            confirm_timeout: Duration::from_secs(3),
        }
    }

    /// Clamp `hold_threshold` to the [40 ms, 250 ms] tuning band from
    /// PLAN §6.1 (faster than 40 ms triggers on accidental brushes;
    /// slower than 250 ms feels laggy). Other fields pass through.
    pub fn clamped(self) -> Self {
        let lo = Duration::from_millis(40);
        let hi = Duration::from_millis(250);
        let hold = self.hold_threshold.clamp(lo, hi);
        Self {
            hold_threshold: hold,
            ..self
        }
    }
}

impl Default for StateConfig {
    fn default() -> Self {
        Self::default_const()
    }
}

/// The §6.1 state machine.
///
/// Construct with [`HotkeyStateMachine::new`], feed [`HotkeyEvent`]s
/// via [`HotkeyStateMachine::handle`], inspect via
/// [`HotkeyStateMachine::state`]. The orchestrator MUST call
/// [`HotkeyStateMachine::complete_processing`] when the pipeline
/// finishes (success or error); without it the machine stays in
/// `Processing` and refuses new `KeyDown`s.
pub struct HotkeyStateMachine {
    state: HotkeyState,
    config: StateConfig,
    paused: bool,
}

impl HotkeyStateMachine {
    /// New machine in [`HotkeyState::Idle`] with the supplied config.
    pub fn new(config: StateConfig) -> Self {
        Self {
            state: HotkeyState::Idle,
            config: config.clamped(),
            paused: false,
        }
    }

    /// New machine with PLAN §6.1 defaults. Equivalent to
    /// `HotkeyStateMachine::new(StateConfig::default())`.
    pub fn with_defaults() -> Self {
        Self::new(StateConfig::default())
    }

    /// Current state. The orchestrator never mutates this directly;
    /// every transition flows through `handle` / `complete_processing`.
    pub fn state(&self) -> &HotkeyState {
        &self.state
    }

    /// Whether the tray pause toggle is currently set.
    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// Process one [`HotkeyEvent`] and return the side effect for the
    /// orchestrator to apply. Pure: same input + state → same output.
    pub fn handle(&mut self, ev: HotkeyEvent) -> StateAction {
        // PauseToggle is orthogonal to the state machine — it just
        // flips a flag. Handle it first so we don't have to thread it
        // through every other branch.
        if let HotkeyEvent::PauseToggle { paused } = ev {
            self.paused = paused;
            return StateAction::None;
        }

        // PipelineComplete is the orchestrator's signal that
        // `complete()` / `discard()` finished. Routes to the
        // existing `complete_processing()` method which transitions
        // `Processing → Idle`. Without this, the machine sticks in
        // `Processing` after the first hold (§6.1 ignores KeyDown
        // there) — silently breaking every subsequent hold.
        //
        // Idempotent: `complete_processing` is a no-op in any state
        // other than `Processing`, so spurious / duplicate
        // PipelineComplete events can't corrupt state.
        if matches!(ev, HotkeyEvent::PipelineComplete) {
            self.complete_processing();
            return StateAction::None;
        }

        match (self.state, ev) {
            // ---- IDLE ----
            (HotkeyState::Idle, HotkeyEvent::KeyDown { vk, at }) => {
                if self.paused {
                    return StateAction::None;
                }
                // Wave 2 wires only HotkeyMode::Normal. Modifier
                // detection (Shift/Ctrl) → Fragment/Verbose lands in
                // Wave 3 alongside the OS hook.
                self.state = HotkeyState::PendingHold {
                    vk,
                    since: at,
                    mode: HotkeyMode::Normal,
                };
                StateAction::None
            }
            (HotkeyState::Idle, _) => StateAction::None,

            // ---- PENDING_HOLD ----
            (HotkeyState::PendingHold { vk, since, mode }, HotkeyEvent::Tick { at }) => {
                if at.saturating_duration_since(since) >= self.config.hold_threshold {
                    self.state = HotkeyState::Recording {
                        vk,
                        mode,
                        since: at,
                    };
                    StateAction::StartCapture(mode)
                } else {
                    StateAction::None
                }
            }
            (
                HotkeyState::PendingHold {
                    vk: held_vk,
                    since: _,
                    mode: _,
                },
                HotkeyEvent::KeyUp { vk, at: _ },
            ) => {
                if vk == held_vk {
                    // Tap — under threshold. Return to Idle without
                    // firing any pipeline action.
                    self.state = HotkeyState::Idle;
                }
                // KeyUp of a different vk while we're discriminating
                // a tap is a no-op (e.g. some other key released).
                StateAction::None
            }
            (HotkeyState::PendingHold { .. }, HotkeyEvent::KeyDown { .. }) => {
                // §6.1: "Two mode hotkeys held simultaneously: first
                // wins; second ignored." Same applies during the
                // PendingHold discriminator window.
                StateAction::None
            }
            (HotkeyState::PendingHold { .. }, _) => StateAction::None,

            // ---- RECORDING ----
            (
                HotkeyState::Recording {
                    vk: held_vk,
                    mode,
                    since: _,
                },
                HotkeyEvent::KeyUp { vk, at: _ },
            ) => {
                if vk == held_vk {
                    self.state = HotkeyState::Processing { mode };
                    StateAction::StopCapture
                } else {
                    // KeyUp of a different vk: ignore.
                    StateAction::None
                }
            }
            (
                HotkeyState::Recording {
                    vk,
                    mode,
                    since: rec_since,
                },
                HotkeyEvent::Escape { at },
            ) => {
                let elapsed = at.saturating_duration_since(rec_since);
                if elapsed < self.config.cancel_threshold {
                    // Immediate discard.
                    self.state = HotkeyState::Idle;
                    StateAction::DiscardAudio
                } else {
                    // Show confirm-cancel toast; second Escape inside
                    // confirm_timeout will discard.
                    self.state = HotkeyState::ConfirmingCancel {
                        vk,
                        mode,
                        recording_since: rec_since,
                        prompt_at: at,
                    };
                    StateAction::ShowConfirmCancel
                }
            }
            (
                HotkeyState::Recording {
                    vk,
                    mode,
                    since: rec_since,
                },
                HotkeyEvent::Tick { at },
            ) => {
                if at.saturating_duration_since(rec_since) >= self.config.max_session {
                    // §6.1: "on duration > 300 s → STOPPED (auto-stop,
                    // treat as key_up)". Same effect as a real KeyUp.
                    let _ = vk; // explicit acknowledgement we don't need it
                    self.state = HotkeyState::Processing { mode };
                    StateAction::StopCapture
                } else {
                    StateAction::None
                }
            }
            (HotkeyState::Recording { .. }, HotkeyEvent::KeyDown { .. }) => {
                // §6.1: second mode hotkey ignored.
                StateAction::None
            }
            (HotkeyState::Recording { .. }, _) => StateAction::None,

            // ---- PROCESSING ----
            (HotkeyState::Processing { .. }, _) => {
                // §6.1: "Same hotkey re-pressed during PROCESSING:
                // ignored." Escape during PROCESSING is also ignored
                // — by the time we're here the audio buffer is sealed
                // and the pipeline is running on a worker; cancelling
                // mid-processing is out of scope for Phase 3.
                StateAction::None
            }

            // ---- CONFIRMING_CANCEL ----
            (HotkeyState::ConfirmingCancel { .. }, HotkeyEvent::Escape { at: _ }) => {
                // Confirmed — discard.
                self.state = HotkeyState::Idle;
                StateAction::DiscardAudio
            }
            (
                HotkeyState::ConfirmingCancel {
                    vk,
                    mode,
                    recording_since,
                    prompt_at,
                },
                HotkeyEvent::Tick { at },
            ) => {
                // Order matters: the 300 s ceiling is a HARD stop
                // that overrides any other transition in this state.
                // Without this precedence a user could sit on the
                // confirm-cancel toast past the ceiling and the
                // recording would keep growing indefinitely.
                if at.saturating_duration_since(recording_since) >= self.config.max_session {
                    self.state = HotkeyState::Processing { mode };
                    StateAction::StopCapture
                } else if at.saturating_duration_since(prompt_at) >= self.config.confirm_timeout {
                    self.state = HotkeyState::Recording {
                        vk,
                        mode,
                        since: recording_since,
                    };
                    StateAction::HideConfirmCancel
                } else {
                    StateAction::None
                }
            }
            (
                HotkeyState::ConfirmingCancel {
                    vk: held_vk,
                    mode,
                    recording_since,
                    prompt_at: _,
                },
                HotkeyEvent::KeyUp { vk, at: _ },
            ) => {
                if vk == held_vk {
                    // User released the key while we were prompting.
                    // §6.1 treats this as a normal stop — the audio
                    // is committed for processing; the confirm toast
                    // is hidden by the orchestrator.
                    let _ = recording_since;
                    self.state = HotkeyState::Processing { mode };
                    StateAction::StopCapture
                } else {
                    StateAction::None
                }
            }
            (HotkeyState::ConfirmingCancel { .. }, HotkeyEvent::KeyDown { .. }) => {
                StateAction::None
            }
            (HotkeyState::ConfirmingCancel { .. }, _) => StateAction::None,
        }
    }

    /// Called by the orchestrator when the processing pipeline
    /// finishes (success or error). Transitions `Processing` → `Idle`;
    /// no-op in any other state.
    pub fn complete_processing(&mut self) {
        if matches!(self.state, HotkeyState::Processing { .. }) {
            self.state = HotkeyState::Idle;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    // -----------------------------------------------------------------
    // Test helpers
    // -----------------------------------------------------------------

    const VK: u32 = 0xA5; // VK_RMENU — matches ADR 0019 default.

    fn t0() -> Instant {
        Instant::now()
    }

    fn at(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    fn machine() -> HotkeyStateMachine {
        HotkeyStateMachine::with_defaults()
    }

    // -----------------------------------------------------------------
    // Config clamping
    // -----------------------------------------------------------------

    #[test]
    fn hold_threshold_clamped_below_floor() {
        let cfg = StateConfig {
            hold_threshold: Duration::from_millis(10),
            ..StateConfig::default()
        }
        .clamped();
        assert_eq!(cfg.hold_threshold, Duration::from_millis(40));
    }

    #[test]
    fn hold_threshold_clamped_above_ceiling() {
        let cfg = StateConfig {
            hold_threshold: Duration::from_millis(500),
            ..StateConfig::default()
        }
        .clamped();
        assert_eq!(cfg.hold_threshold, Duration::from_millis(250));
    }

    #[test]
    fn hold_threshold_within_band_passes_through() {
        let cfg = StateConfig {
            hold_threshold: Duration::from_millis(120),
            ..StateConfig::default()
        }
        .clamped();
        assert_eq!(cfg.hold_threshold, Duration::from_millis(120));
    }

    #[test]
    fn default_config_matches_plan_section_six_one() {
        let cfg = StateConfig::default();
        assert_eq!(cfg.hold_threshold, Duration::from_millis(80));
        assert_eq!(cfg.max_session, Duration::from_secs(300));
        assert_eq!(cfg.cancel_threshold, Duration::from_secs(30));
        assert_eq!(cfg.confirm_timeout, Duration::from_secs(3));
    }

    // -----------------------------------------------------------------
    // Idle → PendingHold → Idle (tap path)
    // -----------------------------------------------------------------

    #[test]
    fn tap_under_threshold_returns_to_idle_with_no_action() {
        let mut m = machine();
        let t = t0();
        assert_eq!(
            m.handle(HotkeyEvent::KeyDown { vk: VK, at: t }),
            StateAction::None
        );
        assert!(matches!(m.state(), HotkeyState::PendingHold { .. }));

        // KeyUp at +50 ms (under 80 ms threshold) — tap.
        assert_eq!(
            m.handle(HotkeyEvent::KeyUp {
                vk: VK,
                at: at(t, 50)
            }),
            StateAction::None
        );
        assert_eq!(*m.state(), HotkeyState::Idle);
    }

    #[test]
    fn tick_below_hold_threshold_stays_in_pending_hold() {
        let mut m = machine();
        let t = t0();
        m.handle(HotkeyEvent::KeyDown { vk: VK, at: t });
        let act = m.handle(HotkeyEvent::Tick { at: at(t, 30) });
        assert_eq!(act, StateAction::None);
        assert!(matches!(m.state(), HotkeyState::PendingHold { .. }));
    }

    // -----------------------------------------------------------------
    // Idle → PendingHold → Recording (hold path)
    // -----------------------------------------------------------------

    #[test]
    fn tick_at_hold_threshold_transitions_to_recording() {
        let mut m = machine();
        let t = t0();
        m.handle(HotkeyEvent::KeyDown { vk: VK, at: t });
        let act = m.handle(HotkeyEvent::Tick { at: at(t, 80) });
        assert_eq!(act, StateAction::StartCapture(HotkeyMode::Normal));
        assert!(matches!(m.state(), HotkeyState::Recording { .. }));
    }

    #[test]
    fn tick_past_hold_threshold_also_transitions() {
        // Tick may fire later than the threshold; we still want a clean
        // transition (no missed event).
        let mut m = machine();
        let t = t0();
        m.handle(HotkeyEvent::KeyDown { vk: VK, at: t });
        let act = m.handle(HotkeyEvent::Tick { at: at(t, 150) });
        assert_eq!(act, StateAction::StartCapture(HotkeyMode::Normal));
    }

    // -----------------------------------------------------------------
    // Recording → Processing (normal stop)
    // -----------------------------------------------------------------

    #[test]
    fn keyup_in_recording_emits_stopcapture_and_enters_processing() {
        let mut m = machine();
        let t = t0();
        m.handle(HotkeyEvent::KeyDown { vk: VK, at: t });
        m.handle(HotkeyEvent::Tick { at: at(t, 80) });
        let act = m.handle(HotkeyEvent::KeyUp {
            vk: VK,
            at: at(t, 5_000),
        });
        assert_eq!(act, StateAction::StopCapture);
        assert!(matches!(m.state(), HotkeyState::Processing { .. }));
    }

    #[test]
    fn keyup_of_unrelated_vk_in_recording_is_ignored() {
        let mut m = machine();
        let t = t0();
        m.handle(HotkeyEvent::KeyDown { vk: VK, at: t });
        m.handle(HotkeyEvent::Tick { at: at(t, 80) });
        let act = m.handle(HotkeyEvent::KeyUp {
            vk: 0x70, // VK_F1
            at: at(t, 1_000),
        });
        assert_eq!(act, StateAction::None);
        assert!(matches!(m.state(), HotkeyState::Recording { .. }));
    }

    // -----------------------------------------------------------------
    // Recording → DiscardAudio (Escape under 30 s)
    // -----------------------------------------------------------------

    #[test]
    fn escape_under_cancel_threshold_discards_immediately() {
        let mut m = machine();
        let t = t0();
        m.handle(HotkeyEvent::KeyDown { vk: VK, at: t });
        m.handle(HotkeyEvent::Tick { at: at(t, 80) });
        let act = m.handle(HotkeyEvent::Escape { at: at(t, 5_000) });
        assert_eq!(act, StateAction::DiscardAudio);
        assert_eq!(*m.state(), HotkeyState::Idle);
    }

    // -----------------------------------------------------------------
    // Recording → ConfirmingCancel (Escape ≥ 30 s)
    // -----------------------------------------------------------------

    #[test]
    fn escape_at_or_above_cancel_threshold_shows_confirm_toast() {
        let mut m = machine();
        let t = t0();
        m.handle(HotkeyEvent::KeyDown { vk: VK, at: t });
        m.handle(HotkeyEvent::Tick { at: at(t, 80) });
        let act = m.handle(HotkeyEvent::Escape { at: at(t, 60_000) });
        assert_eq!(act, StateAction::ShowConfirmCancel);
        assert!(matches!(m.state(), HotkeyState::ConfirmingCancel { .. }));
    }

    #[test]
    fn second_escape_within_confirm_timeout_discards() {
        let mut m = machine();
        let t = t0();
        m.handle(HotkeyEvent::KeyDown { vk: VK, at: t });
        m.handle(HotkeyEvent::Tick { at: at(t, 80) });
        m.handle(HotkeyEvent::Escape { at: at(t, 60_000) });
        let act = m.handle(HotkeyEvent::Escape { at: at(t, 61_500) });
        assert_eq!(act, StateAction::DiscardAudio);
        assert_eq!(*m.state(), HotkeyState::Idle);
    }

    #[test]
    fn confirm_timeout_reverts_to_recording() {
        let mut m = machine();
        let t = t0();
        m.handle(HotkeyEvent::KeyDown { vk: VK, at: t });
        m.handle(HotkeyEvent::Tick { at: at(t, 80) });
        m.handle(HotkeyEvent::Escape { at: at(t, 60_000) });
        let act = m.handle(HotkeyEvent::Tick {
            at: at(t, 63_500), // > 3 s after prompt_at
        });
        assert_eq!(act, StateAction::HideConfirmCancel);
        assert!(matches!(m.state(), HotkeyState::Recording { .. }));
    }

    #[test]
    fn keyup_during_confirming_cancel_commits_recording() {
        // If the user releases the hotkey while we're prompting, the
        // recording finishes normally — they wanted the audio.
        let mut m = machine();
        let t = t0();
        m.handle(HotkeyEvent::KeyDown { vk: VK, at: t });
        m.handle(HotkeyEvent::Tick { at: at(t, 80) });
        m.handle(HotkeyEvent::Escape { at: at(t, 60_000) });
        let act = m.handle(HotkeyEvent::KeyUp {
            vk: VK,
            at: at(t, 60_500),
        });
        assert_eq!(act, StateAction::StopCapture);
        assert!(matches!(m.state(), HotkeyState::Processing { .. }));
    }

    // -----------------------------------------------------------------
    // Recording → Processing (300 s auto-stop)
    // -----------------------------------------------------------------

    #[test]
    fn tick_past_max_session_auto_stops() {
        let mut m = machine();
        let t = t0();
        m.handle(HotkeyEvent::KeyDown { vk: VK, at: t });
        m.handle(HotkeyEvent::Tick { at: at(t, 80) });
        let act = m.handle(HotkeyEvent::Tick {
            at: at(t, 300_500), // past 300 s ceiling
        });
        assert_eq!(act, StateAction::StopCapture);
        assert!(matches!(m.state(), HotkeyState::Processing { .. }));
    }

    #[test]
    fn tick_under_max_session_in_recording_is_noop() {
        let mut m = machine();
        let t = t0();
        m.handle(HotkeyEvent::KeyDown { vk: VK, at: t });
        m.handle(HotkeyEvent::Tick { at: at(t, 80) });
        let act = m.handle(HotkeyEvent::Tick {
            at: at(t, 250_000), // 250 s in
        });
        assert_eq!(act, StateAction::None);
        assert!(matches!(m.state(), HotkeyState::Recording { .. }));
    }

    #[test]
    fn max_session_overrides_confirm_cancel() {
        // Edge: user hits Escape at 290 s, then sits on the confirm
        // toast past the 300 s ceiling. We still auto-stop.
        let mut m = machine();
        let t = t0();
        m.handle(HotkeyEvent::KeyDown { vk: VK, at: t });
        m.handle(HotkeyEvent::Tick { at: at(t, 80) });
        m.handle(HotkeyEvent::Escape { at: at(t, 290_000) });
        // Tick past max_session but inside confirm_timeout.
        let act = m.handle(HotkeyEvent::Tick { at: at(t, 300_500) });
        assert_eq!(act, StateAction::StopCapture);
        assert!(matches!(m.state(), HotkeyState::Processing { .. }));
    }

    // -----------------------------------------------------------------
    // PROCESSING is sticky
    // -----------------------------------------------------------------

    #[test]
    fn keydown_in_processing_is_ignored() {
        let mut m = machine();
        let t = t0();
        m.handle(HotkeyEvent::KeyDown { vk: VK, at: t });
        m.handle(HotkeyEvent::Tick { at: at(t, 80) });
        m.handle(HotkeyEvent::KeyUp {
            vk: VK,
            at: at(t, 1_000),
        });
        assert!(matches!(m.state(), HotkeyState::Processing { .. }));

        let act = m.handle(HotkeyEvent::KeyDown {
            vk: VK,
            at: at(t, 2_000),
        });
        assert_eq!(act, StateAction::None);
        assert!(matches!(m.state(), HotkeyState::Processing { .. }));
    }

    #[test]
    fn escape_in_processing_is_ignored() {
        let mut m = machine();
        let t = t0();
        m.handle(HotkeyEvent::KeyDown { vk: VK, at: t });
        m.handle(HotkeyEvent::Tick { at: at(t, 80) });
        m.handle(HotkeyEvent::KeyUp {
            vk: VK,
            at: at(t, 1_000),
        });
        let act = m.handle(HotkeyEvent::Escape { at: at(t, 1_500) });
        assert_eq!(act, StateAction::None);
        assert!(matches!(m.state(), HotkeyState::Processing { .. }));
    }

    #[test]
    fn complete_processing_returns_to_idle() {
        let mut m = machine();
        let t = t0();
        m.handle(HotkeyEvent::KeyDown { vk: VK, at: t });
        m.handle(HotkeyEvent::Tick { at: at(t, 80) });
        m.handle(HotkeyEvent::KeyUp {
            vk: VK,
            at: at(t, 1_000),
        });
        m.complete_processing();
        assert_eq!(*m.state(), HotkeyState::Idle);
    }

    /// Wave 4.8 regression: PipelineComplete event must route to
    /// `complete_processing()`. Without this routing the second hold
    /// of any session is silently dropped (machine stays in
    /// `Processing` forever — production was missing the only
    /// production caller of complete_processing).
    #[test]
    fn pipeline_complete_event_returns_processing_to_idle() {
        let mut m = machine();
        let t = t0();
        m.handle(HotkeyEvent::KeyDown { vk: VK, at: t });
        m.handle(HotkeyEvent::Tick { at: at(t, 80) });
        m.handle(HotkeyEvent::KeyUp {
            vk: VK,
            at: at(t, 1_000),
        });
        assert!(matches!(m.state(), HotkeyState::Processing { .. }));

        let act = m.handle(HotkeyEvent::PipelineComplete);
        assert_eq!(act, StateAction::None);
        assert_eq!(*m.state(), HotkeyState::Idle);

        // Critical: a new KeyDown after PipelineComplete must be
        // accepted (this is the bug Dustin reported live).
        let act2 = m.handle(HotkeyEvent::KeyDown {
            vk: VK,
            at: at(t, 2_000),
        });
        assert_eq!(act2, StateAction::None); // PendingHold, not yet StartCapture
        assert!(matches!(m.state(), HotkeyState::PendingHold { .. }));
    }

    /// PipelineComplete in any non-Processing state is a tolerated
    /// no-op (idempotent). Guards against double-signal hazards.
    #[test]
    fn pipeline_complete_event_in_idle_is_noop() {
        let mut m = machine();
        let act = m.handle(HotkeyEvent::PipelineComplete);
        assert_eq!(act, StateAction::None);
        assert_eq!(*m.state(), HotkeyState::Idle);
    }

    #[test]
    fn complete_processing_in_idle_is_noop() {
        let mut m = machine();
        m.complete_processing();
        assert_eq!(*m.state(), HotkeyState::Idle);
    }

    // -----------------------------------------------------------------
    // Pause toggle
    // -----------------------------------------------------------------

    #[test]
    fn pause_blocks_keydown_in_idle() {
        let mut m = machine();
        m.handle(HotkeyEvent::PauseToggle { paused: true });
        assert!(m.is_paused());
        let t = t0();
        let act = m.handle(HotkeyEvent::KeyDown { vk: VK, at: t });
        assert_eq!(act, StateAction::None);
        assert_eq!(*m.state(), HotkeyState::Idle);
    }

    #[test]
    fn unpause_re_enables_keydown() {
        let mut m = machine();
        m.handle(HotkeyEvent::PauseToggle { paused: true });
        m.handle(HotkeyEvent::PauseToggle { paused: false });
        let t = t0();
        m.handle(HotkeyEvent::KeyDown { vk: VK, at: t });
        let act = m.handle(HotkeyEvent::Tick { at: at(t, 80) });
        assert_eq!(act, StateAction::StartCapture(HotkeyMode::Normal));
    }

    #[test]
    fn pause_does_not_interrupt_active_recording() {
        // §6.1 phrasing: "key-down events no-op until cleared." Pause
        // toggles flag only. An active recording continues; the
        // protection is at the IDLE → PendingHold edge.
        let mut m = machine();
        let t = t0();
        m.handle(HotkeyEvent::KeyDown { vk: VK, at: t });
        m.handle(HotkeyEvent::Tick { at: at(t, 80) });
        m.handle(HotkeyEvent::PauseToggle { paused: true });
        assert!(matches!(m.state(), HotkeyState::Recording { .. }));
        let act = m.handle(HotkeyEvent::KeyUp {
            vk: VK,
            at: at(t, 1_000),
        });
        assert_eq!(act, StateAction::StopCapture);
    }

    // -----------------------------------------------------------------
    // Double-key / concurrent-press semantics
    // -----------------------------------------------------------------

    #[test]
    fn second_keydown_during_pending_hold_ignored() {
        let mut m = machine();
        let t = t0();
        m.handle(HotkeyEvent::KeyDown { vk: VK, at: t });
        let act = m.handle(HotkeyEvent::KeyDown {
            vk: 0x70, // VK_F1
            at: at(t, 20),
        });
        assert_eq!(act, StateAction::None);
        // Still discriminating the FIRST key.
        if let HotkeyState::PendingHold { vk: pending_vk, .. } = *m.state() {
            assert_eq!(pending_vk, VK);
        } else {
            panic!("expected PendingHold, got {:?}", m.state());
        }
    }

    #[test]
    fn second_keydown_during_recording_ignored() {
        let mut m = machine();
        let t = t0();
        m.handle(HotkeyEvent::KeyDown { vk: VK, at: t });
        m.handle(HotkeyEvent::Tick { at: at(t, 80) });
        let act = m.handle(HotkeyEvent::KeyDown {
            vk: 0x70,
            at: at(t, 500),
        });
        assert_eq!(act, StateAction::None);
        // First mode still wins.
        if let HotkeyState::Recording { vk, .. } = *m.state() {
            assert_eq!(vk, VK);
        } else {
            panic!("expected Recording, got {:?}", m.state());
        }
    }

    // -----------------------------------------------------------------
    // Convenience accessors
    // -----------------------------------------------------------------

    #[test]
    fn is_recording_true_in_recording_and_confirming_cancel() {
        let mut m = machine();
        let t = t0();
        m.handle(HotkeyEvent::KeyDown { vk: VK, at: t });
        m.handle(HotkeyEvent::Tick { at: at(t, 80) });
        assert!(m.state().is_recording());
        m.handle(HotkeyEvent::Escape { at: at(t, 60_000) });
        assert!(m.state().is_recording());
    }

    #[test]
    fn is_processing_only_in_processing() {
        let mut m = machine();
        let t = t0();
        m.handle(HotkeyEvent::KeyDown { vk: VK, at: t });
        m.handle(HotkeyEvent::Tick { at: at(t, 80) });
        assert!(!m.state().is_processing());
        m.handle(HotkeyEvent::KeyUp {
            vk: VK,
            at: at(t, 1_000),
        });
        assert!(m.state().is_processing());
    }
}
