//! KG filing worker thread — Phase 1B Chunk 3 (`mb-eke8`, ADR 0050).
//!
//! Drains `kg_filing_queue` FIFO, runs the 5-pass `run_pipeline` for
//! each queued entry, and commits the result via
//! `kg::store::apply_filed_outcome` + `queue::mark_done` in a single
//! transaction.
//!
//! ## Lifecycle
//!
//! Spawned at app boot from `lib.rs::run()` unconditionally. The
//! `KgGraphEnabled` setting is re-read at the top of every drain
//! loop tick (Phase 1C Wave 1C.1, `mb-7w5f` / ADR 0051 §D6 —
//! supersedes the Phase 1B Decision C read-once-at-boot wiring). On
//! startup, before entering the drain loop, the worker:
//!
//! 1. Calls [`super::store::queue::sweep_orphaned_processing`] to flip
//!    any `state='processing'` rows from a prior crashed process back
//!    to `state='pending'` (Decision E — crash recovery).
//! 2. Calls [`super::store::queue::reap_done_older_than`] with a
//!    30-day cutoff to clean up old `done` rows (Decision G — boot-time
//!    reap, not per-tick).
//!
//! After the boot sweeps the worker enters its main loop (Decision F):
//!
//! ```text
//!   loop {
//!       if shutdown_flag.load() { return; }
//!       match dequeue_next() {
//!           None         => sleep 1s; continue;   // queue drained
//!           Some(row)    => process(row);          // see below
//!       }
//!   }
//! ```
//!
//! Per-row processing lives in [`filing::process_one`] (see that
//! module for the step-by-step contract). On any failure: if
//! `attempt_count < MAX_RETRIES` the row is re-pended for another
//! shot; else `mark_failed` parks it for the Phase 1C failures UI.
//!
//! ## Shutdown
//!
//! Tauri drops [`KgFilingRuntime`] (the managed state holder) on app
//! exit; its `Drop` flips the shared shutdown `AtomicBool`. The worker
//! observes the flag on its next loop iteration and returns. We do NOT
//! join the thread on shutdown — an in-flight pipeline pass can take
//! 20+ seconds against a cold local Ollama; blocking app exit on that
//! would be user-hostile. Worst case the thread is killed by process
//! exit mid-pass; the next boot's `sweep_orphaned_processing` revives
//! whatever was claimed but not finished.
//!
//! ## File layout (Wave 1E.7 Part 2, `mb-5lla`)
//!
//! The worker grew past the 600-LoC cap so its phases were split into
//! cohesive submodules under `worker/`. This root file owns ONLY the
//! runtime struct + main loop + queue-lifecycle helpers (retry / park
//! / KgGraphEnabled poll). Each phase has its own home:
//!
//! - [`filing`] — `process_one`, `build_segment_outputs`, model const
//! - [`projection`] — vault projection (ADR 0053 §D4 steps 2-4)
//! - [`archive`] — history sidecar (ADR 0053 §D7, 1E.4)
//! - [`stubs`] — entity / project / tag stubs (4b / 5a)
//! - [`index_log`] — INDEX.md / LOG.md maintenance (5b / 5c)
//! - [`transcripts`] — shared `transcripts` SELECT helpers
//! - [`time_iso`] — pure ISO-8601 + epoch-ms helpers

#![allow(missing_docs)]
// Most of the worker's helpers compile to `dead_code` warnings until
// `lib.rs::run()` wires `KgFilingRuntime::spawn` in (Task 3). The
// allow stays after Task 3 too because individual helpers like the
// ISO formatter are only reachable through the boot-gated `run` body,
// which clippy's reachability analysis under `--release` can't always
// see when KgGraphEnabled stays false in production builds. Mirrors
// the `kg::ollama::OllamaClient` precedent.
#![allow(dead_code)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use rusqlite::{params, Connection};

use crate::error::AppResult;
use crate::settings::{model::SettingKey, Settings};

use super::ollama::OllamaClient;
use super::schema_loader::Schema;
use super::store::queue::{
    dequeue_next, mark_failed, reap_done_older_than, sweep_orphaned_processing,
};

mod archive;
mod filing;
mod index_log;
mod projection;
mod stubs;
mod time_iso;
mod transcripts;

// Re-export the one helper outside callers (kg::parity, kg::latency_bench)
// reach for. Keeping the public path as `kg::worker::build_segment_outputs`
// means the split is invisible to those modules.
pub(crate) use filing::build_segment_outputs;

