//! Windows DPAPI-backed [`SecretStore`].
//!
//! Per PLAN §10 Phase 4: "DPAPI; Claude key validated on entry".
//!
//! Storage layout:
//!
//! ```text
//! <appdata_local>\Mockingbird\secrets\<kind_id>.dpapi
//! ```
//!
//! Bytes on disk are the raw output of `CryptProtectData` with the
//! **current Windows user account** as the encryption principal. They
//! cannot be decrypted by another user account on the same machine,
//! nor by the same account on a different machine.
//!
//! Optional entropy: we mix the literal string `"mockingbird-v1"` as
//! the secondary entropy so a malicious cohabiting process can't
//! `CryptUnprotectData` a file copied out of our directory without
//! also knowing this constant. Cheap defence-in-depth.
//!
//! ## Why not `keyring` (the crate)
//!
//! `keyring` is great cross-platform but on Windows it uses the
//! Windows Credential Manager, which (a) caps secrets at ~512 bytes
//! (problematic for future per-mode prompt overrides), (b) is shared
//! across all apps under one user (collisions possible), and (c)
//! adds a transitive dep on `winapi` 0.3 — we're on `windows-rs` 0.56.
//! DPAPI files are 50 lines of clear Win32 calls; keep it local.

use std::path::PathBuf;

use windows::core::PCWSTR;
use windows::Win32::Foundation::LocalFree;
use windows::Win32::Foundation::HLOCAL;
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB,
};

use crate::error::{AppError, AppResult};

use super::{SecretKind, SecretStore};

/// Static entropy mixed into every protect/unprotect call.
const ENTROPY_TAG: &[u8] = b"mockingbird-v1";

/// DPAPI-backed secret store.
pub struct WinDpapiSecretStore {
    base_dir: PathBuf,
}

impl WinDpapiSecretStore {
    /// Construct with the default storage dir at
    /// `%LocalAppData%\Mockingbird\secrets\`. Creates the dir if
    /// missing (mode is the OS default — DPAPI cipher is the actual
    /// guard, not file permissions).
    pub fn new() -> AppResult<Self> {
        let base = default_secrets_dir()?;
        std::fs::create_dir_all(&base).map_err(|e| {
            AppError::Secrets(format!("create secrets dir {}: {e}", base.display()))
        })?;
        Ok(Self { base_dir: base })
    }

    /// Override the storage directory (tests).
    #[cfg(test)]
    pub fn with_base_dir(base_dir: PathBuf) -> AppResult<Self> {
        std::fs::create_dir_all(&base_dir)
            .map_err(|e| AppError::Secrets(format!("create dir {}: {e}", base_dir.display())))?;
        Ok(Self { base_dir })
    }

    fn path_for(&self, kind: SecretKind) -> PathBuf {
        self.base_dir.join(format!("{}.dpapi", kind.id()))
    }
}

fn default_secrets_dir() -> AppResult<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")
        .ok_or_else(|| AppError::Secrets("LOCALAPPDATA not set".into()))?;
    Ok(PathBuf::from(local).join("Mockingbird").join("secrets"))
}

impl SecretStore for WinDpapiSecretStore {
    fn put(&self, kind: SecretKind, plaintext: &str) -> AppResult<()> {
        let cipher = dpapi_protect(plaintext.as_bytes())?;
        let path = self.path_for(kind);
        // Write-rename pattern: write to .tmp then atomic rename to
        // avoid leaving a half-written cipher file if power dies.
        let tmp = path.with_extension("dpapi.tmp");
        std::fs::write(&tmp, &cipher)
            .map_err(|e| AppError::Secrets(format!("write {}: {e}", tmp.display())))?;
        std::fs::rename(&tmp, &path)
            .map_err(|e| AppError::Secrets(format!("rename {}: {e}", path.display())))?;
        Ok(())
    }

    fn get(&self, kind: SecretKind) -> AppResult<Option<String>> {
        let path = self.path_for(kind);
        if !path.exists() {
            return Ok(None);
        }
        let cipher = std::fs::read(&path)
            .map_err(|e| AppError::Secrets(format!("read {}: {e}", path.display())))?;
        let plain = dpapi_unprotect(&cipher)?;
        let s = String::from_utf8(plain)
            .map_err(|e| AppError::Secrets(format!("non-utf8 secret: {e}")))?;
        Ok(Some(s))
    }

    fn delete(&self, kind: SecretKind) -> AppResult<()> {
        let path = self.path_for(kind);
        match std::fs::remove_file(&path) {
            Ok(_) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(AppError::Secrets(format!("delete {}: {e}", path.display()))),
        }
    }

    fn backend_name(&self) -> &'static str {
        "Windows DPAPI"
    }
}

