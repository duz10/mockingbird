//! `mac_hotkey_smoke` — judge shim for `mac-p3d-hotkey-roundtrip`
//! (mb-mac-v1.4.4).
//!
//! Proves the macOS CGEventTap translation layer drives the shared
//! §6.1 hotkey state machine through `StartCapture → StopCapture`:
//! a synthesized Right-Option press/release is fed through the *exact*
//! [`mockingbird_lib::hotkey::macos::classify_event`] the live tap
//! callback uses, then handed to the cross-platform
//! [`HotkeyStateMachine`]. Thin wrapper over
//! [`mockingbird_lib::hotkey::judges_macos_v1::hotkey_roundtrip_probe`].
//!
//! This probe is **deterministic** and needs no Input Monitoring
//! grant — it never opens a real event tap. The OS-level tap capture
//! itself (which DOES require Input Monitoring) is verified separately
//! in the permission-gated end-to-end judge `mac-p3-dictation-e2e`.
//!
//! Run (via the Mac wrapper):
//!   scripts/dev/cargo-mac.sh run --release --example mac_hotkey_smoke
//!
//! Exit codes: 0 = roundtrip passed · 1 = failure · 2 = wrong OS.

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("mac_hotkey_smoke is macOS-only (Phase 3 CGEventTap hotkey probe)");
    std::process::exit(2);
}

#[cfg(target_os = "macos")]
fn main() {
    use mockingbird_lib::hotkey::judges_macos_v1::hotkey_roundtrip_probe;

    println!("=== mac_hotkey_smoke (mac-p3d-hotkey-roundtrip) ===");

    let code = match hotkey_roundtrip_probe() {
        Ok(report) => {
            println!(
                "PASS: R-Option press→hold→release drove the state machine \
                 {} then {} (configured keycode: {:#x}).",
                report.start_action, report.stop_action, report.configured_vk
            );
            println!(
                "Note: OS-level event-tap capture (Input Monitoring) is verified \
                 in the permission-gated e2e (mac-p3-dictation-e2e), not here."
            );
            0
        }
        Err(e) => {
            eprintln!("FAIL: {e}");
            1
        }
    };
    std::process::exit(code);
}
