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
use serde::Serialize;

use crate::error::AppResult;

/// One row from `kg_filing_queue` projected for the worker's
/// dequeue loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct QueueRow {
    pub id: i64,
    pub entry_id: i64,
}

/// One failed row projected for the Phase 1C.2 failed-filings UX
/// (ADR 0051). Doubles as the IPC DTO returned by
/// `commands::kg::kg_list_failed_filings` -- the
/// `#[serde(rename_all = "camelCase")]` is the JS-side contract.
///
/// `last_error` is materialized as a non-optional `String` because the
/// UI always has something to render -- we `COALESCE` NULL to empty
/// string in the SELECT. (Schema permits NULL but `mark_failed`
/// always writes a message; the COALESCE is a defensive belt around
/// any future code path that forgets.)
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FailedFiling {
    pub queue_id: i64,
    pub entry_id: i64,
    pub attempt_count: i64,
    pub last_error: String,
    pub enqueued_iso: String,
    pub failed_iso: String,
}

/// Per-state counts + the most recent successful filing's timestamp.
/// Drives the Phase 1C.2 "Filing status" line above the failed-filings
/// list (ADR 0051 D3). Doubles as the IPC DTO returned by
/// `commands::kg::kg_queue_status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueStatus {
    pub pending: u32,
    pub processing: u32,
    pub failed: u32,
    /// `finished_at` of the most recent `state='done'` row, or `None`
    /// if the queue has never produced a success.
    pub last_done_iso: Option<String>,
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

