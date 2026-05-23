//! Command Center orchestrator engine — the pure-Rust core of `drive()`.
//!
//! ## Why this file exists (mb-pxe7 / mb-a0f3 hotfix-of-hotfix)
//!
//! The Command Center has a state machine (pure, in `state.rs`) and an
//! orchestrator (in `mod.rs::drive`) that runs the FSM and dispatches
//! the resulting effects to the world (Tauri windows, runtime starts,
//! state-event emits). The orchestrator's logic is small but
//! load-bearing: it sequences the apply→effect→snapshot→emit dance
//! that the React UI binds to. Two consecutive bugs (`mb-23rh`
//! recursive-emit clobber, `mb-q2if` / `mb-a0f3` empty-black-box) both
//! escaped the unit-test suite because `drive()` was inseparable from
//! the Tauri AppHandle and so untestable at unit scope.
//!
//! This module fixes the testability gap: [`drive_engine`] is a free
//! function over a [`CcEffects`] trait. Production wires it via a
//! `TauriEffects` adapter in `mod.rs`. Tests wire it via a `MockEffects`
//! recorder that captures the full call sequence per input — making
//! every user-facing path (chord press, tile pick, Esc, Stop button,
//! outside-click, mid-flight session-end) trivially unit-testable.
//!
//! ## The single load-bearing invariant
//!
//! `emit_state` must broadcast the ACTUAL post-effect FSM state, not
//! the value captured from `apply()` at the top of the call. Effects
//! like `DispatchStart` recurse into `drive_engine` (with the runtime's
//! reply as a new input), and that inner call has already updated the
//! state mutex and emitted to the UI. If the outer call then emits its
//! stale captured `next`, the UI snaps backwards — the mb-23rh
//! symptom. See the comment inside [`drive_engine`] for the
//! mechanical detail.
//!
//! Everything in this file is `#[cfg(test)]`-friendly: no Tauri,
//! no Win32, no async runtime. Throwaway-crate testable per LESSONS P2.

#![allow(clippy::module_name_repetitions)]

use std::sync::{atomic::AtomicBool, atomic::Ordering, Mutex};

use super::state::{apply, CcEffect, CcInput, CcState, RecordingKind};

/// Outcome of a runtime-start dispatch. Returned synchronously from
/// [`CcEffects::dispatch_start`] so the engine can feed the result
/// back into the FSM without leaking IO concerns into pure logic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchOutcome {
    /// Runtime accepted (`success: true`) or refused (`success: false`).
    /// Engine drives [`CcInput::RuntimeReplied`] next.
    Replied { success: bool },
    /// Runtime has no programmatic start surface (Dictation —
    /// the user holds Right Alt; picking the tile just dismisses
    /// the Command Center). Engine drives [`CcInput::Dismiss`] next.
    NoProgrammaticStart,
}

/// The full IO surface [`drive_engine`] needs. Production impl wraps
/// the Tauri `AppHandle` + runtime handles; test impl records calls.
///
/// All methods are `&self`-only — no `&mut`. The engine serializes
/// state mutations via the `Mutex<CcState>` it owns; the effects
/// trait is for side-effects that don't need engine coordination
/// (each impl manages its own synchronization, if any).
pub trait CcEffects {
    /// Snapshot of "is anything currently recording, and what?". The
    /// FSM uses this to choose between mode-picker and session-card
    /// on `Open`. Production: read from the `current_session` mutex.
    /// Tests: configurable per scenario.
    fn current_session(&self) -> Option<RecordingKind>;

    /// Show the Command Center webview window.
    fn show_window(&self);

    /// Hide the Command Center webview window.
    fn hide_window(&self);

    /// Dispatch the runtime-start for `kind`. Synchronous: see
    /// [`DispatchOutcome`] for the contract.
    ///
    /// On `Replied { success: true }`, the impl is responsible for
    /// recording the new active session (so subsequent
    /// [`Self::current_session`] reads reflect it) BEFORE this
    /// method returns.
    fn dispatch_start(&self, kind: RecordingKind) -> DispatchOutcome;

    /// Tell the runtime to stop a live session of `kind`. Impl
    /// clears its `current_session` on success.
    fn dispatch_stop(&self, kind: RecordingKind);

