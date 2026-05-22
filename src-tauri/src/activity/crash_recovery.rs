//! Activity-capture crash recovery — boot-time pass that fixes up
//! state left behind by an ungraceful shutdown.
//!
//! Phase 10 Wave 5. The kickoff requires three things:
//!
//! 1. **Mark interrupted sessions as `crashed_recovered`.** A session
//!    row with `status='in_progress'` at boot can only be the result
//!    of the previous process dying before it could `Stop`. The
//!    cleanest UX is to promote those rows to the existing
//!    `crashed_recovered` terminal state (defined in `persist.rs`
//!    since Wave 1B) with a synthesized `ended_at`.
//! 2. **Clean orphaned audio chunk_dir subdirs.** The audio pipeline
//!    creates `<activity_audio>/<session_id>/` and the orchestrator
//!    removes that subdir on clean stop (`audio.rs:281`). On crash,
//!    the subdir is leaked. Recovery walks the audio base dir and
//!    deletes any subdir whose name doesn't correspond to a row in
//!    `activity_sessions`. Subdirs whose session row exists are
//!    PRESERVED (their files are still useful provenance — the
//!    `activity_regenerate_summary` IPC can be invoked to rebuild
//!    blocks if needed).
//! 3. **Ensure the abstractor can run on recovered sessions.** This
//!    is naturally satisfied by step 1 — once the session is in a
//!    terminal state (not `in_progress`), the existing
//!    `activity_regenerate_summary` IPC accepts it. The recovery
//!    pass does NOT auto-invoke the abstractor at boot (too heavy;
//!    would block startup minutes-long on large catalogs).
//!
//! ## When this runs
//!
//! Called from `lib.rs::run()`'s `.setup(...)` callback once, after
//! the [`Database::open`] integrity check and BEFORE the
//! [`ActivityCaptureRuntime::spawn`] call. Idempotent — re-running
//! on a clean DB is a no-op.
//!
//! ## Errors
//!
//! All failures are LOGGED + SWALLOWED. A failed recovery should
//! never prevent the app from launching. The worst case is that a
//! few sessions stay `in_progress` until the user manually deletes
//! them — annoying, not fatal.

use std::collections::HashSet;
use std::path::Path;
use std::time::SystemTime;

use rusqlite::Connection;

use crate::error::AppResult;

/// Aggregate report for diagnostic logging. Returned from
/// [`recover_all`] so callers can emit a single structured line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecoveryReport {
    /// Number of `in_progress` rows promoted to `crashed_recovered`.
    pub sessions_recovered: usize,
    /// Number of orphan chunk_dir subdirs deleted.
    pub orphan_dirs_deleted: usize,
    /// Number of chunk_dir subdirs kept because the session row exists.
    pub orphan_dirs_kept: usize,
}

/// Run the full recovery pass. Logs + swallows individual errors so a
/// failed step can't block boot. Returns the aggregate report.
pub fn recover_all(conn: &Connection, audio_base_dir: &Path) -> RecoveryReport {
    let mut report = RecoveryReport::default();

    match mark_interrupted_sessions(conn, now_ms()) {
        Ok(n) => report.sessions_recovered = n,
        Err(e) => {
            tracing::warn!(
                target: "activity::crash_recovery",
                error = %e,
                "failed to mark interrupted sessions; continuing"
            );
        }
    }

    match cleanup_orphan_chunk_dirs(conn, audio_base_dir) {
        Ok((deleted, kept)) => {
            report.orphan_dirs_deleted = deleted;
            report.orphan_dirs_kept = kept;
        }
        Err(e) => {
            tracing::warn!(
                target: "activity::crash_recovery",
                error = %e,
                "failed to clean orphan audio dirs; continuing"
            );
        }
    }

    tracing::info!(
        target: "activity::crash_recovery",
        sessions_recovered = report.sessions_recovered,
        orphan_dirs_deleted = report.orphan_dirs_deleted,
        orphan_dirs_kept = report.orphan_dirs_kept,
        "boot recovery pass completed"
    );
    report
}

