//! Tests for `entity_pages`. Lives in a sibling file (loaded via
//! `#[cfg(test)] #[path]` from the impl) so the impl stays under
//! the 600-line file cap.

use super::super::kg_layout::{bootstrap_kg_subtree, kg_subtree_paths};
use super::*;
use chrono::TimeZone;
use std::fs;
use tempfile::TempDir;

fn ts(y: i32, m: u32, d: u32, hh: u32, mm: u32, ss: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(y, m, d, hh, mm, ss).unwrap()
}

fn fixed_created_at() -> DateTime<Utc> {
    ts(2026, 6, 6, 10, 0, 0)
}

// ────────────────────────────────────────────────────────────
// Slug validation
// ────────────────────────────────────────────────────────────

#[test]
fn slug_rejects_empty() {
    let td = TempDir::new().unwrap();
    let err = ensure_entity_page(td.path(), "", fixed_created_at()).unwrap_err();
    match err {
        AppError::Vault(msg) => assert!(msg.contains("empty"), "msg: {msg}"),
        other => panic!("expected Vault err, got {other:?}"),
    }
}

#[test]
fn slug_rejects_uppercase() {
    let td = TempDir::new().unwrap();
    let err = ensure_entity_page(td.path(), "Maple", fixed_created_at()).unwrap_err();
    match err {
        AppError::Vault(msg) => assert!(msg.contains("kebab-case"), "msg: {msg}"),
        other => panic!("expected Vault err, got {other:?}"),
    }
}

#[test]
fn slug_rejects_path_traversal() {
    let td = TempDir::new().unwrap();
    let err = ensure_entity_page(td.path(), "../etc-passwd", fixed_created_at()).unwrap_err();
    assert!(matches!(err, AppError::Vault(_)));
}

#[test]
fn slug_rejects_slashes() {
    let td = TempDir::new().unwrap();
    let err = ensure_entity_page(td.path(), "foo/bar", fixed_created_at()).unwrap_err();
    assert!(matches!(err, AppError::Vault(_)));
}

#[test]
fn slug_rejects_leading_or_trailing_hyphen() {
    let td = TempDir::new().unwrap();
    assert!(ensure_entity_page(td.path(), "-maple", fixed_created_at()).is_err());
    assert!(ensure_entity_page(td.path(), "maple-", fixed_created_at()).is_err());
}

#[test]
fn slug_rejects_overlong() {
    let td = TempDir::new().unwrap();
    let too_long = "a".repeat(MAX_SLUG_LEN + 1);
    let err = ensure_entity_page(td.path(), &too_long, fixed_created_at()).unwrap_err();
    match err {
        AppError::Vault(msg) => assert!(msg.contains("max length"), "msg: {msg}"),
        other => panic!("expected Vault err, got {other:?}"),
    }
}

#[test]
fn slug_accepts_valid_kebab_case() {
    let td = TempDir::new().unwrap();
    bootstrap_kg_subtree(td.path()).unwrap();
    let report =
        ensure_entity_page(td.path(), "feta-cheese-2", fixed_created_at()).expect("valid slug");
    assert_eq!(report, StubPageReport::Created);
}

// ────────────────────────────────────────────────────────────
// Entity-page write-once contract
// ────────────────────────────────────────────────────────────

#[test]
fn entity_page_writes_when_missing() {
    let td = TempDir::new().unwrap();
    bootstrap_kg_subtree(td.path()).unwrap();
    let report = ensure_entity_page(td.path(), "maple", fixed_created_at()).unwrap();
    assert_eq!(report, StubPageReport::Created);

    let p = kg_subtree_paths(td.path());
    let stub_path = p.entities.join("maple.md");
    assert!(stub_path.is_file(), "stub file must exist");

    let content = fs::read_to_string(&stub_path).unwrap();
    assert!(content.contains("id: \"maple\""));
    assert!(content.contains("type: \"entity\""));
    assert!(content.contains("schema_version: 1"));
    assert!(content.contains("created_at: \"2026-06-06T10:00:00Z\""));
    assert!(content.contains("aliases: []"));
    assert!(content.contains("# maple"));
    assert!(content.contains("```dataview"));
    assert!(content.contains("FROM \"Knowledge Graph/Entries\""));
    // 1E.5 polish: predicate matches BOTH bare and pipe-alias forms.
    assert!(content.contains("any(entities, (e) => contains(e, \"[[Entities/maple]]\") OR contains(e, \"[[Entities/maple|\"))"));
}

