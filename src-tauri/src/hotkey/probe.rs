//! Hotkey conflict probe (ADR 0019).
//!
//! At startup the orchestrator must verify the configured hotkey
//! actually delivers events to our hook. If another app (Riot's
//! Vanguard, Logitech's keyboard daemon, Steam's overlay) is filtering
//! our chosen VK at a lower layer than `WH_KEYBOARD_LL`, we'll never
//! see it and the user will see a dead hotkey. The probe round-trips
//! a synthetic `SendInput` through the OS — if the event appears on
//! our channel within `PROBE_TIMEOUT`, the binding is live.
//!
//! ## Algorithm
//!
//! 1. Drain any pending events on the channel (don't confuse a stale
//!    event for the probe's response).
//! 2. Synthesise `keydown` + `keyup` for the candidate VK via
//!    `SendInput`. The `LLKHF_INJECTED` flag tags these as synthetic
//!    so production code can filter them later if needed.
//! 3. Wait up to [`PROBE_TIMEOUT`] for our channel to receive a
//!    matching `HotkeyEvent::KeyDown { vk: candidate, .. }`.
//! 4. On hit → candidate works; return it.
//!    On miss → continue to the next candidate.
//!
//! Candidate order per ADR 0019: configured VK first, then `VK_F23`
//! (0x86), `VK_F24` (0x87), and finally `Ctrl+Shift+Space`
//! (`VK_SPACE` = 0x20 with modifier handling). If none round-trip,
//! [`probe`] returns [`AppError::Hotkey`].
//!
//! ## Why this lives in its own module
//!
//! The pure decision logic ([`probe_with`]) is testable without any
//! `SendInput` machinery — a unit test passes a closure that simulates
//! "does this VK work?". The OS-side wrapper ([`probe`]) is the thin
//! adapter that wires the closure to real `SendInput` calls and the
//! channel receiver.

use std::sync::mpsc::Receiver;
use std::time::Duration;

use super::HotkeyEvent;
use crate::error::{AppError, AppResult};

/// VK_F23.
pub const VK_F23: u32 = 0x86;
/// VK_F24.
pub const VK_F24: u32 = 0x87;
/// VK_SPACE — used with Ctrl+Shift modifiers as the final fallback.
pub const VK_SPACE: u32 = 0x20;

/// VK_M — Phase MC default meeting main-key.
pub const VK_M: u32 = 0x4D;
/// VK_F13 — Phase MC first fallback main-key (per ADR 0019 §MC.1).
pub const VK_F13: u32 = 0x7C;
/// VK_F14 — Phase MC second fallback main-key.
pub const VK_F14: u32 = 0x7D;

/// How long to wait for a synthetic event to round-trip through the
/// hook. 50 ms is comfortably above measured kernel hook latency
/// (~1–5 ms on a quiet system) without making startup feel sluggish
/// when every candidate fails.
pub const PROBE_TIMEOUT: Duration = Duration::from_millis(50);

/// Build the ordered candidate list per ADR 0019.
///
/// The configured VK comes first; the fallbacks follow in priority
/// order. We dedupe so a configured VK that matches a fallback (e.g.
/// the user already chose F23) doesn't get probed twice.
pub fn candidate_chain(configured_vk: u32) -> Vec<u32> {
    let mut chain = vec![configured_vk];
    for fb in [VK_F23, VK_F24, VK_SPACE] {
        if !chain.contains(&fb) {
            chain.push(fb);
        }
    }
    chain
}

/// Pure probe driver — exposed for testing.
///
/// `probe_fn(vk)` returns `Ok(true)` if `vk` round-trips,
/// `Ok(false)` if it doesn't, and `Err(_)` to abort (don't try
/// further candidates).
///
/// Returns the first VK for which `probe_fn` returned `Ok(true)`, or
/// [`AppError::Hotkey`] if no candidate works.
pub fn probe_with<F>(candidates: &[u32], mut probe_fn: F) -> AppResult<u32>
where
    F: FnMut(u32) -> AppResult<bool>,
{
    if candidates.is_empty() {
        return Err(AppError::Hotkey(
            "conflict probe given empty candidate list".into(),
        ));
    }
    for &vk in candidates {
        if probe_fn(vk)? {
            return Ok(vk);
        }
    }
    Err(AppError::Hotkey(format!(
        "no hotkey candidate survived the conflict probe; tried {} VKs ({})",
        candidates.len(),
        candidates
            .iter()
            .map(|vk| format!("{vk:#x}"))
            .collect::<Vec<_>>()
            .join(", ")
    )))
}

