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
use std::time::{Duration, Instant, SystemTime};

use rusqlite::{params, Connection, OptionalExtension};

use crate::db::sessions::{self as db_sessions, CaptureKind as DbCaptureKind};
use crate::error::{AppError, AppResult};
use crate::settings::{model::SettingKey, Settings};
use crate::vault::entity_pages::{
    ensure_entity_page, ensure_project_page, ensure_tag_page, StubPageReport,
};
use crate::vault::history::{archive_session_history, HistoryArchiveInput};
use crate::vault::index_md::rebuild_index_md;
use crate::vault::log_md::{append_log_line, LogOp};
use crate::vault::markdown_serializer::{
    slugify_title, CaptureKind as VaultCaptureKind, Category as VaultCategory,
    EntryType as VaultEntryType, KgEntry, Status as VaultStatus,
};
use crate::vault::writer::{commit_entry_to_vault, CommitOutcome};

use super::ollama::{GenerateOptions, OllamaClient};
use super::pipeline::{run_pipeline, PipelineResult};
use super::schema::{
    Category as KgCategory, Entry as KgPipelineEntry, EntryType as KgEntryType, Status as KgStatus,
};
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

    // ── Run the 5-pass pipeline ────────────────────────
    // Per-run seed = entry_id so retries are deterministic for a
    // given dictation. PLAN §8.5 stability requires the caller set
    // a seed.
    let options = GenerateOptions {
        temperature: 0.2,
        seed: Some(entry_id),
        num_ctx: 4096,
    };
    let dictation_id = format!("session-{entry_id}");
    let total_t0 = Instant::now();
    let pipeline_t0 = Instant::now();
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
    let pipeline_run_ms = pipeline_t0.elapsed().as_millis() as u64;

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
    let segment_count = segments.len();
    let pt = result.pass_timings.clone();

    // ── Persist kg_* rows in their own txn (step 1 of ADR 0053 §D4) ──
    // Split from `mark_done` so the file-write step gets to run
    // BEFORE the queue is sealed. A failure in the file-write or
    // seal stage leaves the queue row at `processing`; the boot
    // sweep will revive it; `apply_filed_outcome` is idempotent
    // (kg-filing-idempotent invariant) so the retry is safe.
    let store_t0 = Instant::now();
    {
        let mut c = conn.lock().map_err(|_| {
            AppError::Other("db mutex poisoned in process_one (persist)".to_string())
        })?;
        let tx = c.transaction()?;
        let now = now_iso();
        apply_filed_outcome(&tx, entry_id, &segments, &now)?;
        tx.commit()?;
    }
    let store_apply_ms = store_t0.elapsed().as_millis() as u64;

    // ── Vault projection (steps 2 → 5 of ADR 0053 §D4) ─────────
    // Gated on: (a) KG toggle on (already passed at dequeue time,
    // but re-checked just for symmetry with the vault-path gate),
    // (b) VaultPath configured + non-empty, (c) capture_kind is a
    // KG kind (kg-note / kg-note-text), (d) pipeline produced at
    // least one entry. Failure here is non-fatal to the DB-side
    // filing -- the kg_* rows already landed; the file just
    // doesn't exist yet and `reconcile_vault` will sort it out
    // when the user toggles or runs the IPC.
    let vault_outcome = match maybe_commit_to_vault(conn, entry_id, &result, &captured_iso) {
        Ok(opt) => opt,
        Err(e) => {
            tracing::warn!(
                target: "kg::worker",
                queue_id,
                entry_id,
                error = %e,
                "vault projection failed; kg_* rows already filed; will reconcile later"
            );
            None
        }
    };

    // ── Seal (step 5) + queue_done (step 6) atomically ────────
    let seal_t0 = Instant::now();
    {
        let mut c = conn
            .lock()
            .map_err(|_| AppError::Other("db mutex poisoned in process_one (seal)".to_string()))?;
        let tx = c.transaction()?;
        if let Some(ref outcome) = vault_outcome {
            db_sessions::seal_vault_filing(
                &tx,
                entry_id,
                &outcome.entry_id,
                &outcome.vault_relative_path,
            )?;
        }
        let now = now_iso();
        mark_done(&tx, queue_id, &now)?;
        tx.commit()?;
    }
    let seal_ms = seal_t0.elapsed().as_millis() as u64;
    if let Some(ref outcome) = vault_outcome {
        tracing::info!(
            target: "kg::worker",
            queue_id,
            entry_id,
            vault_path = %outcome.vault_relative_path,
            file_hash = %outcome.file_hash,
            seal_ms,
            "vault projection sealed"
        );
    }

    // ── History archive (ADR 0053 §D7, mb-i14b / 1E.4) ─────────
    // Phase 4: runs strictly AFTER seal + mark_done. Failure here
    // is logged + swallowed (the entry + queue are already sealed;
    // `vault::history::reconcile_history` recovers on demand).
    // Gated on the same conditions as the vault projection -- if
    // the entry didn't get a vault projection, there's no entry_id
    // / vault_path / file_hash to archive against, so we skip.
    if let Some(ref outcome) = vault_outcome {
        if let Err(e) = maybe_archive_history(conn, entry_id, outcome, &captured_iso) {
            tracing::warn!(
                target: "kg::worker",
                queue_id,
                entry_id,
                error = %e,
                "history archive failed; entry already sealed; reconcile_history will recover"
            );
        }
    }

    // ── Entity / Project stub pages (ADR 0053 §D11 / §D12, mb-08za) ─
    // Phase 4b (parallel to history archive): runs strictly AFTER
    // seal + mark_done. Each stub call is independently non-fatal.
    // Stub generation only fires when the entry actually projected
    // to disk (i.e. `vault_outcome.is_some()`); without that there
    // is no `Entries/<...>.md` for the stubs' Dataview queries to
    // reference anyway.
    if vault_outcome.is_some() {
        maybe_generate_stub_pages(conn, queue_id, entry_id, &result);
    }

    // ── PKE Phase 5a: tag stub pages (ADR 0054 §F, mb-bgpt) ────
    // Same post-seal non-fatal pattern as 4b. Stubs only have any
    // value once the entry `.md` is on disk (Dataview's
    // `WHERE contains(tags, "<slug>")` query needs an entry to
    // match), so this is gated on `vault_outcome.is_some()`.
    if vault_outcome.is_some() {
        maybe_generate_tag_stub_pages(conn, queue_id, entry_id, &result);
    }

    // ── PKE Phase 5b: INDEX.md rebuild (ADR 0054 §D, mb-bgpt) ──
    // Full rebuild from DB after every filing -- O(N) over filed
    // entries, fine for the scale of a single user's KG. Always
    // safe to run even when `vault_outcome` was None (the new
    // filing didn't land on disk so the rebuild output happens to
    // equal the previous output; no harm done). The atomic write
    // means a crash mid-rebuild leaves the prior INDEX.md intact.
    maybe_rebuild_index_md(conn, queue_id, entry_id);

    // ── PKE Phase 5c: LOG.md append (ADR 0054 §E, mb-bgpt) ─────
    // Only append when we actually projected to disk: an entry that
    // never reached the vault isn't a "capture" event from the
    // chat-LLM's perspective.
    if let Some(ref outcome) = vault_outcome {
        maybe_append_log_capture(conn, queue_id, entry_id, outcome);
    }

    let total_filing_ms = total_t0.elapsed().as_millis() as u64;

    // Phase 1C.0 (`mb-plz9`, ADR 0051) — structured latency event.
    // One emission per successful filing, log-only (no metrics table
    // in 1C.0; deferred to 1C+ if 1C.2 surfaces a UX-visible need).
    // Field shape is the contract the `kg_latency_bench` binary's
    // CSV output mirrors, so an aggregate of these log records on a
    // live machine is comparable to a one-shot bench run.
    tracing::info!(
        target: "kg::worker::latency",
        queue_id,
        entry_id,
        segment_count,
        pipeline_run_ms,
        segment_ms = pt.segment_ms,
        classify_ms_total = pt.classify_ms_total,
        extract_ms_total = pt.extract_ms_total,
        extract_entities_ms_total = pt.extract_entities_ms_total,
        normalize_ms_total = pt.normalize_ms_total,
        store_apply_ms,
        total_filing_ms,
        "filing latency snapshot"
    );
    Ok(())
}

