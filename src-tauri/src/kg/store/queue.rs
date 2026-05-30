//! `kg_filing_queue` FIFO state machine.
//!
//! ```text
//!   pending --dequeue_next--> processing --mark_done--> done
//!                                       \--mark_failed--> failed
//! ```
//!
//! Re-enqueuing the same `entry_id` is a no-op (`UNIQUE(entry_id)` +
//! `INSERT OR IGNORE`) -- the kg-filing-idempotent invariant.
//!
//! ## Crash recovery
//!
//! [`sweep_orphaned_processing`] (called by the worker at boot)
//! re-opens any row stuck in `processing` from a prior process. The
//! state-machine transition is `processing -> pending` so the next
//! drain picks it up; `attempt_count` is incremented so a poison-pill
//! entry is visible to whoever is watching the table.
//!
//! ## Reaping
//!
//! [`reap_done_older_than`] deletes `done` rows past a TTL (30 days
//! in 1B per the wave brief). Failure rows are kept forever in 1B;
//! Phase 1C surfaces a failures UI that owns the lifecycle.

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::AppResult;

/// One row from `kg_filing_queue` projected for the worker's
/// dequeue loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct QueueRow {
    pub id: i64,
    pub entry_id: i64,
}

/// Enqueue one dictation entry for filing. No-op if a row already
/// exists for `entry_id` (re-enqueue collapses to the existing row
/// via `INSERT OR IGNORE`). Returns the resolved queue row id either
/// way.
///
/// **This is the dictation hook's call site for Chunk 4.** Gated by
/// the `KgGraphEnabled` setting check happens at the **caller's**
/// side (ADR 0050 D6 hook clause); this function does not check the
/// setting itself so the same function can be reused from Chunk 3's
/// crash-recovery pass without re-reading state every call.
pub fn enqueue_for_filing(conn: &Connection, entry_id: i64, now_iso: &str) -> AppResult<i64> {
    conn.execute(
        "INSERT OR IGNORE INTO kg_filing_queue \
           (entry_id, state, enqueued_at, attempt_count) \
         VALUES (?1, 'pending', ?2, 0);",
        params![entry_id, now_iso],
    )?;
    // SELECT after INSERT OR IGNORE: last_insert_rowid is unreliable
    // when the row was ignored. Re-resolving via SELECT is the
    // unambiguous form.
    let id: i64 = conn.query_row(
        "SELECT id FROM kg_filing_queue WHERE entry_id = ?1;",
        params![entry_id],
        |r| r.get(0),
    )?;
    Ok(id)
}

