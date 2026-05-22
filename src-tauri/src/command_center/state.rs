//! Command Center pure-Rust state machine.
//!
//! The Command Center is a single bottom-center overlay that surfaces
//! a mode picker (Dictation / Meeting / Activity) when nothing is
//! recording, or a SessionCard with a Stop button when something IS
//! recording. This module owns the logical state transitions only —
//! no IPC, no Tauri, no Win32. The orchestrator in [`super::mod`]
//! drives this state machine in response to OS / IPC events.
//!
//! ## Why a state machine
//!
//! ADR 0037 specifies the surface has several modes (Welcome on first
//! run, mode picker normally, SessionCard mid-recording, Launching\u2026
//! while a runtime dispatches). Each transition has a precondition
//! (e.g. you can't show a SessionCard if no session is live; you
//! can't launch a mode if one is already launching). A typed FSM
//! makes the legal-transition set explicit and testable in
//! milliseconds via the throwaway-crate recipe — the alternative is
//! ad-hoc booleans scattered across `mod.rs`, which is exactly the
//! coupled-flags anti-pattern LESSONS P8 documents.
//!
//! ## Recording-kind discriminator
//!
//! The three modes the Command Center dispatches share a common
//! shape but different runtime owners. [`RecordingKind`] is the
//! discriminator; orchestrators downstream use it to pick which
//! runtime's `start()` / `stop()` to call.
//!
//! Activity is included now (Wave 1A) even though its runtime ships
//! in Wave 1B (`mb-hnl3`) — the state machine has no per-kind
//! special-case logic, so the variant is free. The Activity tile in
//! the UI is rendered disabled until the runtime exists.

#![allow(clippy::module_name_repetitions)]
#![allow(missing_docs)] // Variant prose lives in the module-level ADR + state diagram.

/// Which recording subsystem a session belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecordingKind {
    Dictation,
    Meeting,
    /// Wave 1B onward. Variant exists in 1A so the state machine
    /// doesn't change shape between waves; the UI disables the tile
    /// until the runtime ships.
    Activity,
}

impl RecordingKind {
    /// Stable string identifier for IPC payloads + logs. Mirror on
    /// the TS side at `ui/src/lib/command_center.ts`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Dictation => "dictation",
            Self::Meeting => "meeting",
            Self::Activity => "activity",
        }
    }
}

/// States the Command Center surface can occupy.
///
/// Closed --open--> {ShowingModePicker | ShowingSessionCard}
///   ShowingModePicker --pick--> Launching(kind) --replied--> ShowingSessionCard
///   ShowingSessionCard --stop--> ShowingModePicker (no auto-dismiss)
///   any --dismiss--> Closed
///
/// The `Opening` placeholder state from earlier drafts was elided:
/// Tauri's `show()` call is fast enough that we go straight from
/// `Closed` to `ShowingModePicker` / `ShowingSessionCard`. The
/// first-paint window is handled UI-side via a CSS opacity transition
/// (no FSM state needed for the milliseconds-long gap).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CcState {
    /// Window hidden. Initial + terminal state.
    Closed,
    /// Window visible, no live session — user is looking at the
    /// three mode tiles.
    ShowingModePicker { first_run: bool },
    /// Window visible, a session of `kind` is currently recording.
    /// The card displays elapsed time + a Stop button.
    ShowingSessionCard { kind: RecordingKind },
    /// User picked a mode; we've dispatched to the relevant runtime
    /// and are awaiting its reply (`RuntimeReplied`). Used to debounce
    /// double-clicks: while in this state, further `PickMode` inputs
    /// are ignored.
    Launching { kind: RecordingKind },
}

impl CcState {
    /// Convenient query: is the window visible right now?
    pub const fn is_visible(self) -> bool {
        !matches!(self, Self::Closed)
    }
}