/// Vault projection step (ADR 0053 §D4, steps 2 → 4): build the
/// in-memory [`KgEntry`] from the pipeline result + session row,
/// then run [`commit_entry_to_vault`].
///
/// Returns:
///   - `Ok(Some(outcome))` -- file successfully landed on disk;
///     caller seals (step 5) + marks queue done (step 6).
///   - `Ok(None)` -- vault projection skipped (no vault configured,
///     capture_kind is not a KG kind, pipeline produced no entries,
///     or KG toggle flipped off mid-run); caller marks queue done
///     without sealing.
///   - `Err(e)` -- two-phase commit failed mid-flight (rare; the
///     row is in a reconcile signature). Caller marks queue done
///     anyway since the kg_* rows are durable.
///
/// Why we still mark queue done on failure: the kg_filing_queue's
/// job is "the LLM pipeline has run for this session". The vault
/// projection is a downstream artefact whose own queue-of-sorts is
/// `sessions.vault_path IS NULL`. Conflating the two would tie the
/// LLM pipeline retry budget (3 attempts) to the file-write retry
/// budget (infinite via reconcile), and the reverse-watcher
/// (1E.5) can't tell which is broken from a `failed` queue row.
fn maybe_commit_to_vault(
    conn: &Arc<Mutex<Connection>>,
    session_id: i64,
    result: &PipelineResult,
    captured_iso: &str,
) -> AppResult<Option<CommitOutcome>> {
    // Snapshot the session row + settings + transcript under a
    // single lock so we don't have to round-trip the mutex four
    // times. The transcript snapshot is what we use for the entry
    // body (`mb-wzui` fix) -- pre-1E.4 we wrote `entries[0].body`
    // which is one segment of the segmenter's output, so any KG
    // session that produced N>1 segments (bullet lists, multi-fact
    // notes) silently dropped segments 1..N from the vault file.
    let snapshot = {
        let c = conn
            .lock()
            .map_err(|_| AppError::Other("db mutex poisoned in maybe_commit_to_vault".into()))?;
        // Re-check the KG toggle defensively. A flip from on -> off
        // while a pipeline is mid-run shouldn't produce a file.
        if !Settings::new(&c)
            .get::<bool>(SettingKey::KgGraphEnabled)
            .unwrap_or(false)
        {
            return Ok(None);
        }
        let vault_path: Option<String> = Settings::new(&c)
            .get::<Option<String>>(SettingKey::VaultPath)
            .ok()
            .flatten();
        let session = db_sessions::get_by_id(&c, session_id)?;
        // final -> cleaned -> raw cascade; same precedence as the
        // Dictations view + history archive (P0 user-visible text).
        let transcript = load_dictation_text(&c, session_id)?;
        (vault_path, session, transcript)
    };
    let (vault_path_opt, session_opt, transcript_opt) = snapshot;

    let vault_path_str = match vault_path_opt {
        Some(s) if !s.trim().is_empty() => s,
        _ => return Ok(None), // not configured -- skip projection silently
    };
    let vault_root = std::path::PathBuf::from(&vault_path_str);

    let session = session_opt.ok_or_else(|| {
        AppError::Other(format!(
            "maybe_commit_to_vault: session id={session_id} disappeared mid-run"
        ))
    })?;

    // Only KG captures get a vault projection in 1E.3. Standard
    // dictation rows stay DB-only (Phase 1E ADR 0053 explicit).
    let vault_capture_kind = match session.capture_kind {
        DbCaptureKind::KgNote => VaultCaptureKind::KgNote,
        DbCaptureKind::KgNoteText => VaultCaptureKind::KgNoteText,
        DbCaptureKind::Dictation => return Ok(None),
    };

    // Pick the canonical Entry to project. v1 maps one session to
    // one markdown file (1:1) using entries[0] as the headline +
    // unioning tags/entities across the run. The multi-entry case
    // (rambly KG-note that produces 3 distinct facts) is filed as
    // mb-1E3-multi-entry P3 -- it produces one file with the
    // first entry's classification and the other entries' tags
    // merged in, which loses some structure but never loses bytes.
    let primary: &KgPipelineEntry = match result.entries.first() {
        Some(e) => e,
        None => return Ok(None), // nothing to project
    };

    // Union tags + entities across all surviving entries / segment
    // entity outputs so a multi-entry session's projection is not
    // silently lossy on the metadata axis (per mb-1E3-multi-entry).
    let mut all_tags: Vec<String> = Vec::new();
    let mut tag_seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for e in &result.entries {
        for t in &e.topic_tags {
            if tag_seen.insert(t.clone()) {
                all_tags.push(t.clone());
            }
        }
    }
    let mut all_entities: Vec<String> = Vec::new();
    let mut ent_seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for seg in &result.segment_entities {
        for ent in &seg.entities {
            if ent_seen.insert(ent.name.clone()) {
                all_entities.push(ent.name.clone());
            }
        }
    }

    let entry = KgEntry {
        id: uuid::Uuid::new_v4().to_string(),
        captured_at: parse_iso_to_utc(captured_iso),
        captured_at_local_date: parse_iso_to_local_date(captured_iso),
        capture_kind: vault_capture_kind,
        title: primary.title.clone(),
        category: kg_category_to_vault(primary.category),
        entry_type: kg_entry_type_to_vault(primary.entry_type),
        status: primary.status.map(kg_status_to_vault),
        due_date: primary.due_iso.as_deref().and_then(parse_iso_to_utc_opt),
        tags: all_tags,
        entities: all_entities,
        source_session_uuid: Some(session.uuid.clone()),
        // Body = full cleaned transcript, NOT `entries[0].body`
        // (which is only segment[0] of the segmenter's output --
        // see `mb-wzui` for the bug this fixes). The cleaned
        // transcript is what the Dictations view shows to the
        // user; the vault projection must round-trip the same
        // bytes so multi-bullet notes don't silently lose items.
        //
        // Fallback to `primary.body` when no transcript row exists
        // (defensive; should never happen for KG captures because
        // the dictation/ingest_text persistence layer always writes
        // transcripts before enqueueing). Multi-entry filing is
        // tracked separately as `mb-ng1o`; until that ships, 1:1
        // session->file means embedding the full transcript here
        // doesn't duplicate anything.
        body: pick_vault_body(transcript_opt.as_deref(), &primary.body),
    };

    // Snapshot of mutex traffic: writer takes its own &Connection
    // borrows (each UPDATE auto-commits) so we can drop the lock
    // around the file write.
    let outcome = {
        let c = conn
            .lock()
            .map_err(|_| AppError::Other("db mutex poisoned in maybe_commit_to_vault".into()))?;
        commit_entry_to_vault(&c, session_id, &entry, &vault_root)?
    };
    Ok(Some(outcome))
}

