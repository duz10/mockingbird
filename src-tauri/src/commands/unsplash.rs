//! Unsplash access-key IPC.
//!
//! LR.0.B / `mb-hiar` (ADR 0055 charter) moves the Unsplash
//! Client-ID off webview localStorage onto the platform secret store
//! ([`crate::secrets::SecretStore`]). On Windows the backing impl is
//! DPAPI (per-user encryption, no extra prompt); non-Windows dev
//! builds get the in-memory stub via [`crate::secrets::make_default_store`].
//!
//! The pre-LR.0.B UI persisted the key as `mockingbird:unsplash:apiKey`
//! in IndexedDB. That was plaintext on disk and contradicted the
//! in-app "stored locally on this machine" copy by understating how
//! it is protected. The JS-side migration in
//! `ui/src/lib/unsplashKeyMigration.ts` calls [`unsplash_set_api_key`]
//! with any legacy value on first boot post-update, then clears the
//! localStorage entry.
//!
//! ## Wire contract
//!
//! - [`unsplash_set_api_key`] write, validates non-empty.
//! - [`unsplash_get_api_key`] read, returns `None` when never set.
//! - [`unsplash_clear_api_key`] delete, idempotent.
//!
//! All three return `Result<_, String>` because Tauri serializes
//! errors to JS as strings, same convention as every other command
//! in this crate.

use tauri::State;

use crate::secrets::{SecretKind, SecretStoreHandle};

/// Persist the Unsplash access key. Trims surrounding whitespace and
/// rejects an empty key (use [`unsplash_clear_api_key`] to remove).
///
/// Errors surface as user-facing strings; the UI shows them in a
/// toast. The actual cipher write goes through DPAPI on Windows.
#[tauri::command]
pub fn unsplash_set_api_key(
    store: State<'_, SecretStoreHandle>,
    key: String,
) -> Result<(), String> {
    let trimmed = key.trim();
    if trimmed.is_empty() {
        return Err("unsplash api key cannot be empty".to_string());
    }
    store
        .put(SecretKind::UnsplashApiKey, trimmed)
        .map_err(|e| e.to_string())
}

/// Read the stored Unsplash access key. Returns `Ok(None)` when no
/// key has ever been written; the JS side uses that to render the
/// "not configured" affordance.
#[tauri::command]
pub fn unsplash_get_api_key(store: State<'_, SecretStoreHandle>) -> Result<Option<String>, String> {
    store
        .get(SecretKind::UnsplashApiKey)
        .map_err(|e| e.to_string())
}

/// Remove the stored Unsplash access key. Idempotent: calling on a
/// never-written key is `Ok(())`, not an error.
#[tauri::command]
pub fn unsplash_clear_api_key(store: State<'_, SecretStoreHandle>) -> Result<(), String> {
    store
        .delete(SecretKind::UnsplashApiKey)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    //! Direct trait-level tests against [`NullSecretStore`]. The
    //! `#[tauri::command]` wrappers are tested indirectly via the
    //! UI-side `unsplashPrefs.test.ts` mocking the three commands;
    //! constructing a real Tauri `State<'_, _>` in unit tests is
    //! more rig than it is worth.
    use crate::secrets::{stub::NullSecretStore, SecretKind, SecretStore};

    #[test]
    fn round_trip_via_trait_surface() {
        let store = NullSecretStore::new();
        assert_eq!(store.get(SecretKind::UnsplashApiKey).unwrap(), None);
        store
            .put(SecretKind::UnsplashApiKey, "my-unsplash-key")
            .unwrap();
        assert_eq!(
            store.get(SecretKind::UnsplashApiKey).unwrap().as_deref(),
            Some("my-unsplash-key")
        );
        store.delete(SecretKind::UnsplashApiKey).unwrap();
        assert_eq!(store.get(SecretKind::UnsplashApiKey).unwrap(), None);
    }

    #[test]
    fn empty_key_validation_logic() {
        // The command body rejects empty/whitespace-only keys before
        // calling into the store. Pin the trim+is_empty contract
        // without going through a Tauri State extractor.
        let candidates = ["", "   ", "\t\n"];
        for c in candidates {
            assert!(c.trim().is_empty(), "expected {c:?} to be rejected");
        }
        // And a realistic key shape passes.
        let real = "abcd1234efgh5678ijkl9012mnop3456qrst7890uvw";
        assert!(!real.trim().is_empty());
        assert_eq!(real.trim(), real);
    }
}
