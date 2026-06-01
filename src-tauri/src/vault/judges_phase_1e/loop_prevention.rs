//! J1 - `kg-reverse-watcher-loop-prevention` (Wave 1E.9 / `mb-kazi`).
//!
//! Asserts the **hash-equality short-circuit** in
//! `vault::watcher_reconcile::reconcile_entry_file`:
//!
//! - Our own writes -- where `sessions.vault_file_hash` was
//!   pre-recorded BEFORE the file landed -- must surface as
//!   `ReconcileOutcome::LoopPrevented` and write nothing.
//! - External edits (where the file's SHA-256 diverges from the
//!   recorded hash) must surface as `ReconcileOutcome::Reconciled`
//!   and update the DB's mention rows + refresh the hash.
//!
//! Why this matters: without the short-circuit, the writer's
//! `fs::write` would fire an OS-level modify event, the watcher
//! would reconcile, the reconcile would re-record the hash, the
//! next reconcile would re-fire... etc. Hash equality is the
//! discriminator. Tested here in a single-process probe so a
//! regression to the discriminator is caught at seal time.
//!
//! Spec: `docs/phases/phase-1e.md` §"Wave 1E.9" + ADR 0053 §D5
//! (loop-prevention). The judge complements the unit tests in
//! `vault::watcher_reconcile` (which can't be exercised on this
//! box under `cargo test --release`; LESSONS P2).

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

const ENTRY_ID: &str = "loop-prev-0001";

