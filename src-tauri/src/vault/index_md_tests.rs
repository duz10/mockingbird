//! Tests for [`super`] (`vault::index_md`). Split into a sibling
//! file so `index_md.rs` stays under the 600-line cap (AGENTS.md
//! coding standards; LESSONS PINNED P-cap). Mirrors the
//! `entity_pages.rs` ⇄ `entity_pages_tests.rs` split pattern.

use super::*;
use std::fs;

// ── Pure renderers ────────────────────────────────────────────────

#[test]
fn skeleton_has_all_five_h2_headers_in_order() {
    let out = render_skeleton_index_md();
    let positions: Vec<_> = [H2_SOURCES, H2_ENTITIES, H2_PROJECTS, H2_TAGS, H2_CONCEPTS]
        .iter()
        .map(|h| {
            out.find(h)
                .unwrap_or_else(|| panic!("missing header `{h}`"))
        })
        .collect();
    for w in positions.windows(2) {
        assert!(w[0] < w[1], "headers out of order in skeleton");
    }
}

#[test]
fn skeleton_is_lf_only_and_deterministic() {
    let a = render_skeleton_index_md();
    let b = render_skeleton_index_md();
    assert_eq!(a, b, "skeleton must be deterministic");
    assert!(!a.contains('\r'), "skeleton must be LF-only");
}

#[test]
fn full_render_preserves_existing_concepts_section_verbatim() {
    let snapshot = IndexSnapshot {
        sources: vec![],
        entities: vec![],
        projects: vec![],
        tags: vec![],
    };
    let concepts = "## Concepts\n\n- [[Concepts/karpathy-llm-wiki|Karpathy LLM Wiki]]\n";
    let out = render_index_md(&snapshot, Some(concepts));
    assert!(
        out.contains("Karpathy LLM Wiki"),
        "Concepts body lost: {out}"
    );
    // Concepts header position: must be at the tail (last H2).
    let concepts_pos = out.find("## Concepts").expect("Concepts header missing");
    let tail = &out[concepts_pos..];
    assert!(
        tail.contains("Karpathy LLM Wiki"),
        "Concepts body must follow its header"
    );
}

#[test]
fn full_render_emits_bare_concepts_header_when_absent() {
    let snapshot = IndexSnapshot {
        sources: vec![],
        entities: vec![],
        projects: vec![],
        tags: vec![],
    };
    let out = render_index_md(&snapshot, None);
    assert!(
        out.contains("## Concepts\n"),
        "bare Concepts header missing"
    );
}

#[test]
fn full_render_emits_alphabetical_entities_with_slug_links() {
    let snapshot = IndexSnapshot {
        sources: vec![],
        entities: vec!["Mockingbird".to_string(), "Acme Corp".to_string()],
        projects: vec![],
        tags: vec![],
    };
    let out = render_index_md(&snapshot, None);
    assert!(out.contains("[[Entities/mockingbird|Mockingbird]]"));
    assert!(out.contains("[[Entities/acme-corp|Acme Corp]]"));
}

#[test]
fn full_render_emits_alphabetical_tags() {
    let snapshot = IndexSnapshot {
        sources: vec![],
        entities: vec![],
        projects: vec![],
        tags: vec!["grocery".to_string(), "phase-1e".to_string()],
    };
    let out = render_index_md(&snapshot, None);
    assert!(out.contains("[[Tags/grocery|grocery]]"));
    assert!(out.contains("[[Tags/phase-1e|phase-1e]]"));
}

#[test]
fn full_render_emits_sources_with_started_at_and_link() {
    let snapshot = IndexSnapshot {
        sources: vec![IndexSourceRow {
            started_at: "2026-06-15T14:32:01Z".to_string(),
            vault_path: "Knowledge Graph/Entries/2026-06-15-buy-milk__abcd1234.md".to_string(),
        }],
        entities: vec![],
        projects: vec![],
        tags: vec![],
    };
    let out = render_index_md(&snapshot, None);
    assert!(out.contains("2026-06-15T14:32:01Z"));
    // Title is derived from the filename slug.
    assert!(out.contains("[[Knowledge Graph/Entries/2026-06-15-buy-milk__abcd1234|buy-milk]]"));
}