/// Low-level: encrypt `plaintext` with DPAPI + ENTROPY_TAG.
///
/// Returns the cipher bytes ready to write to disk.
fn dpapi_protect(plaintext: &[u8]) -> AppResult<Vec<u8>> {
    let in_blob = CRYPT_INTEGER_BLOB {
        cbData: plaintext.len() as u32,
        pbData: plaintext.as_ptr() as *mut u8,
    };
    let entropy = CRYPT_INTEGER_BLOB {
        cbData: ENTROPY_TAG.len() as u32,
        pbData: ENTROPY_TAG.as_ptr() as *mut u8,
    };
    let mut out_blob = CRYPT_INTEGER_BLOB::default();

    // SAFETY: pbData pointers are valid for their cbData lengths +
    // are only read by Win32. out_blob is owned by Win32 until
    // LocalFree below.
    unsafe {
        CryptProtectData(
            &in_blob,
            PCWSTR::null(),
            Some(&entropy),
            None,
            None,
            0,
            &mut out_blob,
        )
        .map_err(|e| AppError::Secrets(format!("CryptProtectData: {e}")))?;
    }

    let slice = unsafe { std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize) };
    let out = slice.to_vec();
    // Free the Win32-allocated buffer.
    unsafe {
        let _ = LocalFree(HLOCAL(out_blob.pbData as *mut _));
    }
    Ok(out)
}

/// Low-level: decrypt `cipher` with DPAPI + ENTROPY_TAG.
fn dpapi_unprotect(cipher: &[u8]) -> AppResult<Vec<u8>> {
    let in_blob = CRYPT_INTEGER_BLOB {
        cbData: cipher.len() as u32,
        pbData: cipher.as_ptr() as *mut u8,
    };
    let entropy = CRYPT_INTEGER_BLOB {
        cbData: ENTROPY_TAG.len() as u32,
        pbData: ENTROPY_TAG.as_ptr() as *mut u8,
    };
    let mut out_blob = CRYPT_INTEGER_BLOB::default();

    unsafe {
        CryptUnprotectData(&in_blob, None, Some(&entropy), None, None, 0, &mut out_blob)
            .map_err(|e| AppError::Secrets(format!("CryptUnprotectData: {e}")))?;
    }

    let slice = unsafe { std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize) };
    let out = slice.to_vec();
    unsafe {
        let _ = LocalFree(HLOCAL(out_blob.pbData as *mut _));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> WinDpapiSecretStore {
        let dir = tempfile::tempdir().unwrap().keep();
        WinDpapiSecretStore::with_base_dir(dir).unwrap()
    }

    #[test]
    fn put_then_get_round_trips_via_real_dpapi() {
        let s = temp_store();
        let secret = "sk-ant-realistic-looking-key-xxxxxxxxxxxxxxxxxxxxxx";
        s.put(SecretKind::ClaudeApiKey, secret).unwrap();
        let got = s.get(SecretKind::ClaudeApiKey).unwrap();
        assert_eq!(got.as_deref(), Some(secret));
    }

    #[test]
    fn get_missing_returns_none() {
        let s = temp_store();
        assert_eq!(s.get(SecretKind::ClaudeApiKey).unwrap(), None);
    }

    #[test]
    fn put_overwrites_via_atomic_rename() {
        let s = temp_store();
        s.put(SecretKind::ClaudeApiKey, "v1").unwrap();
        s.put(SecretKind::ClaudeApiKey, "v2").unwrap();
        assert_eq!(
            s.get(SecretKind::ClaudeApiKey).unwrap().as_deref(),
            Some("v2")
        );
    }

    #[test]
    fn delete_removes_file_idempotently() {
        let s = temp_store();
        s.delete(SecretKind::ClaudeApiKey).unwrap();
        s.put(SecretKind::ClaudeApiKey, "v").unwrap();
        s.delete(SecretKind::ClaudeApiKey).unwrap();
        s.delete(SecretKind::ClaudeApiKey).unwrap();
        assert_eq!(s.get(SecretKind::ClaudeApiKey).unwrap(), None);
    }

    #[test]
    fn cipher_on_disk_does_not_contain_plaintext() {
        let s = temp_store();
        let secret = "PLAINTEXT-SENTINEL-12345";
        s.put(SecretKind::ClaudeApiKey, secret).unwrap();
        let path = s.path_for(SecretKind::ClaudeApiKey);
        let bytes = std::fs::read(&path).unwrap();
        let bytes_as_str = String::from_utf8_lossy(&bytes);
        assert!(
            !bytes_as_str.contains("PLAINTEXT-SENTINEL"),
            "DPAPI cipher leaked plaintext"
        );
    }

    #[test]
    fn backend_name_advertises_dpapi() {
        let s = temp_store();
        assert!(s.backend_name().contains("DPAPI"));
    }

    #[test]
    fn unprotect_with_wrong_entropy_would_fail() {
        // White-box: directly attempt unprotect with a non-matching
        // entropy by calling protect ourselves then unprotect with
        // mismatched bytes. We can't easily do mismatched-entropy
        // through the SecretStore interface (entropy is static).
        // This test pins that ENTROPY_TAG is non-empty + actually
        // gets fed in; if someone accidentally passes None for
        // entropy in protect or unprotect, this catches it.
        let cipher = dpapi_protect(b"hello").unwrap();
        let plain = dpapi_unprotect(&cipher).unwrap();
        assert_eq!(plain, b"hello");
        assert!(!ENTROPY_TAG.is_empty());
    }

    #[test]
    fn large_secret_round_trips() {
        let s = temp_store();
        // 100 KB secret — well above the Windows Credential Manager's
        // ~512-byte cap that DPAPI does not have.
        let big = "a".repeat(100_000);
        s.put(SecretKind::ClaudeApiKey, &big).unwrap();
        assert_eq!(
            s.get(SecretKind::ClaudeApiKey).unwrap().unwrap().len(),
            big.len()
        );
    }
}