/// Live probe driver.
///
/// `events_rx` is the channel the installed [`super::HotkeyListener`]
/// is sending into. Each candidate is exercised by synthesising
/// `SendInput` and waiting for a matching event.
///
/// **Caller responsibilities:**
/// - The listener must be installed and emitting into `events_rx`.
/// - The listener must already be bound to the candidate's VK by the
///   time it's probed. Wave 3 binds the listener to `configured_vk`
///   on `install()`; if the probe needs to test a fallback, the
///   caller must `uninstall()` + reinstall with the fallback VK
///   between iterations. (This is why [`probe_with`] is the
///   primitive and `probe_live` is a convenience for the simple case
///   "probe just the configured VK".)
#[cfg(target_os = "windows")]
pub fn probe_live(events_rx: &Receiver<HotkeyEvent>, candidates: &[u32]) -> AppResult<u32> {
    probe_with(candidates, |vk| {
        // Drain stale events so we don't confuse them with our probe.
        while events_rx.try_recv().is_ok() {}

        send_synthetic_keystroke(vk)?;

        // Wait for either KeyDown OR KeyUp for the candidate — either
        // proves the round trip.
        let deadline = std::time::Instant::now() + PROBE_TIMEOUT;
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return Ok(false);
            }
            match events_rx.recv_timeout(remaining) {
                Ok(HotkeyEvent::KeyDown { vk: got, .. })
                | Ok(HotkeyEvent::KeyUp { vk: got, .. })
                    if got == vk =>
                {
                    return Ok(true);
                }
                Ok(_) => continue, // unrelated event — keep waiting
                Err(_) => return Ok(false),
            }
        }
    })
}

#[cfg(target_os = "windows")]
fn send_synthetic_keystroke(vk: u32) -> AppResult<()> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
    };

    let down = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY(vk as u16),
                wScan: 0,
                dwFlags: Default::default(),
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let up = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY(vk as u16),
                wScan: 0,
                dwFlags: KEYEVENTF_KEYUP,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };

    let inputs = [down, up];
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        return Err(AppError::Hotkey(format!(
            "SendInput delivered {sent}/{} events for VK {vk:#x}",
            inputs.len()
        )));
    }
    Ok(())
}

// =====================================================================
// Phase MC — meeting hotkey collision probe (Wave 3 §3.5)
// =====================================================================

/// Phase MC fallback chain for the meeting main-key. Configured VK
/// first, then VK_M, VK_F13, VK_F14 (deduped).
///
/// This is the meeting equivalent of [`candidate_chain`], with a
/// different fallback set per ADR 0019 §MC.1 (the meeting modifier
/// is right-handed Ctrl-shaped, so the main-key falls back to
/// F13/F14 rather than F23/F24 to avoid stomping on dictation's
/// fallback range).
pub fn meeting_candidate_chain(configured_main_vk: u32) -> Vec<u32> {
    let mut chain = vec![configured_main_vk];
    for fb in [VK_M, VK_F13, VK_F14] {
        if !chain.contains(&fb) {
            chain.push(fb);
        }
    }
    chain
}