/// Run J1 - reverse-watcher loop-prevention probe.
///
/// Returns `0` on green (loop-back is short-circuited AND external
/// edits are reconciled), `1` on any assertion failure.
pub fn run_reverse_watcher_loop_prevention_probe() -> i32 {
    println!("J1 - kg-reverse-watcher-loop-prevention (Wave 1E.9 / mb-kazi)");

    match run_inner() {
        Ok(()) => {
            println!();
            println!(
                "J1 GREEN: own writes -> LoopPrevented (0 mention rows touched); external edit -> Reconciled (mention rows refreshed)"
            );
            0
        }
        Err(e) => {
            eprintln!();
            eprintln!("J1 FAILED");
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

    // Stand up a DB with all migrations applied.
    let db_file = td.path().join("mockingbird.sqlite");
    let conn = Connection::open(&db_file).map_err(|e| io_err(format!("open db: {e}")))?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .map_err(|e| io_err(format!("enable FKs: {e}")))?;
    apply_all(&conn)?;

    // Craft an entry + serialize it. The bytes on disk are the
    // exact bytes the writer would produce, so we can pre-record
    // their hash into the sessions row to simulate the writer's
    // "pre-record-then-write" sequence.
    let initial_entry = sample_entry(&["initial-tag"], &["Acme Corp"]);
    let initial_bytes = serialize_entry(&initial_entry);
    let initial_hash = sha256_hex(initial_bytes.as_bytes());
    let filename = filename_for(&initial_entry);
    let entry_path = entries_dir.join(&filename);

    // Sessions row: pre-recorded `vault_file_hash` matches the
    // bytes we're about to land on disk.
    let vault_rel_path = format!("Knowledge Graph/Entries/{filename}");
    conn.execute(
        "INSERT INTO sessions (id, uuid, mode_id, hotkey_pressed, started_at, \
         recording_ended_at, status, audio_duration_ms, capture_kind, \
         entry_id, vault_path, vault_file_hash) \
         VALUES (1, 'loop-prev-uuid', 1, 'RCtrl+Space', '2026-06-08T10:00:00Z', \
                 '2026-06-08T10:00:01Z', 'complete', 1000, 'kg-note', \
                 ?1, ?2, ?3)",
        params![ENTRY_ID, vault_rel_path, initial_hash],
    )
    .map_err(|e| io_err(format!("insert sessions: {e}")))?;

    // Land the file.
    std::fs::write(&entry_path, &initial_bytes).map_err(io_err)?;

    // --- Case A: our own write loops back through the watcher. ---
    let outcome_a = reconcile_entry_file(&entry_path, &kg_root, &conn)?;
    if outcome_a != ReconcileOutcome::LoopPrevented {
        return Err(other(&format!(
            "case A (loop-back from own write): expected LoopPrevented, got {outcome_a:?}"
        )));
    }
    // Mention tables must be empty -- the reconciler must not have
    // even reached the delete-and-reinsert step.
    let tag_rows = count(&conn, "kg_tag_mentions")?;
    let entity_rows = count(&conn, "kg_entity_mentions")?;
    if tag_rows != 0 || entity_rows != 0 {
        return Err(other(&format!(
            "case A: LoopPrevented short-circuit leaked writes ({tag_rows} tag rows, {entity_rows} entity rows; both must be 0)"
        )));
    }
    // Hash must be unchanged.
    let hash_after_a = read_recorded_hash(&conn)?;
    if hash_after_a.as_deref() != Some(initial_hash.as_str()) {
        return Err(other(&format!(
            "case A: recorded hash mutated by loop-prevented path (was {initial_hash}, now {hash_after_a:?})"
        )));
    }
    println!("    case A (own write): LoopPrevented + 0 mention rows + hash unchanged");

    // --- Case B: external edit. ---
    //
    // Simulate the user editing the file in Obsidian: change the
    // tags + entities + body. The file's SHA-256 now diverges from
    // the recorded hash, so the discriminator must route this to
    // the Reconciled branch.
    let edited_entry = sample_entry(&["edited-tag", "second-tag"], &["Acme Corp", "Mom"]);
    let edited_bytes = serialize_entry(&edited_entry);
    let edited_hash = sha256_hex(edited_bytes.as_bytes());
    if edited_hash == initial_hash {
        return Err(other(
            "test setup invariant broken: edited entry serializes to the same hash as initial",
        ));
    }
    std::fs::write(&entry_path, &edited_bytes).map_err(io_err)?;

    let outcome_b = reconcile_entry_file(&entry_path, &kg_root, &conn)?;
    match outcome_b {
        ReconcileOutcome::Reconciled {
            session_id,
            tag_count,
            entity_count,
        } => {
            if session_id != 1 {
                return Err(other(&format!(
                    "case B: reconciled session_id={session_id} (expected 1)"
                )));
            }
            if tag_count != 2 || entity_count != 2 {
                return Err(other(&format!(
                    "case B: tag_count={tag_count} entity_count={entity_count} (expected 2 + 2)"
                )));
            }
        }
        unexpected => {
            return Err(other(&format!(
                "case B (external edit): expected Reconciled, got {unexpected:?}"
            )))
        }
    }

    // Mention rows now reflect the FILE's tags/entities, not the
    // (empty) state before. J2 covers this property in more depth;
    // here we just sanity-check the side effect happened.
    let tag_rows = count(&conn, "kg_tag_mentions")?;
    let entity_rows = count(&conn, "kg_entity_mentions")?;
    if tag_rows != 2 || entity_rows != 2 {
        return Err(other(&format!(
            "case B: mention rows after reconcile -- got {tag_rows} tags, {entity_rows} entities (expected 2 + 2)"
        )));
    }

    // Recorded hash must now match the edited bytes (so a follow-on
    // reconcile of THIS state would loop-prevent in turn).
    let hash_after_b = read_recorded_hash(&conn)?;
    if hash_after_b.as_deref() != Some(edited_hash.as_str()) {
        return Err(other(&format!(
            "case B: hash not refreshed to edited file's sha (expected {edited_hash}, got {hash_after_b:?})"
        )));
    }
    println!(
        "    case B (external edit): Reconciled + 2 tag rows + 2 entity rows + hash refreshed"
    );

    // --- Case C: re-fire on the now-stable state. The hash matches ---
    // again, so a SECOND watcher event for the same bytes must
    // loop-prevent. This is the "stable fixed-point" property.
    let outcome_c = reconcile_entry_file(&entry_path, &kg_root, &conn)?;
    if outcome_c != ReconcileOutcome::LoopPrevented {
        return Err(other(&format!(
            "case C (re-fire on stable bytes): expected LoopPrevented, got {outcome_c:?}"
        )));
    }
    let tag_rows_c = count(&conn, "kg_tag_mentions")?;
    let entity_rows_c = count(&conn, "kg_entity_mentions")?;
    if tag_rows_c != 2 || entity_rows_c != 2 {
        return Err(other(&format!(
            "case C: mention rows mutated by loop-prevented re-fire ({tag_rows_c} tags, {entity_rows_c} entities; expected steady 2+2)"
        )));
    }
    println!("    case C (re-fire on stable state): LoopPrevented + mention rows steady");

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
        title: "Loop-prevention probe".to_string(),
        category: Category::Personal,
        entry_type: EntryType::Note,
        status: None,
        due_date: None,
        tags: tags.iter().map(|s| (*s).to_string()).collect(),
        entities: entities.iter().map(|s| (*s).to_string()).collect(),
        source_session_uuid: None,
        body: "Body content for J1.".to_string(),
    }
}

fn count(conn: &Connection, table: &str) -> AppResult<i64> {
    let sql = format!("SELECT COUNT(*) FROM {table}");
    let n: i64 = conn
        .query_row(&sql, [], |r| r.get(0))
        .map_err(|e| io_err(format!("count {table}: {e}")))?;
    Ok(n)
}

fn read_recorded_hash(conn: &Connection) -> AppResult<Option<String>> {
    conn.query_row(
        "SELECT vault_file_hash FROM sessions WHERE id = 1",
        [],
        |r| r.get::<_, Option<String>>(0),
    )
    .map_err(|e| io_err(format!("read hash: {e}")))
}

fn io_err<D: std::fmt::Display>(e: D) -> AppError {
    AppError::Other(format!("io: {e}"))
}

fn other(msg: &str) -> AppError {
    AppError::Other(msg.to_string())
}