/// THE write-once invariant. A user who has edited their own
/// entity stub MUST get those bytes back byte-identical on the
/// next filing that mentions the same entity.
#[test]
fn entity_page_no_op_when_present_and_user_owned() {
    let td = TempDir::new().unwrap();
    bootstrap_kg_subtree(td.path()).unwrap();
    let p = kg_subtree_paths(td.path());

    // First call writes the stub.
    ensure_entity_page(td.path(), "maple", fixed_created_at()).unwrap();
    let stub_path = p.entities.join("maple.md");

    // User overwrites with their own content.
    let user_content = "# Maple\n\nMy personal notes about Maple.\nDo not nuke me.\n";
    fs::write(&stub_path, user_content).unwrap();

    // Second call — must NOT overwrite.
    let report = ensure_entity_page(td.path(), "maple", ts(2026, 7, 1, 0, 0, 0)).unwrap();
    assert_eq!(report, StubPageReport::AlreadyExists);

    let after = fs::read_to_string(&stub_path).unwrap();
    assert_eq!(
        after, user_content,
        "user-owned stub must survive byte-identical: got {after}"
    );
}

#[test]
fn entity_page_idempotent_back_to_back_calls() {
    let td = TempDir::new().unwrap();
    bootstrap_kg_subtree(td.path()).unwrap();
    let first = ensure_entity_page(td.path(), "eggs", fixed_created_at()).unwrap();
    let second = ensure_entity_page(td.path(), "eggs", fixed_created_at()).unwrap();
    let third = ensure_entity_page(td.path(), "eggs", fixed_created_at()).unwrap();
    assert_eq!(first, StubPageReport::Created);
    assert_eq!(second, StubPageReport::AlreadyExists);
    assert_eq!(third, StubPageReport::AlreadyExists);
}

#[test]
fn entity_page_canonical_form_is_lf_only_with_single_trailing_newline() {
    let td = TempDir::new().unwrap();
    bootstrap_kg_subtree(td.path()).unwrap();
    ensure_entity_page(td.path(), "milk", fixed_created_at()).unwrap();
    let p = kg_subtree_paths(td.path());
    let bytes = fs::read(p.entities.join("milk.md")).unwrap();
    assert!(
        !bytes.contains(&b'\r'),
        "stub must be LF-only (no CR bytes)"
    );
    assert!(bytes.ends_with(b"\n"), "stub must end with newline");
    assert!(
        !bytes.ends_with(b"\n\n"),
        "stub must end with exactly ONE newline"
    );
}

#[test]
fn entity_page_creates_parent_dir_if_missing() {
    // Even without explicit bootstrap, the helper recovers when the
    // parent dir is missing (defensive `create_dir_all`).
    let td = TempDir::new().unwrap();
    let report = ensure_entity_page(td.path(), "dustin", fixed_created_at()).unwrap();
    assert_eq!(report, StubPageReport::Created);
    let p = kg_subtree_paths(td.path());
    assert!(p.entities.join("dustin.md").is_file());
}

// ────────────────────────────────────────────────────────────
// Project-page parallel coverage
// ────────────────────────────────────────────────────────────

#[test]
fn project_page_writes_when_missing() {
    let td = TempDir::new().unwrap();
    bootstrap_kg_subtree(td.path()).unwrap();
    let report = ensure_project_page(td.path(), "mockingbird", fixed_created_at()).unwrap();
    assert_eq!(report, StubPageReport::Created);
    let p = kg_subtree_paths(td.path());
    let stub_path = p.projects.join("mockingbird.md");
    assert!(stub_path.is_file());

    let content = fs::read_to_string(&stub_path).unwrap();
    assert!(content.contains("id: \"mockingbird\""));
    assert!(content.contains("type: \"project\""));
    assert!(content.contains("status: \"active\""));
    // Project stubs do NOT have an `aliases` field — that's
    // entity-only per the ADR amendment.
    assert!(!content.contains("aliases:"));
    assert!(content.contains("# mockingbird"));
    // Dataview body still filters via the `entities` field — a
    // Project entity is still emitted in entries' `entities:` list.
    // 1E.5 polish: predicate matches BOTH bare and pipe-alias forms.
    assert!(content.contains("any(entities, (e) => contains(e, \"[[Entities/mockingbird]]\") OR contains(e, \"[[Entities/mockingbird|\"))"));
}