/// History archive step (ADR 0053 §D7, mb-i14b). Runs AFTER the
/// seal + mark_done transaction so a failure here can't roll back
/// a successful filing. The caller logs + swallows the error.
///
/// Returns `Ok(())` even when the archive was a no-op (idempotent
/// re-call on a session whose sidecar already exists). The
/// distinction is logged via `HistoryArchiveOutcome.archived` inside
/// the helper; the worker doesn't care to differentiate.
fn maybe_archive_history(
    conn: &Arc<Mutex<Connection>>,
    session_id: i64,
    outcome: &CommitOutcome,
    captured_iso: &str,
) -> AppResult<()> {
    // Snapshot under a single short-lived lock: session metadata
    // (uuid, capture_kind, audio_blob_path) + transcripts (raw +
    // cleaned/final) + vault root.
    let (vault_root, session_uuid, capture_kind_db_str, audio_blob_path, raw_text, cleaned_text) = {
        let c = conn
            .lock()
            .map_err(|_| AppError::Other("db mutex poisoned in maybe_archive_history".into()))?;

        let vault_path: Option<String> = Settings::new(&c)
            .get::<Option<String>>(SettingKey::VaultPath)
            .ok()
            .flatten();
        let vault_path_str = match vault_path {
            Some(s) if !s.trim().is_empty() => s,
            _ => {
                // Vault was unconfigured mid-run -- nothing to do.
                return Ok(());
            }
        };

        let session = db_sessions::get_by_id(&c, session_id)?.ok_or_else(|| {
            AppError::Other(format!(
                "maybe_archive_history: session id={session_id} disappeared mid-run"
            ))
        })?;

        let raw = load_transcript_stage(&c, session_id, "raw")?.unwrap_or_default();
        // Prefer the post-cleanup stage; fall back to cleaned, then
        // to empty (sessions whose cleanup pass never ran still
        // archive cleanly).
        let cleaned = load_transcript_stage(&c, session_id, "final")?
            .or(load_transcript_stage(&c, session_id, "cleaned")?)
            .unwrap_or_default();

        (
            std::path::PathBuf::from(vault_path_str),
            session.uuid,
            session.capture_kind.as_db_str().to_string(),
            session.audio_blob_path,
            raw,
            cleaned,
        )
    };

    // Derive the entry filename from the vault-relative POSIX-style
    // path the 1E.3 writer recorded. Last `/`-separated component is
    // the bare filename (always emitted with forward slashes, so the
    // split is host-OS independent).
    let entry_filename = outcome
        .vault_relative_path
        .rsplit('/')
        .next()
        .unwrap_or(&outcome.vault_relative_path)
        .to_string();

    let audio_path_buf = audio_blob_path.as_deref().map(std::path::PathBuf::from);

    let input = HistoryArchiveInput {
        session_id,
        session_uuid: &session_uuid,
        capture_kind: &capture_kind_db_str,
        captured_at: captured_iso,
        raw_transcript: &raw_text,
        cleaned_transcript: &cleaned_text,
        entry_id: &outcome.entry_id,
        entry_filename: &entry_filename,
        vault_file_hash: &outcome.file_hash,
        audio_blob_path: audio_path_buf.as_deref(),
    };

    let archived = archive_session_history(&input, &vault_root)?;
    tracing::info!(
        target: "kg::worker",
        session_id,
        json = %archived.json_path.display(),
        audio = ?archived.audio_path.as_ref().map(|p| p.display().to_string()),
        archived = archived.archived,
        "history archive step complete"
    );
    Ok(())
}