/// List rows currently in `state='failed'`, newest-first by
/// `enqueued_at` (then by `id` for determinism on tie). Hard-capped
/// at `limit` rows. Phase 1C.2 failed-filings UX (ADR 0051 D1).
///
/// `finished_at` is the moment `mark_failed` parked the row, so we
/// project it as `failed_iso` for the IPC contract. `last_error` is
/// `COALESCE`d to empty string so the UI never has to handle NULL
/// (see [`FailedFiling`] docstring for the rationale).
pub(crate) fn list_failed(conn: &Connection, limit: u32) -> AppResult<Vec<FailedFiling>> {
    let mut stmt = conn.prepare(
        "SELECT id, entry_id, attempt_count, \
                COALESCE(last_error, ''), enqueued_at, \
                COALESCE(finished_at, '') \
         FROM kg_filing_queue \
         WHERE state = 'failed' \
         ORDER BY enqueued_at DESC, id DESC \
         LIMIT ?1;",
    )?;
    let rows = stmt.query_map(params![limit], |r| {
        Ok(FailedFiling {
            queue_id: r.get(0)?,
            entry_id: r.get(1)?,
            attempt_count: r.get(2)?,
            last_error: r.get(3)?,
            enqueued_iso: r.get(4)?,
            failed_iso: r.get(5)?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// Per-state queue counts + the most recent successful filing's
/// timestamp. Drives the Phase 1C.2 "Filing status" line (ADR 0051 D2).
///
/// Done in a single SQL pass with `CASE WHEN` aggregation + a
/// `MAX(...)` over `state='done'` rows -- cheaper than 4 round-trips
/// when the worker calls this on every tick.
pub(crate) fn queue_status(conn: &Connection) -> AppResult<QueueStatus> {
    // NB: SUM(CASE WHEN ...) over an empty table returns NULL in
    // SQLite, not 0 -- that bites rusqlite's typed `r.get::<_, i64>(n)`
    // with `InvalidColumnType(... Null)`. COUNT(CASE WHEN ... THEN 1 END)
    // is the right idiom: COUNT always yields an integer, NULL
    // arguments to CASE are filtered out, so on empty -> 0.
    let (pending, processing, failed, last_done_iso): (i64, i64, i64, Option<String>) = conn
        .query_row(
            "SELECT \
               COUNT(CASE WHEN state = 'pending'    THEN 1 END), \
               COUNT(CASE WHEN state = 'processing' THEN 1 END), \
               COUNT(CASE WHEN state = 'failed'     THEN 1 END), \
               MAX(CASE WHEN state = 'done'        THEN finished_at END) \
             FROM kg_filing_queue;",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
    Ok(QueueStatus {
        pending: pending.max(0) as u32,
        processing: processing.max(0) as u32,
        failed: failed.max(0) as u32,
        last_done_iso,
    })
}

/// Flip a `state='failed'` row back to `pending` for another shot,
/// resetting `attempt_count` to 0 and clearing `last_error` and the
/// stale `finished_at`. The dequeue loop's `attempt_count` increment
/// (see [`dequeue_next`]) will count the retry's attempts fresh from
/// zero -- by-design per ADR 0051 J3 ("retry starts over").
///
/// **Idempotent on already-pending rows**: the `WHERE state='failed'`
/// clause yields zero affected rows when the row is already pending
/// (or done, or processing) and returns `Ok(())` regardless. This is
/// the J3 invariant -- a double-click on Retry must not error.
/// Verified by `requeue_failed_is_idempotent_on_pending_row`.
pub(crate) fn requeue_failed(conn: &Connection, queue_id: i64) -> AppResult<()> {
    conn.execute(
        "UPDATE kg_filing_queue \
         SET state = 'pending', \
             attempt_count = 0, \
             last_error = NULL, \
             processing_started_at = NULL, \
             finished_at = NULL \
         WHERE id = ?1 AND state = 'failed';",
        params![queue_id],
    )?;
    Ok(())
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
    fn list_failed_returns_only_failed_rows_newest_first() {
        let mut conn = make_test_conn();
        // Three entries: 1 -> failed (older), 2 -> done, 3 -> failed (newer).
        enqueue_for_filing(&conn, 1, "2026-05-30T00:00:00Z").unwrap();
        enqueue_for_filing(&conn, 2, "2026-05-30T00:00:01Z").unwrap();
        enqueue_for_filing(&conn, 3, "2026-05-30T00:00:02Z").unwrap();
        let r1 = dequeue_next(&mut conn, "t").unwrap().unwrap();
        let r2 = dequeue_next(&mut conn, "t").unwrap().unwrap();
        let r3 = dequeue_next(&mut conn, "t").unwrap().unwrap();
        mark_failed(&conn, r1.id, "boom-1", "2026-05-30T01:00:00Z").unwrap();
        mark_done(&conn, r2.id, "2026-05-30T01:00:01Z").unwrap();
        mark_failed(&conn, r3.id, "boom-3", "2026-05-30T01:00:02Z").unwrap();

        let failed = list_failed(&conn, 50).unwrap();
        assert_eq!(failed.len(), 2, "done row is excluded");
        // Newest enqueued_at first.
        assert_eq!(failed[0].entry_id, 3);
        assert_eq!(failed[0].last_error, "boom-3");
        assert_eq!(failed[0].enqueued_iso, "2026-05-30T00:00:02Z");
        assert_eq!(failed[0].failed_iso, "2026-05-30T01:00:02Z");
        assert_eq!(failed[0].attempt_count, 1);
        assert_eq!(failed[1].entry_id, 1);
        assert_eq!(failed[1].last_error, "boom-1");
    }

    #[test]
    fn list_failed_respects_limit() {
        let mut conn = make_test_conn();
        for entry_id in 1..=3 {
            enqueue_for_filing(&conn, entry_id, &format!("2026-05-30T00:00:0{entry_id}Z")).unwrap();
            let row = dequeue_next(&mut conn, "t").unwrap().unwrap();
            mark_failed(&conn, row.id, "x", "2026-05-30T01:00:00Z").unwrap();
        }
        let capped = list_failed(&conn, 2).unwrap();
        assert_eq!(capped.len(), 2, "hard-cap honoured");
    }

    #[test]
    fn list_failed_returns_empty_on_clean_queue() {
        let conn = make_test_conn();
        let failed = list_failed(&conn, 50).unwrap();
        assert!(failed.is_empty());
    }

    #[test]
    fn requeue_failed_flips_state_resets_attempt_clears_error() {
        let mut conn = make_test_conn();
        enqueue_for_filing(&conn, 1, "t0").unwrap();
        // Burn two attempts so attempt_count = 2 going into the retry.
        let row = dequeue_next(&mut conn, "t1").unwrap().unwrap();
        mark_failed(&conn, row.id, "first", "t2").unwrap();
        // Simulate a second attempt: re-pend manually (the worker's
        // retry path bumps attempt_count via dequeue_next, so we
        // need the row pending again to hit it).
        conn.execute(
            "UPDATE kg_filing_queue SET state='pending', last_error=NULL, finished_at=NULL WHERE id = ?1;",
            params![row.id],
        )
        .unwrap();
        let row2 = dequeue_next(&mut conn, "t3").unwrap().unwrap();
        mark_failed(&conn, row2.id, "second", "t4").unwrap();
        let pre_attempt: i64 = conn
            .query_row(
                "SELECT attempt_count FROM kg_filing_queue WHERE id = ?1;",
                params![row.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(pre_attempt, 2, "two attempts on record before retry");

        // Retry: state -> pending, attempt_count -> 0, last_error -> NULL.
        requeue_failed(&conn, row.id).unwrap();
        let (state, attempt, err, finished): (String, i64, Option<String>, Option<String>) = conn
            .query_row(
                "SELECT state, attempt_count, last_error, finished_at \
                 FROM kg_filing_queue WHERE id = ?1;",
                params![row.id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(state, "pending");
        assert_eq!(attempt, 0);
        assert!(err.is_none());
        assert!(finished.is_none());
    }

    #[test]
    fn requeue_failed_is_idempotent_on_pending_row() {
        // ADR 0051 J3 -- the double-click invariant. requeue_failed
        // called on an already-pending row is a no-op, no error.
        let mut conn = make_test_conn();
        enqueue_for_filing(&conn, 1, "t0").unwrap();
        let row = dequeue_next(&mut conn, "t1").unwrap().unwrap();
        mark_failed(&conn, row.id, "once", "t2").unwrap();
        requeue_failed(&conn, row.id).unwrap();
        let before_attempt: i64 = conn
            .query_row(
                "SELECT attempt_count FROM kg_filing_queue WHERE id = ?1;",
                params![row.id],
                |r| r.get(0),
            )
            .unwrap();
        // Second click: also Ok, and the row is unchanged.
        requeue_failed(&conn, row.id).unwrap();
        let (state, attempt): (String, i64) = conn
            .query_row(
                "SELECT state, attempt_count FROM kg_filing_queue WHERE id = ?1;",
                params![row.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(state, "pending");
        assert_eq!(attempt, before_attempt, "no second reset, no bump");
    }

    #[test]
    fn requeue_failed_no_op_on_missing_id() {
        // Calling with a queue_id that doesn't exist is Ok(()), not
        // an error -- the UI may race with a worker that just reaped
        // the row.
        let conn = make_test_conn();
        requeue_failed(&conn, 9999).unwrap();
    }

    #[test]
    fn queue_status_counts_per_state_and_finds_last_done() {
        let mut conn = make_test_conn();
        // 1 -> done (older), 2 -> done (newer), 3 -> failed.
        // Two more pending will be enqueued after a fresh session row.
        enqueue_for_filing(&conn, 1, "e0").unwrap();
        enqueue_for_filing(&conn, 2, "e1").unwrap();
        enqueue_for_filing(&conn, 3, "e2").unwrap();
        let r1 = dequeue_next(&mut conn, "t").unwrap().unwrap();
        let r2 = dequeue_next(&mut conn, "t").unwrap().unwrap();
        let r3 = dequeue_next(&mut conn, "t").unwrap().unwrap();
        mark_done(&conn, r1.id, "2026-05-30T00:00:00Z").unwrap();
        mark_done(&conn, r2.id, "2026-05-30T01:00:00Z").unwrap();
        mark_failed(&conn, r3.id, "oops", "2026-05-30T02:00:00Z").unwrap();
        // Add two more sessions and enqueue them so we have pending rows.
        conn.execute("INSERT INTO sessions (id) VALUES (4), (5);", [])
            .unwrap();
        enqueue_for_filing(&conn, 4, "e4").unwrap();
        enqueue_for_filing(&conn, 5, "e5").unwrap();

        let status = queue_status(&conn).unwrap();
        assert_eq!(status.pending, 2);
        assert_eq!(status.processing, 0);
        assert_eq!(status.failed, 1);
        // Newest done's finished_at wins.
        assert_eq!(
            status.last_done_iso.as_deref(),
            Some("2026-05-30T01:00:00Z")
        );
    }

    #[test]
    fn queue_status_zeros_on_empty_queue() {
        let conn = make_test_conn();
        let status = queue_status(&conn).unwrap();
        assert_eq!(status.pending, 0);
        assert_eq!(status.processing, 0);
        assert_eq!(status.failed, 0);
        assert!(status.last_done_iso.is_none());
    }

    #[test]
    fn dtos_serialize_camel_case() {
        // IPC contract: wire field names are camelCase per the rest
        // of the IPC surface (kickoff names them snake_case but the
        // `#[serde(rename_all)]` is the source of truth).
        let row = FailedFiling {
            queue_id: 42,
            entry_id: 7,
            attempt_count: 3,
            last_error: "ollama 502".into(),
            enqueued_iso: "2026-05-30T00:00:00Z".into(),
            failed_iso: "2026-05-30T01:00:00Z".into(),
        };
        let row_json = serde_json::to_string(&row).unwrap();
        for field in [
            "queueId",
            "entryId",
            "attemptCount",
            "lastError",
            "enqueuedIso",
            "failedIso",
        ] {
            assert!(row_json.contains(field), "missing {field} in {row_json}");
        }
        let status_json = serde_json::to_string(&QueueStatus {
            pending: 5,
            processing: 1,
            failed: 2,
            last_done_iso: Some("x".into()),
        })
        .unwrap();
        assert!(
            status_json.contains("\"lastDoneIso\":\"x\""),
            "got {status_json}"
        );
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
