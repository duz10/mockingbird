//! `mac_frontmost_smoke` — judge shim for `mac-p3e-frontmost-app-readable`
//! (mb-mac-v1.4.5).
//!
//! Proves that the permission-free `NSWorkspace.frontmostApplication`
//! read returns a non-empty bundle identifier AND a non-empty localized
//! app name for whatever app is frontmost when the probe runs (the
//! test runner's own frontmost GUI app is fine), without panicking. The
//! window *title* is deliberately NOT asserted — it needs Accessibility
//! and is deferred (ADR 0060). Thin wrapper over
//! [`mockingbird_lib::window_context::judges_macos_v1::frontmost_app_readable_probe`].
//!
//! Run (via the Mac wrapper):
//!   scripts/dev/cargo-mac.sh run --example mac_frontmost_smoke
//!
//! Exit codes: 0 = bundle id + app name readable · 1 = failure · 2 = wrong OS.

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("mac_frontmost_smoke is macOS-only (Phase 3 window-context probe)");
    std::process::exit(2);
}

#[cfg(target_os = "macos")]
fn main() {
    use mockingbird_lib::window_context::judges_macos_v1::frontmost_app_readable_probe;

    println!("=== mac_frontmost_smoke (mac-p3e-frontmost-app-readable) ===");

    let code = match frontmost_app_readable_probe() {
        Ok(report) => {
            println!(
                "PASS: frontmost app readable — bundle_id={:?}, name={:?}, pid={}, path={:?}",
                report.bundle_id, report.localized_name, report.pid, report.bundle_path
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
