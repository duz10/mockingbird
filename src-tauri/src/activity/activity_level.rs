//! Coarse user-activity / idle detection.
//!
//! Phase 10 Wave 2 layer. We track whether the user has interacted
//! with the machine (via mouse or keyboard — at the OS level only;
//! we **never** read keystroke content, only the system-wide
//! "last-input tick count") within the last [`DEFAULT_IDLE_MS`].
//! When the user transitions from active → idle the sampler emits
//! an `idle_start` event; the reverse emits `idle_end`.
//!
//! ## Cross-platform discipline (Principle 5)
//!
//! The pure FSM in [`IdleTracker`] is target-agnostic. The Win32
//! syscall (`GetLastInputInfo`) lives in [`read_last_input_age_ms`]
//! behind `#[cfg(target_os = "windows")]`. macOS / Linux receive
//! `Err` (Phase 9 will fill).
//!
//! ## Why this is NOT keystroke capture (Phase 10 invariant)
//!
//! `GetLastInputInfo` returns a **tick count** — the time since the
//! most recent keyboard or mouse event reached the OS message pump.
//! Nothing about *which key* or *which button*. This is the only
//! input-related API Phase 10's invariant judges (`ac-no-keystroke-
//! content.md`) allow outside the chord listener whitelist. The
//! judge greps for the exact symbol name and verifies it's used
//! only here.

#![allow(missing_docs)]

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

/// Default threshold: 60 s of no input -> "idle". Empirically this
/// matches the Win11 lock-on-idle defaults and is consistent with
/// Slack-style "Away" timers.
pub const DEFAULT_IDLE_MS: u64 = 60_000;

/// Coarse user-activity state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityLevel {
    Active,
    Idle,
}

/// Transition emitted by [`IdleTracker::tick`] when the state
/// changes. `ts_ms` is the wall-clock timestamp the sampler should
/// stamp on the event row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdleTransition {
    Started { ts_ms: i64 },
    Ended { ts_ms: i64 },
}

/// Pure-Rust FSM over idle state. The sampler calls [`tick`] each
/// poll, passing the OS's "last-input age" reading. Returns the
/// transition (if any) to be persisted as an event.
#[derive(Debug, Clone)]
pub struct IdleTracker {
    level: ActivityLevel,
    threshold_ms: u64,
}

impl IdleTracker {
    pub fn new(threshold_ms: u64) -> Self {
        Self {
            level: ActivityLevel::Active,
            threshold_ms,
        }
    }

    pub fn level(&self) -> ActivityLevel {
        self.level
    }

    pub fn threshold_ms(&self) -> u64 {
        self.threshold_ms
    }

    /// Process one tick. `last_input_age_ms` is how long ago (in ms)
    /// the OS last saw input. `now_ms` is the wall clock to stamp on
    /// any transition.
    pub fn tick(&mut self, last_input_age_ms: u64, now_ms: i64) -> Option<IdleTransition> {
        let next = if last_input_age_ms >= self.threshold_ms {
            ActivityLevel::Idle
        } else {
            ActivityLevel::Active
        };
        if next == self.level {
            return None;
        }
        self.level = next;
        Some(match next {
            ActivityLevel::Idle => IdleTransition::Started { ts_ms: now_ms },
            ActivityLevel::Active => IdleTransition::Ended { ts_ms: now_ms },
        })
    }
}

impl Default for IdleTracker {
    fn default() -> Self {
        Self::new(DEFAULT_IDLE_MS)
    }
}

// --------------------------------------------------------------------
// Windows last-input probe
// --------------------------------------------------------------------

