//! KG dashboard data assembly — Phase 1D Wave 1D.2 (`mb-j00j`, ADR 0052).
//!
//! Pure-Rust composition of the read-only dashboard payload powering
//! `/knowledge-graph` (the new left-sidebar destination from
//! PHASE-0-5-REPORT §7 / spec §15.3). The IPC layer
//! (`commands::kg::kg_dashboard_snapshot`) is a 3-line wrapper around
//! [`dashboard_snapshot`]; everything testable lives here.
//!
//! ## Why one IPC, not three
//!
//! The Wave 1D.2 kickoff floated three commands
//! (`kg_dashboard_summary`, `kg_recent_activity`, `kg_upcoming_due`);
//! the phase doc (`docs/phases/phase-1d.md` §"Wave 1D.2") collapsed
//! those to one round-trip per dashboard render. We follow the phase
//! doc:
//!
//! - The whole dashboard is one fetch lifecycle, so one IPC is the
//!   right shape (mirrors `kg_entries_summary`'s batched-lookup
//!   precedent — see Wave 1C.3).
//! - Adding bands later (Phase 1E vault projection populates the
//!   upcoming-due band) is a struct-field extension, not a new IPC.
//!
//! ## Composition of existing store helpers
//!
//! The dashboard reads from four places, all of which are already
//! battle-tested in Phase 1B + 1C:
//!
//! - `kg_entities` — total + per-`entity_type` count
//! - `kg_entity_mentions` ∪ `kg_tag_mentions` — distinct
//!   `entry_id` set ⇒ total filed entries
//! - [`super::store::queue::queue_status`] — pending/processing/done/
//!   failed counts + last-success ISO (the `done` field is the 1D.2
//!   addition to that struct)
//! - [`super::store::queue::list_failed`] — flagged-for-review band
//!   (phase doc §1D.2 explicitly equates "flagged" with `state='failed'`
//!   in v1; no schema gap to defer)
//! - per-recent-entry: [`super::store::fetch_session_title_excerpt`]
//!   + [`super::store::search::entries_summary`] for the chip strip
//!
//! Phase 1E will fill the upcoming-due band; for 1D.2 we return an
//! empty `Vec<UpcomingDue>` and the UI renders the empty-state slot
//! per the phase doc instructions.

use std::collections::HashMap;

use rusqlite::Connection;
use serde::Serialize;

use super::store::queue::{self, FailedFiling, QueueStatus};
use super::store::search::{self, EntityRef, EntrySummary, TagRef};
use super::store::{fetch_session_title_excerpt, truncate_title, ENTRY_TITLE_MAX_CHARS};
use crate::error::AppResult;

/// One `(entity_type, count)` row for the counts band.
///
/// `entity_type` is the lowercase wire form per
/// `kg::passes::EntityType::as_str` (`"person"`, `"organization"`,
/// `"object"`, `"place"`, `"project"`); we keep it as a bare `String`
/// so a v1.1+ taxonomy expansion doesn't churn this DTO.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityTypeCount {
    pub entity_type: String,
    pub count: i64,
}

/// Counts band — entity totals + filed-entry totals.
///
/// `entities_by_type` is ordered DESC by `count`, then ASC by
/// `entity_type` (lexicographic) for determinism. `total_entries` is
/// the count of DISTINCT `entry_id` values appearing in either
/// `kg_entity_mentions` or `kg_tag_mentions` — i.e. "dictations that
/// have been filed and produced at least one mention row." A filing
/// that produced zero entities and zero tags would not be counted
/// here; in practice the pipeline always emits at least a category
/// tag, so this is a non-issue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardCounts {
    pub total_entities: i64,
    pub entities_by_type: Vec<EntityTypeCount>,
    pub total_entries: i64,
}

/// One row in the "Recent activity" band — last N filed entries with
/// their per-entry chip strip.
///
/// `title` is the same single-line excerpt the Dictations list shows
/// (final > cleaned > raw fall-through via
/// [`fetch_session_title_excerpt`]), capped at
/// [`ENTRY_TITLE_MAX_CHARS`]. `captured_iso` is `sessions.started_at`.
/// `entities` + `tags` mirror `EntrySummary` (1C.3 shape) so the UI
/// can re-use the existing chip primitives without translation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentActivity {
    pub entry_id: i64,
    pub title: String,
    pub captured_iso: String,
    pub entities: Vec<EntityRef>,
    pub tags: Vec<TagRef>,
}

/// One row in the "Upcoming due dates" band.
///
/// **Always empty in 1D.2** — the underlying field
/// (`Entry::due_iso`) is not persisted to the DB in 1B/1D.1 (only
/// in-memory on the pipeline `Entry` shape; see
/// `kg::schema::Entry::due_iso` + the migration 025 doc note that
/// only `sessions.category` was added in 1D.1). Phase 1E's vault
/// projection ships the persistence + populates this band. The DTO
/// lives here now so 1D.2 ships the slot per the phase doc
/// instructions and Phase 1E adds zero IPC churn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpcomingDue {
    pub entry_id: i64,
    pub title: String,
    pub due_iso: String,
}

