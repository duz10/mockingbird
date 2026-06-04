//! Cross-platform stub — in-memory secret store for tests + CI.
//!
//! Never persists; never encrypts. Safe to use in unit + integration
//! tests where the real OS keychain would be a heavyweight dep.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::error::AppResult;

use super::{SecretKind, SecretStore};

/// In-memory `SecretStore` for tests.
pub struct NullSecretStore {
    inner: Mutex<HashMap<&'static str, String>>,
}

impl NullSecretStore {
    /// Construct an empty store.
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }
}

impl Default for NullSecretStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SecretStore for NullSecretStore {
    fn put(&self, kind: SecretKind, plaintext: &str) -> AppResult<()> {
        let mut g = self
            .inner
            .lock()
            .map_err(|_| crate::error::AppError::Secrets("stub mutex poisoned".into()))?;
        g.insert(kind.id(), plaintext.to_string());
        Ok(())
    }

    fn get(&self, kind: SecretKind) -> AppResult<Option<String>> {
        let g = self
            .inner
            .lock()
            .map_err(|_| crate::error::AppError::Secrets("stub mutex poisoned".into()))?;
        Ok(g.get(kind.id()).cloned())
    }

    fn delete(&self, kind: SecretKind) -> AppResult<()> {
        let mut g = self
            .inner
            .lock()
            .map_err(|_| crate::error::AppError::Secrets("stub mutex poisoned".into()))?;
        g.remove(kind.id());
        Ok(())
    }

    fn backend_name(&self) -> &'static str {
        "in-memory (test stub)"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_then_get_round_trips() {
        let s = NullSecretStore::new();
        s.put(SecretKind::ClaudeApiKey, "sk-ant-test").unwrap();
        let v = s.get(SecretKind::ClaudeApiKey).unwrap();
        assert_eq!(v, Some("sk-ant-test".to_string()));
    }

    #[test]
    fn get_missing_returns_none() {
        let s = NullSecretStore::new();
        assert_eq!(s.get(SecretKind::ClaudeApiKey).unwrap(), None);
    }

    #[test]
    fn put_overwrites_existing() {
        let s = NullSecretStore::new();
        s.put(SecretKind::ClaudeApiKey, "v1").unwrap();
        s.put(SecretKind::ClaudeApiKey, "v2").unwrap();
        assert_eq!(
            s.get(SecretKind::ClaudeApiKey).unwrap().as_deref(),
            Some("v2")
        );
    }

    #[test]
    fn delete_is_idempotent() {
        let s = NullSecretStore::new();
        s.delete(SecretKind::ClaudeApiKey).unwrap();
        s.put(SecretKind::ClaudeApiKey, "v").unwrap();
        s.delete(SecretKind::ClaudeApiKey).unwrap();
        s.delete(SecretKind::ClaudeApiKey).unwrap();
        assert_eq!(s.get(SecretKind::ClaudeApiKey).unwrap(), None);
    }

    #[test]
    fn backend_name_advertises_test_stub() {
        let s = NullSecretStore::new();
        assert!(s.backend_name().contains("test"));
    }

    #[test]
    fn kinds_have_distinct_ids() {
        assert_ne!(SecretKind::ClaudeApiKey.id(), SecretKind::Reserved2.id());
        assert_ne!(
            SecretKind::ClaudeApiKey.id(),
            SecretKind::UnsplashApiKey.id()
        );
        assert_ne!(SecretKind::UnsplashApiKey.id(), SecretKind::Reserved2.id());
    }

    #[test]
    fn unsplash_key_isolated_from_claude_in_stub() {
        // The in-memory stub is keyed by `SecretKind::id()` — pin
        // that writing one kind does not leak into another.
        let s = NullSecretStore::new();
        s.put(SecretKind::ClaudeApiKey, "claude-v").unwrap();
        s.put(SecretKind::UnsplashApiKey, "unsplash-v").unwrap();
        assert_eq!(
            s.get(SecretKind::ClaudeApiKey).unwrap().as_deref(),
            Some("claude-v")
        );
        assert_eq!(
            s.get(SecretKind::UnsplashApiKey).unwrap().as_deref(),
            Some("unsplash-v")
        );
        s.delete(SecretKind::ClaudeApiKey).unwrap();
        assert_eq!(s.get(SecretKind::ClaudeApiKey).unwrap(), None);
        // Deleting one kind must NOT affect the other.
        assert_eq!(
            s.get(SecretKind::UnsplashApiKey).unwrap().as_deref(),
            Some("unsplash-v")
        );
    }
}