/// Atomically claim the oldest `pending` row by flipping it to
/// `processing` and returning it. Returns `None` when the queue is
/// drained.
///
/// Implementation: SELECT-then-UPDATE inside a transaction so two
/// concurrent worker threads (we only spawn one in 1B, but the
/// shape future-proofs the API) cannot claim the same row. The
/// `attempt_count` bump is part of the claim so a row that survives
/// a crash mid-processing surfaces its attempt history.
pub(crate) fn dequeue_next(conn: &mut Connection, now_iso: &str) -> AppResult<Option<QueueRow>> {
    let tx = conn.transaction()?;
    let picked: Option<(i64, i64)> = tx
        .query_row(
            "SELECT id, entry_id FROM kg_filing_queue \
             WHERE state = 'pending' \
             ORDER BY enqueued_at ASC, id ASC LIMIT 1;",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((id, entry_id)) = picked else {
        tx.commit()?;
        return Ok(None);
    };
    tx.execute(
        "UPDATE kg_filing_queue \
         SET state = 'processing', \
             processing_started_at = ?2, \
             attempt_count = attempt_count + 1 \
         WHERE id = ?1;",
        params![id, now_iso],
    )?;
    tx.commit()?;
    Ok(Some(QueueRow { id, entry_id }))
}

/// Mark a `processing` row as `done`. The worker calls this in the
/// same transaction as [`super::apply_filed_outcome`] so a failure
/// mid-write rolls back both.
pub(crate) fn mark_done(conn: &Connection, queue_id: i64, now_iso: &str) -> AppResult<()> {
    conn.execute(
        "UPDATE kg_filing_queue \
         SET state = 'done', finished_at = ?2, last_error = NULL \
         WHERE id = ?1 AND state = 'processing';",
        params![queue_id, now_iso],
    )?;
    Ok(())
}

/// Mark a `processing` row as `failed` with a diagnostic message.
/// The row stays in the table forever in 1B (Phase 1C UI).
pub(crate) fn mark_failed(
    conn: &Connection,
    queue_id: i64,
    err: &str,
    now_iso: &str,
) -> AppResult<()> {
    conn.execute(
        "UPDATE kg_filing_queue \
         SET state = 'failed', finished_at = ?2, last_error = ?3 \
         WHERE id = ?1 AND state = 'processing';",
        params![queue_id, now_iso, err],
    )?;
    Ok(())
}

/// Re-open any rows stuck in `processing` from a prior process
/// (the worker crashed or the process was killed mid-flight). The
/// transition is `processing -> pending` so the next drain picks
/// them up. Returns the count of rows revived. Called from the
/// worker's startup path in Chunk 3.
pub(crate) fn sweep_orphaned_processing(conn: &Connection) -> AppResult<usize> {
    let changed = conn.execute(
        "UPDATE kg_filing_queue \
         SET state = 'pending', processing_started_at = NULL \
         WHERE state = 'processing';",
        [],
    )?;
    Ok(changed)
}

/// Delete `done` rows whose `finished_at` is older than `cutoff_iso`.
/// Failure rows are not touched (1B retention policy). Returns the
/// count of rows reaped. Called from the worker's startup path.
pub(crate) fn reap_done_older_than(conn: &Connection, cutoff_iso: &str) -> AppResult<usize> {
    let changed = conn.execute(
        "DELETE FROM kg_filing_queue \
         WHERE state = 'done' AND finished_at < ?1;",
        params![cutoff_iso],
    )?;
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn make_test_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE sessions (id INTEGER PRIMARY KEY);
             CREATE TABLE kg_filing_queue (
               id INTEGER PRIMARY KEY,
               entry_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
               state TEXT NOT NULL,
               enqueued_at TEXT NOT NULL,
               processing_started_at TEXT,
               finished_at TEXT,
               attempt_count INTEGER NOT NULL DEFAULT 0,
               last_error TEXT,
               UNIQUE(entry_id)
             );
             INSERT INTO sessions (id) VALUES (1), (2), (3);",
        )
        .unwrap();
        conn
    }

    #[test]
    fn enqueue_is_idempotent_per_entry() {
        let conn = make_test_conn();
        let id1 = enqueue_for_filing(&conn, 1, "2026-05-30T00:00:00Z").unwrap();
        let id2 = enqueue_for_filing(&conn, 1, "2026-05-30T00:01:00Z").unwrap();
        assert_eq!(id1, id2, "same entry_id -> same queue row");
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM kg_filing_queue;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn dequeue_claims_in_fifo_order_and_flips_state() {
        let mut conn = make_test_conn();
        enqueue_for_filing(&conn, 1, "2026-05-30T00:00:00Z").unwrap();
        enqueue_for_filing(&conn, 2, "2026-05-30T00:00:01Z").unwrap();

        let first = dequeue_next(&mut conn, "2026-05-30T00:00:02Z")
            .unwrap()
            .unwrap();
        assert_eq!(first.entry_id, 1);
        let state: String = conn
            .query_row(
                "SELECT state FROM kg_filing_queue WHERE id = ?1;",
                params![first.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(state, "processing");
        let attempt: i64 = conn
            .query_row(
                "SELECT attempt_count FROM kg_filing_queue WHERE id = ?1;",
                params![first.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(attempt, 1);

        let second = dequeue_next(&mut conn, "2026-05-30T00:00:03Z")
            .unwrap()
            .unwrap();
        assert_eq!(second.entry_id, 2);

        // Queue drained -> None.
        assert!(dequeue_next(&mut conn, "t").unwrap().is_none());
    }

    #[test]
    fn mark_done_only_transitions_from_processing() {
        let mut conn = make_test_conn();
        enqueue_for_filing(&conn, 1, "t0").unwrap();
        let row = dequeue_next(&mut conn, "t1").unwrap().unwrap();
        mark_done(&conn, row.id, "t2").unwrap();
        let state: String = conn
            .query_row(
                "SELECT state FROM kg_filing_queue WHERE id = ?1;",
                params![row.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(state, "done");
        let finished_at: Option<String> = conn
            .query_row(
                "SELECT finished_at FROM kg_filing_queue WHERE id = ?1;",
                params![row.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(finished_at.as_deref(), Some("t2"));
    }

    #[test]
    fn mark_failed_records_error_and_keeps_row() {
        let mut conn = make_test_conn();
        enqueue_for_filing(&conn, 1, "t0").unwrap();
        let row = dequeue_next(&mut conn, "t1").unwrap().unwrap();
        mark_failed(&conn, row.id, "ollama 502", "t2").unwrap();
        let (state, err): (String, Option<String>) = conn
            .query_row(
                "SELECT state, last_error FROM kg_filing_queue WHERE id = ?1;",
                params![row.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(state, "failed");
        assert_eq!(err.as_deref(), Some("ollama 502"));
    }

    #[test]
    fn sweep_orphaned_processing_revives_stuck_rows() {
        let mut conn = make_test_conn();
        enqueue_for_filing(&conn, 1, "t0").unwrap();
        enqueue_for_filing(&conn, 2, "t0").unwrap();
        let _ = dequeue_next(&mut conn, "t1").unwrap().unwrap();
        // Row 1 is now 'processing'; row 2 is still 'pending'. Simulate
        // a crash -> sweep flips processing back to pending.
        let revived = sweep_orphaned_processing(&conn).unwrap();
        assert_eq!(revived, 1);
        let pending_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM kg_filing_queue WHERE state = 'pending';",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(pending_count, 2);
    }

    #[test]
    fn reap_done_older_than_drops_only_done_past_cutoff() {
        let mut conn = make_test_conn();
        for entry_id in 1..=3 {
            enqueue_for_filing(&conn, entry_id, "t0").unwrap();
            let row = dequeue_next(&mut conn, "t1").unwrap().unwrap();
            // 1 -> done (old), 2 -> done (recent), 3 -> failed (old).
            match entry_id {
                1 => mark_done(&conn, row.id, "2026-05-01T00:00:00Z").unwrap(),
                2 => mark_done(&conn, row.id, "2026-06-01T00:00:00Z").unwrap(),
                3 => mark_failed(&conn, row.id, "boom", "2026-05-01T00:00:00Z").unwrap(),
                _ => unreachable!(),
            }
        }
        // Cutoff in mid-May 2026: drops row 1 (old done), keeps row 2
        // (recent done), keeps row 3 (failed; 1B retains failures forever).
        let reaped = reap_done_older_than(&conn, "2026-05-15T00:00:00Z").unwrap();
        assert_eq!(reaped, 1);
        let remaining: i64 = conn
            .query_row("SELECT COUNT(*) FROM kg_filing_queue;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(remaining, 2);
    }
}