/// Full dashboard payload. Single round-trip for the KG screen
/// render. Phase 1E will populate `upcoming_due`; 1F may add a
/// confidence-based "flagged" subset beyond the current
/// state='failed' set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardSnapshot {
    pub counts: DashboardCounts,
    pub queue_status: QueueStatus,
    pub recent_activity: Vec<RecentActivity>,
    pub flagged_for_review: Vec<FailedFiling>,
    pub upcoming_due: Vec<UpcomingDue>,
}

/// Compose the dashboard payload in one read pass. Caller owns the
/// graph-off gate: callers MUST short-circuit and return an empty
/// snapshot when `KgGraphEnabled = false` rather than hit this
/// function (mirrors the existing `kg_*` IPC graph-off pattern;
/// keeps this function deterministic against the DB shape alone).
///
/// `recent_limit` caps the "Recent activity" rows (phase doc: 10).
/// `flagged_limit` caps the "Flagged for review" rows (phase doc:
/// "same set as failed", which the existing list_failed default of
/// 50 already covers; the dashboard band intentionally takes a
/// smaller slice).
pub(crate) fn dashboard_snapshot(
    conn: &Connection,
    recent_limit: u32,
    flagged_limit: u32,
) -> AppResult<DashboardSnapshot> {
    let counts = compute_counts(conn)?;
    let queue_status = queue::queue_status(conn)?;
    let recent_activity = compute_recent_activity(conn, recent_limit)?;
    let flagged_for_review = queue::list_failed(conn, flagged_limit)?;
    // Phase 1E owns the upcoming-due-dates band; see UpcomingDue
    // docstring. Empty in 1D.2 by design.
    let upcoming_due = Vec::new();
    Ok(DashboardSnapshot {
        counts,
        queue_status,
        recent_activity,
        flagged_for_review,
        upcoming_due,
    })
}

/// Build [`DashboardCounts`] in three indexed reads.
fn compute_counts(conn: &Connection) -> AppResult<DashboardCounts> {
    let total_entities: i64 =
        conn.query_row("SELECT COUNT(*) FROM kg_entities;", [], |r| r.get(0))?;

    // GROUP BY over the existing `idx_kg_entities_type` index.
    // Empty kg_entities -> empty vec (no row in the iterator).
    let mut stmt = conn.prepare(
        "SELECT entity_type, COUNT(*) AS cnt \
         FROM kg_entities \
         GROUP BY entity_type \
         ORDER BY cnt DESC, entity_type ASC;",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(EntityTypeCount {
            entity_type: r.get::<_, String>(0)?,
            count: r.get::<_, i64>(1)?,
        })
    })?;
    let mut entities_by_type = Vec::new();
    for row in rows {
        entities_by_type.push(row?);
    }

    // DISTINCT entry_id across both mention tables.
    // UNION (not UNION ALL) collapses duplicates inside the subquery;
    // wrap with COUNT(*) over a single subquery rather than
    // COUNT(DISTINCT ...) on a CROSS JOIN-ish shape (cheaper plan).
    let total_entries: i64 = conn.query_row(
        "SELECT COUNT(*) FROM ( \
           SELECT entry_id FROM kg_entity_mentions \
           UNION \
           SELECT entry_id FROM kg_tag_mentions \
         );",
        [],
        |r| r.get(0),
    )?;

    Ok(DashboardCounts {
        total_entities,
        entities_by_type,
        total_entries,
    })
}