/// Entity + Project stub-page generation step (ADR 0053 §D11 / §D12,
/// amendment `mb-08za`). Phase 4b, runs strictly AFTER the seal +
/// mark_done transaction commits AND after the history archive. Each
/// stub call is independently non-fatal: a write failure is logged
/// via `tracing::warn!` and the next slug continues. Same
/// retry-budget decoupling as the history archive (failure here
/// never re-opens the queue row).
///
/// Why post-seal: the stub pages reference the entry via Dataview
/// (`contains(entities, "[[Entities/<slug>]]")`); for that query to
/// return ANY results the entry's `.md` must already be on disk in
/// `Entries/`, which is true iff the seal + mark_done txn committed.
///
/// Aggregation: we union (slug, EntityType) across
/// `result.segment_entities` and dedupe by slug. First entity_type
/// wins for a slug — the exotic case of "the same slug appears as
/// both Person AND Project in the same dictation" is so unlikely
/// that picking arbitrarily (= first seen) is fine; the stub is
/// write-once anyway, so a subsequent classification can't
/// retroactively flip a Person stub to a Project stub.
///
/// `result` is the `PipelineResult` whose entities drove the entry
/// projection; the slug rule is shared with the serializer via
/// `vault::markdown_serializer::slugify_title`.
fn maybe_generate_stub_pages(
    conn: &Arc<Mutex<Connection>>,
    queue_id: i64,
    entry_id: i64,
    result: &PipelineResult,
) {
    // Snapshot vault root under a short-lived lock. If the toggle
    // flipped off between seal and now, we still write the stubs
    // (the entry is already on disk; matching stubs is the
    // user-friendly behaviour). If the vault root is unset we
    // can't write anything.
    let vault_root_opt: Option<std::path::PathBuf> = {
        let lock = conn.lock();
        let Ok(c) = lock else {
            tracing::warn!(
                target: "kg::worker",
                queue_id,
                entry_id,
                "db mutex poisoned in maybe_generate_stub_pages; skipping"
            );
            return;
        };
        Settings::new(&c)
            .get::<Option<String>>(SettingKey::VaultPath)
            .ok()
            .flatten()
            .filter(|s| !s.trim().is_empty())
            .map(std::path::PathBuf::from)
    };
    let Some(vault_root) = vault_root_opt else {
        return; // not configured -- nothing to do
    };

    // Aggregate (slug, is_project) across all surviving segment
    // entity outputs. BTreeMap for deterministic iteration order
    // (eases log-driven debugging + tests).
    let mut by_slug: std::collections::BTreeMap<String, bool> = std::collections::BTreeMap::new();
    for seg in &result.segment_entities {
        for ent in &seg.entities {
            let slug = slugify_title(&ent.name);
            // `slugify_title` always returns non-empty ("untitled"
            // fallback); guard anyway against future contract drift.
            if slug.is_empty() || slug == "untitled" {
                // Skip the all-symbols / empty entity name case;
                // stub for "untitled" would be useless.
                continue;
            }
            let is_project = matches!(ent.entity_type, super::passes::EntityType::Project);
            // First-seen wins for entity_type. The OR-merge below
            // means a slug seen as both Person AND Project gets
            // Project (we DO want the Project stub if any
            // classification flagged it). This is the only
            // "merge" semantic; everything else is first-seen.
            by_slug
                .entry(slug)
                .and_modify(|v| *v = *v || is_project)
                .or_insert(is_project);
        }
    }

    let now = chrono::Utc::now();
    let mut entity_created = 0usize;
    let mut entity_already = 0usize;
    let mut project_created = 0usize;
    let mut project_already = 0usize;

    for (slug, is_project) in &by_slug {
        match ensure_entity_page(&vault_root, slug, now) {
            Ok(StubPageReport::Created) => {
                entity_created += 1;
            }
            Ok(StubPageReport::AlreadyExists) => {
                entity_already += 1;
            }
            Err(e) => {
                tracing::warn!(
                    target: "kg::worker",
                    queue_id,
                    entry_id,
                    slug,
                    error = %e,
                    "entity stub generation failed; continuing"
                );
            }
        }
        if *is_project {
            match ensure_project_page(&vault_root, slug, now) {
                Ok(StubPageReport::Created) => {
                    project_created += 1;
                }
                Ok(StubPageReport::AlreadyExists) => {
                    project_already += 1;
                }
                Err(e) => {
                    tracing::warn!(
                        target: "kg::worker",
                        queue_id,
                        entry_id,
                        slug,
                        error = %e,
                        "project stub generation failed; continuing"
                    );
                }
            }
        }
    }

    tracing::info!(
        target: "kg::worker",
        queue_id,
        entry_id,
        entity_created,
        entity_already,
        project_created,
        project_already,
        slug_count = by_slug.len(),
        "stub-page generation complete"
    );
}

