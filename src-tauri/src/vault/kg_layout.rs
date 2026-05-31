//! Phase 1E Wave 1E.1 (`mb-e16d`, ADR 0053 §D1) — KG subtree bootstrap.
//!
//! Owns the idempotent creation of the
//! `<vault>/Knowledge Graph/{Inbox,Entries,History,Entities,Projects}/`
//! subtree beneath the user's configured vault root. The folder name
//! carries a literal space — Obsidian-facing UX per ADR 0053 §D1 /
//! spec §7.1.
//!
//! **Amendment 2026-06-06 (`mb-08za`, ADR 0053 §D1 as amended):**
//! subtree expanded from 3 → 5 folders to host the new auto-generated
//! Entity / Project stub pages (§D11 / §D12). The expansion is
//! purely additive — the original Cell A/B/C semantics still hold;
//! `BootstrapReport::AlreadyExists` now requires ALL five directories
//! to be present-as-dirs.
//!
//! ## Idempotency contract (ADR 0053 §D1)
//!
//! | Cell | Pre-state                          | Post-state            | Result          |
//! |------|------------------------------------|-----------------------|-----------------|
//! | A    | subtree missing                    | subtree created       | `Created`       |
//! | B    | subtree present, empty             | unchanged             | `AlreadyExists` |
//! | C    | subtree present, has user files    | unchanged             | `AlreadyExists` |
//! | D    | vault path unset / empty / unusable| (caller-side error)   | (caller)        |
//!
//! Cell D is enforced one layer up — the IPC wrapper
//! (`commands::kg::kg_subtree_bootstrap`) is the one that reads
//! `SettingKey::VaultPath` and surfaces the structured error. The
//! pure helper here takes a `&Path` directly; that keeps the
//! module unit-testable without a SQLite fixture (LESSONS P2: pure
//! modules go through the throwaway-crate recipe, not the broken
//! `cargo test --release` runner).
//!
//! ## Why a sibling module to `vault::layout` (not an extension of it)
//!
//! `vault::layout::VaultLayout` owns the **ADR 0046 outbound projection
//! zones** (`<vault>/history/`, `<vault>/inbox/`, `<vault>/.mockingbird/`).
//! Those are Mockingbird-DB-canonical, vault-as-disposable-projection
//! (`docs/adr/0046-...md` §1). The KG subtree under
//! `<vault>/Knowledge Graph/` is the inverse: **files are canonical,
//! DB is shadow FTS** (ADR 0048 §Q3 / ADR 0053 §"Context"). Mixing the
//! two zone families on one struct would silently couple the two
//! source-of-truth axes; keeping them as siblings makes the boundary
//! visible to future readers and confines the "the file IS the
//! database" invariant to this module.
//!
//! ## Cross-platform path discipline
//!
//! All path assembly via [`PathBuf::join`]; never string
//! concatenation (per AGENTS.md Principle 5 + ADR 0046 layout
//! precedent). The "Knowledge Graph" literal — with the space — is
//! a `const &'static str` shared between every helper here, so a
//! future typo can't drift one path off the rest. Windows and
//! macOS both handle spaces in paths fine; the Wave 1E.1 unit
//! tests cover the round-trip on whichever target runs them.

use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};

/// Root folder of the KG subtree, relative to the vault root. The
/// literal space is **intentional** and user-visible in Obsidian's
/// file explorer (ADR 0053 §D1 / spec §7.1).
pub const KG_SUBTREE_ROOT_NAME: &str = "Knowledge Graph";

/// Inbox subfolder name (sibling-to-the-ADR-0046 dictation inbox at
/// `<vault>/inbox/`; the positional routing scheme of ADR 0048 §Q2
/// is what distinguishes them).
pub const KG_INBOX_NAME: &str = "Inbox";

/// Entries subfolder name. Wave 1E.3 will start writing
/// `<date>-<slug>__<id8>.md` files here.
pub const KG_ENTRIES_NAME: &str = "Entries";

/// History subfolder name. Wave 1E.4 will start writing per-session
/// `<YYYY-MM>/<session-uuid>.json` sidecars + audio moves here.
pub const KG_HISTORY_NAME: &str = "History";

/// Entities subfolder name. Amendment `mb-08za` (ADR 0053 §D11) — the
/// continuous auto-generation target for entity stub pages. One
/// `.md` per unique entity slug, write-once + user-owns-thereafter.
pub const KG_ENTITIES_NAME: &str = "Entities";

/// Projects subfolder name. Amendment `mb-08za` (ADR 0053 §D12) — the
/// continuous auto-generation target for project stub pages. Same
/// write-once contract as `Entities/`, scoped to entities classified
/// as `EntityType::Project` by the pipeline.
pub const KG_PROJECTS_NAME: &str = "Projects";

