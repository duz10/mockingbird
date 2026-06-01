//! J2 - `kg-file-wins-on-conflict` (Wave 1E.9 / `mb-kazi`).
//!
//! Asserts the **file-canonical** half of ADR 0053's
//! "Obsidian as source of truth" contract: when a Markdown file
//! under `Entries/` and the DB's mention rows disagree, the file
//! wins after reconciliation -- the DB rows are deleted and
//! re-derived from the file's frontmatter.
//!
//! Concretely: pre-seed `kg_tag_mentions` + `kg_entity_mentions`
//! with values that DON'T appear in the file. Run
//! `reconcile_entry_file`. Confirm:
//!
//! - The pre-seeded rows are gone.
//! - New rows reflect the file's frontmatter.
//! - The recorded `vault_file_hash` is the file's SHA-256
//!   (so the next watcher event short-circuits via J1's
//!   discriminator).
//!
//! J1 catches the loop-prevention discriminator; J2 catches the
//! delete-and-reinsert that runs when the discriminator routes
//! into the reconcile branch. Together they pin down the entire
//! reverse-watcher contract.
//!
//! Spec: `docs/phases/phase-1e.md` §"Wave 1E.9" + ADR 0053
//! §"Acceptance gates".

use chrono::{TimeZone, Utc};
use rusqlite::{params, Connection};
use tempfile::TempDir;

use crate::db::migrations::apply_all;
use crate::error::{AppError, AppResult};
use crate::vault::kg_layout::{bootstrap_kg_root_files, bootstrap_kg_subtree, kg_subtree_paths};
use crate::vault::markdown_serializer::{
    filename_for, serialize_entry, CaptureKind, Category, EntryType, KgEntry,
};
use crate::vault::project::sha256_hex;
use crate::vault::watcher_reconcile::{reconcile_entry_file, ReconcileOutcome};

const ENTRY_ID: &str = "file-wins-0001";
const STALE_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// Run J2 - file-wins-on-conflict probe.
///
/// Returns `0` on green (DB mention rows after reconcile mirror
/// the FILE, not the pre-seed), `1` on any assertion failure.
pub fn run_file_wins_on_conflict_probe() -> i32 {
    println!("J2 - kg-file-wins-on-conflict (Wave 1E.9 / mb-kazi)");

    match run_inner() {
        Ok(()) => {
            println!();
            println!(
                "J2 GREEN: pre-seeded DB rows replaced wholesale; new rows mirror the file's frontmatter; hash refreshed"
            );
            0
        }
        Err(e) => {
            eprintln!();
            eprintln!("J2 FAILED");
            eprintln!("    reason: {e}");
            1
        }
    }
}

