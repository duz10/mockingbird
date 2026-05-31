//! KG vault projection writer -- Phase 1E Wave 1E.3 (`mb-k2pk`,
//! ADR 0053 §D4 + §D5).
//!
//! Owns the two-phase commit that takes one filed KG entry from
//! "row exists in `kg_entries` / `kg_entity_mentions` /
//! `kg_tag_mentions`" all the way to "byte-identical markdown file
//! on disk in `<vault>/Knowledge Graph/Entries/` with a recorded
//! SHA-256 hash that the reverse-watcher (1E.5) will key off for
//! loop-prevention".
//!
//! ## Two-phase commit ordering (ADR 0053 §D4, verbatim)
//!
//! For one filed entry, the worker call sequence is:
//!
//! 1. **DB-first** (`kg::store::apply_filed_outcome` -- already shipped
//!    by Wave 1B Chunk 2): insert/upsert entity + mention rows. Owned
//!    by the *caller* (`kg::worker::process_one`); this module assumes
//!    those rows are durable before [`commit_entry_to_vault`] is
//!    called.
//! 2. **Pre-hash**: compute SHA-256 of the serialized markdown bytes.
//!    Pure -- [`compute_artifact`].
//! 3. **DB-record-hash**: `UPDATE sessions.vault_file_hash = <hex>` in
//!    its own committed transaction. After this returns Ok, the hash
//!    is durable and the 1E.5 reverse-watcher will refuse to re-ingest
//!    a file matching this hash. CRITICAL: this MUST commit before
//!    step 4 -- the watcher race window slams shut here, not later.
//! 4. **File-write**: write to a temp sibling + atomic rename. If
//!    this fails, the row stays in the "hash set, paths NULL"
//!    reconcile signature -- safe; the next worker tick / nightly
//!    sweep can retry.
//! 5. **DB-seal**: `UPDATE sessions.entry_id = <uuid>,
//!    vault_path = <relative>` in a second transaction together with
//!    [`super::super::kg::store::queue::mark_done`] (the caller wraps
//!    both in one `tx.commit()`).
//! 6. **Queue-seal** is folded into step 5's transaction.
//!
//! ## Failure modes + reconcile signature (ADR 0053 §D4)
//!
//! | Crash between | DB state                                    | Disk state | Detected by                | Recovery                     |
//! |---------------|---------------------------------------------|------------|----------------------------|------------------------------|
//! | step 2 / 3    | clean: no hash, no paths                    | no file    | queue stays `processing`   | boot sweep -> requeue        |
//! | step 3 / 4    | hash set, paths NULL                        | no file    | [`reconcile_vault`] scan   | re-write file, then seal     |
//! | step 4 / 5    | hash set, paths NULL                        | file on disk (matches hash) | [`reconcile_vault`] scan | seal entry_id + path from filename suffix |
//! | step 5 / 6    | n/a -- folded into the same transaction     | n/a        | n/a                        | n/a                          |
//!
//! `reconcile_vault` is the on-demand sweep ([`reconcile_vault`]); a
//! timer-driven nightly variant is deferred to a P3 bead.
//!
//! ## Toggle-off / vault-unconfigured handling
//!
//! Neither check lives here -- [`commit_entry_to_vault`] requires a
//! caller-resolved absolute vault path. The worker
//! (`kg::worker::process_one`) is responsible for short-circuiting on
//! `KgGraphEnabled = false` or `VaultPath` empty BEFORE calling this
//! module; that keeps `writer.rs` pure and testable against a
//! `TempDir` (LESSONS P2).
//!
//! ## Why a sibling module to `vault::project` (not part of it)
//!
//! `vault::project` owns the ADR 0046 outbound projection
//! (vault-as-disposable-projection-of-DB). `vault::writer` owns the
//! ADR 0053 outbound projection (vault-as-source-of-truth bootstrap).
//! Same direction of data flow, opposite source-of-truth axis. Mixing
//! the two on one module would silently couple them; keep them
//! sibling so the boundary is visible.

