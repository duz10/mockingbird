//! macOS Keychain-backed [`SecretStore`].
//!
//! The Apple-Silicon parity of [`super::windows::WinDpapiSecretStore`].
//! Where Windows leans on DPAPI (per-user `CryptProtectData` blobs on
//! disk), macOS stores each secret as a **generic-password** item in
//! the user's login Keychain via the `security-framework` crate.
//!
//! See ADR-0056 for the WHY (crate choice, namespacing, unsigned-app
//! behaviour). Leaf: mb-mac-v1.4.1.
//!
//! ## Storage layout
//!
//! Each [`SecretKind`] maps to one generic-password item:
//!
//! ```text
//! service = "com.dustin.mockingbird"   (constant; namespaces the app)
//! account = SecretKind::id()           ("claude_api_key", ...)
//! data    = the UTF-8 secret bytes
//! ```
//!
//! Mirrors the Windows `<kind_id>.dpapi` per-kind filename namespacing:
//! distinct `account` per kind means writing one secret never clobbers
//! another, and `(service, account)` is the lookup key.
//!
//! ## Why not the `keyring` crate
//!
//! Same call as on Windows (see [`super::windows`]): `keyring` is a
//! thin cross-platform wrapper, but it pulls a heavier dependency tree
//! and abstracts away the very `(service, account)` namespacing we want
//! explicit control over. `security-framework` is the canonical, Apple
//! crate maintained by the Rust security WG, exposes exactly the
//! generic-password primitives we need, and is what `keyring` itself
//! delegates to on macOS. Go straight to the source.

#![cfg(target_os = "macos")]

use security_framework::passwords::{
    delete_generic_password, get_generic_password, set_generic_password,
};

use crate::error::{AppError, AppResult};

use super::{SecretKind, SecretStore};

/// Default Keychain `service` string — namespaces every Mockingbird
/// generic-password item so we never collide with other apps' items.
const DEFAULT_SERVICE: &str = "com.dustin.mockingbird";

/// `errSecItemNotFound` (`Security.framework`). Returned by the
/// Keychain when a `(service, account)` lookup/delete finds nothing.
/// Hard-coded to avoid pulling in `security-framework-sys` just for one
/// constant. See <https://developer.apple.com/documentation/security/errsecitemnotfound>.
const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;

/// macOS Keychain-backed secret store.
pub struct KeychainSecretStore {
    /// The Keychain `service` string all items are filed under.
    service: String,
}

impl KeychainSecretStore {
    /// Construct against the default app service (`com.dustin.mockingbird`).
    pub fn new() -> AppResult<Self> {
        Ok(Self {
            service: DEFAULT_SERVICE.to_string(),
        })
    }

    /// Construct against a caller-supplied service string.
    ///
    /// Used by the `mac-p3a-keychain-roundtrip` judge (and unit tests)
    /// to operate on a TEST-SCOPED service so probing never pollutes
    /// the real app's Keychain items.
    pub fn with_service(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }
}

impl SecretStore for KeychainSecretStore {
    fn put(&self, kind: SecretKind, plaintext: &str) -> AppResult<()> {
        // `set_generic_password` adds-or-updates: a second `put` for the
        // same (service, account) overwrites, matching the trait contract
        // and the Windows atomic-rename overwrite semantics.
        set_generic_password(&self.service, kind.id(), plaintext.as_bytes())
            .map_err(|e| AppError::Secrets(format!("Keychain put {}: {e}", kind.id())))
    }

    fn get(&self, kind: SecretKind) -> AppResult<Option<String>> {
        match get_generic_password(&self.service, kind.id()) {
            Ok(bytes) => {
                let s = String::from_utf8(bytes)
                    .map_err(|e| AppError::Secrets(format!("non-utf8 secret: {e}")))?;
                Ok(Some(s))
            }
            Err(e) if is_not_found(&e) => Ok(None),
            Err(e) => Err(AppError::Secrets(format!(
                "Keychain get {}: {e}",
                kind.id()
            ))),
        }
    }