#[test]
fn full_render_is_byte_deterministic_for_a_fixed_snapshot() {
    let snapshot = IndexSnapshot {
        sources: vec![IndexSourceRow {
            started_at: "2026-06-15T14:32:01Z".to_string(),
            vault_path: "Knowledge Graph/Entries/2026-06-15-buy-milk__abcd1234.md".to_string(),
        }],
        entities: vec!["acme".to_string(), "mockingbird".to_string()],
        projects: vec!["mockingbird".to_string()],
        tags: vec!["grocery".to_string()],
    };
    let a = render_index_md(&snapshot, None);
    let b = render_index_md(&snapshot, None);
    assert_eq!(
        a, b,
        "INDEX.md must be byte-deterministic given the same snapshot"
    );
    assert!(!a.contains('\r'), "INDEX.md must be LF-only");
}

// ── Concepts-section extraction ────────────────────────────────────

#[test]
fn extract_concepts_returns_section_including_header() {
    let existing = "# INDEX\n\n## Sources\n\n## Concepts\n\nbody here\n";
    let got = extract_concepts_section(existing).unwrap();
    assert!(got.starts_with("## Concepts"));
    assert!(got.contains("body here"));
}

#[test]
fn extract_concepts_returns_none_when_missing() {
    let existing = "# INDEX\n\n## Sources\n";
    assert!(extract_concepts_section(existing).is_none());
}

// ── DB-driven snapshot + rebuild (in-memory rusqlite) ──────────────

use rusqlite::Connection as Conn;

/// Spin up a fresh in-memory DB with every migration applied. Uses
/// `apply_all` -- the same runner the production startup uses, so
/// these tests catch schema drift between migration files and the
/// snapshot queries.
fn fresh_db() -> Conn {
    let conn = Conn::open_in_memory().expect("open in-memory");
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    crate::db::migrations::apply_all(&conn).expect("apply migrations");
    conn
}

/// Insert a session row representing a successfully-filed KG entry
/// (migration-026 columns `entry_id` + `vault_path` + `vault_file_hash`
/// populated). Uses the actual sessions schema -- there is no
/// `transcript_title` column; titles for INDEX.md come from the
/// vault_path filename.
fn insert_filed_session(conn: &Conn, id: i64, uuid: &str, started_at: &str, vault_path: &str) {
    conn.execute(
        "INSERT INTO sessions (id, uuid, mode_id, hotkey_pressed, started_at, \
         recording_ended_at, status, audio_duration_ms, capture_kind, \
         entry_id, vault_path, vault_file_hash) \
         VALUES (?1, ?2, 1, 'RCtrl+Space', ?3, ?3, 'complete', 1000, 'kg-note', \
                 ?4, ?5, 'deadbeef')",
        rusqlite::params![id, uuid, started_at, format!("entry-{id}"), vault_path],
    )
    .unwrap();
}

#[test]
fn snapshot_returns_empty_collections_on_fresh_db() {
    let conn = fresh_db();
    let snap = snapshot_from_db(&conn).unwrap();
    assert!(snap.sources.is_empty());
    assert!(snap.entities.is_empty());
    assert!(snap.projects.is_empty());
    assert!(snap.tags.is_empty());
}

#[test]
fn snapshot_picks_up_filed_sources_in_recent_first_order() {
    let conn = fresh_db();
    insert_filed_session(
        &conn,
        1,
        "uuid-1",
        "2026-06-10T10:00:00Z",
        "Knowledge Graph/Entries/2026-06-10-older__01.md",
    );
    insert_filed_session(
        &conn,
        2,
        "uuid-2",
        "2026-06-15T10:00:00Z",
        "Knowledge Graph/Entries/2026-06-15-newer__02.md",
    );
    let snap = snapshot_from_db(&conn).unwrap();
    assert_eq!(snap.sources.len(), 2);
    assert!(
        snap.sources[0].vault_path.contains("newer"),
        "newest source must lead; got {:?}",
        snap.sources[0]
    );
    assert!(snap.sources[1].vault_path.contains("older"));
}