#[test]
fn project_page_write_once_user_owns_thereafter() {
    let td = TempDir::new().unwrap();
    bootstrap_kg_subtree(td.path()).unwrap();
    ensure_project_page(td.path(), "phase-1e", fixed_created_at()).unwrap();
    let p = kg_subtree_paths(td.path());
    let stub_path = p.projects.join("phase-1e.md");

    let user_edit = "# Phase 1E\n\nstatus: \"paused\"\n\nMy roadmap notes here.\n";
    fs::write(&stub_path, user_edit).unwrap();

    let report = ensure_project_page(td.path(), "phase-1e", ts(2027, 1, 1, 0, 0, 0)).unwrap();
    assert_eq!(report, StubPageReport::AlreadyExists);
    assert_eq!(fs::read_to_string(&stub_path).unwrap(), user_edit);
}

/// Entity + Project stubs are independent files; creating a Project
/// stub does NOT also create the Entity stub for the same slug
/// (and vice versa). The worker fires BOTH calls for a Project-typed
/// entity; this test pins that they don't share a single file.
#[test]
fn entity_and_project_stubs_are_independent_files() {
    let td = TempDir::new().unwrap();
    bootstrap_kg_subtree(td.path()).unwrap();
    ensure_entity_page(td.path(), "mockingbird", fixed_created_at()).unwrap();
    ensure_project_page(td.path(), "mockingbird", fixed_created_at()).unwrap();
    let p = kg_subtree_paths(td.path());
    assert!(p.entities.join("mockingbird.md").is_file());
    assert!(p.projects.join("mockingbird.md").is_file());

    // Byte-identical second calls — both report AlreadyExists.
    let e2 = ensure_entity_page(td.path(), "mockingbird", fixed_created_at()).unwrap();
    let p2 = ensure_project_page(td.path(), "mockingbird", fixed_created_at()).unwrap();
    assert_eq!(e2, StubPageReport::AlreadyExists);
    assert_eq!(p2, StubPageReport::AlreadyExists);
}

// ────────────────────────────────────────────────────────────
// Render-pure unit tests (no I/O)
// ────────────────────────────────────────────────────────────

#[test]
fn render_entity_stub_includes_aliases_not_status() {
    let body = render_stub(StubKind::Entity, "feta-cheese", fixed_created_at());
    assert!(body.contains("aliases: []"));
    assert!(!body.contains("status:"));
    assert!(body.contains("type: \"entity\""));
}

#[test]
fn render_project_stub_includes_status_not_aliases() {
    let body = render_stub(StubKind::Project, "mockingbird", fixed_created_at());
    assert!(body.contains("status: \"active\""));
    assert!(!body.contains("aliases:"));
    assert!(body.contains("type: \"project\""));
}

#[test]
fn render_stub_frontmatter_field_order_pinned() {
    let body = render_stub(StubKind::Entity, "maple", fixed_created_at());
    let order = ["id:", "type:", "schema_version:", "created_at:", "aliases:"];
    let mut prev = 0usize;
    for key in order {
        let pos = body
            .find(key)
            .unwrap_or_else(|| panic!("missing {key}: {body}"));
        assert!(pos > prev, "{key} out of order: {body}");
        prev = pos;
    }
}

#[test]
fn render_stub_dataview_block_references_canonical_wiki_link() {
    let body = render_stub(StubKind::Entity, "feta-cheese", fixed_created_at());
    // Pin the exact dataview WHERE clause so a future drift surfaces
    // here, not at user-visible-broken-Dataview-rendering time.
    // 1E.5 polish: matches BOTH the bare `[[Entities/<slug>]]` (pre-
    // polish) and the pipe-alias `[[Entities/<slug>|...]]` (current)
    // forms so existing on-disk entries remain discoverable.
    assert!(body.contains("any(entities, (e) => contains(e, \"[[Entities/feta-cheese]]\") OR contains(e, \"[[Entities/feta-cheese|\"))"));
    assert!(body.contains("```dataview"));
    assert!(body.contains("FROM \"Knowledge Graph/Entries\""));
    assert!(body.contains("SORT captured_at DESC"));
}