fn run_inner() -> AppResult<()> {
    let td = TempDir::new().map_err(io_err)?;
    let vault = td.path().to_path_buf();
    bootstrap_kg_subtree(&vault)?;
    bootstrap_kg_root_files(&vault)?;
    let entries_dir = kg_subtree_paths(&vault).entries;
    let kg_root = vault.join("Knowledge Graph");

    let db_file = td.path().join("mockingbird.sqlite");
    let conn = Connection::open(&db_file).map_err(|e| io_err(format!("open db: {e}")))?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .map_err(|e| io_err(format!("enable FKs: {e}")))?;
    apply_all(&conn)?;

    // The file's frontmatter -- what the user "has now". This is
    // the canonical truth that reconcile must converge to.
    let file_tags = vec!["green-tag-one", "green-tag-two", "green-tag-three"];
    // Entities here are the ALREADY-SLUGGED form, which is what
    // ends up on disk after the serializer's
    // `[[Entities/<slug>|<slug>]]` round-trip + parser strip. The
    // reverse-watcher stores the slug in `kg_entity_mentions.surface_form`
    // because that's all the file recovers; canonical names live
    // in `kg_entities`. Using the already-slugged form keeps the
    // assertion below trivial -- no second slugify call inside the
    // judge.
    let file_entities = vec!["acme-corp", "mom", "mockingbird"];
    let entry = sample_entry(&file_tags, &file_entities);
    let bytes = serialize_entry(&entry);
    let filename = filename_for(&entry);
    let entry_path = entries_dir.join(&filename);
    std::fs::write(&entry_path, &bytes).map_err(io_err)?;

    // Sessions row: hash deliberately STALE (all-zero) so the
    // discriminator routes into the reconcile branch.
    let vault_rel_path = format!("Knowledge Graph/Entries/{filename}");
    conn.execute(
        "INSERT INTO sessions (id, uuid, mode_id, hotkey_pressed, started_at, \
         recording_ended_at, status, audio_duration_ms, capture_kind, \
         entry_id, vault_path, vault_file_hash) \
         VALUES (1, 'file-wins-uuid', 1, 'RCtrl+Space', '2026-06-08T10:00:00Z', \
                 '2026-06-08T10:00:01Z', 'complete', 1000, 'kg-note', \
                 ?1, ?2, ?3)",
        params![ENTRY_ID, vault_rel_path, STALE_HASH],
    )
    .map_err(|e| io_err(format!("insert sessions: {e}")))?;

    // Pre-seed mention rows with values that DON'T appear in the
    // file. These are what an out-of-date DB looks like.
    let now = "2026-06-08T09:55:00Z";
    for (idx, slug) in ["stale-db-tag-a", "stale-db-tag-b"].iter().enumerate() {
        conn.execute(
            "INSERT INTO kg_tag_mentions (entry_id, segment_idx, tag_slug, created_at) \
             VALUES (?1, ?2, ?3, ?4)",
            params![1_i64, idx as i64, slug, now],
        )
        .map_err(|e| io_err(format!("seed tag mention: {e}")))?;
    }
    // Pre-seed an entity row that the file will NOT reference,
    // plus an associated mention row. The entity row should
    // SURVIVE (entities are a separate canonical set; we only
    // refresh mentions). The mention row should be deleted.
    conn.execute(
        "INSERT INTO kg_entities (name, entity_type, created_at, updated_at) \
         VALUES ('Stale Person', 'person', ?1, ?1)",
        params![now],
    )
    .map_err(|e| io_err(format!("seed kg_entities: {e}")))?;
    let stale_entity_id: i64 = conn
        .query_row(
            "SELECT id FROM kg_entities WHERE name = 'Stale Person'",
            [],
            |r| r.get(0),
        )
        .map_err(|e| io_err(format!("read seeded entity id: {e}")))?;
    conn.execute(
        "INSERT INTO kg_entity_mentions (entry_id, entity_id, segment_idx, surface_form, created_at) \
         VALUES (?1, ?2, 0, 'Stale Person', ?3)",
        params![1_i64, stale_entity_id, now],
    )
    .map_err(|e| io_err(format!("seed entity mention: {e}")))?;

    // Sanity check the pre-seed.
    if count(&conn, "kg_tag_mentions")? != 2 {
        return Err(other("pre-seed: expected 2 stale tag mention rows"));
    }
    if count(&conn, "kg_entity_mentions")? != 1 {
        return Err(other("pre-seed: expected 1 stale entity mention row"));
    }

    // Reconcile.
    let outcome = reconcile_entry_file(&entry_path, &kg_root, &conn)?;
    match outcome {
        ReconcileOutcome::Reconciled {
            session_id,
            tag_count,
            entity_count,
        } => {
            if session_id != 1 {
                return Err(other(&format!(
                    "reconcile: unexpected session_id {session_id}"
                )));
            }
            if tag_count != file_tags.len() || entity_count != file_entities.len() {
                return Err(other(&format!(
                    "reconcile: counts diverge from file -- got tag_count={tag_count}, entity_count={entity_count}; wanted {} + {}",
                    file_tags.len(),
                    file_entities.len()
                )));
            }
        }
        unexpected => {
            return Err(other(&format!(
                "reconcile: expected Reconciled, got {unexpected:?}"
            )))
        }
    }

    // 1. Tag mentions now mirror the FILE.
    let actual_tags = read_tag_slugs(&conn)?;
    let mut expected_tags: Vec<String> = file_tags.iter().map(|s| s.to_string()).collect();
    expected_tags.sort();
    let mut actual_tags_sorted = actual_tags.clone();
    actual_tags_sorted.sort();
    if actual_tags_sorted != expected_tags {
        return Err(other(&format!(
            "tag mentions diverged from file: got {actual_tags_sorted:?}, wanted {expected_tags:?}"
        )));
    }
    // Stale rows are gone.
    if actual_tags.iter().any(|t| t.starts_with("stale-db-tag")) {
        return Err(other(
            "stale-db-tag-* rows survived reconcile (file should win)",
        ));
    }

    // 2. Entity mentions now mirror the FILE.
    let actual_entities = read_entity_mention_texts(&conn)?;
    let mut expected_entities: Vec<String> = file_entities.iter().map(|s| s.to_string()).collect();
    expected_entities.sort();
    let mut actual_entities_sorted = actual_entities.clone();
    actual_entities_sorted.sort();
    if actual_entities_sorted != expected_entities {
        return Err(other(&format!(
            "entity mentions diverged from file: got {actual_entities_sorted:?}, wanted {expected_entities:?}"
        )));
    }
    // The pre-seeded "Stale Person" mention is gone, but the
    // kg_entities row itself survives (canonical set is not
    // mutated by mention churn).
    let stale_entity_still_exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM kg_entities WHERE name = 'Stale Person'",
            [],
            |r| r.get(0),
        )
        .map_err(|e| io_err(format!("recount stale entity: {e}")))?;
    if stale_entity_still_exists != 1 {
        return Err(other(
            "kg_entities canonical row for 'Stale Person' was mutated by reconcile (entities are a separate canonical set; only mentions should churn)",
        ));
    }

    // 3. Hash refreshed to the file's actual SHA -- this is what
    // makes the next watcher event loop-prevent (J1's case C).
    let file_hash = sha256_hex(bytes.as_bytes());
    let recorded: Option<String> = conn
        .query_row(
            "SELECT vault_file_hash FROM sessions WHERE id = 1",
            [],
            |r| r.get(0),
        )
        .map_err(|e| io_err(format!("read recorded hash: {e}")))?;
    if recorded.as_deref() != Some(file_hash.as_str()) {
        return Err(other(&format!(
            "recorded hash not refreshed to file's sha (expected {file_hash}, got {recorded:?})"
        )));
    }

    println!(
        "    OK: {} tag mentions + {} entity mentions mirror the file; stale DB rows deleted; canonical entity preserved; hash refreshed",
        actual_tags.len(),
        actual_entities.len()
    );
    Ok(())
}

