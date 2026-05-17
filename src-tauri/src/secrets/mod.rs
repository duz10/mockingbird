//! Secrets storage abstraction.
//!
//! v1 stores one secret (the Anthropic API key) on Windows via DPAPI
//! ([`windows::WinDpapiSecretStore`]). Cross-platform stubs in
//! [`stub::NullSecretStore`] for tests + CI.
//!
//! ## Why a trait
//!
//! macOS Keychain (Phase 9) plugs in via the same trait. Settings UI
//! holds a `Box<dyn SecretStore>` so it never directly touches the
//! OS surface — keeps the UI testable.
//!
//! ## What's NOT here
//!
//! - The Anthropic API key wire-format. That's `cleanup::ClaudeProvider`'s
//!   business; the secret store just round-trips opaque bytes.
//! - User credentials of any kind. Mockingbird has no accounts.
//! - The Whisper / Ollama model files. Those are content-addressable
//!   downloads, not secrets.

use crate::error::AppResult;

/// Tag a stored secret. Distinct tags so a future Phase-7 OAuth flow
/// can store its tokens without clashing with the Claude key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SecretKind {
    /// User-supplied Anthropic API key (Phase 4).
    ClaudeApiKey,
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

/// Construct the platform-default secret store.
///
/// Windows: DPAPI-backed (per-user encryption keys, no extra prompt).
/// Other: panics in production (Phase 9 fills in macOS / Linux).
/// Tests should construct `NullSecretStore` directly.
pub fn make_default_store() -> AppResult<Box<dyn SecretStore>> {
    #[cfg(target_os = "windows")]
    {
        Ok(Box::new(windows::WinDpapiSecretStore::new()?))
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err(crate::error::AppError::Secrets(
            "secret store: Phase 9 will add non-Windows backends".into(),
        ))
    }
}