#[test]
fn snapshot_excludes_sessions_without_vault_path() {
    let conn = fresh_db();
    // Filed -> in INDEX.
    insert_filed_session(
        &conn,
        1,
        "uuid-1",
        "2026-06-15T10:00:00Z",
        "Knowledge Graph/Entries/x__01.md",
    );
    // Standard dictation: no vault_path -> NOT in INDEX.
    conn.execute(
        "INSERT INTO sessions (id, uuid, mode_id, hotkey_pressed, started_at, \
         recording_ended_at, status, audio_duration_ms, capture_kind) \
         VALUES (2, 'uuid-2', 1, 'RCtrl+Space', '2026-06-15T11:00:00Z', \
                 '2026-06-15T11:00:01Z', 'complete', 1000, 'dictation')",
        [],
    )
    .unwrap();
    let snap = snapshot_from_db(&conn).unwrap();
    assert_eq!(snap.sources.len(), 1, "only filed entries surface");
}

#[test]
fn snapshot_pulls_entities_and_projects_alphabetically() {
    let conn = fresh_db();
    let now = "2026-06-15T10:00:00Z";
    for (name, kind) in [
        ("Mockingbird", "project"),
        ("Acme Corp", "organization"),
        ("Mom", "person"),
    ] {
        conn.execute(
            "INSERT INTO kg_entities (name, entity_type, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?3)",
            rusqlite::params![name, kind, now],
        )
        .unwrap();
    }
    let snap = snapshot_from_db(&conn).unwrap();
    assert_eq!(
        snap.entities,
        vec![
            "Acme Corp".to_string(),
            "Mockingbird".to_string(),
            "Mom".to_string(),
        ]
    );
    assert_eq!(snap.projects, vec!["Mockingbird".to_string()]);
}

#[test]
fn snapshot_distinct_tags_alphabetical() {
    let conn = fresh_db();
    insert_filed_session(
        &conn,
        1,
        "uuid-1",
        "2026-06-15T10:00:00Z",
        "Knowledge Graph/Entries/x__01.md",
    );
    let now = "2026-06-15T10:00:00Z";
    for (seg, slug) in [(0, "phase-1e"), (1, "grocery"), (2, "grocery")] {
        conn.execute(
            "INSERT INTO kg_tag_mentions (entry_id, segment_idx, tag_slug, created_at) \
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![1_i64, seg as i64, slug, now],
        )
        .unwrap();
    }
    let snap = snapshot_from_db(&conn).unwrap();
    assert_eq!(
        snap.tags,
        vec!["grocery".to_string(), "phase-1e".to_string()]
    );
}

#[test]
fn rebuild_writes_index_md_atomically_and_preserves_concepts() {
    let td = tempfile::TempDir::new().unwrap();
    crate::vault::kg_layout::bootstrap_kg_subtree(td.path()).unwrap();
    crate::vault::kg_layout::bootstrap_kg_root_files(td.path()).unwrap();

    // Seed a chat-LLM-style Concepts section.
    let r = crate::vault::kg_layout::kg_root_file_paths(td.path());
    let with_concepts = "# INDEX\n\n## Sources\n\n## Entities\n\n## Projects\n\n## Tags\n\n## Concepts\n\n- [[Concepts/karpathy-llm-wiki|Karpathy LLM Wiki]]\n";
    fs::write(&r.index_md, with_concepts).unwrap();

    let conn = fresh_db();
    insert_filed_session(
        &conn,
        1,
        "uuid-1",
        "2026-06-15T10:00:00Z",
        "Knowledge Graph/Entries/2026-06-15-buy-milk__abcd1234.md",
    );

    let outcome = rebuild_index_md(&conn, td.path()).unwrap();
    assert_eq!(outcome.sources_emitted, 1);
    assert!(outcome.concepts_preserved, "Concepts must be preserved");

    let bytes = fs::read(&r.index_md).unwrap();
    let s = String::from_utf8(bytes).unwrap();
    assert!(s.contains("Karpathy LLM Wiki"), "Concepts body lost: {s}");
    assert!(s.contains("buy-milk"));
    assert!(!s.contains('\r'), "INDEX.md must be LF-only");

    // No leftover .mb-tmp.
    let mut tmp = r.index_md.as_os_str().to_owned();
    tmp.push(".mb-tmp");
    assert!(
        !std::path::PathBuf::from(tmp).exists(),
        "atomic write must clean up its tmp"
    );
}
