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
//! **Amendment 2026-06-06 #2 (`mb-bgpt`, ADR 0054 §C/§D/§E/§F):**
//! subtree expanded from 5 → 6 folders to host the new auto-generated
//! Tag stub pages, AND three vault-resident root files were added
//! (`SCHEMA.md`, `INDEX.md`, `LOG.md`) for the Personal Knowledge
//! Engine substrate. Folder expansion follows the same purely-
//! additive idempotency contract; root files have their own bootstrap
//! helper ([`bootstrap_kg_root_files`]) with write-once semantics --
//! they are not folders, so the existing folder-only contract has
//! no business gating their creation.
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

/// Tags subfolder name. Amendment `mb-bgpt` (ADR 0054 §F) -- the
/// auto-generation target for tag stub pages, one `.md` per unique
/// tag slug. Same write-once + user-owns-thereafter contract as
/// `Entities/` and `Projects/`; bodies host a Dataview rollup of
/// `Entries/` files carrying the tag.
pub const KG_TAGS_NAME: &str = "Tags";

/// Filename of the operational-contract document. Write-once on
/// first KG activation, user-owned thereafter. The chat-LLM reads
/// this file to operate Ingest/Query/Lint over the vault (ADR 0054
/// §C).
pub const KG_SCHEMA_MD_NAME: &str = "SCHEMA.md";

/// Filename of the auto-maintained content catalog. Rebuilt from
/// DB state after every successful KG filing by the worker; ADR
/// 0054 §D file-wins-vs-DB-wins contract: the DB wins (INDEX.md is
/// a derived catalog, not a primary).
pub const KG_INDEX_MD_NAME: &str = "INDEX.md";

/// Filename of the append-only operations log. Mockingbird only
/// ever appends `capture` operations; the chat-LLM appends
/// `ingest`/`query`/`lint` operations. Both append; nobody
/// rewrites historical lines. Crash-safe via atomic temp-sibling-
/// rename of the full file on each append. ADR 0054 §E.
pub const KG_LOG_MD_NAME: &str = "LOG.md";

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
    /// `<vault>/Knowledge Graph/Tags/` (amendment `mb-bgpt`).
    pub tags: PathBuf,
}

/// Companion to [`KgSubtreePaths`] for the three vault-resident root
/// files added by amendment `mb-bgpt`. Co-located with the subtree
/// paths so future readers see the full file-system surface of the
/// KG in one place; kept as a separate struct because folders and
/// files have different bootstrap semantics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KgRootFilePaths {
    /// `<vault>/Knowledge Graph/SCHEMA.md`
    pub schema_md: PathBuf,
    /// `<vault>/Knowledge Graph/INDEX.md`
    pub index_md: PathBuf,
    /// `<vault>/Knowledge Graph/LOG.md`
    pub log_md: PathBuf,
}

