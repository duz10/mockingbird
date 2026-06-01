//! Tests for `kg_layout`. Lives in a sibling file (loaded via
//! `#[cfg(test)] #[path]` from the impl) so the impl stays under
//! the 600-line file cap. Split during Wave 1E.7 Part 2 (`mb-5lla`).

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