/// Returns how long (in ms) since the OS last received user input.
/// Wraps `GetLastInputInfo` on Windows; returns `Err` on other
/// platforms (Phase 9 fills).
///
/// ## Edge cases
///
/// - `LASTINPUTINFO.dwTime` is a `GetTickCount` value (ms since
///   boot). On systems uptime exceeding ~49.7 days the tick rolls
///   over; we tolerate the wrap by returning 0 (treat the user as
///   just-interacted) instead of a multi-day age. Rare and benign
///   compared to falsely promoting to idle.
/// - `GetLastInputInfo` can fail on locked sessions (Winlogon's
///   secure desktop). We surface that as `Err`; the sampler treats
///   the tick as "no change", which is correct — we don't WANT to
///   emit idle_start on the lock screen anyway.
#[cfg(target_os = "windows")]
pub fn read_last_input_age_ms() -> AppResult<u64> {
    use windows::Win32::Foundation::BOOL;
    use windows::Win32::System::SystemInformation::GetTickCount;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

    let mut lii = LASTINPUTINFO {
        cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    let ok: BOOL = unsafe { GetLastInputInfo(&mut lii) };
    if !ok.as_bool() {
        return Err(AppError::ActivitySampler(
            "GetLastInputInfo returned false".into(),
        ));
    }
    let now = unsafe { GetTickCount() };
    Ok(saturating_age_ms(lii.dwTime, now))
}

#[cfg(not(target_os = "windows"))]
pub fn read_last_input_age_ms() -> AppResult<u64> {
    Err(AppError::ActivitySampler(
        "GetLastInputInfo unavailable on this platform (Phase 9)".into(),
    ))
}

/// Compute (now - last_input) in ms with wraparound safety.
///
/// Both inputs are `GetTickCount` values (u32, 49.7-day rollover).
/// If `now < last_input` we assume a wrap and return 0.
#[allow(dead_code)] // Used only in the Windows path; the test harness exercises it directly.
fn saturating_age_ms(last_input_tick: u32, now_tick: u32) -> u64 {
    now_tick.saturating_sub(last_input_tick) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracker_defaults_to_active() {
        let t = IdleTracker::default();
        assert_eq!(t.level(), ActivityLevel::Active);
        assert_eq!(t.threshold_ms(), DEFAULT_IDLE_MS);
    }

    #[test]
    fn active_to_idle_emits_started() {
        let mut t = IdleTracker::new(1_000);
        let r = t.tick(1_500, 100);
        assert_eq!(r, Some(IdleTransition::Started { ts_ms: 100 }));
        assert_eq!(t.level(), ActivityLevel::Idle);
    }

    #[test]
    fn idle_to_active_emits_ended() {
        let mut t = IdleTracker::new(1_000);
        t.tick(1_500, 100); // go idle
        let r = t.tick(0, 200);
        assert_eq!(r, Some(IdleTransition::Ended { ts_ms: 200 }));
        assert_eq!(t.level(), ActivityLevel::Active);
    }

    #[test]
    fn no_transition_on_same_state() {
        let mut t = IdleTracker::new(1_000);
        assert_eq!(t.tick(0, 1), None);
        assert_eq!(t.tick(500, 2), None);
        assert_eq!(t.tick(999, 3), None);
        // now flip
        assert!(t.tick(1_000, 4).is_some());
        // stay idle — no further transition
        assert_eq!(t.tick(5_000, 5), None);
        assert_eq!(t.tick(10_000, 6), None);
    }

    #[test]
    fn threshold_boundary_is_inclusive() {
        // age == threshold should count as idle (>=)
        let mut t = IdleTracker::new(1_000);
        assert_eq!(t.tick(1_000, 1), Some(IdleTransition::Started { ts_ms: 1 }));
    }

    #[test]
    fn rapid_flip_emits_both_transitions() {
        let mut t = IdleTracker::new(500);
        assert!(matches!(
            t.tick(600, 1),
            Some(IdleTransition::Started { .. })
        ));
        assert!(matches!(t.tick(0, 2), Some(IdleTransition::Ended { .. })));
        assert!(matches!(
            t.tick(600, 3),
            Some(IdleTransition::Started { .. })
        ));
    }

    #[test]
    fn saturating_age_handles_normal_case() {
        assert_eq!(saturating_age_ms(100, 1_100), 1_000);
    }

    #[test]
    fn saturating_age_handles_wraparound_as_zero() {
        // now < last_input → assume wrap, treat as just-interacted
        assert_eq!(saturating_age_ms(u32::MAX - 100, 50), 0);
    }

    #[test]
    fn saturating_age_handles_zero_diff() {
        assert_eq!(saturating_age_ms(1_000, 1_000), 0);
    }

    #[test]
    fn custom_threshold_takes_effect() {
        let mut t = IdleTracker::new(5_000);
        assert_eq!(t.tick(4_000, 1), None);
        assert!(t.tick(5_001, 2).is_some());
    }

    #[test]
    fn activity_level_serializes_to_snake_case() {
        let j = serde_json::to_string(&ActivityLevel::Idle).unwrap();
        assert_eq!(j, "\"idle\"");
        let j2 = serde_json::to_string(&ActivityLevel::Active).unwrap();
        assert_eq!(j2, "\"active\"");
    }
}
