//! macOS Keychain roundtrip judge (`mac-p3a-keychain-roundtrip` /
//! mb-mac-v1.4.1).
//!
//! Proves [`super::macos::KeychainSecretStore`] honours the
//! [`SecretStore`] contract against the **real** macOS Keychain:
//! set -> get (byte-equal) -> delete -> get-after-delete is the
//! not-found case. Mirrors the in-tree probe pattern
//! ([`crate::audio::judges_macos_v1`], [`crate::stt::judges_macos_v1`]).
//!
//! Uses a TEST-SCOPED service so it never touches the app's real items,
//! and best-effort cleans up its probe item even on failure. macOS-only;
//! compiles to nothing elsewhere.

#![cfg(target_os = "macos")]

use super::macos::KeychainSecretStore;
use super::{SecretKind, SecretStore};

/// Test-scoped Keychain service — isolated from the real app's
/// `com.dustin.mockingbird` items so the probe is non-destructive.
const PROBE_SERVICE: &str = "com.dustin.mockingbird.keychain-probe";

/// Outcome of the Keychain roundtrip probe.
#[derive(Debug, Clone)]
pub struct KeychainProbeReport {
    /// The test-scoped service string the probe operated against.
    pub service: String,
    /// The backend name the store advertised (sanity check it routed to
    /// the Keychain impl, not the null stub).
    pub backend: String,
}

/// Run the full set/get/delete roundtrip against the real Keychain.
///
/// Returns `Ok` iff every step behaved per contract; `Err(String)`
/// carries a human-readable failure reason. The probe item is deleted
/// best-effort regardless of outcome.
pub fn keychain_roundtrip_probe() -> Result<KeychainProbeReport, String> {
    let store = KeychainSecretStore::with_service(PROBE_SERVICE);
    let kind = SecretKind::ClaudeApiKey;
    // Non-ASCII to also exercise the UTF-8 round-trip path.
    let secret = "mockingbird-keychain-probe-secret-\u{1f510}";

    // Pre-clean any leftover from an earlier aborted run so a stale item
    // can't mask a broken `put`.
    let _ = store.delete(kind);

    let result = run_roundtrip(&store, kind, secret);

    // Best-effort cleanup even on failure.
    let _ = store.delete(kind);

    result.map(|()| KeychainProbeReport {
        service: PROBE_SERVICE.to_string(),
        backend: store.backend_name().to_string(),
    })
}

fn run_roundtrip(
    store: &KeychainSecretStore,
    kind: SecretKind,
    secret: &str,
) -> Result<(), String> {
    // set
    store
        .put(kind, secret)
        .map_err(|e| format!("put failed (Keychain denied / access prompt dismissed?): {e}"))?;

    // get -> assert byte-equality
    let got = store
        .get(kind)
        .map_err(|e| format!("get failed: {e}"))?
        .ok_or_else(|| "get returned None immediately after put".to_string())?;
    if got.as_bytes() != secret.as_bytes() {
        return Err(format!(
            "byte mismatch: stored {} bytes, read back {} bytes",
            secret.len(),
            got.len()
        ));
    }

    // delete
    store
        .delete(kind)
        .map_err(|e| format!("delete failed: {e}"))?;

    // get-after-delete -> must be the not-found (None) case
    match store.get(kind) {
        Ok(None) => Ok(()),
        Ok(Some(_)) => Err("secret still present after delete".to_string()),
        Err(e) => Err(format!("get-after-delete failed: {e}")),
    }
}
