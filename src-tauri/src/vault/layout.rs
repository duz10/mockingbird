//! Vault zone management per ADR 0046 §1.
//!
//! Defines the canonical layout under a user-chosen vault root:
//!
//! ```text
//! <vault>/
//! ├── history/                  # outbound projection of canonical DB
//! │   └── _archive/             # retention-narrowed records (never deleted)
//! ├── inbox/                    # inbound from iOS Shortcut (Iter 3)
//! │   ├── _failed/              # quarantined couriers + .error.json sidecars
//! │   └── _keep/                # dev/debug retention (Iter 4 toggle)
//! └── .mockingbird/             # app bookkeeping, hidden in Obsidian
//!     └── manifest.json         # see [`super::manifest`]
//! ```
//!
//! ## Idempotency contract
//!
//! [`VaultLayout::ensure_zones`] is the single zone-creation entry
//! point. It is safe to call on every export job invocation, on
//! settings-toggle, on app boot, and on every other event that
//! might fire before vault state has been touched -- the
//! reconciliation engine relies on this so it can recover from a
//! user deleting the vault folder mid-run and just keep going.
//!
//! ## What this module does NOT do
//!
//! - No I/O on per-record files. The projection engine
//!   ([`super::export_job`] in Phase C) owns the actual
//!   `history/*.md` writes.
//! - No manifest reads / writes. See [`super::manifest`].
//! - No platform-specific path quoting. `PathBuf::join` is enough --
//!   Obsidian Sync canonicalizes its own slashes on the iOS side.

use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};

/// A vault rooted at some user-chosen directory.
///
/// Borrow-style (carries `&Path` rather than `PathBuf`) because the
/// export-job runtime owns the root and hands out short-lived
/// views; there's no use case for an owned layout that outlives
/// its root.
#[derive(Debug, Clone, Copy)]
pub struct VaultLayout<'a> {
    /// Absolute path to the vault root directory (the same folder
    /// Obsidian Sync watches).
    pub root: &'a Path,
}

impl<'a> VaultLayout<'a> {
    /// Construct a layout view over the given root.
    pub fn new(root: &'a Path) -> Self {
        Self { root }
    }

    /// `<vault>/history/`
    pub fn history(&self) -> PathBuf {
        self.root.join("history")
    }

    /// `<vault>/history/_archive/`
    pub fn history_archive(&self) -> PathBuf {
        self.history().join("_archive")
    }

    /// `<vault>/inbox/`
    pub fn inbox(&self) -> PathBuf {
        self.root.join("inbox")
    }

    /// `<vault>/inbox/_failed/`
    pub fn inbox_failed(&self) -> PathBuf {
        self.inbox().join("_failed")
    }

    /// `<vault>/inbox/_keep/` -- only created when the debug
    /// `VaultDebugKeepCouriers` toggle is on (Iter 4).
    pub fn inbox_keep(&self) -> PathBuf {
        self.inbox().join("_keep")
    }

    /// `<vault>/.mockingbird/` -- leading-dot hides it from
    /// Obsidian's default file list.
    pub fn mockingbird(&self) -> PathBuf {
        self.root.join(".mockingbird")
    }

    /// `<vault>/.mockingbird/manifest.json`
    pub fn manifest_path(&self) -> PathBuf {
        self.mockingbird().join("manifest.json")
    }

    /// Create every always-on zone if missing. Idempotent --
    /// callers should invoke this at the top of every export job
    /// pass and the watcher's bootstrap. `_keep/` is omitted (it's
    /// only made when the debug toggle is on; the courier flow
    /// will materialise it on demand in Iter 3+).
    ///
    /// Errors: wraps `std::io::Error` as
    /// [`AppError::Vault`](crate::error::AppError::Vault) with the
    /// offending path. The error is *not* `AppError::Io` because
    /// the IPC surface needs to distinguish "vault is misconfigured"
    /// (recoverable, surface as a UI nag) from a generic I/O
    /// failure (which gets a different toast).
    pub fn ensure_zones(&self) -> AppResult<()> {
        for dir in [
            self.history(),
            self.history_archive(),
            self.inbox(),
            self.inbox_failed(),
            self.mockingbird(),
        ] {
            std::fs::create_dir_all(&dir).map_err(|e| {
                AppError::Vault(format!(
                    "ensure_zones: failed to create {} -- {}",
                    dir.display(),
                    e
                ))
            })?;
        }
        Ok(())
    }

