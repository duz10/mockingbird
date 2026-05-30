//! KG persistence layer (DB-with-subsystem precedent: `activity/persist.rs`,
//! `meetings/repo.rs`).
//!
//! Owns the five tables created by migration 024 (`kg_entities`,
//! `kg_canonical_tags`, `kg_entity_mentions`, `kg_tag_mentions`,
//! `kg_filing_queue`) and the two concept-page VIEWs. The module is
//! split by table to stay under the 600-LoC cap and to keep each
//! file's mental model small:
//!
//! - [`entities`] — `kg_entities` row reads/writes (canonical name +
//!   type + JSON alias list).
//! - [`mentions`] — `kg_entity_mentions` + `kg_tag_mentions`
//!   per-segment writes (idempotent via UNIQUE constraints).
//! - [`queue`]    — `kg_filing_queue` FIFO state machine
//!   (pending -> processing -> done | failed) + the crash-recovery
//!   sweep + 30-day done-row reaper.
//!
//! ## Public surface (binding parameter D6)
//!
//! Only the worker-side entry points are `pub`:
//!
//! - [`enqueue_for_filing`] — the dictation hook's call site (Chunk 4).
//! - [`apply_filed_outcome`] — the worker's commit-the-result entry
//!   point (Chunk 3).
//!
//! Everything else is `pub(crate)` so the worker (a sibling module in
//! `kg::`) can compose the lower-level pieces without widening the
//! published API.
//!
//! ## Why a store-layer `SegmentOutput` instead of extending `PipelineResult`
//!
//! `kg::PipelineResult` does not today expose per-segment entity data
//! (`run_pipeline` produces `Vec<Entry>` where each `Entry.body` is
//! the segment text but no entity extraction has occurred — the
//! `extract_entities` pass exists but is unwired per its module
//! docstring). Chunk 2's job is to land the persistence half. Whether
//! Chunk 3 populates `SegmentOutput` by wiring `extract_entities` into
//! `run_pipeline` OR by calling it post-hoc in the filing worker is a
//! Chunk 3 design call. Defining `SegmentOutput` here (consumer-side)
//! keeps both paths open and avoids dead code on `PipelineResult` in
//! the interim.
//!
//! ## Idempotency contract (kg-filing-idempotent invariant)
//!
//! [`apply_filed_outcome`] is safe to call twice with the same
//! `(entry_id, &[SegmentOutput])` — the second call collapses to
//! existing rows via SQL UNIQUE constraints + `INSERT OR IGNORE`.
//! Phase 1D backfill and Chunk 3's crash-recovery sweep both depend
//! on this contract. The store-layer tests prove it; the Chunk 5
//! parity probe's `--persist` mode will re-prove it across the
//! fixture matrix.

#![allow(missing_docs)]
// Chunk 2 lands the store-layer surface; Chunk 3's worker is the
// first consumer of `apply_filed_outcome`, `SegmentOutput`, and the
// `queue` sub-module's internals. The dead-code allow keeps the
// clippy gate green without hiding genuine dead code -- the
// `pub(crate)` visibilities mean rustc will start emitting these
// warnings again the moment Chunk 3 fails to wire something.
// Mirrors the `kg::embeddings` precedent.
#![allow(dead_code)]

use rusqlite::Connection;
use serde::Serialize;

use crate::error::AppResult;

use super::passes::ExtractedEntity;

pub(crate) mod entities;
pub(crate) mod mentions;
pub(crate) mod queue;
pub(crate) mod search;

// Re-export the call sites the rest of the crate consumes.
pub use queue::enqueue_for_filing;

/// One entry's summary row, used by the concept-modal recent-entries
/// lists (Wave 1C.4 / ADR 0051 D1). Shared by both
/// [`entities::entity_detail`] and [`search::tag_detail`].
///
/// `title` is a single-line excerpt derived from the dictation
/// transcript (final > cleaned > raw fall-through, mirroring the
/// `pick_summary_text` shape that drives the Dictations list page);
/// truncated to [`ENTRY_TITLE_MAX_CHARS`] so a 50-row modal renders
/// without horizontal scrolling.
///
/// `captured_iso` is `sessions.started_at` (RFC-3339 string on disk).
///
/// `category` is `Option<String>` and is **always `None` in 1C.4**
/// (the pipeline parses an Entry.category but the persistence change
/// is tracked separately as `mb-oji5` -- needs a sessions-table
/// column + a worker write, not in 1C.4 scope). The field stays on
/// the wire so 1C.5+ adds zero IPC churn when `mb-oji5` lands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryRef {
    pub entry_id: i64,
    pub title: String,
    pub captured_iso: String,
    pub category: Option<String>,
}