/// Phase 5a (ADR 0054 §F, mb-bgpt) -- tag stub-page generation.
///
/// Mirrors [`maybe_generate_stub_pages`] but unions tag slugs across
/// `result.entries[*].topic_tags` instead of entity slugs across
/// `result.segment_entities`. Same non-fatal-per-slug semantics: a
/// failed stub write is logged and the loop continues. The pages
/// are write-once (see [`crate::vault::entity_pages::ensure_tag_page`]),
/// so re-firing for a slug that already has a stub is a no-op.
///
/// Vault root resolution + the "toggle flipped off mid-flight" /
/// "vault unconfigured" early returns are identical to the entity
/// path -- copied (not factored) because the read-locked block is
/// 8 lines and a shared helper would obscure the per-phase log
/// targets that LESSONS values for triage.
fn maybe_generate_tag_stub_pages(
    conn: &Arc<Mutex<Connection>>,
    queue_id: i64,
    entry_id: i64,
    result: &PipelineResult,
) {
    let vault_root_opt: Option<std::path::PathBuf> = {
        let lock = conn.lock();
        let Ok(c) = lock else {
            tracing::warn!(
                target: "kg::worker",
                queue_id,
                entry_id,
                "db mutex poisoned in maybe_generate_tag_stub_pages; skipping"
            );
            return;
        };
        Settings::new(&c)
            .get::<Option<String>>(SettingKey::VaultPath)
            .ok()
            .flatten()
            .filter(|s| !s.trim().is_empty())
            .map(std::path::PathBuf::from)
    };
    let Some(vault_root) = vault_root_opt else {
        return;
    };

    // Union tag slugs across every Entry the pipeline produced for
    // this dictation. `topic_tags` is already normalized + canonical
    // by `passes::normalize` / `passes::tag_validator`, so we don't
    // re-slugify -- the canonical form IS the slug.
    let mut slugs: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for entry in &result.entries {
        for tag in &entry.topic_tags {
            let t = tag.trim();
            if t.is_empty() {
                continue;
            }
            slugs.insert(t.to_string());
        }
    }

    let now = chrono::Utc::now();
    let mut created = 0usize;
    let mut already = 0usize;
    for slug in &slugs {
        match ensure_tag_page(&vault_root, slug, now) {
            Ok(StubPageReport::Created) => created += 1,
            Ok(StubPageReport::AlreadyExists) => already += 1,
            Err(e) => {
                tracing::warn!(
                    target: "kg::worker",
                    queue_id,
                    entry_id,
                    slug = %slug,
                    error = %e,
                    "tag stub generation failed; continuing"
                );
            }
        }
    }

    tracing::info!(
        target: "kg::worker",
        queue_id,
        entry_id,
        tag_created = created,
        tag_already = already,
        slug_count = slugs.len(),
        "tag-stub generation complete"
    );
}

