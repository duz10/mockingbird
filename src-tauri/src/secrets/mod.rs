//! Secrets storage abstraction.
//!
//! Stores user-supplied API keys (Anthropic + Unsplash) per platform:
//! Windows via DPAPI ([`windows::WinDpapiSecretStore`]); macOS via the
//! login Keychain ([`macos::KeychainSecretStore`], ADR-0056). Other
//! targets fall back to the in-memory [`stub::NullSecretStore`] for
//! tests + CI + Linux dev builds.
//!
//! ## Why a trait
//!
//! Each OS backend plugs in via the same trait. The Tauri IPC layer
//! holds an [`Arc<dyn SecretStore>`] ([`SecretStoreHandle`]) so command
//! handlers never directly touch the OS surface — keeps the UI testable
//! + the platform seam clean.
//!
//! ## What's NOT here
//!
//! - The Anthropic API key wire-format. That's `cleanup::ClaudeProvider`'s
//!   business; the secret store just round-trips opaque bytes.
//! - The Unsplash Client-ID wire-format. That's the JS-side fetch
//!   wrapper's business; same opaque-bytes contract.
//! - User credentials of any kind. Mockingbird has no accounts.
//! - The Whisper / Ollama model files. Those are content-addressable
//!   downloads, not secrets.

use std::sync::Arc;

use crate::error::AppResult;

/// Tag a stored secret. Distinct tags so a future Phase-7 OAuth flow
/// can store its tokens without clashing with existing keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SecretKind {
    /// User-supplied Anthropic API key (Phase 4).
    ClaudeApiKey,
    /// User-supplied Unsplash access key (a.k.a. Client-ID), used by
    /// the photo background feature. LR.0.B (ADR 0055) moved this
    /// off localStorage onto DPAPI.
    UnsplashApiKey,
    /// Reserved for Phase 7+ — future cloud-provider keys.
    #[allow(dead_code)]
    Reserved2,
}

impl SecretKind {
    /// Canonical lowercase id used in the on-disk filename. Tests +
    /// the cross-platform stub key the in-memory map off this.
    pub fn id(self) -> &'static str {
        match self {
            SecretKind::ClaudeApiKey => "claude_api_key",
            SecretKind::UnsplashApiKey => "unsplash_api_key",
            SecretKind::Reserved2 => "reserved2",
        }
    }
}

/// Cross-platform secret-store trait. Sync — secret access happens
/// off the dictation hot path.
pub trait SecretStore: Send + Sync {
    /// Write a secret. Overwrites any existing value. UTF-8 strings
    /// only by design — the only consumer (Claude) ships UTF-8 keys.
    fn put(&self, kind: SecretKind, plaintext: &str) -> AppResult<()>;

    /// Read a secret. `Ok(None)` if never written.
    fn get(&self, kind: SecretKind) -> AppResult<Option<String>>;

    /// Remove a secret. Idempotent (no-op if missing).
    fn delete(&self, kind: SecretKind) -> AppResult<()>;

    /// Backend identifier — surfaces in the Settings → Advanced UI so
    /// the user can see "stored via Windows DPAPI" / "stored via
    /// macOS Keychain" / "in-memory (test mode)".
    fn backend_name(&self) -> &'static str;
}

pub mod stub;

#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "macos")]
pub mod judges_macos_v1;

/// Tauri-managed handle for a [`SecretStore`]. `Arc<dyn …>` so the
/// concrete platform impl is hidden behind the trait + the same
/// handle can be cloned cheaply into any command extractor that
/// wants it.
///
/// Registered on `tauri::Builder::manage` PRE-`.setup()` per
/// LESSONS PINNED P16 (the AppState propagation pattern). Commands
/// extract via `tauri::State<'_, SecretStoreHandle>`.
pub type SecretStoreHandle = Arc<dyn SecretStore>;

/// Construct the platform-default secret store wrapped in the
/// shareable [`SecretStoreHandle`].
///
/// - Windows: DPAPI-backed (per-user encryption keys, no extra prompt).
/// - macOS: Keychain-backed ([`macos::KeychainSecretStore`], ADR-0056 /
///   mb-mac-v1.4.1) — generic-password items in the login Keychain.
/// - Other targets: fall back to the in-memory [`stub::NullSecretStore`]
///   so the binary still links + the trait seam stays exercised on
///   Linux CI / dev boxes.
///
/// Tests should construct [`stub::NullSecretStore`] directly rather
/// than going through this helper.
pub fn make_default_store() -> AppResult<SecretStoreHandle> {
    #[cfg(target_os = "windows")]
    {
        Ok(Arc::new(windows::WinDpapiSecretStore::new()?))
    }
    #[cfg(target_os = "macos")]
    {
        Ok(Arc::new(macos::KeychainSecretStore::new()?))
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        // Non-Windows/macOS dev builds get a non-persistent in-memory
        // store so the binary still links + the trait seam stays
        // exercised.
        Ok(Arc::new(stub::NullSecretStore::new()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_kinds_have_distinct_ids() {
        // Filename collisions on disk would silently overwrite
        // unrelated secrets, so this is load-bearing.
        let ids = [
            SecretKind::ClaudeApiKey.id(),
            SecretKind::UnsplashApiKey.id(),
            SecretKind::Reserved2.id(),
        ];
        let unique: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(unique.len(), ids.len(), "SecretKind ids must be unique");
    }

    #[test]
    fn unsplash_kind_id_is_canonical_snake_case() {
        // Pins the wire format — changing this would orphan any
        // already-stored DPAPI files on user disks post-upgrade.
        assert_eq!(SecretKind::UnsplashApiKey.id(), "unsplash_api_key");
    }
}