/// Inputs the state machine reacts to. Every variant has at most one
/// effect (visible in the `Transition::effect` field returned from
/// [`apply`]); ambiguous combos are resolved by the precondition
/// matrix below, never by mutating shared state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CcInput {
    /// Chord hotkey fired, tray menu clicked, or first-run boot —
    /// the orchestrator distinguishes via the `first_run` bool and
    /// the `current_session` parameter on [`apply`].
    Open,
    /// User dismissed via Esc / outside-click / second chord press /
    /// tray re-click.
    Dismiss,
    /// User picked a mode tile.
    PickMode { kind: RecordingKind },
    /// The runtime we dispatched to replied (success or failure).
    /// `success=false` means the runtime refused (e.g. mic in use);
    /// state returns to ModePicker so the user can try again.
    RuntimeReplied { success: bool },
    /// User clicked the SessionCard's Stop button.
    Stop,
    /// The currently-running session ended on its own (timer, mic
    /// unplugged, Right-Alt-released for dictation). If we were on a
    /// SessionCard, return to the mode picker.
    SessionEnded { kind: RecordingKind },
}

/// Side effect the orchestrator should run after a transition. The
/// state machine itself is pure; effects are what cross the boundary
/// into IPC / Tauri / runtime calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CcEffect {
    /// No side effect (e.g. dismissed when already closed).
    None,
    /// Show the Tauri window. Caller passes `first_run` to the React
    /// side via the open IPC payload.
    ShowWindow { first_run: bool },
    /// Hide the Tauri window. Caller also flips
    /// `command_center_seen_v1 = true` on the first close.
    HideWindow,
    /// Dispatch to the runtime that owns this kind. Caller calls
    /// `dictation::start_from_command_center()` / `meetings::
    /// start_meeting(default_source)` / (Wave 1B) `activity::start()`.
    DispatchStart { kind: RecordingKind },
    /// Tell the runtime to stop the live session.
    DispatchStop { kind: RecordingKind },
}

/// One state transition. Returned from [`apply`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Transition {
    pub next: CcState,
    pub effect: CcEffect,
}

/// Snapshot of the world the state machine needs at decision time:
/// which (if any) recording is currently live? `None` = nothing
/// recording; `Some(kind)` = a session of that kind is in flight.
///
/// When multiple are concurrently live (rare; user pressed dictation
/// during a meeting), the orchestrator picks the most-recently-started
/// per ADR 0037 §4 and passes that one here. The state machine
/// itself doesn't know about concurrency.
pub type CurrentSession = Option<RecordingKind>;