/// Promote any `in_progress` session to `crashed_recovered`. The
/// synthesized `ended_at` is `MAX(updated_at, started_at)`, i.e. the
/// last timestamp we know was alive — NOT `now_ms`, because the user
/// may not have launched the app for days, and "session ended at
/// today" would be misleading.
///
/// Returns the number of rows promoted.
pub fn mark_interrupted_sessions(conn: &Connection, _now_ms: i64) -> AppResult<usize> {
    let n = conn.execute(
        "UPDATE activity_sessions \
         SET status = 'crashed_recovered', \
             ended_at = COALESCE(ended_at, MAX(updated_at, started_at)), \
             updated_at = MAX(updated_at, started_at) \
         WHERE status = 'in_progress'",
        [],
    )?;
    Ok(n)
}

/// Walk `audio_base_dir` and delete any subdirectory whose name is
/// not a known `session_id`. Returns `(deleted, kept)`.
///
/// Subdirectories belonging to KNOWN sessions are kept regardless of
/// the session's status. After recovery, recovered sessions are
/// `crashed_recovered` — their chunk files are still valid input to
/// `activity_regenerate_summary`.
pub fn cleanup_orphan_chunk_dirs(
    conn: &Connection,
    audio_base_dir: &Path,
) -> AppResult<(usize, usize)> {
    if !audio_base_dir.exists() {
        // No audio dir = nothing to clean. Not an error — audio
        // capture is opt-in and may have never been enabled.
        return Ok((0, 0));
    }

    let known = load_known_session_ids(conn)?;

    let mut deleted = 0usize;
    let mut kept = 0usize;

    let entries = match std::fs::read_dir(audio_base_dir) {
        Ok(it) => it,
        Err(e) => {
            tracing::warn!(
                target: "activity::crash_recovery",
                error = %e,
                path = %audio_base_dir.display(),
                "failed to read audio base dir"
            );
            return Ok((0, 0));
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if known.contains(name) {
            kept += 1;
            tracing::debug!(
                target: "activity::crash_recovery",
                session_id = %name,
                "audio dir kept for known session"
            );
            continue;
        }
        match std::fs::remove_dir_all(&path) {
            Ok(()) => {
                deleted += 1;
                tracing::info!(
                    target: "activity::crash_recovery",
                    session_id = %name,
                    path = %path.display(),
                    "removed orphan audio chunk_dir"
                );
            }
            Err(e) => {
                tracing::warn!(
                    target: "activity::crash_recovery",
                    error = %e,
                    path = %path.display(),
                    "failed to remove orphan audio chunk_dir"
                );
            }
        }
    }

    Ok((deleted, kept))
}

fn load_known_session_ids(conn: &Connection) -> AppResult<HashSet<String>> {
    let mut stmt = conn.prepare("SELECT id FROM activity_sessions")?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    let mut out = HashSet::new();
    for r in rows {
        out.insert(r?);
    }
    Ok(out)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity::persist::{insert_session, SessionStatus};
    use crate::db::Database;
    use rusqlite::params;

    fn fresh_db() -> Database {
        Database::open_in_memory().expect("open in-memory db")
    }

    /// Force a session row into `in_progress` and back-date its
    /// `updated_at` so the recovery pass has something to find.
    fn seed_in_progress(conn: &Connection, started_at: i64) -> String {
        let id = insert_session(conn, started_at).unwrap();
        conn.execute(
            "UPDATE activity_sessions SET updated_at = ?1 WHERE id = ?2",
            params![started_at + 5_000, id],
        )
        .unwrap();
        id
    }

    #[test]
    fn mark_interrupted_promotes_in_progress_to_crashed_recovered() {
        let db = fresh_db();
        let sid = seed_in_progress(&db.conn, 1_000_000);
        let n = mark_interrupted_sessions(&db.conn, 9_999_999).unwrap();
        assert_eq!(n, 1);
        let (status, ended_at): (String, Option<i64>) = db
            .conn
            .query_row(
                "SELECT status, ended_at FROM activity_sessions WHERE id = ?1",
                params![sid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(status, SessionStatus::CrashedRecovered.as_db_str());
        // ended_at should be the last-known-alive timestamp
        // (max(started_at, updated_at)), NOT now_ms.
        assert_eq!(ended_at, Some(1_005_000));
    }

    #[test]
    fn mark_interrupted_is_idempotent() {
        let db = fresh_db();
        let _sid = seed_in_progress(&db.conn, 1_000_000);
        assert_eq!(mark_interrupted_sessions(&db.conn, 0).unwrap(), 1);
        assert_eq!(mark_interrupted_sessions(&db.conn, 0).unwrap(), 0);
    }

    #[test]
    fn mark_interrupted_leaves_completed_rows_alone() {
        let db = fresh_db();
        let sid = insert_session(&db.conn, 1_000_000).unwrap();
        db.conn
            .execute(
                "UPDATE activity_sessions SET status = 'completed', ended_at = 1_500_000 WHERE id = ?1",
                params![sid],
            )
            .unwrap();
        let n = mark_interrupted_sessions(&db.conn, 0).unwrap();
        assert_eq!(n, 0);
        let status: String = db
            .conn
            .query_row(
                "SELECT status FROM activity_sessions WHERE id = ?1",
                params![sid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(status, "completed");
    }

    #[test]
    fn cleanup_orphan_chunk_dirs_deletes_unknown_and_keeps_known() {
        let db = fresh_db();
        let known_id = insert_session(&db.conn, 1_000_000).unwrap();

        let base = std::env::temp_dir().join(format!(
            "mb_test_recovery_{}",
            now_ms().wrapping_add(std::process::id() as i64)
        ));
        let known_dir = base.join(&known_id);
        let orphan_dir = base.join("orphan-session-xyz");
        std::fs::create_dir_all(&known_dir).unwrap();
        std::fs::create_dir_all(&orphan_dir).unwrap();
        std::fs::write(known_dir.join("placeholder.wav"), b"\x00").unwrap();
        std::fs::write(orphan_dir.join("placeholder.wav"), b"\x00").unwrap();

        let (deleted, kept) = cleanup_orphan_chunk_dirs(&db.conn, &base).unwrap();
        assert_eq!(deleted, 1);
        assert_eq!(kept, 1);
        assert!(known_dir.exists());
        assert!(!orphan_dir.exists());

        // Cleanup.
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn cleanup_handles_missing_audio_base_dir() {
        let db = fresh_db();
        let nonexistent = std::env::temp_dir().join("mb_test_recovery_does_not_exist");
        // Make sure we have a clean slate.
        let _ = std::fs::remove_dir_all(&nonexistent);
        let (deleted, kept) = cleanup_orphan_chunk_dirs(&db.conn, &nonexistent).unwrap();
        assert_eq!(deleted, 0);
        assert_eq!(kept, 0);
    }

    #[test]
    fn recover_all_combines_steps() {
        let db = fresh_db();
        // One in-progress + one orphan dir.
        let _ = seed_in_progress(&db.conn, 1_000_000);

        let base = std::env::temp_dir().join(format!(
            "mb_test_recovery_all_{}",
            now_ms().wrapping_add(std::process::id() as i64)
        ));
        std::fs::create_dir_all(base.join("orphan-id")).unwrap();

        let report = recover_all(&db.conn, &base);
        assert_eq!(report.sessions_recovered, 1);
        assert_eq!(report.orphan_dirs_deleted, 1);
        assert_eq!(report.orphan_dirs_kept, 0);

        let _ = std::fs::remove_dir_all(&base);
    }
}
