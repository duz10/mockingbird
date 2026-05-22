//! Activity-capture lifecycle: pure-Rust FSM.
//!
//! ADR 0036 §Decision-item 3: a session is `Idle → Active → Paused?
//! → Stopped`. This module owns ONLY the legal-transition matrix.
//! No DB, no Tauri, no Win32 — those live in [`super::runtime`] +
//! [`super::persist`].
//!
//! ## Why a state machine
//!
//! The orchestrator in Wave 2+ will receive concurrent inputs from:
//!
//! - User clicks (`Start`/`Pause`/`Resume`/`Stop` via Command Center)
//! - Sampler thread reporting `LayerError`
//! - Process shutdown hook reporting `ShutdownRequested`
//! - Tray menu interactions (Wave 5+)
//!
//! Centralizing the legal moves here means the orchestrator (the IO
//! boundary) becomes a thin shell over [`apply`] — exactly mirroring
//! the Command Center pattern in `command_center/state.rs`, which
//! works well and tests fast via the throwaway-crate recipe (LESSONS
//! 2026-05-17).
//!
//! ## Invariants the FSM upholds
//!
//! 1. **One active session at a time.** `Start` from a non-`Idle`
//!    state is a no-op (idempotent — clicking "Activity" twice in the
//!    Command Center mode picker must not spawn two sessions).
//! 2. **Pause is only legal from Active.** From `Paused`, `Pause` is
//!    idempotent. From `Idle`/`Stopped`, `Pause` is a no-op.
//! 3. **Stop is always legal from any non-terminal state.** Stop from
//!    `Stopped` is a no-op (idempotent). Stop produces a single
//!    `CloseSession` effect; the orchestrator's persist call is
//!    write-once.
//! 4. **`ShutdownRequested` is equivalent to `Stop`** semantically —
//!    we just want the persist layer to mark the row with the
//!    `crashed_recovered`-eligible path if needed. We surface a
//!    distinct effect variant so the orchestrator can tag the
//!    persist row appropriately without an extra in-band flag.
//!
//! ## What is NOT in scope
//!
//! - The IDLE detection ladder (per ADR 0036 §Decision-item 4) is a
//!   sampler-thread concern. The sampler emits `idle_start` /
//!   `idle_end` *events* into `activity_events`; those are data
//!   rows, not lifecycle transitions. The session remains `Active`
//!   while the user is idle (per ADR — idle is annotated, not
//!   reflexively pausing).
//! - Exclusion list checks (sensitive apps). The sampler suppresses
//!   captures for excluded windows; the FSM doesn't care.
//! - Block segmentation. That's a Wave-3+ concern (`segmenter.rs`),
//!   not lifecycle.

use std::fmt;

/// Lifecycle state of an activity-capture session.
///
/// `Idle` and `Stopped` are both terminal-ish — the distinction
/// matters only to the persistence layer (Stopped means a real row
/// was written; Idle means we never started). The orchestrator
/// flips from `Stopped` back to `Idle` once it has cleared its
/// in-memory session-id; the FSM treats them as the same input
/// universe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LifecycleState {
    /// No live session. Initial state.
    Idle,
    /// Session running; sampler is emitting events.
    Active,
    /// Session running but suppressed; sampler events are dropped
    /// (the runtime gates the sampler-thread sink on this state).
    Paused,
    /// Session has been closed. Terminal until the orchestrator
    /// resets to `Idle` (one-shot transition handled outside the
    /// FSM — same way `command_center/state.rs` handles its
    /// Closed → ShowingModePicker reset).
    Stopped,
}

impl fmt::Display for LifecycleState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Idle => write!(f, "idle"),
            Self::Active => write!(f, "active"),
            Self::Paused => write!(f, "paused"),
            Self::Stopped => write!(f, "stopped"),
        }
    }
}

/// Inputs the FSM reacts to. Each is a user-visible or
/// system-visible event the orchestrator translates into a
/// transition request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleInput {
    /// User clicked the "Activity" tile in the Command Center.
    Start,
    /// User clicked Pause in the SessionCard.
    Pause,
    /// User clicked Resume in the SessionCard.
    Resume,
    /// User clicked Stop in the SessionCard. Closes the session.
    Stop,
    /// Process shutdown hook fired. Semantically same as `Stop` but
    /// the orchestrator routes the persist call differently
    /// (marks the row as a graceful shutdown rather than a user
    /// stop). The FSM doesn't care about that detail; we just
    /// emit a distinct `Effect` so the orchestrator can branch.
    ShutdownRequested,
}