/// Phase 5b (ADR 0054 §D, mb-bgpt) -- INDEX.md rebuild from DB.
///
/// Non-fatal on failure: the entry + queue are already sealed, so an
/// INDEX rebuild glitch can't unwind the filing. The next successful
/// rebuild (= next filing) reconciles state. Vault-unconfigured ⇒
/// silent no-op.
fn maybe_rebuild_index_md(conn: &Arc<Mutex<Connection>>, queue_id: i64, entry_id: i64) {
    let lock = conn.lock();
    let Ok(c) = lock else {
        tracing::warn!(
            target: "kg::worker",
            queue_id,
            entry_id,
            "db mutex poisoned in maybe_rebuild_index_md; skipping"
        );
        return;
    };
    let vault_root: Option<std::path::PathBuf> = Settings::new(&c)
        .get::<Option<String>>(SettingKey::VaultPath)
        .ok()
        .flatten()
        .filter(|s| !s.trim().is_empty())
        .map(std::path::PathBuf::from);
    let Some(vault_root) = vault_root else {
        return;
    };
    match rebuild_index_md(&c, &vault_root) {
        Ok(outcome) => tracing::info!(
            target: "kg::worker",
            queue_id,
            entry_id,
            sources = outcome.sources_emitted,
            entities = outcome.entities_emitted,
            projects = outcome.projects_emitted,
            tags = outcome.tags_emitted,
            concepts_preserved = outcome.concepts_preserved,
            "INDEX.md rebuild complete"
        ),
        Err(e) => tracing::warn!(
            target: "kg::worker",
            queue_id,
            entry_id,
            error = %e,
            "INDEX.md rebuild failed; next filing will retry"
        ),
    }
}

/// Phase 5c (ADR 0054 §E, mb-bgpt) -- LOG.md append.
///
/// Subject is derived from the entry filename (slug between the
/// date prefix and the `__id8` suffix). Pulling the title via a
/// fresh DB lookup would round-trip more bytes than the slug is
/// worth for an operations log meant to be skimmed by a human.
fn maybe_append_log_capture(
    conn: &Arc<Mutex<Connection>>,
    queue_id: i64,
    entry_id: i64,
    outcome: &CommitOutcome,
) {
    let vault_root_opt: Option<std::path::PathBuf> = {
        let lock = conn.lock();
        let Ok(c) = lock else {
            tracing::warn!(
                target: "kg::worker",
                queue_id,
                entry_id,
                "db mutex poisoned in maybe_append_log_capture; skipping"
            );
            return;
        };
        Settings::new(&c)
            .get::<Option<String>>(SettingKey::VaultPath)
            .ok()
            .flatten()
            .filter(|s| !s.trim().is_empty())
            .map(std::path::PathBuf::from)
    };
    let Some(vault_root) = vault_root_opt else {
        return;
    };

    let subject = log_subject_from_vault_path(&outcome.vault_relative_path);
    match append_log_line(&vault_root, chrono::Utc::now(), LogOp::Capture, &subject) {
        Ok(o) => tracing::info!(
            target: "kg::worker",
            queue_id,
            entry_id,
            log = %o.path.display(),
            line = %o.line.trim_end(),
            "LOG.md append complete"
        ),
        Err(e) => tracing::warn!(
            target: "kg::worker",
            queue_id,
            entry_id,
            error = %e,
            "LOG.md append failed; entry already sealed"
        ),
    }
}