    /// Verify the vault root is usable before treating it as a
    /// Mockingbird vault. Returns Ok only when the path exists, is
    /// a directory, and is writable. The Settings UI calls this
    /// before flipping `MobileSyncEnabled` ON so a user typo in
    /// the path field is surfaced as a clear validation failure
    /// rather than a silent "no exports happening" mystery.
    ///
    /// The writability probe creates and deletes a sentinel file
    /// under `.mockingbird/` so the user's `history/` and `inbox/`
    /// stay byte-clean even on the probe path.
    pub fn validate(&self) -> AppResult<()> {
        if !self.root.exists() {
            return Err(AppError::Vault(format!(
                "vault path does not exist: {}",
                self.root.display()
            )));
        }
        if !self.root.is_dir() {
            return Err(AppError::Vault(format!(
                "vault path is not a directory: {}",
                self.root.display()
            )));
        }
        // Probe writability via a sentinel under .mockingbird/.
        // Create the parent if missing -- we'd do it anyway on the
        // next ensure_zones, and a fresh vault root looks identical
        // to a writable-but-uninitialised one until we try.
        let mb = self.mockingbird();
        std::fs::create_dir_all(&mb).map_err(|e| {
            AppError::Vault(format!("validate: cannot create {} -- {}", mb.display(), e))
        })?;
        let probe = mb.join(".write-probe");
        std::fs::write(&probe, b"mockingbird-write-probe\n").map_err(|e| {
            AppError::Vault(format!(
                "validate: write probe failed at {} -- {}",
                probe.display(),
                e
            ))
        })?;
        // Best-effort cleanup; if the delete fails the probe file
        // is harmless leftover under a hidden directory.
        let _ = std::fs::remove_file(&probe);
        Ok(())
    }
}

// --------------------------------------------------------------------
// Nested-vault trap detection (ADR 0046 Iter 4 / mb-3xww)
// --------------------------------------------------------------------

/// Walk the directory tree UPWARDS from `candidate`, looking for an
/// ancestor that contains a `.obsidian/` subfolder. If any ancestor
/// is itself an Obsidian vault, the proposed root is *nested*
/// inside that vault — both vaults would race to own the same
/// `.obsidian/` config and Obsidian Sync loops on the conflict.
///
/// Returns:
/// - `None` when no ancestor has `.obsidian/` (the safe case).
/// - `Some(parent_vault_path)` when a nested-vault trap is detected;
///   the Settings UI surfaces a guided dialog so the user can pick
///   a sibling location instead.
///
/// `candidate` itself is intentionally NOT inspected — a user who
/// picks an existing Obsidian vault root as their Mockingbird vault
/// is doing the supported "share the same root" setup.
pub fn detect_nested_vault(candidate: &Path) -> Option<PathBuf> {
    let mut current = candidate.parent()?;
    loop {
        if current.join(".obsidian").is_dir() {
            return Some(current.to_path_buf());
        }
        current = current.parent()?;
    }
}

