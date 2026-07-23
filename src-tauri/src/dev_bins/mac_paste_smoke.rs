//! `mac_paste_smoke` — judge shim for `mac-p3b-paste-clipboard-saverestore`
//! (mb-mac-v1.4.2).
//!
//! Proves the macOS clipboard save/restore dance: plant sentinel `X` →
//! run the save/restore flow (writes payload `Y`, no-op paste, restores
//! `X`) → assert the pasteboard is back to `X`. Thin wrapper over
//! [`mockingbird_lib::injection::judges_macos_v1::paste_clipboard_saverestore_probe`].
//!
//! The Cmd+V keypost itself is Accessibility-gated and verified in the
//! permission-gated `mac-p3-dictation-e2e`; this probe asserts only the
//! deterministic save/restore.
//!
//! Run (via the Mac wrapper):
//!   scripts/dev/cargo-mac.sh run --example mac_paste_smoke
//!
//! Exit codes: 0 = save/restore passed · 1 = failure · 2 = wrong OS.

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("mac_paste_smoke is macOS-only (Phase 3 paste save/restore probe)");
    std::process::exit(2);
}

#[cfg(target_os = "macos")]
fn main() {
    use mockingbird_lib::injection::judges_macos_v1::paste_clipboard_saverestore_probe;

    println!("=== mac_paste_smoke (mac-p3b-paste-clipboard-saverestore) ===");

    let code = match paste_clipboard_saverestore_probe() {
        Ok(report) => {
            println!(
                "PASS: clipboard save/restore round-tripped (outcome: {:?}, sentinel restored).",
                report.outcome
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