#![allow(missing_docs)]
// Module is gated behind the KG toggle in production; some helpers
// (`reconcile_vault`, the failure-mode internals) compile to
// `dead_code` until 1E.5 + the IPC layer wire them up. Same pattern
// as `kg::worker`'s `#![allow(dead_code)]`.
#![allow(dead_code)]

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use sha2::{Digest, Sha256};

use crate::db::sessions;
use crate::error::{AppError, AppResult};

use super::kg_layout::{kg_subtree_paths, KG_ENTRIES_NAME, KG_SUBTREE_ROOT_NAME};
use super::markdown_serializer::{filename_for, serialize_entry, KgEntry};

/// Output of a successful two-phase commit. Returned by
/// [`commit_entry_to_vault`] so the caller (`kg::worker`) can log
/// structured fields + the future reverse-watcher's test harness can
/// inspect what landed where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitOutcome {
    /// Absolute on-disk path of the written file.
    pub absolute_path: PathBuf,
    /// Vault-relative POSIX-style path (what gets recorded in
    /// `sessions.vault_path`). Always uses `/` separators regardless
    /// of host OS so the value is portable across an Obsidian Sync
    /// vault opened on a different machine.
    pub vault_relative_path: String,
    /// Lowercase hex SHA-256 of the bytes written.
    pub file_hash: String,
    /// The entry id sealed into `sessions.entry_id` (same as
    /// `entry.id` -- echoed for caller convenience).
    pub entry_id: String,
}

/// Pure helper: compute the (absolute_path, bytes, hex_hash) triple
/// the two-phase commit needs WITHOUT touching disk or DB.
///
/// Split out from [`commit_entry_to_vault`] for two reasons:
///
/// 1. Testability against a `TempDir` without spinning up SQLite.
/// 2. The future reverse-watcher (1E.5) needs the same hash function
///    on file-event inbound, and a single source of truth for the
///    "compute SHA-256 of the canonical bytes" formula avoids drift.
///
/// `vault_root` is the user's vault path (e.g. `C:\Users\foo\Vault`).
/// The `Knowledge Graph/Entries/` subpath is appended internally so
/// callers can't accidentally hand a path that bypasses the subtree
/// layout.
pub fn compute_artifact(entry: &KgEntry, vault_root: &Path) -> (PathBuf, Vec<u8>, String) {
    let subtree = kg_subtree_paths(vault_root);
    let filename = filename_for(entry);
    let absolute_path = subtree.entries.join(&filename);
    let body = serialize_entry(entry).into_bytes();
    let mut hasher = Sha256::new();
    hasher.update(&body);
    let hash_hex = format!("{:x}", hasher.finalize());
    (absolute_path, body, hash_hex)
}

/// Build the vault-relative POSIX-style path string for an entry.
/// Pure -- mirrors [`compute_artifact`] but cheaper when the caller
/// only needs the relative form (e.g. reconcile).
pub fn vault_relative_path_for(entry: &KgEntry) -> String {
    // Always emit forward slashes; the value travels to other
    // machines via Obsidian Sync and a literal `\` would mis-parse
    // on macOS / iOS.
    let filename = filename_for(entry);
    format!("{KG_SUBTREE_ROOT_NAME}/{KG_ENTRIES_NAME}/{filename}")
}

