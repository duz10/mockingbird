//! `mac_secure_input_smoke` — judge shim for `mac-p3c-secure-input-aborts`
//! (mb-mac-v1.4.3).
//!
//! Proves that a focused secure text field (`AXSecureTextField`) — or a
//! system-wide `IsSecureEventInputEnabled()` — makes the macOS secure
//! guard report secure and makes the guarded inject path ABORT without
//! pasting, while a normal field lets injection proceed. The AX query
//! sits behind a mockable seam, so no real Accessibility grant is
//! needed. Thin wrapper over
//! [`mockingbird_lib::injection::judges_macos_v1::secure_input_aborts_probe`].
//!
//! Run (via the Mac wrapper):
//!   scripts/dev/cargo-mac.sh run --example mac_secure_input_smoke
//!
//! Exit codes: 0 = abort/proceed logic correct · 1 = failure · 2 = wrong OS.

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("mac_secure_input_smoke is macOS-only (Phase 3 secure-input probe)");
    std::process::exit(2);
}

#[cfg(target_os = "macos")]
fn main() {
    use mockingbird_lib::injection::judges_macos_v1::secure_input_aborts_probe;

    println!("=== mac_secure_input_smoke (mac-p3c-secure-input-aborts) ===");

    let code = match secure_input_aborts_probe() {
        Ok(report) => {
            println!(
                "PASS: secure field ({}) aborts injection (no paste); \
                 system-wide secure input aborts; normal field proceeds.",
                report.secure_role
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