/// Suggest a SIBLING vault path next to the detected parent vault.
/// Used by the nested-vault dialog's "Use a sibling location"
/// recommendation: takes the parent Obsidian vault path and returns
/// `<parent_of_obsidian_vault>/<obsidian-vault-name>-mockingbird`.
/// If the parent has no parent (root drive), returns `None` — the
/// UI falls back to letting the user pick manually.
pub fn suggest_sibling_vault(parent_vault: &Path) -> Option<PathBuf> {
    let grandparent = parent_vault.parent()?;
    let name = parent_vault.file_name()?.to_str()?;
    Some(grandparent.join(format!("{name}-mockingbird")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn layout_at(td: &TempDir) -> VaultLayout<'_> {
        VaultLayout::new(td.path())
    }

    #[test]
    fn paths_are_built_under_root() {
        let td = TempDir::new().unwrap();
        let l = layout_at(&td);
        assert_eq!(l.history(), td.path().join("history"));
        assert_eq!(
            l.history_archive(),
            td.path().join("history").join("_archive")
        );
        assert_eq!(l.inbox(), td.path().join("inbox"));
        assert_eq!(l.inbox_failed(), td.path().join("inbox").join("_failed"));
        assert_eq!(l.inbox_keep(), td.path().join("inbox").join("_keep"));
        assert_eq!(l.mockingbird(), td.path().join(".mockingbird"));
        assert_eq!(
            l.manifest_path(),
            td.path().join(".mockingbird").join("manifest.json")
        );
    }

    #[test]
    fn ensure_zones_creates_the_always_on_set() {
        let td = TempDir::new().unwrap();
        let l = layout_at(&td);
        l.ensure_zones().unwrap();
        assert!(l.history().is_dir());
        assert!(l.history_archive().is_dir());
        assert!(l.inbox().is_dir());
        assert!(l.inbox_failed().is_dir());
        assert!(l.mockingbird().is_dir());
        // _keep is opt-in; not created here.
        assert!(!l.inbox_keep().exists());
    }

    #[test]
    fn ensure_zones_is_idempotent() {
        let td = TempDir::new().unwrap();
        let l = layout_at(&td);
        l.ensure_zones().unwrap();
        // Drop a marker file in history/ to prove the second call
        // doesn't wipe existing content.
        let marker = l.history().join("marker.txt");
        fs::write(&marker, b"keep me").unwrap();
        l.ensure_zones().unwrap();
        assert!(marker.exists(), "marker file must survive re-ensure");
    }

    #[test]
    fn validate_rejects_missing_root() {
        let td = TempDir::new().unwrap();
        let ghost = td.path().join("does-not-exist");
        let l = VaultLayout::new(&ghost);
        let err = l.validate().unwrap_err();
        match err {
            AppError::Vault(msg) => assert!(msg.contains("does not exist")),
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn validate_rejects_non_directory() {
        let td = TempDir::new().unwrap();
        let file = td.path().join("vault.file");
        fs::write(&file, b"i am a file, not a vault").unwrap();
        let l = VaultLayout::new(&file);
        let err = l.validate().unwrap_err();
        match err {
            AppError::Vault(msg) => assert!(msg.contains("not a directory")),
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn detect_nested_vault_flags_ancestor_obsidian_folder() {
        // <td>/my-obsidian/.obsidian/ (parent vault) +
        // <td>/my-obsidian/mockingbird/ (proposed nested vault).
        let td = TempDir::new().unwrap();
        let parent_vault = td.path().join("my-obsidian");
        let mockingbird = parent_vault.join("mockingbird");
        fs::create_dir_all(parent_vault.join(".obsidian")).unwrap();
        fs::create_dir_all(&mockingbird).unwrap();

        let detected = detect_nested_vault(&mockingbird).expect("nested");
        assert_eq!(detected, parent_vault);
    }

    #[test]
    fn detect_nested_vault_returns_none_for_clean_path() {
        let td = TempDir::new().unwrap();
        let clean = td.path().join("fresh-vault");
        fs::create_dir_all(&clean).unwrap();
        assert!(detect_nested_vault(&clean).is_none());
    }

    #[test]
    fn detect_nested_vault_ignores_candidate_own_obsidian_dir() {
        // Candidate itself has .obsidian/ — that's the user picking
        // their Obsidian vault root as the Mockingbird vault. Allowed.
        let td = TempDir::new().unwrap();
        let candidate = td.path().join("shared-vault");
        fs::create_dir_all(candidate.join(".obsidian")).unwrap();
        assert!(detect_nested_vault(&candidate).is_none());
    }

    #[test]
    fn detect_nested_vault_walks_multiple_levels_up() {
        let td = TempDir::new().unwrap();
        let parent_vault = td.path().join("top-vault");
        let deep = parent_vault.join("a").join("b").join("c").join("nested");
        fs::create_dir_all(parent_vault.join(".obsidian")).unwrap();
        fs::create_dir_all(&deep).unwrap();
        let detected = detect_nested_vault(&deep).expect("nested");
        assert_eq!(detected, parent_vault);
    }

    #[test]
    fn suggest_sibling_vault_appends_mockingbird_suffix() {
        let parent = PathBuf::from("C:\\Users\\you\\my-vault");
        let suggested = suggest_sibling_vault(&parent).expect("sibling");
        assert_eq!(
            suggested,
            PathBuf::from("C:\\Users\\you\\my-vault-mockingbird")
        );
    }

    #[test]
    fn validate_accepts_writable_directory_and_cleans_probe() {
        let td = TempDir::new().unwrap();
        let l = layout_at(&td);
        l.validate().unwrap();
        // The probe file under .mockingbird/ must not persist.
        let probe = l.mockingbird().join(".write-probe");
        assert!(!probe.exists(), "validate must clean up its probe file");
        // .mockingbird/ itself is allowed to remain -- ensure_zones
        // would have made it anyway, and leaving it doesn't pollute
        // any user-visible Obsidian zone.
    }
}