/// Run steps 2 -> 4 of the two-phase commit (pre-hash + DB-record-
/// hash + atomic file write). Returns [`CommitOutcome`] so the
/// caller (`kg::worker`) can fold the step-5 seal + step-6
/// `mark_done` into ONE transaction.
///
/// Splitting the seal out of this function (vs. doing it inline
/// with its own UPDATE here) preserves the atomicity contract:
/// "file exists on disk" + "queue marked done" + "sessions row
/// sealed" all flip together, or none do. If we sealed inline,
/// a crash between seal and `mark_done` would leave a sealed
/// session with a `processing` queue row -- which the boot sweep
/// would then re-claim, triggering a redundant pipeline run.
/// Folding seal+done into the caller's outer txn makes that race
/// impossible.
///
/// On failure the DB is left in one of the reconcile signatures
/// documented in the module docs -- the caller MUST NOT mark the
/// queue row `done`.
///
/// Idempotency: re-calling after a partial success is safe.
/// Step 3 records the same hash (no-op UPDATE). Step 4 overwrites
/// an existing file with identical bytes.
pub fn commit_entry_to_vault(
    conn: &Connection,
    session_id: i64,
    entry: &KgEntry,
    vault_root: &Path,
) -> AppResult<CommitOutcome> {
    // Step 2 -- pre-hash. Pure; failure here would be impossible
    // unless the serializer panicked, which is a contract bug we
    // want to surface uncaught.
    let (absolute_path, body, hash_hex) = compute_artifact(entry, vault_root);
    let vault_relative_path = vault_relative_path_for(entry);

    // Ensure the destination directory exists. Idempotent; the
    // subtree was already created by `bootstrap_kg_subtree` at
    // toggle-on time, but a user who deleted the folder mid-session
    // shouldn't lose the entry. `create_dir_all` is a no-op on the
    // happy path.
    if let Some(parent) = absolute_path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            AppError::Vault(format!(
                "commit_entry_to_vault: failed to ensure entries dir {} -- {}",
                parent.display(),
                e
            ))
        })?;
    }

    // Step 3 -- DB-record-hash. Auto-committed UPDATE so the durable
    // hash record exists BEFORE the file appears on disk (closes
    // the reverse-watcher race per ADR 0053 §D5).
    sessions::record_vault_hash(conn, session_id, &hash_hex).map_err(|e| {
        AppError::Vault(format!(
            "commit_entry_to_vault: record_vault_hash failed for session_id={session_id} -- {e}"
        ))
    })?;

    // Step 4 -- atomic file write. Write to a temp sibling in the
    // same directory + rename. Same-directory rename is atomic on
    // both NTFS and APFS for our size class. If the rename fails
    // the row reaches "hash set, paths NULL" which
    // [`reconcile_vault`] will pick up.
    write_atomic(&absolute_path, &body).map_err(|e| {
        AppError::Vault(format!(
            "commit_entry_to_vault: atomic write to {} failed -- {}",
            absolute_path.display(),
            e
        ))
    })?;

    // Step 5 is owned by the caller (folded into its outer txn).
    // We DO NOT call `seal_vault_filing` here.

    Ok(CommitOutcome {
        absolute_path,
        vault_relative_path,
        file_hash: hash_hex,
        entry_id: entry.id.clone(),
    })
}

/// Atomic file write: serialize to a temp sibling + rename.
///
/// Sibling-rename is atomic on both NTFS and APFS for files of this
/// size class (single-digit KB markdown). We *don't* fsync the
/// directory afterwards -- it adds 5-50ms per write on Windows and
/// the durability we care about is "the file exists OR doesn't",
/// not "the file survives a kernel panic 100ms later". Obsidian
/// Sync's own ingest already debounces by a few seconds.
///
/// The temp filename uses the target filename with a `.mb-tmp`
/// suffix (NOT a random uuid) so a crash-leaked temp is recognizable
/// to a human + to `reconcile_vault`. Multiple concurrent writes to
/// the same final path are not supported -- the worker is
/// single-threaded by design.
fn write_atomic(target: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut tmp = target.as_os_str().to_owned();
    tmp.push(".mb-tmp");
    let tmp_path = PathBuf::from(tmp);

    // Write the temp file. If a stale `.mb-tmp` exists from a prior
    // crash, overwrite it -- the worker is single-threaded so we
    // can't be racing another writer.
    fs::write(&tmp_path, bytes)?;

    // Rename onto the target. `rename` is atomic on the same
    // filesystem; cross-filesystem moves are not a concern because
    // the temp sibling is in the same directory by construction.
    fs::rename(&tmp_path, target)?;
    Ok(())
}