// ----------------------------------------------------------------
// Helpers
// ----------------------------------------------------------------

fn sample_entry(tags: &[&str], entities: &[&str]) -> KgEntry {
    let ts = Utc.with_ymd_and_hms(2026, 6, 8, 10, 0, 0).unwrap();
    KgEntry {
        id: ENTRY_ID.to_string(),
        captured_at: ts,
        captured_at_local_date: ts.date_naive(),
        capture_kind: CaptureKind::KgNote,
        title: "File-wins-on-conflict probe".to_string(),
        category: Category::Personal,
        entry_type: EntryType::Note,
        status: None,
        due_date: None,
        tags: tags.iter().map(|s| (*s).to_string()).collect(),
        entities: entities.iter().map(|s| (*s).to_string()).collect(),
        source_session_uuid: None,
        body: "Body content for J2.".to_string(),
    }
}

fn count(conn: &Connection, table: &str) -> AppResult<i64> {
    let sql = format!("SELECT COUNT(*) FROM {table}");
    conn.query_row(&sql, [], |r| r.get(0))
        .map_err(|e| io_err(format!("count {table}: {e}")))
}

fn read_tag_slugs(conn: &Connection) -> AppResult<Vec<String>> {
    let mut stmt = conn
        .prepare("SELECT tag_slug FROM kg_tag_mentions ORDER BY id")
        .map_err(|e| io_err(format!("prepare tag-slugs: {e}")))?;
    let rows = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|e| io_err(format!("query tag-slugs: {e}")))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| io_err(format!("row tag-slug: {e}")))?);
    }
    Ok(out)
}

fn read_entity_mention_texts(conn: &Connection) -> AppResult<Vec<String>> {
    let mut stmt = conn
        .prepare("SELECT surface_form FROM kg_entity_mentions ORDER BY id")
        .map_err(|e| io_err(format!("prepare entity-mentions: {e}")))?;
    let rows = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|e| io_err(format!("query entity-mentions: {e}")))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| io_err(format!("row entity-mention: {e}")))?);
    }
    Ok(out)
}

fn io_err<D: std::fmt::Display>(e: D) -> AppError {
    AppError::Other(format!("io: {e}"))
}

fn other(msg: &str) -> AppError {
    AppError::Other(msg.to_string())
}