    /// Broadcast the current FSM state + `first_run` flag to the
    /// React UI (production: `app.emit(STATE_EVENT, …)`).
    fn emit_state(&self, state: CcState, first_run: bool);

    /// Persist `command_center_seen_v1 = true` to the settings store.
    /// Called exactly once, on the first dismiss after first-run boot.
    fn persist_seen_flag(&self);
}

/// The orchestrator's pure core. Drives one FSM transition for `input`,
/// runs the resulting effect (possibly recursing for
/// `DispatchStart` → `RuntimeReplied`), then emits the post-effect
/// state to the UI. Idempotent w.r.t. repeated calls; the FSM's own
/// no-op transitions (e.g. `Dismiss` while Closed) keep this safe.
///
/// ## Concurrency
///
/// `state` is a `Mutex` so concurrent inputs serialize cleanly. The
/// engine releases the lock BEFORE running the effect (so an effect
/// that recurses doesn't deadlock on itself, and so external effects
/// don't hold the state lock across IO). `first_run` is an
/// `AtomicBool` because it's read often and only flipped once.
///
/// ## The load-bearing post-effect snapshot
///
/// After the effect runs, we re-read the state mutex and emit THAT —
/// not the `next` value we cached before the effect. This is the
/// mb-23rh / mb-q2if hotfix: recursive `RuntimeReplied` already
/// advanced the FSM and emitted; emitting the stale outer `next`
/// would snap the UI backwards. The cost is one extra mutex acquire
/// per drive; the benefit is the modal not freezing on every tile
/// pick.
pub fn drive_engine<E: CcEffects>(
    state: &Mutex<CcState>,
    first_run: &AtomicBool,
    effects: &E,
    input: CcInput,
) {
    // ---- FSM step (critical section) ----
    let (effect, captured_first_run) = {
        let mut guard = state.lock().expect("cc state mutex must be unpoisoned");
        let session = effects.current_session();
        let cfr = first_run.load(Ordering::Relaxed);
        let t = apply(*guard, input, session, cfr);
        *guard = t.next;
        (t.effect, cfr)
    };

    tracing::info!(
        target: "command_center",
        ?input,
        ?effect,
        "fsm step"
    );

    // ---- Run the effect ----
    //
    // DispatchStart/Stop may recurse via drive_engine; that's why we
    // dropped the lock above (and why the engine is a free function
    // rather than a method — recursion through &mut self would
    // require interior mutability anyway).
    match effect {
        CcEffect::None => {}
        CcEffect::ShowWindow { .. } => effects.show_window(),
        CcEffect::HideWindow => effects.hide_window(),
        CcEffect::DispatchStart { kind } => match effects.dispatch_start(kind) {
            DispatchOutcome::Replied { success } => {
                drive_engine(
                    state,
                    first_run,
                    effects,
                    CcInput::RuntimeReplied { success },
                );
            }
            DispatchOutcome::NoProgrammaticStart => {
                drive_engine(state, first_run, effects, CcInput::Dismiss);
            }
        },
        CcEffect::DispatchStop { kind } => effects.dispatch_stop(kind),
    }

    // ---- Post-effect emit (the load-bearing snapshot) ----
    let actual = *state.lock().expect("cc state mutex must be unpoisoned");
    let actual_first_run = first_run.load(Ordering::Relaxed);
    effects.emit_state(actual, actual_first_run);

    // ---- First-dismiss seen-flag flip ----
    // Captured `first_run` from before the effect ran; if a recursive
    // Dismiss already flipped it, we don't want to re-persist (no-op
    // harm but extra DB write).
    if matches!(input, CcInput::Dismiss) && captured_first_run {
        first_run.store(false, Ordering::Relaxed);
        effects.persist_seen_flag();
    }
}