/// Report of an on-demand reconcile pass.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ReconcileReport {
    /// Sessions that had `vault_file_hash` set but no `vault_path`
    /// AND the expected file was missing on disk. Recovery: re-run
    /// the writer for this session (out-of-scope for this wave's
    /// `reconcile_vault`; logged for the operator).
    pub missing_file_count: usize,
    /// Sessions where the on-disk file matched the recorded hash
    /// AND `vault_path` was NULL. Recovery: seal the row by
    /// recovering filename + entry_id from disk.
    pub sealed_count: usize,
    /// Files on disk that exist in `Knowledge Graph/Entries/` but
    /// whose hash is not recorded against any session. v1: just
    /// counted + logged; nothing destructive happens.
    pub orphan_files_count: usize,
}

/// On-demand reconcile of the vault subtree against `sessions`.
///
/// Wave 1E.3 ships the SCAFFOLD only: the scan + count surfaces are
/// implemented but the "seal-from-disk-suffix" recovery is deferred
/// (filed as P3) because the canonical recovery path needs the
/// reverse-watcher's filename parser, which is 1E.5 scope. For 1E.3
/// the report counts get logged so a human operator can see when
/// reconcile is needed.
///
/// Vault path / KG toggle gating lives at the IPC layer; this
/// function trusts its caller.
pub fn reconcile_vault(conn: &Connection, vault_root: &Path) -> AppResult<ReconcileReport> {
    let subtree = kg_subtree_paths(vault_root);
    let entries_dir = &subtree.entries;

    let mut report = ReconcileReport::default();

    // (1) Sessions with hash recorded but vault_path NULL. Two
    //     sub-cases: file missing (operator must re-write) or file
    //     present (orphan recovery).
    let mut stmt = conn.prepare(
        "SELECT id, vault_file_hash FROM sessions \
         WHERE vault_file_hash IS NOT NULL AND vault_path IS NULL",
    )?;
    let rows = stmt.query_map([], |r| {
        let id: i64 = r.get(0)?;
        let hash: String = r.get(1)?;
        Ok((id, hash))
    })?;
    for row in rows {
        let (id, expected_hash) = row?;
        if let Some(found_path) = find_file_with_hash(entries_dir, &expected_hash)? {
            report.sealed_count += 1;
            tracing::warn!(
                target: "vault::reconcile",
                session_id = id,
                found = %found_path.display(),
                "reconcile: found orphan vault file matching recorded hash; \
                 seal-from-disk-suffix is 1E.5 scope -- logging for operator"
            );
        } else {
            report.missing_file_count += 1;
            tracing::warn!(
                target: "vault::reconcile",
                session_id = id,
                expected_hash = %expected_hash,
                "reconcile: session has recorded hash but no matching file on disk"
            );
        }
    }

    // (2) Files on disk that no session knows about (zero-hit hash).
    //     Read-only count for now; destructive cleanup is explicitly
    //     out of scope per the kickoff's "no destructive cleanup"
    //     discipline.
    if entries_dir.is_dir() {
        let known_hashes = load_known_vault_hashes(conn)?;
        for entry in fs::read_dir(entries_dir).map_err(|e| {
            AppError::Vault(format!(
                "reconcile_vault: read_dir({}) failed -- {}",
                entries_dir.display(),
                e
            ))
        })? {
            let entry = entry
                .map_err(|e| AppError::Vault(format!("reconcile_vault: read_dir iter -- {e}")))?;
            let path = entry.path();
            if !is_kg_markdown_file(&path) {
                continue;
            }
            let bytes = match fs::read(&path) {
                Ok(b) => b,
                Err(e) => {
                    tracing::warn!(
                        target: "vault::reconcile",
                        path = %path.display(),
                        error = %e,
                        "reconcile: failed to read file for hash check; skipping"
                    );
                    continue;
                }
            };
            let mut hasher = Sha256::new();
            hasher.update(&bytes);
            let hash = format!("{:x}", hasher.finalize());
            if !known_hashes.contains(&hash) {
                report.orphan_files_count += 1;
                tracing::warn!(
                    target: "vault::reconcile",
                    path = %path.display(),
                    "reconcile: orphan vault file (no session has this hash)"
                );
            }
        }
    }

    tracing::info!(
        target: "vault::reconcile",
        missing_file = report.missing_file_count,
        sealed = report.sealed_count,
        orphan_files = report.orphan_files_count,
        "reconcile pass complete"
    );

    Ok(report)
}