/// Derive a human-skimmable subject from a vault-relative entry
/// path like
/// `Knowledge Graph/Entries/2026-06-15-buy-milk__abcd1234.md`. The
/// 10-char date prefix + 10-char `__id8.md` suffix get stripped so
/// the middle slug is what surfaces in LOG.md.
fn log_subject_from_vault_path(vault_rel: &str) -> String {
    let basename = vault_rel.rsplit('/').next().unwrap_or(vault_rel);
    let stem = basename.strip_suffix(".md").unwrap_or(basename);
    let without_id = match stem.rfind("__") {
        Some(idx) => &stem[..idx],
        None => stem,
    };
    // Strip `YYYY-MM-DD-` prefix if present.
    let trimmed = if without_id.len() > 11
        && without_id
            .chars()
            .take(10)
            .all(|c| c.is_ascii_digit() || c == '-')
        && without_id.chars().nth(10) == Some('-')
    {
        &without_id[11..]
    } else {
        without_id
    };
    if trimmed.is_empty() {
        basename.to_string()
    } else {
        trimmed.to_string()
    }
}

/// Single-stage transcript fetch. Like `load_dictation_text` but
/// returns just one stage's text -- the history archive wants both
/// raw + cleaned independently, not the first-available cascade.
fn load_transcript_stage(
    conn: &Connection,
    session_id: i64,
    stage: &str,
) -> AppResult<Option<String>> {
    conn.query_row(
        "SELECT text FROM transcripts WHERE session_id = ?1 AND stage = ?2",
        params![session_id, stage],
        |r| r.get::<_, String>(0),
    )
    .optional()
    .map_err(Into::into)
}

// ── KG → vault enum mappings ────────────────────────────
//
// The KG pipeline's vocabulary (5 entry types: Task/Research/Idea/
// Note/Reference; 3 categories; 3 statuses) and the vault markdown
// vocabulary (5 entry types: Note/Task/Idea/Question/Decision; 3
// categories; 3 statuses) drifted apart between Wave 1A (KG schema)
// and Wave 1E.2 (vault serializer). The two sets agree on
// Category and Status but DON'T fully agree on EntryType.
//
// Filed mb-1E3-vocab-drift (P3) to reconcile via a follow-up ADR.
// For 1E.3 we lossy-map the mismatched cases to Note (the
// catch-all). This is durable -- the original kg_entries.entry_type
// is still in the DB; only the vault projection is lossy.

fn kg_category_to_vault(c: KgCategory) -> VaultCategory {
    match c {
        KgCategory::Personal => VaultCategory::Personal,
        KgCategory::Professional => VaultCategory::Professional,
        KgCategory::Objective => VaultCategory::Objective,
    }
}

fn kg_status_to_vault(s: KgStatus) -> VaultStatus {
    match s {
        KgStatus::Todo => VaultStatus::Todo,
        KgStatus::Doing => VaultStatus::Doing,
        KgStatus::Done => VaultStatus::Done,
    }
}

fn kg_entry_type_to_vault(t: KgEntryType) -> VaultEntryType {
    // KG-side has {Task, Research, Idea, Note, Reference}; vault-
    // side now has the nine knowledge shapes from ADR 0054 §G:
    // {Source, Note, Concept, Entity, Project, Question, Decision,
    // Reference, Observation}.
    //
    // The classifier prompt realignment (`mb-qw7n`) is a separate
    // dispatch -- until that ships, the KG pipeline still emits the
    // legacy 5-variant set, so this mapping is the migration bridge:
    //
    //   - Reference -> Reference (pass-through; identical shape)
    //   - Note      -> Note      (pass-through; identical shape)
    //   - Research  -> Reference (research notes point to external
    //                             material being studied -- closer
    //                             to Reference than Note)
    //   - Task      -> Note      (task semantics dropped per ADR 0054
    //                             §G; no semantically richer fit until
    //                             the classifier learns Decision)
    //   - Idea      -> Observation (an idea is the inchoate noticing
    //                               of a pattern -- closest knowledge
    //                               shape is Observation, which the
    //                               chat-LLM Lint pass can
    //                               crystallize into a Concept page
    //                               later)
    //
    // Lossy in the Task -> Note direction (`mb-il83` was originally
    // filed against this gap; ADR 0054 closes the bead by redefining
    // the canonical set rather than by reconciling 5-variant Task).
    match t {
        KgEntryType::Task => VaultEntryType::Note,
        KgEntryType::Idea => VaultEntryType::Observation,
        KgEntryType::Note => VaultEntryType::Note,
        KgEntryType::Research => VaultEntryType::Reference,
        KgEntryType::Reference => VaultEntryType::Reference,
    }
}

