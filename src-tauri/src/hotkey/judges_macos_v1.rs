//! macOS global-hotkey roundtrip judge (`mac-p3d-hotkey-roundtrip` /
//! mb-mac-v1.4.4).
//!
//! ## What this probe DOES verify (deterministically)
//!
//! The translation boundary: that a synthesized Right-Option
//! press/release — fed through [`super::macos::classify_event`] (the
//! exact fn the live `CGEventTap` callback uses) — drives the shared
//! §6.1 [`HotkeyStateMachine`] through
//! `StartCapture → StopCapture`. This exercises the macOS keycode/
//! modifier → [`HotkeyEvent`] mapping and the hand-off into the
//! cross-platform state machine, end to end, with **no OS tap** and
//! **no Input Monitoring grant** required. It is therefore safe to run
//! in CI / unsigned.
//!
//! ## What this probe does NOT verify (and why)
//!
//! The OS-level event-tap capture itself — `CGEventTapCreate`
//! receiving real key events — requires the **Input Monitoring**
//! permission, which cannot be granted deterministically in CI. That
//! half is verified in the permission-gated end-to-end judge
//! (`mac-p3-dictation-e2e`), not here. Conflating the two would make
//! this probe non-deterministic (it would depend on a TCC grant), so
//! we deliberately keep the honest, reproducible boundary.
//!
//! Mirrors the in-tree probe pattern
//! ([`crate::secrets::judges_macos_v1`], etc.): a library fn returning
//! `Result<Report, String>`, driven by a thin `[[example]]` shim
//! (`mac_hotkey_smoke`).

#![cfg(target_os = "macos")]

use std::time::{Duration, Instant};

use super::macos::{classify_event, MacEventKind, KVK_RIGHT_OPTION};
use super::state::{HotkeyMode, HotkeyStateMachine, StateAction, StateConfig};
use super::HotkeyEvent;

/// Device-dependent Right-Option flag bit (IOKit `NX_DEVICERALTKEYMASK`).
/// Set in the synthetic "press" flagsChanged, cleared for "release".
const RALT_FLAG: u64 = 0x0000_0040;

/// Outcome of the hotkey roundtrip probe.
#[derive(Debug, Clone)]
pub struct HotkeyProbeReport {
    /// The configured macOS keycode the probe drove (Right Option).
    pub configured_vk: u32,
    /// The action emitted when the hold crossed the threshold.
    pub start_action: String,
    /// The action emitted on release.
    pub stop_action: String,
}

/// Run the synthesized press → hold → release roundtrip through the
/// real translation fn and the shared state machine.
///
/// Returns `Ok` iff the machine emitted `StartCapture(Normal)` then
/// `StopCapture`; `Err(String)` with a human-readable reason
/// otherwise. Fully deterministic — uses manufactured `Instant`s, not
/// wall-clock ticks.
pub fn hotkey_roundtrip_probe() -> Result<HotkeyProbeReport, String> {
    let configured = KVK_RIGHT_OPTION;
    let mut machine = HotkeyStateMachine::new(StateConfig::default());

    let t0 = Instant::now();

    // 1. Synthesize a Right-Option PRESS as a flagsChanged with the
    //    R-Option bit set, translated by the production classifier.
    let down = classify_event(
        MacEventKind::FlagsChanged,
        configured,
        RALT_FLAG,
        configured,
        t0,
    )
    .ok_or_else(|| "classify_event did not produce a KeyDown for R-Option press".to_string())?;
    if !matches!(down, HotkeyEvent::KeyDown { .. }) {
        return Err(format!(
            "expected KeyDown from press translation, got {down:?}"
        ));
    }
    if machine.handle(down) != StateAction::None {
        return Err("state machine should stay silent on initial KeyDown (PendingHold)".into());
    }

    // 2. Drive a Tick past the 80 ms hold threshold → StartCapture.
    let start = machine.handle(HotkeyEvent::Tick {
        at: t0 + Duration::from_millis(120),
    });
    if start != StateAction::StartCapture(HotkeyMode::Normal) {
        return Err(format!(
            "expected StartCapture(Normal) after crossing hold threshold, got {start:?}"
        ));
    }

    // 3. Synthesize the RELEASE as a flagsChanged with the R-Option bit
    //    cleared, translated by the production classifier → KeyUp.
    let up = classify_event(
        MacEventKind::FlagsChanged,
        configured,
        0,
        configured,
        t0 + Duration::from_millis(220),
    )
    .ok_or_else(|| "classify_event did not produce a KeyUp for R-Option release".to_string())?;
    if !matches!(up, HotkeyEvent::KeyUp { .. }) {
        return Err(format!(
            "expected KeyUp from release translation, got {up:?}"
        ));
    }
    let stop = machine.handle(up);
    if stop != StateAction::StopCapture {
        return Err(format!("expected StopCapture on release, got {stop:?}"));
    }

    Ok(HotkeyProbeReport {
        configured_vk: configured,
        start_action: format!("{start:?}"),
        stop_action: format!("{stop:?}"),
    })
}