/// Compute the next state + the side effect to run.
///
/// **Pure function.** No IO, no allocation. The orchestrator calls
/// this synchronously inside a `Mutex<CcState>` critical section so
/// concurrent inputs serialize cleanly. Effects (window show / IPC
/// dispatch) run AFTER the lock is released so they can be async.
///
/// `current_session` is the orchestrator's read of which recording is
/// (most recently) live at the moment this input fires. The state
/// machine uses it to decide whether `Open` lands on the mode picker
/// or on a SessionCard.
///
/// `first_run` is `true` iff `command_center_seen_v1 == false`. The
/// orchestrator reads this once at boot and threads it in; flipping
/// the setting back to true is the orchestrator's job (not this
/// module's) so the FSM stays IO-free.
pub fn apply(
    state: CcState,
    input: CcInput,
    current_session: CurrentSession,
    first_run: bool,
) -> Transition {
    use CcEffect as E;
    use CcInput as I;
    use CcState as S;

    match (state, input) {
        // -------------------- Open --------------------
        (S::Closed, I::Open) => match current_session {
            Some(kind) => Transition {
                next: S::ShowingSessionCard { kind },
                effect: E::ShowWindow { first_run },
            },
            None => Transition {
                next: S::ShowingModePicker { first_run },
                effect: E::ShowWindow { first_run },
            },
        },
        // Re-entrant Open (chord pressed while already open) collapses
        // to a no-op: the window's already up. Avoids the "second
        // press toggles closed" surprise we don't want (the user
        // pressed the chord to OPEN it; same press to close should be
        // Esc / outside-click). Listed explicitly so exhaustiveness
        // checking stays honest: a guard arm wouldn't prove coverage.
        (S::ShowingModePicker { .. }, I::Open)
        | (S::ShowingSessionCard { .. }, I::Open)
        | (S::Launching { .. }, I::Open) => Transition {
            next: state,
            effect: E::None,
        },

        // -------------------- Dismiss --------------------
        (S::Closed, I::Dismiss) => Transition {
            next: S::Closed,
            effect: E::None,
        },
        (_, I::Dismiss) => Transition {
            next: S::Closed,
            effect: E::HideWindow,
        },

        // -------------------- PickMode --------------------
        (S::ShowingModePicker { .. }, I::PickMode { kind }) => Transition {
            next: S::Launching { kind },
            effect: E::DispatchStart { kind },
        },
        // PickMode in any other visible state (Launching, SessionCard)
        // is ignored: double-click debounce + "you already picked
        // something" guard. The user has to Stop / Dismiss first.
        // PickMode while Closed shouldn't happen (UI shouldn't be
        // clickable), but be defensive. Listed explicitly so
        // exhaustiveness checking covers every variant.
        (S::Closed, I::PickMode { .. })
        | (S::ShowingSessionCard { .. }, I::PickMode { .. })
        | (S::Launching { .. }, I::PickMode { .. }) => Transition {
            next: state,
            effect: E::None,
        },

        // -------------------- RuntimeReplied --------------------
        // Success: the runtime accepted; user now sees the SessionCard.
        (S::Launching { kind }, I::RuntimeReplied { success: true }) => Transition {
            next: S::ShowingSessionCard { kind },
            effect: E::None,
        },
        // Failure: return to mode picker so the user can try again.
        (S::Launching { .. }, I::RuntimeReplied { success: false }) => Transition {
            next: S::ShowingModePicker { first_run },
            effect: E::None,
        },
        // Spurious replies (we weren't launching) drop on the floor.
        (s, I::RuntimeReplied { .. }) => Transition {
            next: s,
            effect: E::None,
        },

        // -------------------- Stop --------------------
        // Stop the live recording, then return to the mode picker
        // (per ADR 0037 §Q2: "do not auto-dismiss — user can
        // immediately start a different mode").
        (S::ShowingSessionCard { kind }, I::Stop) => Transition {
            next: S::ShowingModePicker { first_run },
            effect: E::DispatchStop { kind },
        },
        (s, I::Stop) => Transition {
            next: s,
            effect: E::None,
        },

        // -------------------- SessionEnded --------------------
        // The runtime told us its session ended (Right Alt released,
        // meeting Stop button on the meeting overlay, etc.). If we
        // were on the SessionCard for that kind, flip back to picker.
        (S::ShowingSessionCard { kind }, I::SessionEnded { kind: ended }) if kind == ended => {
            Transition {
                next: S::ShowingModePicker { first_run },
                effect: E::None,
            }
        }
        // SessionEnded for a kind we weren't showing is just noise.
        (s, I::SessionEnded { .. }) => Transition {
            next: s,
            effect: E::None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helpers to make test bodies short + readable. Tests are the
    // primary deliverable here (\u226530 per ADR 0037 + phase doc).

    fn picker(first_run: bool) -> CcState {
        CcState::ShowingModePicker { first_run }
    }
    fn card(k: RecordingKind) -> CcState {
        CcState::ShowingSessionCard { kind: k }
    }
    fn launch(k: RecordingKind) -> CcState {
        CcState::Launching { kind: k }
    }

    // ===== RecordingKind sanity =====
    #[test]
    fn recording_kind_strings_are_stable() {
        assert_eq!(RecordingKind::Dictation.as_str(), "dictation");
        assert_eq!(RecordingKind::Meeting.as_str(), "meeting");
        assert_eq!(RecordingKind::Activity.as_str(), "activity");
    }

    #[test]
    fn closed_is_invisible() {
        assert!(!CcState::Closed.is_visible());
    }
    #[test]
    fn picker_is_visible() {
        assert!(picker(false).is_visible());
    }
    #[test]
    fn card_is_visible() {
        assert!(card(RecordingKind::Meeting).is_visible());
    }
    #[test]
    fn launching_is_visible() {
        assert!(launch(RecordingKind::Dictation).is_visible());
    }

    // ===== Open from Closed =====
    #[test]
    fn open_from_closed_no_session_goes_to_picker() {
        let t = apply(CcState::Closed, CcInput::Open, None, false);
        assert_eq!(t.next, picker(false));
        assert_eq!(t.effect, CcEffect::ShowWindow { first_run: false });
    }

    #[test]
    fn open_from_closed_first_run_shows_welcome_picker() {
        let t = apply(CcState::Closed, CcInput::Open, None, true);
        assert_eq!(t.next, picker(true));
        assert_eq!(t.effect, CcEffect::ShowWindow { first_run: true });
    }

    #[test]
    fn open_from_closed_with_dictation_live_goes_to_session_card() {
        let t = apply(
            CcState::Closed,
            CcInput::Open,
            Some(RecordingKind::Dictation),
            false,
        );
        assert_eq!(t.next, card(RecordingKind::Dictation));
        assert_eq!(t.effect, CcEffect::ShowWindow { first_run: false });
    }

    #[test]
    fn open_from_closed_with_meeting_live_goes_to_session_card() {
        let t = apply(
            CcState::Closed,
            CcInput::Open,
            Some(RecordingKind::Meeting),
            false,
        );
        assert_eq!(t.next, card(RecordingKind::Meeting));
    }

    #[test]
    fn open_from_closed_with_activity_live_goes_to_session_card() {
        // Activity-kind is wired here even though its runtime ships
        // in Wave 1B — the state machine doesn't care.
        let t = apply(
            CcState::Closed,
            CcInput::Open,
            Some(RecordingKind::Activity),
            false,
        );
        assert_eq!(t.next, card(RecordingKind::Activity));
    }

    // ===== Re-entrant Open =====
    #[test]
    fn open_while_on_picker_is_noop() {
        let t = apply(picker(false), CcInput::Open, None, false);
        assert_eq!(t.next, picker(false));
        assert_eq!(t.effect, CcEffect::None);
    }

    #[test]
    fn open_while_on_session_card_is_noop() {
        let s = card(RecordingKind::Meeting);
        let t = apply(s, CcInput::Open, Some(RecordingKind::Meeting), false);
        assert_eq!(t.next, s);
        assert_eq!(t.effect, CcEffect::None);
    }

    #[test]
    fn open_while_launching_is_noop() {
        let s = launch(RecordingKind::Dictation);
        let t = apply(s, CcInput::Open, None, false);
        assert_eq!(t.next, s);
        assert_eq!(t.effect, CcEffect::None);
    }

    // ===== Dismiss =====
    #[test]
    fn dismiss_from_picker_closes_and_hides() {
        let t = apply(picker(false), CcInput::Dismiss, None, false);
        assert_eq!(t.next, CcState::Closed);
        assert_eq!(t.effect, CcEffect::HideWindow);
    }

    #[test]
    fn dismiss_from_session_card_closes_but_does_not_stop_recording() {
        // Esc on SessionCard dismisses the window WITHOUT stopping
        // the recording (ADR 0037 §4). Effect is HideWindow only.
        let t = apply(
            card(RecordingKind::Meeting),
            CcInput::Dismiss,
            Some(RecordingKind::Meeting),
            false,
        );
        assert_eq!(t.next, CcState::Closed);
        assert_eq!(t.effect, CcEffect::HideWindow);
    }

    #[test]
    fn dismiss_from_launching_closes() {
        let t = apply(
            launch(RecordingKind::Dictation),
            CcInput::Dismiss,
            None,
            false,
        );
        assert_eq!(t.next, CcState::Closed);
        assert_eq!(t.effect, CcEffect::HideWindow);
    }

    #[test]
    fn dismiss_when_already_closed_is_noop() {
        let t = apply(CcState::Closed, CcInput::Dismiss, None, false);
        assert_eq!(t.next, CcState::Closed);
        assert_eq!(t.effect, CcEffect::None);
    }

    // ===== PickMode =====
    #[test]
    fn pick_dictation_from_picker_dispatches_start() {
        let t = apply(
            picker(false),
            CcInput::PickMode {
                kind: RecordingKind::Dictation,
            },
            None,
            false,
        );
        assert_eq!(t.next, launch(RecordingKind::Dictation));
        assert_eq!(
            t.effect,
            CcEffect::DispatchStart {
                kind: RecordingKind::Dictation
            }
        );
    }

    #[test]
    fn pick_meeting_from_picker_dispatches_start() {
        let t = apply(
            picker(false),
            CcInput::PickMode {
                kind: RecordingKind::Meeting,
            },
            None,
            false,
        );
        assert_eq!(t.next, launch(RecordingKind::Meeting));
        assert_eq!(
            t.effect,
            CcEffect::DispatchStart {
                kind: RecordingKind::Meeting
            }
        );
    }

    #[test]
    fn pick_activity_from_picker_dispatches_start() {
        // Wave 1A wires the dispatch even though the activity runtime
        // doesn't exist yet — the state machine doesn't gate this.
        // The orchestrator's downstream `DispatchStart` handler will
        // log + no-op for Activity until Wave 1B lands the runtime.
        let t = apply(
            picker(true),
            CcInput::PickMode {
                kind: RecordingKind::Activity,
            },
            None,
            true,
        );
        assert_eq!(t.next, launch(RecordingKind::Activity));
    }

    #[test]
    fn pick_mode_while_launching_is_debounced() {
        let s = launch(RecordingKind::Dictation);
        let t = apply(
            s,
            CcInput::PickMode {
                kind: RecordingKind::Meeting,
            },
            None,
            false,
        );
        assert_eq!(t.next, s, "double-pick must not re-dispatch");
        assert_eq!(t.effect, CcEffect::None);
    }

    #[test]
    fn pick_mode_while_on_session_card_is_noop() {
        let s = card(RecordingKind::Meeting);
        let t = apply(
            s,
            CcInput::PickMode {
                kind: RecordingKind::Dictation,
            },
            Some(RecordingKind::Meeting),
            false,
        );
        assert_eq!(t.next, s);
        assert_eq!(t.effect, CcEffect::None);
    }

    #[test]
    fn pick_mode_while_closed_is_noop() {
        let t = apply(
            CcState::Closed,
            CcInput::PickMode {
                kind: RecordingKind::Dictation,
            },
            None,
            false,
        );
        assert_eq!(t.next, CcState::Closed);
        assert_eq!(t.effect, CcEffect::None);
    }

    // ===== RuntimeReplied =====
    #[test]
    fn runtime_replied_success_transitions_to_session_card() {
        let t = apply(
            launch(RecordingKind::Meeting),
            CcInput::RuntimeReplied { success: true },
            Some(RecordingKind::Meeting),
            false,
        );
        assert_eq!(t.next, card(RecordingKind::Meeting));
        assert_eq!(t.effect, CcEffect::None);
    }

    #[test]
    fn runtime_replied_failure_returns_to_picker() {
        let t = apply(
            launch(RecordingKind::Meeting),
            CcInput::RuntimeReplied { success: false },
            None,
            false,
        );
        assert_eq!(t.next, picker(false));
        assert_eq!(t.effect, CcEffect::None);
    }

    #[test]
    fn runtime_replied_failure_preserves_first_run_flag() {
        // If the user fails-out of their first-run pick, the Welcome
        // band should reappear when they fall back to the picker.
        let t = apply(
            launch(RecordingKind::Dictation),
            CcInput::RuntimeReplied { success: false },
            None,
            true,
        );
        assert_eq!(t.next, picker(true));
    }

    #[test]
    fn spurious_runtime_replied_in_picker_is_dropped() {
        let t = apply(
            picker(false),
            CcInput::RuntimeReplied { success: true },
            None,
            false,
        );
        assert_eq!(t.next, picker(false));
        assert_eq!(t.effect, CcEffect::None);
    }

    #[test]
    fn spurious_runtime_replied_when_closed_is_dropped() {
        let t = apply(
            CcState::Closed,
            CcInput::RuntimeReplied { success: true },
            None,
            false,
        );
        assert_eq!(t.next, CcState::Closed);
        assert_eq!(t.effect, CcEffect::None);
    }

    // ===== Stop =====
    #[test]
    fn stop_from_session_card_dispatches_stop_and_returns_to_picker() {
        // ADR 0037 §Q2: "do not auto-dismiss; user can immediately
        // start a different mode."
        let t = apply(
            card(RecordingKind::Meeting),
            CcInput::Stop,
            Some(RecordingKind::Meeting),
            false,
        );
        assert_eq!(t.next, picker(false));
        assert_eq!(
            t.effect,
            CcEffect::DispatchStop {
                kind: RecordingKind::Meeting
            }
        );
    }

    #[test]
    fn stop_from_picker_is_noop() {
        let t = apply(picker(false), CcInput::Stop, None, false);
        assert_eq!(t.next, picker(false));
        assert_eq!(t.effect, CcEffect::None);
    }

    #[test]
    fn stop_from_closed_is_noop() {
        let t = apply(CcState::Closed, CcInput::Stop, None, false);
        assert_eq!(t.next, CcState::Closed);
        assert_eq!(t.effect, CcEffect::None);
    }

    #[test]
    fn stop_from_launching_is_noop() {
        let t = apply(launch(RecordingKind::Dictation), CcInput::Stop, None, false);
        assert_eq!(t.next, launch(RecordingKind::Dictation));
        assert_eq!(t.effect, CcEffect::None);
    }

    // ===== SessionEnded =====
    #[test]
    fn session_ended_for_displayed_kind_returns_to_picker() {
        // Right Alt released while we're on the Dictation SessionCard.
        let t = apply(
            card(RecordingKind::Dictation),
            CcInput::SessionEnded {
                kind: RecordingKind::Dictation,
            },
            None,
            false,
        );
        assert_eq!(t.next, picker(false));
        assert_eq!(t.effect, CcEffect::None);
    }

    #[test]
    fn session_ended_for_different_kind_is_noop() {
        // Showing the dictation card; meeting session ended. Stay put.
        let t = apply(
            card(RecordingKind::Dictation),
            CcInput::SessionEnded {
                kind: RecordingKind::Meeting,
            },
            Some(RecordingKind::Dictation),
            false,
        );
        assert_eq!(t.next, card(RecordingKind::Dictation));
        assert_eq!(t.effect, CcEffect::None);
    }

    #[test]
    fn session_ended_while_closed_is_noop() {
        let t = apply(
            CcState::Closed,
            CcInput::SessionEnded {
                kind: RecordingKind::Meeting,
            },
            None,
            false,
        );
        assert_eq!(t.next, CcState::Closed);
        assert_eq!(t.effect, CcEffect::None);
    }

    #[test]
    fn session_ended_while_on_picker_is_noop() {
        let t = apply(
            picker(false),
            CcInput::SessionEnded {
                kind: RecordingKind::Meeting,
            },
            None,
            false,
        );
        assert_eq!(t.next, picker(false));
        assert_eq!(t.effect, CcEffect::None);
    }

    // ===== End-to-end "happy path" scenarios =====
    #[test]
    fn happy_path_open_pick_meeting_runtime_ok_then_stop() {
        let s = CcState::Closed;
        let s = apply(s, CcInput::Open, None, false).next;
        assert_eq!(s, picker(false));
        let s = apply(
            s,
            CcInput::PickMode {
                kind: RecordingKind::Meeting,
            },
            None,
            false,
        )
        .next;
        assert_eq!(s, launch(RecordingKind::Meeting));
        let s = apply(
            s,
            CcInput::RuntimeReplied { success: true },
            Some(RecordingKind::Meeting),
            false,
        )
        .next;
        assert_eq!(s, card(RecordingKind::Meeting));
        let s = apply(s, CcInput::Stop, Some(RecordingKind::Meeting), false).next;
        assert_eq!(s, picker(false));
    }

    #[test]
    fn first_run_path_open_dismiss_clears_welcome_on_reopen() {
        // First open: Welcome banner. Dismiss flips the seen flag (in
        // the orchestrator); next open is the plain picker.
        let s = apply(CcState::Closed, CcInput::Open, None, true).next;
        assert_eq!(s, picker(true));
        let s = apply(s, CcInput::Dismiss, None, true).next;
        assert_eq!(s, CcState::Closed);
        // Re-open with first_run=false (the orchestrator wrote the
        // setting between these two calls).
        let s = apply(s, CcInput::Open, None, false).next;
        assert_eq!(s, picker(false));
    }

    #[test]
    fn open_during_meeting_then_dismiss_does_not_stop_meeting() {
        // Critical UX invariant: dismissing the SessionCard via Esc
        // must NOT emit DispatchStop. The user wanted to dismiss the
        // CC, not stop their meeting.
        let s = apply(
            CcState::Closed,
            CcInput::Open,
            Some(RecordingKind::Meeting),
            false,
        )
        .next;
        assert_eq!(s, card(RecordingKind::Meeting));
        let t = apply(s, CcInput::Dismiss, Some(RecordingKind::Meeting), false);
        assert_eq!(t.next, CcState::Closed);
        // The only effect is HideWindow — NEVER DispatchStop.
        assert_eq!(t.effect, CcEffect::HideWindow);
    }

    #[test]
    fn double_pick_does_not_double_dispatch() {
        let s = apply(CcState::Closed, CcInput::Open, None, false).next;
        let t1 = apply(
            s,
            CcInput::PickMode {
                kind: RecordingKind::Dictation,
            },
            None,
            false,
        );
        assert_eq!(
            t1.effect,
            CcEffect::DispatchStart {
                kind: RecordingKind::Dictation
            }
        );
        let t2 = apply(
            t1.next,
            CcInput::PickMode {
                kind: RecordingKind::Meeting,
            },
            None,
            false,
        );
        assert_eq!(t2.effect, CcEffect::None, "second pick must be debounced");
    }

    #[test]
    fn runtime_failure_then_retry_works() {
        // Pick dictation, runtime refuses (mic busy), user retries
        // with meeting.
        let s = apply(CcState::Closed, CcInput::Open, None, false).next;
        let s = apply(
            s,
            CcInput::PickMode {
                kind: RecordingKind::Dictation,
            },
            None,
            false,
        )
        .next;
        let s = apply(s, CcInput::RuntimeReplied { success: false }, None, false).next;
        assert_eq!(s, picker(false));
        let t = apply(
            s,
            CcInput::PickMode {
                kind: RecordingKind::Meeting,
            },
            None,
            false,
        );
        assert_eq!(t.next, launch(RecordingKind::Meeting));
        assert_eq!(
            t.effect,
            CcEffect::DispatchStart {
                kind: RecordingKind::Meeting
            }
        );
    }

    // ===== "Every (state, input) pair has a defined outcome" =====
    // The match in `apply` covers all 5 \u00d7 6 = 30 combinations (5
    // states, 6 input kinds with variants treated as their parent).
    // The exhaustiveness is enforced by the compiler; here we just
    // smoke-check that `apply` is total — no panic on any of them.
    #[test]
    fn apply_is_total_over_state_input_cross_product() {
        let states = [
            CcState::Closed,
            picker(false),
            picker(true),
            card(RecordingKind::Dictation),
            card(RecordingKind::Meeting),
            card(RecordingKind::Activity),
            launch(RecordingKind::Dictation),
            launch(RecordingKind::Meeting),
            launch(RecordingKind::Activity),
        ];
        let inputs = [
            CcInput::Open,
            CcInput::Dismiss,
            CcInput::PickMode {
                kind: RecordingKind::Dictation,
            },
            CcInput::PickMode {
                kind: RecordingKind::Meeting,
            },
            CcInput::PickMode {
                kind: RecordingKind::Activity,
            },
            CcInput::RuntimeReplied { success: true },
            CcInput::RuntimeReplied { success: false },
            CcInput::Stop,
            CcInput::SessionEnded {
                kind: RecordingKind::Dictation,
            },
            CcInput::SessionEnded {
                kind: RecordingKind::Meeting,
            },
            CcInput::SessionEnded {
                kind: RecordingKind::Activity,
            },
        ];
        for &s in &states {
            for &i in &inputs {
                // Just make sure it doesn't panic.
                let _ = apply(s, i, None, false);
                let _ = apply(s, i, Some(RecordingKind::Dictation), false);
                let _ = apply(s, i, None, true);
            }
        }
    }
}