    fn delete(&self, kind: SecretKind) -> AppResult<()> {
        // Idempotent: a missing item is success, mirroring the Windows
        // `NotFound` -> `Ok(())` branch.
        match delete_generic_password(&self.service, kind.id()) {
            Ok(()) => Ok(()),
            Err(e) if is_not_found(&e) => Ok(()),
            Err(e) => Err(AppError::Secrets(format!(
                "Keychain delete {}: {e}",
                kind.id()
            ))),
        }
    }

    fn backend_name(&self) -> &'static str {
        "macOS Keychain"
    }
}

/// True iff the `security-framework` error is `errSecItemNotFound`,
/// i.e. the `(service, account)` pair has no Keychain item. Everything
/// else (auth failure, malformed query, ...) is a real error.
fn is_not_found(err: &security_framework::base::Error) -> bool {
    err.code() == ERR_SEC_ITEM_NOT_FOUND
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each test uses a unique service so concurrent test runs + the
    /// real app never alias the same Keychain item. Best-effort cleaned
    /// up at the end of each test.
    fn probe_store(tag: &str) -> KeychainSecretStore {
        KeychainSecretStore::with_service(format!(
            "com.dustin.mockingbird.test.{tag}.{}",
            std::process::id()
        ))
    }

    #[test]
    fn put_then_get_round_trips_via_real_keychain() {
        let s = probe_store("roundtrip");
        let secret = "sk-ant-realistic-looking-key-xxxxxxxxxxxxxxxxxxxxxx";
        s.put(SecretKind::ClaudeApiKey, secret).unwrap();
        let got = s.get(SecretKind::ClaudeApiKey).unwrap();
        s.delete(SecretKind::ClaudeApiKey).ok();
        assert_eq!(got.as_deref(), Some(secret));
    }

    #[test]
    fn get_missing_returns_none() {
        let s = probe_store("missing");
        assert_eq!(s.get(SecretKind::ClaudeApiKey).unwrap(), None);
    }

    #[test]
    fn put_overwrites_existing() {
        let s = probe_store("overwrite");
        s.put(SecretKind::ClaudeApiKey, "v1").unwrap();
        s.put(SecretKind::ClaudeApiKey, "v2").unwrap();
        let got = s.get(SecretKind::ClaudeApiKey).unwrap();
        s.delete(SecretKind::ClaudeApiKey).ok();
        assert_eq!(got.as_deref(), Some("v2"));
    }

    #[test]
    fn delete_is_idempotent() {
        let s = probe_store("delete");
        s.delete(SecretKind::ClaudeApiKey).unwrap();
        s.put(SecretKind::ClaudeApiKey, "v").unwrap();
        s.delete(SecretKind::ClaudeApiKey).unwrap();
        s.delete(SecretKind::ClaudeApiKey).unwrap();
        assert_eq!(s.get(SecretKind::ClaudeApiKey).unwrap(), None);
    }

    #[test]
    fn unsplash_and_claude_keys_are_isolated() {
        let s = probe_store("isolation");
        s.put(SecretKind::ClaudeApiKey, "claude-v").unwrap();
        s.put(SecretKind::UnsplashApiKey, "unsplash-v").unwrap();
        let claude = s.get(SecretKind::ClaudeApiKey).unwrap();
        // Deleting one kind must NOT affect the other.
        s.delete(SecretKind::ClaudeApiKey).unwrap();
        let unsplash_after = s.get(SecretKind::UnsplashApiKey).unwrap();
        s.delete(SecretKind::UnsplashApiKey).ok();
        assert_eq!(claude.as_deref(), Some("claude-v"));
        assert_eq!(unsplash_after.as_deref(), Some("unsplash-v"));
    }

    #[test]
    fn backend_name_advertises_keychain() {
        let s = KeychainSecretStore::new().unwrap();
        assert!(s.backend_name().contains("Keychain"));
    }
}