use filing::process_one;
use time_iso::{now_iso, retention_cutoff_iso};

/// Max attempts before a row is parked at `state='failed'`. The
/// `attempt_count` is bumped by `dequeue_next` on each claim, so a
/// row reaches this cap after this many full drain cycles.
const MAX_RETRIES: i64 = 3;

/// How long to nap when the queue is drained or after a failure (to
/// avoid busy-spinning + to give the shutdown flag a chance to flip).
const IDLE_SLEEP: Duration = Duration::from_secs(1);

/// 30-day TTL for `done` rows, ISO-millis baseline. Applied once at
/// worker boot via [`reap_done_older_than`].
const DONE_RETENTION_DAYS: i64 = 30;

/// Managed-state holder for the filing worker. The struct itself is
/// near-empty — its job is to live in Tauri's managed-state registry
/// so its `Drop` fires on app exit and flips the shutdown flag.
pub struct KgFilingRuntime {
    shutdown: Arc<AtomicBool>,
    /// Held only so the thread's lifetime is observable from tests;
    /// the `Option` lets us `take()` it in `Drop` to avoid leaving a
    /// dangling handle if a future variant ever wants to join. Today
    /// we deliberately do NOT join on drop (see module docs).
    _handle: Option<JoinHandle<()>>,
}

impl KgFilingRuntime {
    /// Spawn the filing worker thread.
    ///
    /// The thread runs the boot sweeps synchronously before entering
    /// its drain loop, so this call returns once both
    /// `sweep_orphaned_processing` + `reap_done_older_than` have
    /// either succeeded or logged + swallowed their error. (They run
    /// inside the thread, not on the caller's thread, so app boot is
    /// never blocked on them.)
    pub fn spawn(conn: Arc<Mutex<Connection>>) -> Self {
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_clone = Arc::clone(&shutdown);
        let handle = thread::Builder::new()
            .name("kg-filing-worker".to_string())
            .spawn(move || run(conn, shutdown_clone))
            .expect("kg-filing-worker thread spawn must succeed at boot");
        Self {
            shutdown,
            _handle: Some(handle),
        }
    }

    /// Test-only handle to the shutdown flag. Production code should
    /// rely on `Drop`.
    #[cfg(test)]
    pub fn shutdown_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.shutdown)
    }
}

impl Drop for KgFilingRuntime {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        tracing::info!(target: "kg::worker", "shutdown flag flipped; worker will exit on next loop tick");
    }
}

