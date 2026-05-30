//! KG filing worker thread — Phase 1B Chunk 3 (`mb-eke8`, ADR 0050).
//!
//! Drains `kg_filing_queue` FIFO, runs the 5-pass `run_pipeline` for
//! each queued entry, and commits the result via
//! `kg::store::apply_filed_outcome` + `queue::mark_done` in a single
//! transaction.
//!
//! ## Lifecycle
//!
//! Spawned at app boot from `lib.rs::run()` *iff* `KgGraphEnabled =
//! true` (Decision C — read-once-at-boot). On startup, before entering
//! the drain loop, the worker:
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
//! Per-row processing:
//!
//! 1. Load the dictation text from `transcripts` (prefer `stage='final'`,
//!    fall back to `'cleaned'` then `'raw'`). AGENTS.md Principle 1
//!    holds — we only SELECT from `transcripts`, never UPDATE.
//! 2. Call [`super::pipeline::run_pipeline`] (5 passes, real `OllamaClient`).
//! 3. Materialize `Vec<SegmentOutput>` from `PipelineResult.segment_entities`
//!    + `PipelineResult.entries[i].topic_tags` (per-segment join by idx).
//! 4. Open a transaction; call `apply_filed_outcome` + `mark_done`; commit.
//!    A failure mid-write rolls both back (Chunk 2 store-layer contract).
//! 5. On any failure: if `attempt_count < MAX_RETRIES` the row is
//!    re-pended for another shot; else `mark_failed` parks it for the
//!    Phase 1C failures UI.
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
use std::time::{Duration, SystemTime};

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::{AppError, AppResult};

use super::ollama::{GenerateOptions, OllamaClient};
use super::pipeline::{run_pipeline, PipelineResult};
use super::schema_loader::Schema;
use super::store::queue::{
    dequeue_next, mark_done, mark_failed, reap_done_older_than, sweep_orphaned_processing,
};
use super::store::{apply_filed_outcome, SegmentOutput};

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
const MS_PER_DAY: i64 = 86_400_000;

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

/// Process one claimed queue row end-to-end. Returns `Ok(())` if the
/// entry was filed + the row was marked `done`; `Err(_)` otherwise
/// (caller decides retry vs park).
fn process_one(
    conn: &Arc<Mutex<Connection>>,
    schema: &Schema,
    ollama: &OllamaClient,
    queue_id: i64,
    entry_id: i64,
) -> AppResult<()> {
    // ── Load dictation text + the captured timestamp ───────────
    let (dictation_text, captured_iso) = {
        let c = conn
            .lock()
            .map_err(|_| AppError::Other("db mutex poisoned in process_one".to_string()))?;
        let text = load_dictation_text(&c, entry_id)?.ok_or_else(|| {
            AppError::Other(format!(
                "no transcripts row for session_id={entry_id} (stages tried: final, cleaned, raw)"
            ))
        })?;
        // The `transcripts.created_at` column is the closest stable
        // wall-clock for a finalized session; the parity probe and
        // store layer both want an ISO string.
        let started_at: String = c.query_row(
            "SELECT started_at FROM sessions WHERE id = ?1",
            params![entry_id],
            |r| r.get(0),
        )?;
        (text, started_at)
    };

    // ── Run the 5-pass pipeline ────────────────────────────────
    // Per-run seed = entry_id so retries are deterministic for a
    // given dictation. PLAN §8.5 stability requires the caller set
    // a seed.
    let options = GenerateOptions {
        temperature: 0.2,
        seed: Some(entry_id),
        num_ctx: 4096,
    };
    let dictation_id = format!("session-{entry_id}");
    let result = run_pipeline(
        ollama,
        schema,
        None, // synonym map: not wired for v1
        DEFAULT_FILING_MODEL,
        &dictation_id,
        &dictation_text,
        &captured_iso,
        &options,
        None, // production callers don't dump per-pass artifacts
    );

    // A pipeline that produced any per-pass error AND nothing to
    // file is a hard failure — retry. A pipeline with partial
    // failures but some entries is success-with-warnings (we file
    // what we got + log the warnings).
    if result.entries.is_empty() && !result.per_pass_errors.is_empty() {
        let first = &result.per_pass_errors[0];
        return Err(AppError::Other(format!(
            "pipeline produced no entries for entry_id={entry_id}; first error: {} -> {}",
            first.0, first.1
        )));
    }

    let segments = build_segment_outputs(&result);

    // ── Persist + mark_done atomically ─────────────────────────
    let mut c = conn
        .lock()
        .map_err(|_| AppError::Other("db mutex poisoned in process_one (persist)".to_string()))?;
    let tx = c.transaction()?;
    let now = now_iso();
    apply_filed_outcome(&tx, entry_id, &segments, &now)?;
    mark_done(&tx, queue_id, &now)?;
    tx.commit()?;
    Ok(())
}