/// Build [`RecentActivity`] rows — the last `limit` filings in the
/// `done` state, newest-first by `finished_at`. Joins to `sessions`
/// for the `started_at` timestamp; reuses
/// [`fetch_session_title_excerpt`] + [`search::entries_summary`] for
/// title + chip-strip composition.
fn compute_recent_activity(conn: &Connection, limit: u32) -> AppResult<Vec<RecentActivity>> {
    // Step 1: which entries are "recently filed"?  Newest done rows
    // win the order; cap at `limit`. Joining to `sessions` here picks
    // up `started_at` in the same pass.
    let mut stmt = conn.prepare(
        "SELECT q.entry_id, s.started_at \
         FROM kg_filing_queue q \
         JOIN sessions s ON s.id = q.entry_id \
         WHERE q.state = 'done' \
         ORDER BY q.finished_at DESC, q.id DESC \
         LIMIT ?1;",
    )?;
    let rows = stmt.query_map(rusqlite::params![limit], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
    })?;
    let mut order: Vec<(i64, String)> = Vec::new();
    for row in rows {
        order.push(row?);
    }
    if order.is_empty() {
        return Ok(Vec::new());
    }

    // Step 2: titles. One indexed point read per entry; mirrors the
    // Dictations-list cost.
    let mut titles: HashMap<i64, String> = HashMap::with_capacity(order.len());
    for (entry_id, _) in &order {
        let raw = fetch_session_title_excerpt(conn, *entry_id)?;
        // fetch_session_title_excerpt already truncates; this is a
        // belt-and-braces for the empty/short cases.
        titles.insert(*entry_id, truncate_title(&raw, ENTRY_TITLE_MAX_CHARS));
    }

    // Step 3: chip strips. Reuses the 1C.3 batched lookup so we
    // ride the same SQL the Dictations list uses — no duplicate
    // query authoring.
    let entry_ids: Vec<i64> = order.iter().map(|(id, _)| *id).collect();
    let summaries: HashMap<i64, EntrySummary> = search::entries_summary(conn, &entry_ids)?;

    // Step 4: stitch. Preserve the order from Step 1; the HashMap
    // lookups are O(1) so this is linear in `limit`.
    let mut out = Vec::with_capacity(order.len());
    for (entry_id, started_at) in order {
        let title = titles.remove(&entry_id).unwrap_or_default();
        let (entities, tags) = match summaries.get(&entry_id) {
            Some(s) => (s.entities.clone(), s.tags.clone()),
            None => (Vec::new(), Vec::new()),
        };
        out.push(RecentActivity {
            entry_id,
            title,
            captured_iso: started_at,
            entities,
            tags,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pin the camelCase wire contract — UI deserializes the IPC
    /// payload by these names; a typo here is a silent UX break
    /// (blank counts, missing chips). Mirrors the
    /// `dtos_serialize_camel_case` precedent in `queue.rs` /
    /// `search.rs`.
    #[test]
    fn dashboard_snapshot_serializes_camel_case() {
        let snap = DashboardSnapshot {
            counts: DashboardCounts {
                total_entities: 3,
                entities_by_type: vec![EntityTypeCount {
                    entity_type: "person".into(),
                    count: 2,
                }],
                total_entries: 5,
            },
            queue_status: QueueStatus {
                pending: 1,
                processing: 0,
                failed: 2,
                done: 7,
                last_done_iso: Some("2026-06-04T00:00:00Z".into()),
            },
            recent_activity: vec![RecentActivity {
                entry_id: 42,
                title: "Hello".into(),
                captured_iso: "2026-06-04T00:00:00Z".into(),
                entities: Vec::new(),
                tags: Vec::new(),
            }],
            flagged_for_review: Vec::new(),
            // One item so the element-level `dueIso` camelCase field
            // actually appears in the serialized output -- with an
            // empty vec the field name is never emitted and the
            // camelCase assertion below has nothing to match.
            // (mb-mac-v1.9: fixture previously left this empty.)
            upcoming_due: vec![UpcomingDue {
                entry_id: 7,
                title: "Renew domain".into(),
                due_iso: "2026-06-30T00:00:00Z".into(),
            }],
        };
        let s = serde_json::to_string(&snap).unwrap();
        for field in [
            "\"counts\"",
            "\"queueStatus\"",
            "\"recentActivity\"",
            "\"flaggedForReview\"",
            "\"upcomingDue\"",
            "\"totalEntities\"",
            "\"entitiesByType\"",
            "\"totalEntries\"",
            "\"entityType\"",
            "\"capturedIso\"",
            "\"dueIso\"",
            // The done field added to QueueStatus in 1D.2.
            "\"done\":7",
        ] {
            assert!(s.contains(field), "missing {field} in {s}");
        }
    }

    #[test]
    fn entity_type_count_orders_by_count_desc_then_name_asc() {
        // Deterministic ordering is the property the UI's counts band
        // depends on; this pins the SQL `ORDER BY` clause.
        let mut rows = [
            EntityTypeCount {
                entity_type: "place".into(),
                count: 2,
            },
            EntityTypeCount {
                entity_type: "person".into(),
                count: 5,
            },
            EntityTypeCount {
                entity_type: "object".into(),
                count: 2,
            },
        ];
        rows.sort_by(|a, b| {
            b.count
                .cmp(&a.count)
                .then(a.entity_type.cmp(&b.entity_type))
        });
        assert_eq!(
            rows.iter()
                .map(|r| r.entity_type.as_str())
                .collect::<Vec<_>>(),
            vec!["person", "object", "place"],
        );
    }

    #[test]
    fn upcoming_due_is_empty_in_1d2() {
        // Phase 1E will populate; in 1D.2 the band is intentionally
        // empty. Re-pinning here so a future drive-by fill that
        // forgets to update the phase doc surfaces as a test
        // failure.
        let snap = DashboardSnapshot {
            counts: DashboardCounts {
                total_entities: 0,
                entities_by_type: Vec::new(),
                total_entries: 0,
            },
            queue_status: QueueStatus {
                pending: 0,
                processing: 0,
                failed: 0,
                done: 0,
                last_done_iso: None,
            },
            recent_activity: Vec::new(),
            flagged_for_review: Vec::new(),
            upcoming_due: Vec::new(),
        };
        assert!(snap.upcoming_due.is_empty());
    }
}