/// Pure path-assembly view over the KG subtree under a given vault
/// root. Constructed via [`kg_subtree_paths`]. No I/O; the helpers
/// just compose `PathBuf`s.
///
/// Owned (not borrowed) `PathBuf`s because the IPC layer hands these
/// back through `Result<T, String>` and we want the boundary to be
/// `'static`-friendly. The allocation cost is negligible
/// (4 × short paths, one call per toggle-on or app boot).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KgSubtreePaths {
    /// `<vault>/Knowledge Graph/`
    pub root: PathBuf,
    /// `<vault>/Knowledge Graph/Inbox/`
    pub inbox: PathBuf,
    /// `<vault>/Knowledge Graph/Entries/`
    pub entries: PathBuf,
    /// `<vault>/Knowledge Graph/History/`
    pub history: PathBuf,
    /// `<vault>/Knowledge Graph/Entities/` (amendment `mb-08za`).
    pub entities: PathBuf,
    /// `<vault>/Knowledge Graph/Projects/` (amendment `mb-08za`).
    pub projects: PathBuf,
}

/// Compose the four KG subtree paths from a vault root. Pure — no
/// I/O, no allocation beyond the four `PathBuf`s.
///
/// Returns paths regardless of whether they exist on disk; the
/// caller is responsible for [`bootstrap_kg_subtree`]ing before
/// expecting reads/writes to succeed.
pub fn kg_subtree_paths(vault_path: &Path) -> KgSubtreePaths {
    let root = vault_path.join(KG_SUBTREE_ROOT_NAME);
    let inbox = root.join(KG_INBOX_NAME);
    let entries = root.join(KG_ENTRIES_NAME);
    let history = root.join(KG_HISTORY_NAME);
    let entities = root.join(KG_ENTITIES_NAME);
    let projects = root.join(KG_PROJECTS_NAME);
    KgSubtreePaths {
        root,
        inbox,
        entries,
        history,
        entities,
        projects,
    }
}

/// Result of a [`bootstrap_kg_subtree`] call. Distinguishes
/// "subtree was missing and is now present" from "subtree was
/// already present; no work to do" so the IPC layer can pick the
/// right log level (`info!` vs `warn!`) and the UI can render the
/// right toast copy.
///
/// Both variants are success — neither is an error condition. The
/// boundary between them is the contract test in ADR 0053 §D1's
/// idempotency table (cells A vs B/C).
///
/// `serde(rename_all = "camelCase")` matches the rest of the KG
/// IPC surface (`KgSettingsSnapshot`, `Vocabularies`, etc.) so the
/// JS side sees `"created"` / `"alreadyExists"` directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BootstrapReport {
    /// At least one of the four subtree directories was missing
    /// and has now been created by this call.
    Created,
    /// All four subtree directories were already present. No I/O
    /// other than the idempotent [`std::fs::create_dir_all`]
    /// no-ops fired by this function.
    AlreadyExists,
}