/// Build the `Vec<SegmentOutput>` the store layer consumes from the
/// `PipelineResult`. Matches segment entities by `segment_idx` against
/// the assembled entries' `topic_tags`. A segment that produced an
/// `Entry` but somehow has no matching `segment_entities` row falls
/// back to empty entities (defensive — shouldn't happen, but we
/// prefer dropping entity provenance over crashing the worker).
///
/// `pub(crate)` so the Chunk 5 extended parity probe (`kg::parity`'s
/// `--persist` mode, ADR 0050 §D8 gate 1) can reuse the same
/// `PipelineResult -> Vec<SegmentOutput>` join the production worker
/// uses. Keeping a single source of truth avoids drift between the
/// gate and the live path.
pub(crate) fn build_segment_outputs(result: &PipelineResult) -> Vec<SegmentOutput> {
    // Build a lookup so the per-entry walk is O(N) not O(N²). Each
    // segment idx appears at most once in segment_entities (the
    // pipeline pushes one row per surviving segment).
    let mut by_idx: std::collections::HashMap<usize, Vec<super::passes::ExtractedEntity>> =
        std::collections::HashMap::with_capacity(result.segment_entities.len());
    for se in &result.segment_entities {
        by_idx.insert(se.segment_idx, se.entities.clone());
    }

    result
        .entries
        .iter()
        .enumerate()
        .map(|(idx, entry)| SegmentOutput {
            segment_idx: idx,
            entities: by_idx.remove(&idx).unwrap_or_default(),
            tag_slugs: entry.topic_tags.clone(),
        })
        .collect()
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

/// Read the dictation text for a session by trying transcript stages
/// in priority order. ADR 0050 § D6 wires the dictation hook *after*
/// the session is finalized, so `stage='final'` should always exist
/// — the fallbacks defend against partial-cleanup states the
/// dictation orchestrator might land in during Phase 1B+.
fn load_dictation_text(conn: &Connection, session_id: i64) -> AppResult<Option<String>> {
    for stage in ["final", "cleaned", "raw"] {
        let text: Option<String> = conn
            .query_row(
                "SELECT text FROM transcripts WHERE session_id = ?1 AND stage = ?2",
                params![session_id, stage],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(t) = text {
            return Ok(Some(t));
        }
    }
    Ok(None)
}

fn now_iso() -> String {
    iso_from_ms(now_ms())
}

fn retention_cutoff_iso(days: i64) -> String {
    let cutoff_ms = now_ms().saturating_sub(days.saturating_mul(MS_PER_DAY));
    iso_from_ms(cutoff_ms)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn iso_from_ms(ms: i64) -> String {
    // Minimal RFC3339 / ISO-8601 without an extra crate. The store
    // layer + queue.rs both stringly compare ISO timestamps; only
    // lexicographic ordering is asserted, which holds for this shape.
    let secs = ms / 1000;
    let millis_part = (ms % 1000).abs();
    let (y, mo, d, h, mi, se) = epoch_secs_to_ymdhms(secs);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{se:02}.{millis_part:03}Z")
}

/// Tiny calendar-math helper: epoch seconds → (Y,M,D,h,m,s) UTC.
/// Honest-to-goodness Gregorian; covers 1970-9999 cleanly.
fn epoch_secs_to_ymdhms(secs: i64) -> (i64, u32, u32, u32, u32, u32) {
    let mut total = secs.max(0) as u64;
    let se = (total % 60) as u32;
    total /= 60;
    let mi = (total % 60) as u32;
    total /= 60;
    let h = (total % 24) as u32;
    total /= 24;
    let mut days = total as i64;

    let mut y: i64 = 1970;
    loop {
        let dy = if is_leap_year(y) { 366 } else { 365 };
        if days < dy {
            break;
        }
        days -= dy;
        y += 1;
    }
    let months_in = if is_leap_year(y) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut mo: u32 = 1;
    for m_len in months_in.iter() {
        if days < *m_len {
            break;
        }
        days -= m_len;
        mo += 1;
    }
    let d = (days + 1) as u32;
    (y, mo, d, h, mi, se)
}

fn is_leap_year(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

/// Default filing-pipeline model id. Matches the Wave 0.5.4 fixture
/// model (qwen2.5:7b mid-confident profile). Phase 1C can promote
/// this to a `SettingKey` once the user-facing knob lands; for 1B
/// the hardcoded default keeps the surface tight.
const DEFAULT_FILING_MODEL: &str = "qwen2.5:7b-instruct-q4_K_M";

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

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
    fn iso_from_ms_round_trips_known_dates() {
        // Anchor checks: epoch + a verifiable wall clock.
        assert_eq!(iso_from_ms(0), "1970-01-01T00:00:00.000Z");
        // 2024-01-01T00:00:00Z = 1704067200 epoch seconds.
        assert_eq!(iso_from_ms(1_704_067_200_000), "2024-01-01T00:00:00.000Z");
    }

    #[test]
    fn iso_lexicographic_ordering_matches_chronology() {
        // The queue's reap_done_older_than relies on lexicographic
        // ISO comparison being chronological.
        let earlier = iso_from_ms(1_700_000_000_000);
        let later = iso_from_ms(1_800_000_000_000);
        assert!(earlier < later, "earlier={earlier} later={later}");
    }

    #[test]
    fn retention_cutoff_iso_is_in_the_past() {
        // 30 days ago < now. We compute both as the same call so they
        // both use the same `now_ms()` snapshot... ish. Allow a small
        // window of clock-tick wobble.
        let cutoff = retention_cutoff_iso(30);
        let now = now_iso();
        assert!(cutoff < now, "cutoff={cutoff} now={now}");
    }

    #[test]
    fn load_dictation_text_prefers_final_over_cleaned() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE transcripts (
               id INTEGER PRIMARY KEY,
               session_id INTEGER NOT NULL,
               stage TEXT NOT NULL,
               text TEXT NOT NULL,
               UNIQUE(session_id, stage)
             );
             INSERT INTO transcripts (session_id, stage, text) VALUES
               (1, 'raw', 'raw-text'),
               (1, 'cleaned', 'cleaned-text'),
               (1, 'final', 'final-text'),
               (2, 'cleaned', 'cleaned-text-2'),
               (3, 'raw', 'raw-text-3');",
        )
        .unwrap();
        assert_eq!(
            load_dictation_text(&conn, 1).unwrap().as_deref(),
            Some("final-text")
        );
        assert_eq!(
            load_dictation_text(&conn, 2).unwrap().as_deref(),
            Some("cleaned-text-2")
        );
        assert_eq!(
            load_dictation_text(&conn, 3).unwrap().as_deref(),
            Some("raw-text-3")
        );
        assert!(load_dictation_text(&conn, 999).unwrap().is_none());
    }

    #[test]
    fn build_segment_outputs_joins_entities_with_topic_tags_by_idx() {
        use super::super::passes::{EntityType, ExtractedEntity};
        use super::super::pipeline::{PipelineResult, SegmentEntities};
        use super::super::schema::{Category, Entry, EntryType, Status};

        let result = PipelineResult {
            entries: vec![
                Entry {
                    title: "t0".into(),
                    category: Category::Personal,
                    entry_type: EntryType::Task,
                    status: Some(Status::Todo),
                    topic_tags: vec!["a".into(), "b".into()],
                    due_iso: None,
                    captured_iso: "x".into(),
                    body: "seg0".into(),
                },
                Entry {
                    title: "t1".into(),
                    category: Category::Professional,
                    entry_type: EntryType::Task,
                    status: Some(Status::Todo),
                    topic_tags: vec!["c".into()],
                    due_iso: None,
                    captured_iso: "x".into(),
                    body: "seg1".into(),
                },
            ],
            per_pass_errors: vec![],
            new_tag_requests: vec![],
            segment_entities: vec![
                SegmentEntities {
                    segment_idx: 0,
                    entities: vec![ExtractedEntity {
                        name: "becca".into(),
                        entity_type: EntityType::Person,
                        aliases: vec![],
                    }],
                },
                SegmentEntities {
                    segment_idx: 1,
                    entities: vec![],
                },
            ],
        };
        let segs = build_segment_outputs(&result);
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0].segment_idx, 0);
        assert_eq!(segs[0].entities.len(), 1);
        assert_eq!(segs[0].entities[0].name, "becca");
        assert_eq!(segs[0].tag_slugs, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(segs[1].segment_idx, 1);
        assert!(segs[1].entities.is_empty());
        assert_eq!(segs[1].tag_slugs, vec!["c".to_string()]);
    }
}