/// Permissive ISO-8601 parser that returns a chrono `DateTime<Utc>`.
/// On parse failure falls back to the Unix epoch -- the worker
/// already logs the row context, and the vault projection is
/// non-critical-path, so we don't propagate a typed error here.
fn parse_iso_to_utc(iso: &str) -> chrono::DateTime<chrono::Utc> {
    parse_iso_to_utc_opt(iso).unwrap_or(chrono::DateTime::<chrono::Utc>::UNIX_EPOCH)
}

fn parse_iso_to_utc_opt(iso: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(iso)
        .ok()
        .map(|dt| dt.with_timezone(&chrono::Utc))
}

/// Best-effort local-date extraction from an ISO-8601 string. Used
/// only to drive the markdown filename's `YYYY-MM-DD` prefix.
/// Mirrors `parse_iso_to_utc`'s defensive posture.
fn parse_iso_to_local_date(iso: &str) -> chrono::NaiveDate {
    parse_iso_to_utc_opt(iso)
        .map(|dt| {
            use chrono::TimeZone as _;
            chrono::Local
                .from_utc_datetime(&dt.naive_utc())
                .date_naive()
        })
        .unwrap_or_else(|| {
            chrono::NaiveDate::from_ymd_opt(1970, 1, 1).expect("1970-01-01 is a valid date")
        })
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

/// Pick the body for the vault projection. Prefers the full cleaned
/// transcript (what the user sees in the Dictations view); falls
/// back to the pipeline's first-entry-body only when no transcript
/// row exists at all.
///
/// Extracted as a free function so the body-selection rule is
/// trivially unit-testable without standing up a Connection +
/// filesystem. See `mb-wzui` for the bug this fixes (multi-bullet
/// KG notes were dropping segments 1..N from the vault file because
/// `entries[0].body` is just one segment of the segmenter's output).
///
/// Whitespace-only transcripts fall through to the segment fallback
/// on the theory that an empty transcript is a data-loss signal we
/// shouldn't propagate into the vault file. (The serializer would
/// happily emit a body-less file -- defensible -- but losing the
/// only available content would be worse.)
fn pick_vault_body(transcript: Option<&str>, fallback_segment: &str) -> String {
    match transcript {
        Some(t) if !t.trim().is_empty() => t.to_string(),
        _ => fallback_segment.to_string(),
    }
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
    fn pick_vault_body_prefers_cleaned_transcript_over_segment_zero() {
        // The bug `mb-wzui` fixed: the segmenter slices a bulleted
        // list into N segments, so `entries[0].body` is just the
        // first bullet. The cleaned transcript is the full list.
        let cleaned = "Need to make a quick grocery list. I need to get:\n\
                       - feta cheese\n\
                       - eggs\n\
                       - milk";
        let seg0 = "Need to make a quick grocery list. I need to get: feta cheese";
        let body = pick_vault_body(Some(cleaned), seg0);
        assert_eq!(body, cleaned);
        // The bug symptom -- segment[0] surviving alone -- must not
        // recur. Pinning the negation explicitly.
        assert!(
            body.contains("eggs") && body.contains("milk"),
            "body must contain all bullets, got: {body}"
        );
    }

    #[test]
    fn pick_vault_body_falls_back_when_transcript_missing() {
        let seg0 = "the segment body";
        assert_eq!(pick_vault_body(None, seg0), seg0);
    }

    #[test]
    fn pick_vault_body_falls_back_when_transcript_is_whitespace() {
        // Defensive: a transcripts row that exists but only carries
        // whitespace would otherwise produce a body-less vault file
        // even though the segmenter saw real content.
        let seg0 = "useful fallback";
        assert_eq!(pick_vault_body(Some("   \n\t \n"), seg0), seg0);
    }

    #[test]
    fn pick_vault_body_round_trips_markdown_bullets_verbatim() {
        // The serializer trims trailing newlines but preserves the
        // body content -- so the body we pass MUST be the source of
        // truth, not a lossy summary.
        let cleaned = "- one\n- two\n- three\n";
        let body = pick_vault_body(Some(cleaned), "one");
        assert!(body.contains("- one"));
        assert!(body.contains("- two"));
        assert!(body.contains("- three"));
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

    #[test]
    fn build_segment_outputs_joins_entities_with_topic_tags_by_idx() {
        use super::super::passes::{EntityType, ExtractedEntity};
        use super::super::pipeline::{PassTimings, PipelineResult, SegmentEntities};
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
            pass_timings: PassTimings::default(),
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
