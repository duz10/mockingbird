//! `mac_keychain_smoke` — judge shim for `mac-p3a-keychain-roundtrip`
//! (mb-mac-v1.4.1).
//!
//! Proves the macOS `KeychainSecretStore` round-trips a secret through
//! the real login Keychain: set -> get (byte-equal) -> delete ->
//! get-after-delete is the not-found case. Thin wrapper over
//! [`mockingbird_lib::secrets::judges_macos_v1::keychain_roundtrip_probe`].
//!
//! Uses a TEST-SCOPED service (`com.dustin.mockingbird.keychain-probe`)
//! so it never touches the app's real items, and cleans up after itself.
//!
//! Run (via the Mac wrapper):
//!   scripts/dev/cargo-mac.sh run --release --example mac_keychain_smoke
//!
//! Exit codes: 0 = roundtrip passed · 1 = failure · 2 = wrong OS.
//!
//! Unsigned-binary note: for a single-process roundtrip the creating
//! process is trusted for its own freshly-created item, so no Keychain
//! access prompt should appear. If one does (or access is denied), the
//! `put`/`get` returns an error and the probe prints FAIL with the
//! reason rather than hanging.

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("mac_keychain_smoke is macOS-only (Phase 3a Keychain probe)");
    std::process::exit(2);
}

#[cfg(target_os = "macos")]
fn main() {
    use mockingbird_lib::secrets::judges_macos_v1::keychain_roundtrip_probe;

    println!("=== mac_keychain_smoke (mac-p3a-keychain-roundtrip) ===");

    let code = match keychain_roundtrip_probe() {
        Ok(report) => {
            println!(
                "PASS: Keychain roundtrip set->get->delete->get-none OK \
                 (service: {}, backend: {}).",
                report.service, report.backend
            );
            println!("Prompt: none observed — roundtrip completed without blocking.");
            0
        }
        Err(e) => {
            eprintln!("FAIL: {e}");
            eprintln!(
                "Prompt: if a Keychain access dialog appeared, the failure \
                 above is the dismissed/denied result (not a hang)."
            );
            1
        }
    };
    std::process::exit(code);
}