/// Idempotently create the KG subtree under `vault_path`. Cells A,
/// B, C of the ADR 0053 §D1 table. Cell D (`VaultPath` unset) is
/// the caller's responsibility — this helper assumes it has a
/// caller-validated path.
///
/// Implementation:
///
/// 1. Compute the four target paths.
/// 2. Probe whether ALL four already exist as directories — this is
///    the discriminator between `Created` and `AlreadyExists`.
/// 3. `create_dir_all` each path (idempotent; a no-op on existing
///    directories per the std-lib contract).
/// 4. Return `AlreadyExists` if step 2 saw a fully-present subtree;
///    `Created` otherwise.
///
/// Errors:
///
/// - [`AppError::Vault`] wrapping the underlying [`std::io::Error`]
///   if any `create_dir_all` fails (permission denied, parent path
///   does not exist, path component is a file not a directory,
///   etc.). The error message names the specific path that
///   failed — critical for the user-facing toast since "Knowledge
///   Graph/" is the most common typo / permission-mismatch surface.
///
/// **Does NOT validate `vault_path` itself** (existence, writability,
/// nested-vault collision). That's
/// `vault::layout::VaultLayout::validate` + the `vault_check_path`
/// IPC's job; this helper trusts its caller to have run those
/// checks already.
pub fn bootstrap_kg_subtree(vault_path: &Path) -> AppResult<BootstrapReport> {
    let paths = kg_subtree_paths(vault_path);

    // Probe step. `is_dir()` is the right predicate here (not
    // `exists()`): a file named `Knowledge Graph` would satisfy
    // `exists()` but `create_dir_all` would fail at step 3, and the
    // failure mode "user has a stray file by that name" is the
    // exact case we want to surface as an error rather than mask
    // as `AlreadyExists`.
    let all_present = paths.root.is_dir()
        && paths.inbox.is_dir()
        && paths.entries.is_dir()
        && paths.history.is_dir()
        && paths.entities.is_dir()
        && paths.projects.is_dir();

    for dir in [
        &paths.root,
        &paths.inbox,
        &paths.entries,
        &paths.history,
        &paths.entities,
        &paths.projects,
    ] {
        std::fs::create_dir_all(dir).map_err(|e| {
            AppError::Vault(format!(
                "bootstrap_kg_subtree: failed to create {} -- {}",
                dir.display(),
                e
            ))
        })?;
    }

    if all_present {
        tracing::warn!(
            target: "kg::vault_bootstrap",
            vault = %vault_path.display(),
            "KG subtree already present; bootstrap is a no-op"
        );
        Ok(BootstrapReport::AlreadyExists)
    } else {
        tracing::info!(
            target: "kg::vault_bootstrap",
            vault = %vault_path.display(),
            root = %paths.root.display(),
            "\u{1f9e0} KG subtree bootstrapped under vault"
        );
        Ok(BootstrapReport::Created)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Cell A — pristine vault root, no subtree. Expect `Created`
    /// and all four directories present on disk.
    #[test]
    fn bootstrap_creates_subtree_when_missing() {
        let td = TempDir::new().unwrap();
        let report = bootstrap_kg_subtree(td.path()).expect("bootstrap must succeed");
        assert_eq!(report, BootstrapReport::Created);

        let p = kg_subtree_paths(td.path());
        assert!(p.root.is_dir(), "root must exist");
        assert!(p.inbox.is_dir(), "inbox must exist");
        assert!(p.entries.is_dir(), "entries must exist");
        assert!(p.history.is_dir(), "history must exist");
        assert!(p.entities.is_dir(), "entities must exist (mb-08za)");
        assert!(p.projects.is_dir(), "projects must exist (mb-08za)");
    }

    /// Cell B — subtree already exists, empty. Expect
    /// `AlreadyExists` and no observable changes on disk.
    #[test]
    fn bootstrap_is_no_op_when_subtree_exists_empty() {
        let td = TempDir::new().unwrap();
        // Pre-create.
        bootstrap_kg_subtree(td.path()).unwrap();
        // Second call.
        let report = bootstrap_kg_subtree(td.path()).expect("second call must succeed");
        assert_eq!(report, BootstrapReport::AlreadyExists);
    }

    /// Cell C — subtree exists AND contains user content. The
    /// bootstrap MUST NOT touch the user's files. This is the
    /// most important invariant of the wave; failure here would
    /// silently nuke notes.
    #[test]
    fn bootstrap_preserves_user_files_when_subtree_populated() {
        let td = TempDir::new().unwrap();
        let p = kg_subtree_paths(td.path());

        // Pre-create with user content in each subfolder.
        fs::create_dir_all(&p.entries).unwrap();
        fs::create_dir_all(&p.inbox).unwrap();
        fs::create_dir_all(&p.history).unwrap();

        let user_entry = p.entries.join("my-precious-note.md");
        let user_inbox_audio = p.inbox.join("memo.m4a");
        let user_history_blob = p.history.join("2026-06").join("session.json");
        fs::write(&user_entry, b"# do not delete me").unwrap();
        fs::write(&user_inbox_audio, b"\x00\x01\x02fake-audio").unwrap();
        fs::create_dir_all(user_history_blob.parent().unwrap()).unwrap();
        fs::write(&user_history_blob, b"{\"keep\":true}").unwrap();

        // Bootstrap.
        let report = bootstrap_kg_subtree(td.path()).expect("bootstrap must succeed");
        assert_eq!(report, BootstrapReport::AlreadyExists);

        // Every user file must survive byte-identical.
        assert_eq!(fs::read(&user_entry).unwrap(), b"# do not delete me");
        assert_eq!(
            fs::read(&user_inbox_audio).unwrap(),
            b"\x00\x01\x02fake-audio"
        );
        assert_eq!(fs::read(&user_history_blob).unwrap(), b"{\"keep\":true}");
    }

    /// Partial-presence variant — `Knowledge Graph/` exists but
    /// `Entries/` is missing. Bootstrap must fill in the missing
    /// pieces and report `Created` (something WAS created).
    #[test]
    fn bootstrap_completes_partial_subtree_and_reports_created() {
        let td = TempDir::new().unwrap();
        let p = kg_subtree_paths(td.path());

        // Only root + inbox present; entries + history + entities +
        // projects all missing.
        fs::create_dir_all(&p.root).unwrap();
        fs::create_dir_all(&p.inbox).unwrap();

        let report = bootstrap_kg_subtree(td.path()).expect("bootstrap must succeed");
        assert_eq!(
            report,
            BootstrapReport::Created,
            "partial subtree must report Created, not AlreadyExists"
        );
        assert!(p.entries.is_dir(), "entries must now exist");
        assert!(p.history.is_dir(), "history must now exist");
        assert!(p.entities.is_dir(), "entities must now exist (mb-08za)");
        assert!(p.projects.is_dir(), "projects must now exist (mb-08za)");
    }

    /// Amendment `mb-08za` regression guard: a pre-existing
    /// three-folder subtree (Inbox + Entries + History present;
    /// Entities + Projects missing) must report `Created` rather than
    /// `AlreadyExists`, because the expansion DID create something.
    /// This is the upgrade path for users who toggled the KG on
    /// pre-amendment.
    #[test]
    fn bootstrap_upgrades_pre_amendment_three_folder_subtree() {
        let td = TempDir::new().unwrap();
        let p = kg_subtree_paths(td.path());

        // Plant the pre-amendment shape: only the original three.
        fs::create_dir_all(&p.root).unwrap();
        fs::create_dir_all(&p.inbox).unwrap();
        fs::create_dir_all(&p.entries).unwrap();
        fs::create_dir_all(&p.history).unwrap();

        let report = bootstrap_kg_subtree(td.path()).expect("bootstrap must succeed");
        assert_eq!(
            report,
            BootstrapReport::Created,
            "three-folder pre-amendment subtree must upgrade to Created"
        );
        assert!(p.entities.is_dir(), "entities must now exist");
        assert!(p.projects.is_dir(), "projects must now exist");
    }

    /// Paths are composed via `PathBuf::join`; the literal "Knowledge
    /// Graph" with a space survives. Smoke-tests the cross-platform
    /// path discipline contract.
    #[test]
    fn paths_carry_the_literal_space_in_knowledge_graph() {
        let root = PathBuf::from(if cfg!(windows) {
            r"C:\vault"
        } else {
            "/tmp/vault"
        });
        let p = kg_subtree_paths(&root);
        // Use the components iterator so the test isn't sensitive
        // to the platform's separator character.
        let kg_component = p
            .root
            .components()
            .last()
            .expect("root must have a leaf component");
        assert_eq!(
            kg_component.as_os_str(),
            std::ffi::OsStr::new("Knowledge Graph"),
            "root leaf must be the literal 'Knowledge Graph' with the space",
        );
        // Sub-leaf names.
        assert_eq!(
            p.inbox.components().last().unwrap().as_os_str(),
            std::ffi::OsStr::new("Inbox"),
        );
        assert_eq!(
            p.entries.components().last().unwrap().as_os_str(),
            std::ffi::OsStr::new("Entries"),
        );
        assert_eq!(
            p.history.components().last().unwrap().as_os_str(),
            std::ffi::OsStr::new("History"),
        );
        assert_eq!(
            p.entities.components().last().unwrap().as_os_str(),
            std::ffi::OsStr::new("Entities"),
        );
        assert_eq!(
            p.projects.components().last().unwrap().as_os_str(),
            std::ffi::OsStr::new("Projects"),
        );
    }

    /// Error path — a regular file masquerading as the subtree
    /// root forces `create_dir_all` to fail. The wrapped error
    /// must name the offending path so the user toast is
    /// actionable.
    #[test]
    fn bootstrap_errors_when_subtree_root_is_a_file() {
        let td = TempDir::new().unwrap();
        let p = kg_subtree_paths(td.path());
        // Plant a regular file at the would-be `Knowledge Graph/`
        // path. `create_dir_all` should refuse to convert it.
        fs::write(&p.root, b"i am a file, not a directory").unwrap();

        let err =
            bootstrap_kg_subtree(td.path()).expect_err("file-at-subtree-root must fail bootstrap");
        match err {
            AppError::Vault(msg) => {
                assert!(
                    msg.contains("bootstrap_kg_subtree"),
                    "error must name the helper: {msg}",
                );
                assert!(
                    msg.contains("Knowledge Graph"),
                    "error must name the offending path: {msg}",
                );
            }
            other => panic!("expected AppError::Vault, got: {other:?}"),
        }
    }

    /// `BootstrapReport` serializes to the camelCase shape the UI
    /// expects. Pin the wire contract here so a future enum
    /// refactor (e.g. adding a `Partial` variant) doesn't silently
    /// break the JS side.
    #[test]
    fn report_serializes_camel_case() {
        let created = serde_json::to_string(&BootstrapReport::Created).unwrap();
        assert_eq!(created, "\"created\"");
        let exists = serde_json::to_string(&BootstrapReport::AlreadyExists).unwrap();
        assert_eq!(exists, "\"alreadyExists\"");
    }
}