/// Cap for the single-line excerpt in [`EntryRef::title`]. 80 chars
/// holds about one screen-line in the modal at the design system's
/// body type-scale and matches the truncation the Dictations list
/// row uses for its first-line preview. Larger values wrap; smaller
/// values feel telegraphic.
pub(crate) const ENTRY_TITLE_MAX_CHARS: usize = 80;

/// Truncate to at most `max_chars` user-perceived characters
/// (grapheme-naive — UTF-8 chars; titles are derived from cleaned
/// dictation transcripts which are overwhelmingly ASCII). Appends an
/// ellipsis when truncation happens so the UI can render the
/// shortened form without re-measuring.
pub(crate) fn truncate_title(s: &str, max_chars: usize) -> String {
    let trimmed = s.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let mut out: String = trimmed.chars().take(max_chars).collect();
    out.push('\u{2026}'); // ellipsis
    out
}

/// Fetch the single-line title excerpt for one session, following
/// the same final > cleaned > raw fall-through that
/// `commands/sessions.rs::pick_summary_text` uses for the Dictations
/// list page. Lives here (and not in commands/) so the store layer
/// has no reverse-direction dependency on the commands layer.
///
/// Returns an empty string when no transcript stage exists — the UI
/// renders this as "(empty)" rather than the row disappearing.
pub(crate) fn fetch_session_title_excerpt(conn: &Connection, session_id: i64) -> AppResult<String> {
    // Order matches the documented fall-through. `transcripts` has
    // UNIQUE(session_id, stage) so each lookup is a single indexed
    // point read; total cost is bounded at 3 reads per session and
    // typically resolves on the first (final).
    for stage in ["final", "cleaned", "raw"] {
        let text: Option<String> = conn
            .query_row(
                "SELECT text FROM transcripts WHERE session_id = ?1 AND stage = ?2;",
                rusqlite::params![session_id, stage],
                |r| r.get::<_, String>(0),
            )
            .ok();
        if let Some(t) = text {
            return Ok(truncate_title(&t, ENTRY_TITLE_MAX_CHARS));
        }
    }
    Ok(String::new())
}

/// One segment's worth of pipeline output, in the shape the store
/// layer needs to persist provenance.
///
/// `segment_idx` is the 0-based pipeline segment ordinal (matches
/// `kg_entity_mentions.segment_idx` + `kg_tag_mentions.segment_idx`).
/// `entities` carries the post-extract `ExtractedEntity` rows the
/// `extract_entities` pass produced for this segment; `tag_slugs` is
/// the normalized open-vocab tag list (the same shape `Entry.topic_tags`
/// already carries today).
#[derive(Debug, Clone)]
pub struct SegmentOutput {
    pub segment_idx: usize,
    pub entities: Vec<ExtractedEntity>,
    pub tag_slugs: Vec<String>,
}

/// Persist one filed dictation's worth of segments to the KG tables.
///
/// Wraps a transaction. Upserts canonical entities (merging new
/// aliases into the JSON array), then writes per-segment entity +
/// tag mention rows via `INSERT OR IGNORE`. Idempotent on
/// `(entry_id, &segments)` per the kg-filing-idempotent invariant.
///
/// Callers (the filing worker in Chunk 3) decide whether to call
/// this directly or wrap it in the queue's `mark_done` flow; the
/// store layer doesn't prescribe the transactional boundary.
///
/// `now_iso` is the captured timestamp used for `created_at` /
/// `updated_at`. Threading it as a parameter (rather than
/// `chrono::Utc::now`) keeps the function deterministic — every
/// test (and the parity probe) wants control over the wall clock.
pub fn apply_filed_outcome(
    conn: &Connection,
    entry_id: i64,
    segments: &[SegmentOutput],
    now_iso: &str,
) -> AppResult<()> {
    // We don't open a transaction here on purpose: the caller owns
    // the transactional boundary (the worker pairs this with a
    // `queue::mark_done` in the same txn). Calling sites that want
    // a standalone txn wrap this in `conn.transaction()?`.
    for seg in segments {
        for ent in &seg.entities {
            let entity_id = entities::upsert_entity(
                conn,
                &ent.name,
                ent.entity_type.as_str(),
                &ent.aliases,
                now_iso,
            )?;
            mentions::insert_entity_mention(
                conn,
                entry_id,
                entity_id,
                seg.segment_idx as i64,
                &ent.name,
                now_iso,
            )?;
        }
        for slug in &seg.tag_slugs {
            mentions::insert_tag_mention(conn, entry_id, seg.segment_idx as i64, slug, now_iso)?;
        }
    }
    Ok(())
}