/// Phase MC startup collision probe. Returns the first VK in the
/// meeting fallback chain that **does not equal** the configured
/// dictation main-key.
///
/// Unlike [`probe_with`], this is a pure value-comparison — there's
/// no `SendInput` round-trip. The OS-level conflict probe ([`probe_live`])
/// is for the dictation install; meeting startup just needs to verify
/// its chord doesn't share its main-key VK with dictation (the
/// independent second `WH_KEYBOARD_LL` hook handles its own delivery
/// guarantee via [`super::probe::probe_live`] in Wave 4 if the user
/// reports a dead meeting hotkey).
///
/// # Errors
///
/// `AppError::Hotkey` if every VK in the chain collides with
/// `dictation_main_vk`. With a 4-entry chain and a single dictation
/// VK that's structurally impossible — the error path is defensive.
pub fn probe_meeting_main_vk(configured_main_vk: u32, dictation_main_vk: u32) -> AppResult<u32> {
    let chain = meeting_candidate_chain(configured_main_vk);
    for &vk in &chain {
        if vk != dictation_main_vk {
            return Ok(vk);
        }
    }
    Err(AppError::Hotkey(format!(
        "all meeting hotkey candidates ({}) collide with dictation main VK {dictation_main_vk:#x}",
        chain
            .iter()
            .map(|vk| format!("{vk:#x}"))
            .collect::<Vec<_>>()
            .join(", ")
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_chain_starts_with_configured() {
        let chain = candidate_chain(0xA5);
        assert_eq!(chain[0], 0xA5);
    }

    #[test]
    fn candidate_chain_includes_all_fallbacks() {
        let chain = candidate_chain(0xA5);
        assert!(chain.contains(&VK_F23));
        assert!(chain.contains(&VK_F24));
        assert!(chain.contains(&VK_SPACE));
    }

    #[test]
    fn candidate_chain_dedupes_configured_match() {
        let chain = candidate_chain(VK_F23);
        // F23 should appear exactly once (as the configured slot, not
        // as a fallback).
        assert_eq!(
            chain.iter().filter(|&&vk| vk == VK_F23).count(),
            1,
            "configured-matches-fallback should dedupe: {chain:?}"
        );
    }

    #[test]
    fn candidate_chain_order_matches_adr_0019() {
        // Configured first, then F23, F24, SPACE.
        let chain = candidate_chain(0xA5);
        assert_eq!(chain, vec![0xA5, VK_F23, VK_F24, VK_SPACE]);
    }

    #[test]
    fn probe_with_picks_first_working_candidate() {
        let chain = vec![0xA5, VK_F23, VK_F24];
        let mut tried: Vec<u32> = Vec::new();
        let result = probe_with(&chain, |vk| {
            tried.push(vk);
            // Only F23 works.
            Ok(vk == VK_F23)
        });
        assert_eq!(result.unwrap(), VK_F23);
        assert_eq!(tried, vec![0xA5, VK_F23]);
    }

    #[test]
    fn probe_with_returns_first_candidate_if_it_works() {
        let chain = vec![0xA5, VK_F23, VK_F24];
        let mut tried = 0u32;
        let result = probe_with(&chain, |_vk| {
            tried += 1;
            Ok(true)
        });
        assert_eq!(result.unwrap(), 0xA5);
        assert_eq!(
            tried, 1,
            "should not try further candidates after first hit"
        );
    }

    #[test]
    fn probe_with_returns_hotkey_error_when_all_fail() {
        let chain = vec![0xA5, VK_F23, VK_F24, VK_SPACE];
        let result = probe_with(&chain, |_vk| Ok(false));
        match result {
            Err(AppError::Hotkey(msg)) => {
                assert!(
                    msg.contains("conflict probe") && msg.contains("0xa5"),
                    "error message should reference the probe and listed VKs: {msg}"
                );
            }
            other => panic!("expected Hotkey error, got {other:?}"),
        }
    }

    #[test]
    fn probe_with_propagates_inner_error() {
        let chain = vec![0xA5, VK_F23];
        let result = probe_with(&chain, |vk| {
            if vk == 0xA5 {
                Err(AppError::Hotkey("simulated SendInput failure".into()))
            } else {
                Ok(true)
            }
        });
        match result {
            Err(AppError::Hotkey(msg)) => assert!(msg.contains("simulated")),
            other => panic!("expected propagated error, got {other:?}"),
        }
    }

    // ---------- Phase MC meeting probe ----------

    #[test]
    fn probe_meeting_returns_configured_vk_when_no_collision() {
        // configured = VK_M, dictation = VK_F23 (no collision)
        let r = probe_meeting_main_vk(VK_M, VK_F23).unwrap();
        assert_eq!(r, VK_M);
    }

    #[test]
    fn probe_meeting_walks_chain_on_collision() {
        // configured = VK_M, dictation = VK_M → first survivor is VK_F13
        let r = probe_meeting_main_vk(VK_M, VK_M).unwrap();
        assert_eq!(r, VK_F13);
    }

    #[test]
    fn meeting_candidate_chain_dedupes_configured_match() {
        // configured = VK_F13 → chain is [VK_F13, VK_M, VK_F14]
        // (VK_F13 appears once, not as configured AND fallback).
        let chain = meeting_candidate_chain(VK_F13);
        assert_eq!(
            chain.iter().filter(|&&vk| vk == VK_F13).count(),
            1,
            "chain must dedupe: {chain:?}"
        );
        assert_eq!(chain, vec![VK_F13, VK_M, VK_F14]);
    }

    #[test]
    fn probe_with_rejects_empty_candidate_list() {
        let result = probe_with(&[], |_vk| Ok(true));
        assert!(matches!(result, Err(AppError::Hotkey(_))));
    }
}