// =====================================================================
// Tests — covers all 7 user-facing paths from the kickoff acceptance
// criteria, plus the regressions both hotfixes were meant to address.
// Pure-Rust; runs via `cargo test` and via the LESSONS P2 throwaway-
// crate recipe.
// =====================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    /// Records every effect-trait call. Tests assert against `calls`
    /// to verify the sequence and contents of the orchestrator's
    /// emissions for a given input.
    #[derive(Debug, Default)]
    struct MockEffects {
        /// Append-only call log. Each variant captures the args
        /// passed to the trait method.
        calls: StdMutex<Vec<Call>>,
        /// Configurable: what `current_session` returns. Tests set
        /// this once before driving.
        current_session: StdMutex<Option<RecordingKind>>,
        /// Configurable: per-kind dispatch outcome. Defaults to
        /// Replied{success:true} for Meeting/Activity,
        /// NoProgrammaticStart for Dictation.
        start_outcomes: StdMutex<Vec<(RecordingKind, DispatchOutcome)>>,
        /// Side-effect: on a successful `dispatch_start` for
        /// Meeting/Activity, set current_session to Some(kind) so
        /// subsequent reads see it (matches production semantics).
        auto_set_session_on_start: StdMutex<bool>,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Call {
        ShowWindow,
        HideWindow,
        DispatchStart(RecordingKind),
        DispatchStop(RecordingKind),
        EmitState { state: CcState, first_run: bool },
        PersistSeenFlag,
    }

    impl MockEffects {
        fn new() -> Self {
            let me = Self::default();
            *me.auto_set_session_on_start.lock().unwrap() = true;
            me
        }

        fn with_session(self, kind: Option<RecordingKind>) -> Self {
            *self.current_session.lock().unwrap() = kind;
            self
        }

        fn with_start_outcome(self, kind: RecordingKind, outcome: DispatchOutcome) -> Self {
            self.start_outcomes.lock().unwrap().push((kind, outcome));
            self
        }

        fn calls(&self) -> Vec<Call> {
            self.calls.lock().unwrap().clone()
        }

        fn emit_payloads(&self) -> Vec<(CcState, bool)> {
            self.calls()
                .into_iter()
                .filter_map(|c| match c {
                    Call::EmitState { state, first_run } => Some((state, first_run)),
                    _ => None,
                })
                .collect()
        }
    }

    impl CcEffects for MockEffects {
        fn current_session(&self) -> Option<RecordingKind> {
            *self.current_session.lock().unwrap()
        }
        fn show_window(&self) {
            self.calls.lock().unwrap().push(Call::ShowWindow);
        }
        fn hide_window(&self) {
            self.calls.lock().unwrap().push(Call::HideWindow);
        }
        fn dispatch_start(&self, kind: RecordingKind) -> DispatchOutcome {
            self.calls.lock().unwrap().push(Call::DispatchStart(kind));
            let mut outcomes = self.start_outcomes.lock().unwrap();
            let outcome = outcomes
                .iter()
                .position(|(k, _)| *k == kind)
                .map(|i| outcomes.remove(i).1)
                .unwrap_or_else(|| match kind {
                    RecordingKind::Dictation => DispatchOutcome::NoProgrammaticStart,
                    _ => DispatchOutcome::Replied { success: true },
                });
            // Production-semantic: a successful start of Meeting/Activity
            // records the active session so the FSM's next current_session()
            // read reflects it.
            if matches!(outcome, DispatchOutcome::Replied { success: true })
                && !matches!(kind, RecordingKind::Dictation)
                && *self.auto_set_session_on_start.lock().unwrap()
            {
                *self.current_session.lock().unwrap() = Some(kind);
            }
            outcome
        }
        fn dispatch_stop(&self, kind: RecordingKind) {
            self.calls.lock().unwrap().push(Call::DispatchStop(kind));
            *self.current_session.lock().unwrap() = None;
        }
        fn emit_state(&self, state: CcState, first_run: bool) {
            self.calls
                .lock()
                .unwrap()
                .push(Call::EmitState { state, first_run });
        }
        fn persist_seen_flag(&self) {
            self.calls.lock().unwrap().push(Call::PersistSeenFlag);
        }
    }

    fn fresh(first_run: bool) -> (Mutex<CcState>, AtomicBool) {
        (Mutex::new(CcState::Closed), AtomicBool::new(first_run))
    }

    // ---------- Path 1: first-run chord press ----------

    #[test]
    fn path1_first_run_chord_open_emits_modepicker_with_welcome() {
        let (state, fr) = fresh(true);
        let fx = MockEffects::new();
        drive_engine(&state, &fr, &fx, CcInput::Open);

        assert_eq!(
            *state.lock().unwrap(),
            CcState::ShowingModePicker { first_run: true }
        );
        assert_eq!(
            fx.calls(),
            vec![
                Call::ShowWindow,
                Call::EmitState {
                    state: CcState::ShowingModePicker { first_run: true },
                    first_run: true,
                },
            ],
            "first-run open must show window then emit modePicker with first_run=true",
        );
    }

    // ---------- Path 2: subsequent (non-first-run) chord press ----------

    #[test]
    fn path2_subsequent_chord_open_emits_modepicker_no_welcome() {
        let (state, fr) = fresh(false);
        let fx = MockEffects::new();
        drive_engine(&state, &fr, &fx, CcInput::Open);

        assert_eq!(
            *state.lock().unwrap(),
            CcState::ShowingModePicker { first_run: false }
        );
        assert_eq!(
            fx.emit_payloads(),
            vec![(CcState::ShowingModePicker { first_run: false }, false)],
        );
    }

    #[test]
    fn path2b_reentrant_chord_is_noop() {
        // Window already open on modepicker — second chord must NOT
        // toggle closed; per ADR 0037 §4, Esc / outside-click is how
        // you close. Engine should still emit (so UI stays consistent)
        // but neither show nor hide.
        let (state, fr) = fresh(false);
        *state.lock().unwrap() = CcState::ShowingModePicker { first_run: false };
        let fx = MockEffects::new();
        drive_engine(&state, &fr, &fx, CcInput::Open);

        let calls = fx.calls();
        assert!(!calls.contains(&Call::ShowWindow), "no duplicate show");
        assert!(!calls.contains(&Call::HideWindow), "definitely no hide");
        assert_eq!(
            fx.emit_payloads(),
            vec![(CcState::ShowingModePicker { first_run: false }, false)],
        );
    }

    // ---------- Path 3: tile pick (Dictation — push-to-talk) ----------

    #[test]
    fn path3_dictation_tile_pick_dismisses_and_emits_closed() {
        // Dictation has no programmatic start; engine should treat
        // the DispatchStart as a NoProgrammaticStart and recurse with
        // Dismiss → HideWindow → emit Closed.
        let (state, fr) = fresh(false);
        *state.lock().unwrap() = CcState::ShowingModePicker { first_run: false };
        let fx = MockEffects::new(); // Default Dictation outcome is NoProgrammaticStart

        drive_engine(
            &state,
            &fr,
            &fx,
            CcInput::PickMode {
                kind: RecordingKind::Dictation,
            },
        );

        assert_eq!(*state.lock().unwrap(), CcState::Closed);
        let calls = fx.calls();
        // Sequence: DispatchStart(Dictation), HideWindow (from inner Dismiss),
        // EmitState(Closed) from inner Dismiss, EmitState(Closed) from outer.
        // Both emits are Closed — same payload, React de-dupes.
        assert!(matches!(
            calls.first(),
            Some(Call::DispatchStart(RecordingKind::Dictation))
        ));
        assert!(
            calls.contains(&Call::HideWindow),
            "must hide window on Dictation pick (push-to-talk hint)"
        );
        // Final emit must be Closed (the load-bearing post-effect snapshot
        // invariant — without it, the outer would emit the stale Launching).
        assert_eq!(
            fx.emit_payloads().last(),
            Some(&(CcState::Closed, false)),
            "outer emit must reflect actual post-effect state, NOT stale Launching{{Dictation}}",
        );
    }

    // ---------- Path 3b: tile pick (Meeting — synchronous-success runtime) ----------

    #[test]
    fn path3b_meeting_tile_pick_lands_on_sessioncard() {
        let (state, fr) = fresh(false);
        *state.lock().unwrap() = CcState::ShowingModePicker { first_run: false };
        let fx = MockEffects::new(); // Meeting defaults to Replied{success:true}

        drive_engine(
            &state,
            &fr,
            &fx,
            CcInput::PickMode {
                kind: RecordingKind::Meeting,
            },
        );

        assert_eq!(
            *state.lock().unwrap(),
            CcState::ShowingSessionCard {
                kind: RecordingKind::Meeting
            }
        );
        // Most-critical assertion: the FINAL emit must be SessionCard,
        // not Launching. mb-23rh / mb-q2if regression guard.
        assert_eq!(
            fx.emit_payloads().last(),
            Some(&(
                CcState::ShowingSessionCard {
                    kind: RecordingKind::Meeting
                },
                false
            )),
            "outer drive must NOT clobber the inner RuntimeReplied emit with stale Launching",
        );
    }

    #[test]
    fn path3c_activity_tile_pick_lands_on_sessioncard() {
        let (state, fr) = fresh(false);
        *state.lock().unwrap() = CcState::ShowingModePicker { first_run: false };
        let fx = MockEffects::new();

        drive_engine(
            &state,
            &fr,
            &fx,
            CcInput::PickMode {
                kind: RecordingKind::Activity,
            },
        );

        assert_eq!(
            *state.lock().unwrap(),
            CcState::ShowingSessionCard {
                kind: RecordingKind::Activity
            }
        );
        assert_eq!(
            fx.emit_payloads().last(),
            Some(&(
                CcState::ShowingSessionCard {
                    kind: RecordingKind::Activity
                },
                false
            )),
        );
    }

    #[test]
    fn path3d_meeting_tile_pick_runtime_refuses_returns_to_picker() {
        // Runtime refuses (mic busy, etc.). FSM goes Launching →
        // back to ModePicker; user can retry or pick a different tile.
        let (state, fr) = fresh(false);
        *state.lock().unwrap() = CcState::ShowingModePicker { first_run: false };
        let fx = MockEffects::new().with_start_outcome(
            RecordingKind::Meeting,
            DispatchOutcome::Replied { success: false },
        );

        drive_engine(
            &state,
            &fr,
            &fx,
            CcInput::PickMode {
                kind: RecordingKind::Meeting,
            },
        );

        assert_eq!(
            *state.lock().unwrap(),
            CcState::ShowingModePicker { first_run: false },
            "refused runtime → back to picker so user can retry",
        );
        assert_eq!(
            fx.emit_payloads().last(),
            Some(&(CcState::ShowingModePicker { first_run: false }, false)),
        );
    }

    // ---------- Path 4: Esc dismiss ----------

    #[test]
    fn path4_esc_dismiss_from_modepicker_hides_and_emits_closed() {
        let (state, fr) = fresh(false);
        *state.lock().unwrap() = CcState::ShowingModePicker { first_run: false };
        let fx = MockEffects::new();

        drive_engine(&state, &fr, &fx, CcInput::Dismiss);

        assert_eq!(*state.lock().unwrap(), CcState::Closed);
        assert_eq!(
            fx.calls(),
            vec![
                Call::HideWindow,
                Call::EmitState {
                    state: CcState::Closed,
                    first_run: false
                },
            ],
        );
    }

    #[test]
    fn path4b_first_dismiss_flips_seen_flag_and_persists() {
        let (state, fr) = fresh(true); // first-run = true
        *state.lock().unwrap() = CcState::ShowingModePicker { first_run: true };
        let fx = MockEffects::new();

        drive_engine(&state, &fr, &fx, CcInput::Dismiss);

        assert!(
            !fr.load(Ordering::Relaxed),
            "first_run atomic must flip to false"
        );
        let calls = fx.calls();
        assert!(
            calls.contains(&Call::PersistSeenFlag),
            "first-dismiss must persist the seen-v1 flag",
        );
        // Order matters: HideWindow → EmitState → PersistSeenFlag.
        let positions: Vec<_> = calls
            .iter()
            .enumerate()
            .filter_map(|(i, c)| match c {
                Call::HideWindow => Some(("hide", i)),
                Call::EmitState { .. } => Some(("emit", i)),
                Call::PersistSeenFlag => Some(("persist", i)),
                _ => None,
            })
            .collect();
        assert_eq!(
            positions.iter().map(|(n, _)| *n).collect::<Vec<_>>(),
            vec!["hide", "emit", "persist"],
        );
    }

    #[test]
    fn path4c_second_dismiss_does_not_re_persist() {
        let (state, fr) = fresh(false); // already-flipped first_run
        *state.lock().unwrap() = CcState::ShowingModePicker { first_run: false };
        let fx = MockEffects::new();

        drive_engine(&state, &fr, &fx, CcInput::Dismiss);

        assert!(
            !fx.calls().contains(&Call::PersistSeenFlag),
            "non-first-run dismiss must not write the settings row",
        );
    }

    #[test]
    fn path4d_dismiss_while_already_closed_is_pure_noop() {
        // Defensive: a stray dismiss after the window's hidden must
        // not re-hide, re-emit (or, well, MUST emit Closed once for
        // idempotency w/ React), and must not persist.
        let (state, fr) = fresh(false);
        let fx = MockEffects::new();

        drive_engine(&state, &fr, &fx, CcInput::Dismiss);

        let calls = fx.calls();
        assert!(!calls.contains(&Call::HideWindow), "no redundant hide");
        assert!(!calls.contains(&Call::PersistSeenFlag));
        // One emit (Closed) is OK — keeps React in sync.
        assert_eq!(fx.emit_payloads(), vec![(CcState::Closed, false)]);
    }

    // ---------- Path 5: outside-click dismiss (same FSM path as Esc) ----------
    //
    // Path 5 dispatches identically to Path 4 from the engine's POV
    // (both end up as `CcInput::Dismiss`); covered transitively. The
    // UI-side wiring lives in `ui/src/CommandCenter.tsx`.

    // ---------- Path 6: Stop button on the SessionCard ----------

    #[test]
    fn path6_stop_button_dispatches_stop_and_returns_to_picker() {
        let (state, fr) = fresh(false);
        *state.lock().unwrap() = CcState::ShowingSessionCard {
            kind: RecordingKind::Meeting,
        };
        let fx = MockEffects::new().with_session(Some(RecordingKind::Meeting));

        drive_engine(&state, &fr, &fx, CcInput::Stop);

        assert_eq!(
            *state.lock().unwrap(),
            CcState::ShowingModePicker { first_run: false }
        );
        let calls = fx.calls();
        assert!(
            calls.contains(&Call::DispatchStop(RecordingKind::Meeting)),
            "Stop must dispatch the runtime stop",
        );
        assert_eq!(
            fx.emit_payloads().last(),
            Some(&(CcState::ShowingModePicker { first_run: false }, false)),
        );
    }

    // ---------- Path 7: re-chord while session card is showing ----------

    #[test]
    fn path7_rechord_during_session_collapses_to_noop_on_card() {
        // SessionCard is up (e.g. user opened CC during an active
        // meeting). Another chord press: per the FSM, a re-entrant
        // Open is a no-op (window already up). Engine still emits
        // current state for UI safety.
        let (state, fr) = fresh(false);
        *state.lock().unwrap() = CcState::ShowingSessionCard {
            kind: RecordingKind::Meeting,
        };
        let fx = MockEffects::new().with_session(Some(RecordingKind::Meeting));

        drive_engine(&state, &fr, &fx, CcInput::Open);

        let calls = fx.calls();
        assert!(!calls.contains(&Call::ShowWindow), "no duplicate show");
        assert!(!calls.contains(&Call::HideWindow));
        assert_eq!(
            fx.emit_payloads(),
            vec![(
                CcState::ShowingSessionCard {
                    kind: RecordingKind::Meeting
                },
                false
            )],
        );
    }

    #[test]
    fn path7b_chord_open_while_session_live_lands_on_card_not_picker() {
        // Window was Closed; current_session = Some(Activity). Open
        // must land on SessionCard, NOT ModePicker (you can't start
        // a second session while one is live).
        let (state, fr) = fresh(false);
        let fx = MockEffects::new().with_session(Some(RecordingKind::Activity));

        drive_engine(&state, &fr, &fx, CcInput::Open);

        assert_eq!(
            *state.lock().unwrap(),
            CcState::ShowingSessionCard {
                kind: RecordingKind::Activity
            }
        );
        let calls = fx.calls();
        assert!(calls.contains(&Call::ShowWindow));
        assert_eq!(
            fx.emit_payloads().last(),
            Some(&(
                CcState::ShowingSessionCard {
                    kind: RecordingKind::Activity
                },
                false
            )),
        );
    }

    // ---------- mb-23rh original-symptom regression guard ----------

    #[test]
    fn regression_mb23rh_outer_emit_never_clobbers_inner_with_stale_launching() {
        // The exact scenario that burned the first hotfix iteration:
        // user clicks Activity tile, runtime succeeds, FSM goes
        // ModePicker → Launching{Activity} → (inner) → SessionCard{Activity}.
        // The pre-hotfix outer drive emitted captured `next` =
        // Launching{Activity} AFTER the inner had already emitted
        // SessionCard. UI snapped backwards → all tiles disabled
        // forever. Post-hotfix: outer re-snapshots and emits SessionCard.
        let (state, fr) = fresh(false);
        *state.lock().unwrap() = CcState::ShowingModePicker { first_run: false };
        let fx = MockEffects::new();

        drive_engine(
            &state,
            &fr,
            &fx,
            CcInput::PickMode {
                kind: RecordingKind::Activity,
            },
        );

        let emits = fx.emit_payloads();
        // Two emits expected: inner (post-RuntimeReplied) + outer
        // (post-effect snapshot). Both must be SessionCard. The bug
        // pre-hotfix would have the second emit be
        // Launching{Activity}; assert that's NOT the case.
        for (i, (st, _fr)) in emits.iter().enumerate() {
            assert!(
                !matches!(st, CcState::Launching { .. }),
                "emit #{i} must NEVER be a Launching state — that's the mb-23rh clobber",
            );
        }
        // The final emit specifically must be SessionCard{Activity}.
        assert_eq!(
            emits.last(),
            Some(&(
                CcState::ShowingSessionCard {
                    kind: RecordingKind::Activity
                },
                false
            )),
        );
    }

    // ---------- mb-a0f3 / mb-q2if regression guard ----------

    #[test]
    fn regression_mb_a0f3_chord_open_emits_visible_state_synchronously() {
        // The empty-black-box symptom was: window shows, but the
        // React UI never sees a state event (it landed in `Closed`
        // and rendered null). Root cause was the capabilities omission
        // in default.json; this test guards the ENGINE's contribution:
        // a chord-press Open MUST emit a visible state (not Closed)
        // in the same call. If a future refactor drops the emit, we
        // catch it here.
        let (state, fr) = fresh(false);
        let fx = MockEffects::new();

        drive_engine(&state, &fr, &fx, CcInput::Open);

        let emits = fx.emit_payloads();
        assert!(!emits.is_empty(), "chord-press Open must emit");
        let (final_state, _) = emits.last().unwrap();
        assert!(
            final_state.is_visible(),
            "chord-press Open MUST end with a visible-state emit, got {final_state:?}",
        );
    }

    #[test]
    fn regression_mb_q2if_esc_from_visible_window_hides_and_emits_closed() {
        // The "won't dismiss" symptom: Esc / outside-click yielded
        // no hide. Engine-side guarantee: any Dismiss while the
        // FSM is in a visible state MUST produce a HideWindow call.
        for start_state in [
            CcState::ShowingModePicker { first_run: false },
            CcState::ShowingModePicker { first_run: true },
            CcState::ShowingSessionCard {
                kind: RecordingKind::Meeting,
            },
            CcState::ShowingSessionCard {
                kind: RecordingKind::Activity,
            },
            CcState::Launching {
                kind: RecordingKind::Meeting,
            },
        ] {
            let (state, fr) = fresh(false);
            *state.lock().unwrap() = start_state;
            let fx = MockEffects::new();

            drive_engine(&state, &fr, &fx, CcInput::Dismiss);

            assert_eq!(
                *state.lock().unwrap(),
                CcState::Closed,
                "Dismiss from {start_state:?} must land on Closed",
            );
            assert!(
                fx.calls().contains(&Call::HideWindow),
                "Dismiss from {start_state:?} must call HideWindow",
            );
        }
    }

    // ---------- Bonus: SessionEnded mid-session-card returns to picker ----------

    #[test]
    fn session_ended_on_card_returns_to_modepicker() {
        // Timer expires / mic unplugged / Right-Alt released — the
        // runtime emits its own end event which the orchestrator
        // forwards as SessionEnded. UI flips back to picker.
        let (state, fr) = fresh(false);
        *state.lock().unwrap() = CcState::ShowingSessionCard {
            kind: RecordingKind::Dictation,
        };
        let fx = MockEffects::new();

        drive_engine(
            &state,
            &fr,
            &fx,
            CcInput::SessionEnded {
                kind: RecordingKind::Dictation,
            },
        );

        assert_eq!(
            *state.lock().unwrap(),
            CcState::ShowingModePicker { first_run: false }
        );
        assert_eq!(
            fx.emit_payloads().last(),
            Some(&(CcState::ShowingModePicker { first_run: false }, false)),
        );
    }
}