/// Worker body. Runs on its own thread; the boot sweeps + main drain
/// loop live here. Split out from `KgFilingRuntime::spawn` so tests
/// can drive it inline.
fn run(conn: Arc<Mutex<Connection>>, shutdown: Arc<AtomicBool>) {
    tracing::info!(target: "kg::worker", "kg filing worker started");

    // ── Boot sweeps (Decisions E + G) ────────────────────────────
    // Both are best-effort: a failure here logs + continues so a
    // transient DB-lock contention can't block worker liveness.
    match conn.lock() {
        Ok(c) => {
            match sweep_orphaned_processing(&c) {
                Ok(n) if n > 0 => tracing::info!(
                    target: "kg::worker",
                    revived = n,
                    "boot sweep: revived orphaned processing rows"
                ),
                Ok(_) => {}
                Err(e) => tracing::warn!(
                    target: "kg::worker",
                    error = %e,
                    "boot sweep: sweep_orphaned_processing failed"
                ),
            }
            let cutoff = retention_cutoff_iso(DONE_RETENTION_DAYS);
            match reap_done_older_than(&c, &cutoff) {
                Ok(n) if n > 0 => tracing::info!(
                    target: "kg::worker",
                    reaped = n,
                    cutoff = %cutoff,
                    "boot sweep: reaped done rows older than 30d"
                ),
                Ok(_) => {}
                Err(e) => tracing::warn!(
                    target: "kg::worker",
                    error = %e,
                    "boot sweep: reap_done_older_than failed"
                ),
            }
        }
        Err(_) => tracing::warn!(
            target: "kg::worker",
            "boot sweep: db mutex poisoned; skipping"
        ),
    }

    // ── Long-lived per-thread resources ──────────────────────────
    // Built ONCE so subsequent loop iterations reuse the connection
    // pool (OllamaClient's ureq::Agent) and the parsed prompt set
    // (Schema). A schema-load failure aborts the worker — there's
    // nothing it can usefully do without prompts.
    let schema = match Schema::load_default() {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(
                target: "kg::worker",
                error = %e,
                "schema load failed; worker exiting"
            );
            return;
        }
    };
    let ollama = OllamaClient::new();

    // ── Main drain loop ──────────────────────────────────────────
    loop {
        if shutdown.load(Ordering::SeqCst) {
            tracing::info!(target: "kg::worker", "shutdown observed; worker exiting cleanly");
            return;
        }

        // Phase 1C Wave 1C.1 (`mb-7w5f`, ADR 0051 §D6) — per-tick
        // KgGraphEnabled poll. When the user has the toggle off (the
        // default) we skip the dequeue entirely and nap. This costs
        // one SELECT per IDLE_SLEEP tick (~1ms) and lets the Settings
        // KG tab flip take effect inside one tick without an app
        // restart. A poisoned mutex falls through to the existing
        // sleep/continue path on the next iteration.
        if !is_graph_enabled(&conn) {
            tracing::trace!(target: "kg::worker", "KgGraphEnabled=false; skipping dequeue");
            thread::sleep(IDLE_SLEEP);
            continue;
        }

        let claimed = match conn.lock() {
            Ok(mut c) => match dequeue_next(&mut c, &now_iso()) {
                Ok(opt) => opt,
                Err(e) => {
                    tracing::warn!(target: "kg::worker", error = %e, "dequeue_next failed");
                    drop(c);
                    thread::sleep(IDLE_SLEEP);
                    continue;
                }
            },
            Err(_) => {
                tracing::warn!(target: "kg::worker", "db mutex poisoned; sleeping");
                thread::sleep(IDLE_SLEEP);
                continue;
            }
        };

        let Some(row) = claimed else {
            // Queue drained — nap before checking again. Also gives
            // shutdown a chance to be observed.
            thread::sleep(IDLE_SLEEP);
            continue;
        };

        tracing::debug!(
            target: "kg::worker",
            queue_id = row.id,
            entry_id = row.entry_id,
            "claimed row for filing"
        );

        match process_one(&conn, &schema, &ollama, row.id, row.entry_id) {
            Ok(()) => tracing::info!(
                target: "kg::worker",
                queue_id = row.id,
                entry_id = row.entry_id,
                "filing complete"
            ),
            Err(e) => {
                tracing::warn!(
                    target: "kg::worker",
                    queue_id = row.id,
                    entry_id = row.entry_id,
                    error = %e,
                    "filing failed; will retry or park"
                );
                handle_failure(&conn, row.id, &e.to_string());
            }
        }
    }
}

/// Failure handler. If `attempt_count < MAX_RETRIES` the row goes
/// back to `pending` for another shot; else `mark_failed` parks it.
/// Either path is best-effort: a DB failure here logs + swallows so
/// the worker keeps draining.
fn handle_failure(conn: &Arc<Mutex<Connection>>, queue_id: i64, err_msg: &str) {
    let c = match conn.lock() {
        Ok(c) => c,
        Err(_) => {
            tracing::warn!(target: "kg::worker", "db mutex poisoned during failure handling");
            return;
        }
    };
    let attempt: i64 = match c.query_row(
        "SELECT attempt_count FROM kg_filing_queue WHERE id = ?1",
        params![queue_id],
        |r| r.get(0),
    ) {
        Ok(n) => n,
        Err(e) => {
            tracing::warn!(
                target: "kg::worker",
                queue_id,
                error = %e,
                "failed to read attempt_count; cannot decide retry vs park"
            );
            return;
        }
    };

    let now = now_iso();
    if attempt >= MAX_RETRIES {
        if let Err(e) = mark_failed(&c, queue_id, err_msg, &now) {
            tracing::warn!(
                target: "kg::worker",
                queue_id,
                error = %e,
                "mark_failed db error; row may stay processing"
            );
        } else {
            tracing::warn!(
                target: "kg::worker",
                queue_id,
                attempts = attempt,
                "row parked at state='failed' after exhausting retries"
            );
        }
    } else if let Err(e) = requeue_for_retry(&c, queue_id, err_msg) {
        tracing::warn!(
            target: "kg::worker",
            queue_id,
            error = %e,
            "requeue failed; falling back to mark_failed"
        );
        let _ = mark_failed(&c, queue_id, err_msg, &now);
    } else {
        tracing::info!(
            target: "kg::worker",
            queue_id,
            attempts = attempt,
            "row re-pended for retry"
        );
    }
    // Throttle so we don't immediately re-claim a failing row.
    drop(c);
    thread::sleep(IDLE_SLEEP);
}