/// Side effects the orchestrator must perform after a transition.
///
/// Effects are emitted as data (`#[must_use]` on the parent
/// [`Transition`]) — the FSM never performs them directly. This is
/// what makes the FSM pure and trivially unit-testable from a
/// throwaway crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleEffect {
    /// No-op. Either the input wasn't valid from the current state,
    /// or it was idempotent.
    None,
    /// Insert a new row into `activity_sessions` with
    /// `status='in_progress'` and spawn the sampler.
    OpenSession,
    /// Insert a `paused` event into `activity_events`. The runtime
    /// then gates the sampler sink.
    EmitPausedEvent,
    /// Insert a `resumed` event; un-gate the sampler.
    EmitResumedEvent,
    /// Update the session row: `status='completed'`, set `ended_at`.
    /// Triggered by user `Stop`.
    CloseSession,
    /// Same as `CloseSession` but the orchestrator persists with
    /// `status='partial'` because we never received a clean user
    /// stop (the process is exiting). Wave 1B writes the same row
    /// shape; the distinction shows up in
    /// `activity_sessions.status` for Wave 2's crash-recovery
    /// pass.
    CloseSessionForShutdown,
}

/// One state-machine step.
///
/// Construct with `Transition { next, effect }` — both fields are
/// public so the orchestrator can match on them. The struct is
/// `#[must_use]` to make sure callers don't drop the effect on the
/// floor.
#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Transition {
    /// The state to transition into.
    pub next: LifecycleState,
    /// The side effect to perform, if any.
    pub effect: LifecycleEffect,
}

