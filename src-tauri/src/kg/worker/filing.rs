//! Core filing transaction: claim a queue row → run the 5-pass
//! pipeline → persist + project + seal + post-seal artifacts.
//!
//! Owns the end-to-end orchestration; the per-phase work
//! (projection / archive / stubs / index_log) lives in dedicated
//! sibling submodules. Split out of `worker.rs` during Wave 1E.7
//! Part 2 (`mb-5lla`). Behaviour is unchanged.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use rusqlite::{params, Connection};

use crate::db::sessions as db_sessions;
use crate::error::{AppError, AppResult};

use super::super::ollama::{GenerateOptions, OllamaClient};
use super::super::passes::ExtractedEntity;
use super::super::pipeline::{run_pipeline, PipelineResult};
use super::super::schema_loader::Schema;
use super::super::store::queue::mark_done;
use super::super::store::{apply_filed_outcome, SegmentOutput};
use super::archive::maybe_archive_history;
use super::index_log::{maybe_append_log_capture, maybe_rebuild_index_md};
use super::projection::maybe_commit_to_vault;
use super::stubs::{maybe_generate_stub_pages, maybe_generate_tag_stub_pages};
use super::time_iso::now_iso;
use super::transcripts::load_dictation_text;

/// Default filing-pipeline model id. Matches the Wave 0.5.4 fixture
/// model (qwen2.5:7b mid-confident profile). Phase 1C can promote
/// this to a `SettingKey` once the user-facing knob lands; for 1B
/// the hardcoded default keeps the surface tight.
pub(super) const DEFAULT_FILING_MODEL: &str = "qwen2.5:7b-instruct-q4_K_M";

/// Process one claimed queue row end-to-end. Returns `Ok(())` if the
/// entry was filed + the row was marked `done`; `Err(_)` otherwise
/// (caller decides retry vs park).
pub(super) fn process_one(
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
    let mut by_idx: std::collections::HashMap<usize, Vec<ExtractedEntity>> =
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

#[cfg(test)]
mod tests {
    use super::super::super::passes::EntityType;
    use super::super::super::pipeline::{PassTimings, SegmentEntities};
    use super::super::super::schema::{Category, Entry, EntryType, Status};
    use super::*;

    #[test]
    fn build_segment_outputs_joins_entities_with_topic_tags_by_idx() {
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