/// Helper for [`reconcile_vault`]: load every recorded
/// `vault_file_hash` into a `HashSet` for O(1) membership probes.
fn load_known_vault_hashes(conn: &Connection) -> AppResult<std::collections::HashSet<String>> {
    let mut stmt =
        conn.prepare("SELECT vault_file_hash FROM sessions WHERE vault_file_hash IS NOT NULL")?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    let mut out = std::collections::HashSet::new();
    for r in rows {
        out.insert(r?);
    }
    Ok(out)
}

/// Helper for [`reconcile_vault`]: scan the entries dir for a file
/// whose SHA-256 matches `expected_hash`. Returns the first hit
/// (vault entries are unique by hash by construction). `None` if
/// nothing matches.
fn find_file_with_hash(entries_dir: &Path, expected_hash: &str) -> AppResult<Option<PathBuf>> {
    if !entries_dir.is_dir() {
        return Ok(None);
    }
    for entry in fs::read_dir(entries_dir).map_err(|e| {
        AppError::Vault(format!(
            "find_file_with_hash: read_dir({}) failed -- {}",
            entries_dir.display(),
            e
        ))
    })? {
        let entry =
            entry.map_err(|e| AppError::Vault(format!("find_file_with_hash: iter -- {e}")))?;
        let path = entry.path();
        if !is_kg_markdown_file(&path) {
            continue;
        }
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let hash = format!("{:x}", hasher.finalize());
        if hash == expected_hash {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

/// True for files whose extension is `.md` -- skips the temp
/// `.mb-tmp` artifacts + any non-markdown the user may have dropped.
fn is_kg_markdown_file(path: &Path) -> bool {
    path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("md")
}

// ──────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrations::apply_all;
    use crate::vault::kg_layout::bootstrap_kg_subtree;
    use crate::vault::markdown_serializer::{CaptureKind, Category, EntryType, Status};
    use chrono::{NaiveDate, TimeZone, Utc};
    use rusqlite::params;
    use tempfile::TempDir;

    fn fresh_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        apply_all(&conn).unwrap();
        conn
    }

    fn seed_session(conn: &Connection, id: i64, uuid: &str) {
        // Migration 026 leaves the three vault-linkage columns NULL
        // at insert time -- the worker fills them via the two-phase
        // commit. We seed only the columns the FK + NOT-NULL
        // constraints require.
        conn.execute(
            "INSERT INTO sessions (id, uuid, mode_id, hotkey_pressed, started_at, \
                recording_ended_at, status, audio_duration_ms, capture_kind) \
             VALUES (?1, ?2, 1, 'RCtrl+Space', '2026-06-04T10:00:00Z', \
                '2026-06-04T10:00:05Z', 'complete', 5000, 'kg-note')",
            params![id, uuid],
        )
        .unwrap();
    }

    fn sample_entry(id: &str, title: &str) -> KgEntry {
        KgEntry {
            id: id.to_string(),
            captured_at: Utc.with_ymd_and_hms(2026, 6, 4, 10, 0, 0).unwrap(),
            captured_at_local_date: NaiveDate::from_ymd_opt(2026, 6, 4).unwrap(),
            capture_kind: CaptureKind::KgNote,
            title: title.to_string(),
            category: Category::Personal,
            entry_type: EntryType::Note,
            status: None,
            due_date: None,
            tags: vec!["one".into(), "two".into()],
            entities: vec!["Becca".into()],
            source_session_uuid: Some("sess-uuid-1".into()),
            body: "This is the note body.".into(),
        }
    }

    /// `compute_artifact` is pure: same input -> same output bytes
    /// AND same hash. This is the substrate the 1E.5 reverse-watcher
    /// relies on for loop-prevention.
    #[test]
    fn compute_artifact_is_deterministic() {
        let td = TempDir::new().unwrap();
        let entry = sample_entry("abc12345-0000-4000-8000-000000000000", "Buy Milk");
        let (p1, b1, h1) = compute_artifact(&entry, td.path());
        let (p2, b2, h2) = compute_artifact(&entry, td.path());
        assert_eq!(p1, p2);
        assert_eq!(b1, b2);
        assert_eq!(h1, h2);
        // Hash format: lowercase hex, 64 chars.
        assert_eq!(h1.len(), 64);
        assert!(h1
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }

    /// Happy path: full two-phase commit lands the file at the
    /// expected path with the recorded hash; sessions row has all
    /// three new columns populated.
    #[test]
    fn commit_entry_to_vault_happy_path() {
        let td = TempDir::new().unwrap();
        bootstrap_kg_subtree(td.path()).unwrap();

        let conn = fresh_db();
        seed_session(&conn, 1, "sess-uuid-1");
        let entry = sample_entry("abc12345-0000-4000-8000-000000000000", "Buy Milk");

        let outcome = commit_entry_to_vault(&conn, 1, &entry, td.path()).unwrap();

        // File landed at the right spot, bytes match.
        assert!(outcome.absolute_path.exists());
        let on_disk = fs::read(&outcome.absolute_path).unwrap();
        let (_, expected_bytes, expected_hash) = compute_artifact(&entry, td.path());
        assert_eq!(on_disk, expected_bytes);
        assert_eq!(outcome.file_hash, expected_hash);

        // Vault-relative path uses forward slashes regardless of OS.
        assert!(outcome
            .vault_relative_path
            .starts_with("Knowledge Graph/Entries/"));
        assert!(!outcome.vault_relative_path.contains('\\'));

        // Sessions row: hash recorded (step 3); entry_id +
        // vault_path NOT yet sealed (step 5 is the caller's
        // responsibility, folded into its outer txn with mark_done).
        let (entry_id, vault_path, vault_hash): (Option<String>, Option<String>, Option<String>) =
            conn.query_row(
                "SELECT entry_id, vault_path, vault_file_hash FROM sessions WHERE id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(entry_id, None);
        assert_eq!(vault_path, None);
        assert_eq!(vault_hash, Some(expected_hash));

        // Caller-side seal step: prove the seal helper works.
        sessions::seal_vault_filing(&conn, 1, &entry.id, &outcome.vault_relative_path).unwrap();
        let (entry_id, vault_path, _): (Option<String>, Option<String>, Option<String>) = conn
            .query_row(
                "SELECT entry_id, vault_path, vault_file_hash FROM sessions WHERE id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(entry_id.as_deref(), Some(entry.id.as_str()));
        assert_eq!(vault_path, Some(outcome.vault_relative_path));
    }

    /// Idempotency: calling commit twice with the same input is a
    /// no-op (same bytes, same hash, same paths). This is what the
    /// crash-recovery sweep relies on -- re-running after a partial
    /// success must converge.
    #[test]
    fn commit_entry_to_vault_is_idempotent() {
        let td = TempDir::new().unwrap();
        bootstrap_kg_subtree(td.path()).unwrap();

        let conn = fresh_db();
        seed_session(&conn, 1, "sess-uuid-1");
        let entry = sample_entry("abc12345-0000-4000-8000-000000000000", "Buy Milk");

        let first = commit_entry_to_vault(&conn, 1, &entry, td.path()).unwrap();
        let bytes_1 = fs::read(&first.absolute_path).unwrap();
        let second = commit_entry_to_vault(&conn, 1, &entry, td.path()).unwrap();
        let bytes_2 = fs::read(&second.absolute_path).unwrap();

        assert_eq!(first.absolute_path, second.absolute_path);
        assert_eq!(first.file_hash, second.file_hash);
        assert_eq!(bytes_1, bytes_2);
    }

    /// File write failure surfaces as `AppError::Vault` and leaves
    /// the row in the canonical reconcile signature (hash set,
    /// vault_path + entry_id NULL).
    #[test]
    fn commit_entry_to_vault_file_write_failure_leaves_reconcile_signature() {
        let td = TempDir::new().unwrap();
        // DO NOT bootstrap. Then plant a regular file at the
        // entries-dir path -- create_dir_all will refuse to convert
        // it into a directory, forcing the write step to fail.
        let kg_root = td.path().join("Knowledge Graph");
        fs::create_dir_all(&kg_root).unwrap();
        fs::write(kg_root.join("Entries"), b"i am a file blocking the dir").unwrap();

        let conn = fresh_db();
        seed_session(&conn, 1, "sess-uuid-1");
        let entry = sample_entry("abc12345-0000-4000-8000-000000000000", "Buy Milk");

        let err = commit_entry_to_vault(&conn, 1, &entry, td.path()).unwrap_err();
        match err {
            AppError::Vault(msg) => assert!(
                msg.contains("commit_entry_to_vault"),
                "error must name the helper: {msg}"
            ),
            other => panic!("expected AppError::Vault, got {other:?}"),
        }

        // Hash was NOT recorded because create_dir_all failed BEFORE
        // record_vault_hash. Row is fully clean -- safe to retry.
        let (entry_id, vault_path, vault_hash): (Option<String>, Option<String>, Option<String>) =
            conn.query_row(
                "SELECT entry_id, vault_path, vault_file_hash FROM sessions WHERE id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(entry_id, None);
        assert_eq!(vault_path, None);
        assert_eq!(vault_hash, None);
    }

    /// File write failure AFTER hash record: simulate by making the
    /// target path a directory (rename onto a dir is a hard fail).
    /// The post-condition is the reconcile signature: hash present,
    /// paths NULL.
    #[test]
    fn file_write_failure_after_hash_record_yields_reconcile_signature() {
        let td = TempDir::new().unwrap();
        bootstrap_kg_subtree(td.path()).unwrap();

        let conn = fresh_db();
        seed_session(&conn, 1, "sess-uuid-1");
        let entry = sample_entry("abc12345-0000-4000-8000-000000000000", "Buy Milk");

        // Plant a DIRECTORY at the would-be target path so the
        // rename step fails after the hash is already recorded.
        let (target, _, _) = compute_artifact(&entry, td.path());
        fs::create_dir_all(&target).unwrap();

        let err = commit_entry_to_vault(&conn, 1, &entry, td.path()).unwrap_err();
        assert!(matches!(err, AppError::Vault(_)));

        let (entry_id, vault_path, vault_hash): (Option<String>, Option<String>, Option<String>) =
            conn.query_row(
                "SELECT entry_id, vault_path, vault_file_hash FROM sessions WHERE id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert!(
            vault_hash.is_some(),
            "hash MUST be durable before file write per ADR 0053 §D5"
        );
        assert_eq!(
            entry_id, None,
            "entry_id MUST stay NULL when file write fails"
        );
        assert_eq!(
            vault_path, None,
            "vault_path MUST stay NULL when file write fails"
        );
    }

    /// `reconcile_vault` correctly identifies the "hash set, paths
    /// NULL" reconcile signature when the file exists on disk.
    #[test]
    fn reconcile_finds_orphan_file_matching_recorded_hash() {
        let td = TempDir::new().unwrap();
        bootstrap_kg_subtree(td.path()).unwrap();

        let conn = fresh_db();
        seed_session(&conn, 1, "sess-uuid-1");
        let entry = sample_entry("abc12345-0000-4000-8000-000000000000", "Buy Milk");

        // Successful step-2-to-4 commit; we deliberately DO NOT
        // run the caller-side step-5 seal, mirroring "crash between
        // step 4 and step 5".
        commit_entry_to_vault(&conn, 1, &entry, td.path()).unwrap();

        let report = reconcile_vault(&conn, td.path()).unwrap();
        assert_eq!(report.sealed_count, 1);
        assert_eq!(report.missing_file_count, 0);
        assert_eq!(report.orphan_files_count, 0);
    }

    /// `reconcile_vault` flags the "hash set, no file" signature
    /// (the step-3-to-4 crash) as missing_file_count.
    #[test]
    fn reconcile_flags_missing_file_when_hash_recorded_but_no_disk_artifact() {
        let td = TempDir::new().unwrap();
        bootstrap_kg_subtree(td.path()).unwrap();

        let conn = fresh_db();
        seed_session(&conn, 1, "sess-uuid-1");
        // Manually record a hash WITHOUT writing the file -- the
        // crash-between-step-3-and-step-4 simulation.
        conn.execute(
            "UPDATE sessions SET vault_file_hash = ?1 WHERE id = 1",
            params!["deadbeef".repeat(8)],
        )
        .unwrap();

        let report = reconcile_vault(&conn, td.path()).unwrap();
        assert_eq!(report.missing_file_count, 1);
        assert_eq!(report.sealed_count, 0);
    }

    /// Orphan file detection: a `.md` file in `Entries/` whose hash
    /// nobody recorded shows up as orphan_files_count but is NOT
    /// deleted (read-only sweep).
    #[test]
    fn reconcile_counts_orphan_files_but_does_not_delete() {
        let td = TempDir::new().unwrap();
        bootstrap_kg_subtree(td.path()).unwrap();

        let conn = fresh_db();
        let subtree = kg_subtree_paths(td.path());
        let orphan = subtree.entries.join("2026-06-04-stray__deadbeef.md");
        fs::write(&orphan, b"i am an orphan markdown file").unwrap();

        let report = reconcile_vault(&conn, td.path()).unwrap();
        assert_eq!(report.orphan_files_count, 1);
        assert!(orphan.exists(), "reconcile must NOT delete orphan files");
    }

    /// Status-bearing entry round-trips through the writer. The
    /// serializer emits a Tasks checkbox; the bytes-on-disk must
    /// equal the bytes we hashed.
    #[test]
    fn task_entry_with_status_round_trips() {
        let td = TempDir::new().unwrap();
        bootstrap_kg_subtree(td.path()).unwrap();

        let conn = fresh_db();
        seed_session(&conn, 1, "sess-uuid-1");

        let mut entry = sample_entry("abc12345-0000-4000-8000-000000000000", "Buy Milk");
        entry.entry_type = EntryType::Task;
        entry.status = Some(Status::Todo);

        let outcome = commit_entry_to_vault(&conn, 1, &entry, td.path()).unwrap();
        let bytes = fs::read(&outcome.absolute_path).unwrap();
        let expected = serialize_entry(&entry);
        assert_eq!(bytes, expected.as_bytes());

        // Tasks checkbox glyph should be on disk.
        let as_str = String::from_utf8(bytes).unwrap();
        assert!(
            as_str.contains("- [ ]"),
            "todo status must serialize as `- [ ]` checkbox: {as_str}"
        );
    }
}