/// Apply one input to one state. Pure. No allocation. No IO.
///
/// Returns `Transition { next, effect }`. Invalid inputs from a
/// given state produce `Transition { next: <unchanged>, effect:
/// LifecycleEffect::None }` — the FSM is total over the input
/// universe, mirroring the Command Center pattern.
pub const fn apply(state: LifecycleState, input: LifecycleInput) -> Transition {
    use LifecycleEffect as E;
    use LifecycleInput as I;
    use LifecycleState as S;
    match (state, input) {
        // Start from Idle: open a new session.
        (S::Idle, I::Start) => Transition {
            next: S::Active,
            effect: E::OpenSession,
        },
        // Active → Paused on user Pause.
        (S::Active, I::Pause) => Transition {
            next: S::Paused,
            effect: E::EmitPausedEvent,
        },
        // Paused → Active on user Resume.
        (S::Paused, I::Resume) => Transition {
            next: S::Active,
            effect: E::EmitResumedEvent,
        },
        // Stop from any live state closes the session.
        (S::Active | S::Paused, I::Stop) => Transition {
            next: S::Stopped,
            effect: E::CloseSession,
        },
        // Shutdown from any live state closes for shutdown.
        (S::Active | S::Paused, I::ShutdownRequested) => Transition {
            next: S::Stopped,
            effect: E::CloseSessionForShutdown,
        },
        // Anything else is a no-op. Idempotent inputs land here:
        //   Start from Active/Paused/Stopped
        //   Pause from Paused/Idle/Stopped
        //   Resume from Active/Idle/Stopped
        //   Stop/ShutdownRequested from Idle/Stopped
        _ => Transition {
            next: state,
            effect: E::None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Happy path -------------------------------------------------------

    #[test]
    fn idle_plus_start_opens_session() {
        let t = apply(LifecycleState::Idle, LifecycleInput::Start);
        assert_eq!(t.next, LifecycleState::Active);
        assert_eq!(t.effect, LifecycleEffect::OpenSession);
    }

    #[test]
    fn active_plus_pause_emits_paused_event() {
        let t = apply(LifecycleState::Active, LifecycleInput::Pause);
        assert_eq!(t.next, LifecycleState::Paused);
        assert_eq!(t.effect, LifecycleEffect::EmitPausedEvent);
    }

    #[test]
    fn paused_plus_resume_emits_resumed_event() {
        let t = apply(LifecycleState::Paused, LifecycleInput::Resume);
        assert_eq!(t.next, LifecycleState::Active);
        assert_eq!(t.effect, LifecycleEffect::EmitResumedEvent);
    }

    #[test]
    fn active_plus_stop_closes_session() {
        let t = apply(LifecycleState::Active, LifecycleInput::Stop);
        assert_eq!(t.next, LifecycleState::Stopped);
        assert_eq!(t.effect, LifecycleEffect::CloseSession);
    }

    #[test]
    fn paused_plus_stop_closes_session() {
        let t = apply(LifecycleState::Paused, LifecycleInput::Stop);
        assert_eq!(t.next, LifecycleState::Stopped);
        assert_eq!(t.effect, LifecycleEffect::CloseSession);
    }

    // --- Shutdown semantics -----------------------------------------------

    #[test]
    fn active_plus_shutdown_closes_for_shutdown() {
        let t = apply(LifecycleState::Active, LifecycleInput::ShutdownRequested);
        assert_eq!(t.next, LifecycleState::Stopped);
        assert_eq!(t.effect, LifecycleEffect::CloseSessionForShutdown);
    }

    #[test]
    fn paused_plus_shutdown_closes_for_shutdown() {
        let t = apply(LifecycleState::Paused, LifecycleInput::ShutdownRequested);
        assert_eq!(t.next, LifecycleState::Stopped);
        assert_eq!(t.effect, LifecycleEffect::CloseSessionForShutdown);
    }

    // --- Idempotence of Start ---------------------------------------------

    #[test]
    fn start_from_active_is_noop() {
        let t = apply(LifecycleState::Active, LifecycleInput::Start);
        assert_eq!(t.next, LifecycleState::Active);
        assert_eq!(t.effect, LifecycleEffect::None);
    }

    #[test]
    fn start_from_paused_is_noop() {
        let t = apply(LifecycleState::Paused, LifecycleInput::Start);
        assert_eq!(t.next, LifecycleState::Paused);
        assert_eq!(t.effect, LifecycleEffect::None);
    }

    #[test]
    fn start_from_stopped_is_noop() {
        // The orchestrator must reset Stopped → Idle before another
        // Start can take effect. This is deliberate: it forces the
        // orchestrator to clear its in-memory session-id (so a stale
        // pointer can't accidentally clobber a fresh row).
        let t = apply(LifecycleState::Stopped, LifecycleInput::Start);
        assert_eq!(t.next, LifecycleState::Stopped);
        assert_eq!(t.effect, LifecycleEffect::None);
    }

    // --- Idempotence of Pause ---------------------------------------------

    #[test]
    fn pause_from_paused_is_noop() {
        let t = apply(LifecycleState::Paused, LifecycleInput::Pause);
        assert_eq!(t.next, LifecycleState::Paused);
        assert_eq!(t.effect, LifecycleEffect::None);
    }

    #[test]
    fn pause_from_idle_is_noop() {
        let t = apply(LifecycleState::Idle, LifecycleInput::Pause);
        assert_eq!(t.next, LifecycleState::Idle);
        assert_eq!(t.effect, LifecycleEffect::None);
    }

    #[test]
    fn pause_from_stopped_is_noop() {
        let t = apply(LifecycleState::Stopped, LifecycleInput::Pause);
        assert_eq!(t.next, LifecycleState::Stopped);
        assert_eq!(t.effect, LifecycleEffect::None);
    }

    // --- Idempotence of Resume --------------------------------------------

    #[test]
    fn resume_from_active_is_noop() {
        let t = apply(LifecycleState::Active, LifecycleInput::Resume);
        assert_eq!(t.next, LifecycleState::Active);
        assert_eq!(t.effect, LifecycleEffect::None);
    }

    #[test]
    fn resume_from_idle_is_noop() {
        let t = apply(LifecycleState::Idle, LifecycleInput::Resume);
        assert_eq!(t.next, LifecycleState::Idle);
        assert_eq!(t.effect, LifecycleEffect::None);
    }

    #[test]
    fn resume_from_stopped_is_noop() {
        let t = apply(LifecycleState::Stopped, LifecycleInput::Resume);
        assert_eq!(t.next, LifecycleState::Stopped);
        assert_eq!(t.effect, LifecycleEffect::None);
    }

    // --- Idempotence of Stop ----------------------------------------------

    #[test]
    fn stop_from_idle_is_noop() {
        let t = apply(LifecycleState::Idle, LifecycleInput::Stop);
        assert_eq!(t.next, LifecycleState::Idle);
        assert_eq!(t.effect, LifecycleEffect::None);
    }

    #[test]
    fn stop_from_stopped_is_noop() {
        let t = apply(LifecycleState::Stopped, LifecycleInput::Stop);
        assert_eq!(t.next, LifecycleState::Stopped);
        assert_eq!(t.effect, LifecycleEffect::None);
    }

    // --- Shutdown from terminal states ------------------------------------

    #[test]
    fn shutdown_from_idle_is_noop() {
        let t = apply(LifecycleState::Idle, LifecycleInput::ShutdownRequested);
        assert_eq!(t.next, LifecycleState::Idle);
        assert_eq!(t.effect, LifecycleEffect::None);
    }

    #[test]
    fn shutdown_from_stopped_is_noop() {
        let t = apply(LifecycleState::Stopped, LifecycleInput::ShutdownRequested);
        assert_eq!(t.next, LifecycleState::Stopped);
        assert_eq!(t.effect, LifecycleEffect::None);
    }

    // --- Display + Debug round-trip ---------------------------------------

    #[test]
    fn state_display_strings() {
        assert_eq!(format!("{}", LifecycleState::Idle), "idle");
        assert_eq!(format!("{}", LifecycleState::Active), "active");
        assert_eq!(format!("{}", LifecycleState::Paused), "paused");
        assert_eq!(format!("{}", LifecycleState::Stopped), "stopped");
    }

    // --- Full happy-path walk ---------------------------------------------

    /// Walks the canonical lifecycle once, asserting both the state
    /// sequence and the effect sequence. This is the closest thing to
    /// a property test in the suite — if any edge of the FSM gets
    /// rewired silently, this scenario will surface it.
    #[test]
    fn canonical_lifecycle_walk() {
        let mut s = LifecycleState::Idle;
        let mut log: Vec<(LifecycleState, LifecycleEffect)> = vec![];

        for input in [
            LifecycleInput::Start,
            LifecycleInput::Pause,
            LifecycleInput::Resume,
            LifecycleInput::Pause,
            LifecycleInput::Resume,
            LifecycleInput::Stop,
        ] {
            let t = apply(s, input);
            s = t.next;
            log.push((t.next, t.effect));
        }

        assert_eq!(
            log,
            vec![
                (LifecycleState::Active, LifecycleEffect::OpenSession),
                (LifecycleState::Paused, LifecycleEffect::EmitPausedEvent),
                (LifecycleState::Active, LifecycleEffect::EmitResumedEvent),
                (LifecycleState::Paused, LifecycleEffect::EmitPausedEvent),
                (LifecycleState::Active, LifecycleEffect::EmitResumedEvent),
                (LifecycleState::Stopped, LifecycleEffect::CloseSession),
            ]
        );
    }

    /// Shutdown variant — same path but ending in a shutdown rather
    /// than a user Stop.
    #[test]
    fn shutdown_lifecycle_walk() {
        let mut s = LifecycleState::Idle;
        let log: Vec<(LifecycleState, LifecycleEffect)> = [
            LifecycleInput::Start,
            LifecycleInput::Pause,
            LifecycleInput::ShutdownRequested,
        ]
        .into_iter()
        .map(|i| {
            let t = apply(s, i);
            s = t.next;
            (t.next, t.effect)
        })
        .collect();

        assert_eq!(
            log,
            vec![
                (LifecycleState::Active, LifecycleEffect::OpenSession),
                (LifecycleState::Paused, LifecycleEffect::EmitPausedEvent),
                (
                    LifecycleState::Stopped,
                    LifecycleEffect::CloseSessionForShutdown
                ),
            ]
        );
    }
}