/// Compose the three KG root-file paths from a vault root. Pure --
/// no I/O. Mirrors [`kg_subtree_paths`] semantics.
pub fn kg_root_file_paths(vault_path: &Path) -> KgRootFilePaths {
    let root = vault_path.join(KG_SUBTREE_ROOT_NAME);
    KgRootFilePaths {
        schema_md: root.join(KG_SCHEMA_MD_NAME),
        index_md: root.join(KG_INDEX_MD_NAME),
        log_md: root.join(KG_LOG_MD_NAME),
    }
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
    let tags = root.join(KG_TAGS_NAME);
    KgSubtreePaths {
        root,
        inbox,
        entries,
        history,
        entities,
        projects,
        tags,
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
        && paths.projects.is_dir()
        && paths.tags.is_dir();

    for dir in [
        &paths.root,
        &paths.inbox,
        &paths.entries,
        &paths.history,
        &paths.entities,
        &paths.projects,
        &paths.tags,
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

/// Idempotently create the three KG root files under `vault_path`.
///
/// **Write-once contract** (ADR 0054 §C/§E): each file is written
/// ONLY when it does not already exist. A user-edited
/// `SCHEMA.md`, a chat-LLM-appended `LOG.md`, or a freshly
/// worker-rebuilt `INDEX.md` is NEVER overwritten by this helper.
/// Worker phases 5b/5c (`maybe_rebuild_index_md`,
/// `maybe_append_log_md`) take over post-bootstrap and own those
/// files' steady-state contents.
///
/// File-by-file semantics:
///
/// - **`SCHEMA.md`**: rendered fresh from
///   [`crate::vault::schema_md::render_schema_md`] when missing.
///   Never re-rendered (the user owns it after bootstrap).
/// - **`INDEX.md`**: written as a minimal skeleton (`# INDEX` +
///   the five H2 section headers, empty) when missing; the worker
///   replaces this on the next filing via
///   [`crate::vault::index_md::rebuild_index_md`].
/// - **`LOG.md`**: written with a header comment + a bootstrap
///   line (`## [<now>] bootstrap | KG activated`) when missing;
///   the worker appends on every subsequent filing via
///   [`crate::vault::log_md::append_log_line`].
///
/// Returns a [`RootFilesReport`] distinguishing "all three already
/// existed; no-op" from "at least one was missing and is now
/// written".
///
/// **Caller contract**: invoke AFTER [`bootstrap_kg_subtree`] has
/// succeeded so the parent `Knowledge Graph/` directory exists.
/// The wiring in [`commands::kg::kg_subtree_bootstrap`] handles
/// this ordering.
pub fn bootstrap_kg_root_files(vault_path: &Path) -> AppResult<RootFilesReport> {
    let paths = kg_root_file_paths(vault_path);
    let mut created_any = false;

    if !paths.schema_md.exists() {
        let bytes = crate::vault::schema_md::render_schema_md();
        std::fs::write(&paths.schema_md, bytes.as_bytes()).map_err(|e| {
            AppError::Vault(format!(
                "bootstrap_kg_root_files: failed to write {} -- {}",
                paths.schema_md.display(),
                e
            ))
        })?;
        created_any = true;
        tracing::info!(
            target: "kg::vault_bootstrap",
            path = %paths.schema_md.display(),
            "\u{1f4dc} SCHEMA.md written (write-once; user owns thereafter)"
        );
    }

    if !paths.index_md.exists() {
        let bytes = crate::vault::index_md::render_skeleton_index_md();
        std::fs::write(&paths.index_md, bytes.as_bytes()).map_err(|e| {
            AppError::Vault(format!(
                "bootstrap_kg_root_files: failed to write {} -- {}",
                paths.index_md.display(),
                e
            ))
        })?;
        created_any = true;
        tracing::info!(
            target: "kg::vault_bootstrap",
            path = %paths.index_md.display(),
            "\u{1f5c2} INDEX.md skeleton written; worker will rebuild on next filing"
        );
    }

    if !paths.log_md.exists() {
        let bytes = crate::vault::log_md::render_bootstrap_log_md();
        std::fs::write(&paths.log_md, bytes.as_bytes()).map_err(|e| {
            AppError::Vault(format!(
                "bootstrap_kg_root_files: failed to write {} -- {}",
                paths.log_md.display(),
                e
            ))
        })?;
        created_any = true;
        tracing::info!(
            target: "kg::vault_bootstrap",
            path = %paths.log_md.display(),
            "\u{1f4dc} LOG.md bootstrap line written; worker + chat-LLM both append"
        );
    }

    Ok(if created_any {
        RootFilesReport::Created
    } else {
        RootFilesReport::AlreadyExists
    })
}

/// Report from [`bootstrap_kg_root_files`]. Same shape as
/// [`BootstrapReport`], split into its own enum so the wire
/// contract for the (folder) subtree bootstrap remains untouched
/// by amendment `mb-bgpt` -- existing IPC consumers see the same
/// `"created"` / `"alreadyExists"` strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RootFilesReport {
    /// At least one of the three root files was missing and has
    /// now been written by this call.
    Created,
    /// All three root files were already present. No I/O.
    AlreadyExists,
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
        assert!(p.tags.is_dir(), "tags must exist (mb-bgpt)");
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
        assert!(p.tags.is_dir(), "tags must now exist (mb-bgpt)");
    }

    /// Amendment `mb-bgpt` regression guard: a pre-amendment
    /// 5-folder subtree (Inbox + Entries + History + Entities +
    /// Projects present; Tags missing) must report `Created` so the
    /// upgrade path lifts existing 1E.6-era installs cleanly. The
    /// older 3-folder upgrade path is still pinned below.
    #[test]
    fn bootstrap_upgrades_pre_bgpt_five_folder_subtree() {
        let td = TempDir::new().unwrap();
        let p = kg_subtree_paths(td.path());

        fs::create_dir_all(&p.root).unwrap();
        fs::create_dir_all(&p.inbox).unwrap();
        fs::create_dir_all(&p.entries).unwrap();
        fs::create_dir_all(&p.history).unwrap();
        fs::create_dir_all(&p.entities).unwrap();
        fs::create_dir_all(&p.projects).unwrap();

        let report = bootstrap_kg_subtree(td.path()).expect("bootstrap must succeed");
        assert_eq!(
            report,
            BootstrapReport::Created,
            "five-folder pre-bgpt subtree must upgrade to Created when Tags lands"
        );
        assert!(p.tags.is_dir(), "tags must now exist");
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
        assert!(p.tags.is_dir(), "tags must now exist (mb-bgpt)");
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
        assert_eq!(
            p.tags.components().last().unwrap().as_os_str(),
            std::ffi::OsStr::new("Tags"),
        );
    }

    // ----------------------------------------------------------
    // bootstrap_kg_root_files coverage (amendment `mb-bgpt`).
    // ----------------------------------------------------------

    /// Cell A for root files -- pristine subtree, no root files.
    /// All three (SCHEMA, INDEX, LOG) get written and report
    /// `Created`.
    #[test]
    fn root_files_bootstrap_creates_all_three_when_missing() {
        let td = TempDir::new().unwrap();
        bootstrap_kg_subtree(td.path()).unwrap();

        let report = bootstrap_kg_root_files(td.path()).expect("root-file bootstrap must succeed");
        assert_eq!(report, RootFilesReport::Created);

        let r = kg_root_file_paths(td.path());
        assert!(r.schema_md.is_file(), "SCHEMA.md must exist");
        assert!(r.index_md.is_file(), "INDEX.md must exist");
        assert!(r.log_md.is_file(), "LOG.md must exist");
    }

    /// Cell B for root files -- all three already present. No-op.
    #[test]
    fn root_files_bootstrap_is_no_op_when_all_present() {
        let td = TempDir::new().unwrap();
        bootstrap_kg_subtree(td.path()).unwrap();
        bootstrap_kg_root_files(td.path()).unwrap();

        let report = bootstrap_kg_root_files(td.path()).expect("second call must succeed");
        assert_eq!(report, RootFilesReport::AlreadyExists);
    }

    /// **Write-once contract** -- the most important invariant.
    /// User-edited SCHEMA.md / chat-LLM-appended LOG.md must
    /// survive a re-bootstrap byte-identical.
    #[test]
    fn root_files_bootstrap_preserves_user_edits() {
        let td = TempDir::new().unwrap();
        bootstrap_kg_subtree(td.path()).unwrap();
        let r = kg_root_file_paths(td.path());

        let user_schema = b"# my custom SCHEMA\n\nUser edits here.\n";
        let user_log = b"## [2026-06-06 12:00] capture | hand-written log line\n";
        fs::write(&r.schema_md, user_schema).unwrap();
        fs::write(&r.log_md, user_log).unwrap();
        // INDEX.md still missing -- we want to confirm the helper
        // writes the missing one without touching the other two.

        let report = bootstrap_kg_root_files(td.path()).expect("bootstrap must succeed");
        assert_eq!(
            report,
            RootFilesReport::Created,
            "missing INDEX.md must trigger Created even when other two are user-owned"
        );

        assert_eq!(
            fs::read(&r.schema_md).unwrap(),
            user_schema,
            "SCHEMA.md must survive byte-identical (write-once)"
        );
        assert_eq!(
            fs::read(&r.log_md).unwrap(),
            user_log,
            "LOG.md must survive byte-identical (write-once)"
        );
        assert!(r.index_md.is_file(), "INDEX.md must have been written");
    }

    /// Root files use canonical LF line endings -- pinned because
    /// LESSONS PINNED P12 Finding 1 has bitten previous waves.
    #[test]
    fn root_files_have_lf_only_line_endings() {
        let td = TempDir::new().unwrap();
        bootstrap_kg_subtree(td.path()).unwrap();
        bootstrap_kg_root_files(td.path()).unwrap();
        let r = kg_root_file_paths(td.path());

        for path in [&r.schema_md, &r.index_md, &r.log_md] {
            let bytes = fs::read(path).unwrap();
            assert!(
                !bytes.contains(&b'\r'),
                "{} must be LF-only on disk; found CR",
                path.display()
            );
        }
    }

    /// `RootFilesReport` wire-contract test, mirroring
    /// `report_serializes_camel_case`.
    #[test]
    fn root_files_report_serializes_camel_case() {
        assert_eq!(
            serde_json::to_string(&RootFilesReport::Created).unwrap(),
            "\"created\""
        );
        assert_eq!(
            serde_json::to_string(&RootFilesReport::AlreadyExists).unwrap(),
            "\"alreadyExists\""
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