/// Flip a `processing` row back to `pending` so the next drain picks
/// it up. Mirrors `sweep_orphaned_processing`'s shape but scoped to
/// one row + records the last error.
fn requeue_for_retry(conn: &Connection, queue_id: i64, err_msg: &str) -> AppResult<()> {
    conn.execute(
        "UPDATE kg_filing_queue \
         SET state = 'pending', processing_started_at = NULL, last_error = ?2 \
         WHERE id = ?1 AND state = 'processing';",
        params![queue_id, err_msg],
    )?;
    Ok(())
}

/// Read [`SettingKey::KgGraphEnabled`] under a short-lived DB lock.
/// Returns `false` on any failure (mutex poisoning, missing row
/// recovery via [`Settings::get`], deserialize error) so the
/// graph-off invariant holds even when the settings layer is
/// transiently unhealthy — the worker fails closed.
fn is_graph_enabled(conn: &Arc<Mutex<Connection>>) -> bool {
    let Ok(guard) = conn.lock() else {
        tracing::warn!(target: "kg::worker", "db mutex poisoned in KgGraphEnabled poll; treating as false");
        return false;
    };
    Settings::new(&guard)
        .get::<bool>(SettingKey::KgGraphEnabled)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal schema the worker's failure-handling tests need. Real
    /// migration tests live in `db::migrations` + the parity probe
    /// covers end-to-end persistence.
    fn test_conn() -> Connection {
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
    fn requeue_for_retry_flips_processing_to_pending() {
        let conn = test_conn();
        super::super::store::enqueue_for_filing(&conn, 1, "t0").unwrap();
        // Manually flip to processing (skipping the FIFO claim path
        // since this test only exercises the requeue helper).
        conn.execute(
            "UPDATE kg_filing_queue SET state='processing', processing_started_at='t1', attempt_count=1 WHERE entry_id=1",
            [],
        )
        .unwrap();

        requeue_for_retry(&conn, 1, "boom").unwrap();

        let (state, last_err, started_at): (String, Option<String>, Option<String>) = conn
            .query_row(
                "SELECT state, last_error, processing_started_at FROM kg_filing_queue WHERE id = ?1",
                params![1],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(state, "pending");
        assert_eq!(last_err.as_deref(), Some("boom"));
        assert!(started_at.is_none(), "processing_started_at must clear");
    }

    #[test]
    fn is_graph_enabled_returns_false_when_settings_table_missing() {
        // Fail-closed: a DB without the settings table errors on the
        // SELECT, which `is_graph_enabled` swallows to `false`. This
        // is the graph-off-untouched invariant's safety net for a
        // transiently broken settings layer.
        let conn = Connection::open_in_memory().unwrap();
        let shared = Arc::new(Mutex::new(conn));
        assert!(!is_graph_enabled(&shared));
    }

    #[test]
    fn is_graph_enabled_defaults_to_false_when_row_absent() {
        // Settings table exists but no `kg_graph_enabled` row → the
        // `Settings::get` default-fallback returns `false` (mirrors
        // `SettingKey::KgGraphEnabled::default_value`).
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);")
            .unwrap();
        let shared = Arc::new(Mutex::new(conn));
        assert!(!is_graph_enabled(&shared));
    }

    #[test]
    fn is_graph_enabled_reflects_setting_writes() {
        // Per-tick poll contract: flipping the setting changes the
        // observed value WITHOUT recreating the worker.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);\n             INSERT INTO settings (key, value) VALUES ('kg_graph_enabled', 'true');",
        )
        .unwrap();
        let shared = Arc::new(Mutex::new(conn));
        assert!(is_graph_enabled(&shared), "true value must read back true");

        // Flip to false → next poll observes the flip.
        shared
            .lock()
            .unwrap()
            .execute(
                "UPDATE settings SET value = 'false' WHERE key = 'kg_graph_enabled'",
                [],
            )
            .unwrap();
        assert!(
            !is_graph_enabled(&shared),
            "flipping the setting must take effect on the next poll"
        );
    }
}
