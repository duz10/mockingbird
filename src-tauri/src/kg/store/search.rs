//! Retrieval-axis search + per-row summary helpers (Phase 1C Wave 1C.3 / ADR 0051).
//!
//! This module is the read-side companion to [`super::mentions`] +
//! [`super::entities`] + [`super::queue`]. Three public surfaces feed
//! the Dictations page's KG retrieval UX:
//!
//! 1. [`search_entry_ids`] — combinable retrieval filter. **Within-axis
//!    OR, across-axis AND** per ADR 0051 D1 / Wave 1C.3 brief. Returns
//!    the matched `entry_id` set (== `sessions.id`); the caller
//!    intersects with the existing list rendering.
//! 2. [`list_entities`] / [`list_tags`] — prefix-autocomplete for the
//!    filter-bar chip pickers. Ordered by `mention_count DESC` so
//!    the most-referenced concepts surface first.
//! 3. [`entries_summary`] — batch (`Vec<entry_id>` in, `HashMap` out)
//!    per-row chip + filing-state lookup. Single round-trip from the
//!    UI's perspective so we don't fire-per-row on a 50-element list.
//!
//! ## Why a Rust-side intersect over `INTERSECT` SQL
//!
//! Each retrieval axis is computed by a small parameterized query
//! over the indexed mention tables and the results are intersected
//! in Rust using `HashSet`s. Two reasons:
//!
//! - **No dynamic `IN (?, ?, ...)` placeholder gymnastics.** rusqlite
//!   doesn't bind `Vec<i64>` to an `IN` clause as cleanly as some
//!   ORMs; per-item subqueries with a fixed shape are easier to
//!   audit + test.
//! - **Cheap at UI scale.** A filter-bar realistically carries < 10
//!   chips per axis; per-axis queries are indexed (`idx_kg_entity_mentions_entity`
//!   / `idx_kg_tag_mentions_slug`) and complete sub-millisecond on
//!   a 10k-mention DB. The composition cost in Rust is negligible.
//!
//! Per LESSONS 2026-05-30 Wave 1C.2 Finding 3, every aggregate-style
//! count uses `COUNT(CASE WHEN ... THEN 1 END)` instead of
//! `SUM(CASE WHEN ... THEN 1 ELSE 0 END)` to avoid the
//! NULL-on-empty-table footgun.

use std::collections::{HashMap, HashSet};

use rusqlite::{params, Connection};
use serde::Serialize;

use crate::error::AppResult;

/// Combinable retrieval filter (ADR 0051 D1).
///
/// All three axes are optional / multi-valued; the resolver applies
/// **within-axis OR** + **across-axis AND**:
///
/// - `entities` empty AND `tags` empty AND `query` is None  =>
///   no filter active (caller should defer to the base `list_sessions`
///   path). [`search_entry_ids`] still returns a sensible answer in
///   that case (every entry_id that appears in any mention table)
///   so an unconditional call is safe; it's just wasted work.
/// - `entities = [1, 2]`, others empty => entry_ids that mention
///   entity 1 OR entity 2.
/// - `entities = [1]`, `tags = ["family"]` => entry_ids that mention
///   entity 1 AND tag "family".
/// - `query = Some("foo")` => entry_ids that mention any entity
///   whose canonical name contains "foo" OR any tag whose slug
///   contains "foo" (case-insensitive substring; no FTS in 1B).
///
/// Tag filtering uses `tag_slug` (the open-vocab string from
/// migration 024) NOT a synthetic tag id — `kg_canonical_tags` is
/// inert in 1B so the slug IS the identifier. The IPC contract
/// surfaces this directly; no synthesized ids on the wire.
#[derive(Debug, Clone, Default)]
pub struct SearchFilter {
    pub entities: Vec<i64>,
    pub tags: Vec<String>,
    pub query: Option<String>,
}

impl SearchFilter {
    /// True when every axis is empty / unset — caller should skip
    /// the search and use the base session list instead.
    pub fn is_empty(&self) -> bool {
        // `Option::is_none_or` would be tidier here but it's only
        // stable since 1.82 and our MSRV is 1.77 (AGENTS.md "Rust"
        // section). `map_or` with the empty-string default is the
        // 1.77-compatible equivalent.
        self.entities.is_empty()
            && self.tags.is_empty()
            && self.query.as_ref().map_or(true, |q| q.trim().is_empty())
    }
}

/// Autocomplete suggestion for the entity chip picker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntitySuggestion {
    pub entity_id: i64,
    pub canonical_name: String,
    pub entity_type: String,
    pub mention_count: i64,
}

/// Autocomplete suggestion for the tag chip picker. In 1B tags are
/// open-vocab and identified by their `tag_slug` string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagSuggestion {
    pub tag_slug: String,
    pub mention_count: i64,
}

/// Per-row entity reference for the Dictations list chip strip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityRef {
    pub entity_id: i64,
    pub canonical_name: String,
    pub entity_type: String,
}

/// Per-row tag reference. Slug doubles as identifier + display label
/// in 1B (open-vocab).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagRef {
    pub tag_slug: String,
}

/// Per-row filing-status pill state. `NotEnqueued` is distinct from
/// `Done` so legacy / never-filed sessions (pre-Phase-1B rows,
/// dictations made while `KgGraphEnabled=false`) render no pill
/// rather than misleadingly claiming success.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FilingState {
    NotEnqueued,
    Pending,
    Processing,
    Done,
    Failed,
}

/// Batch per-entry summary returned by [`entries_summary`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntrySummary {
    pub entities: Vec<EntityRef>,
    pub tags: Vec<TagRef>,
    pub filing_state: FilingState,
}

/// Resolve `filter` to the matching set of `entry_id`s.
///
/// Empty filter (every axis unset) returns every `entry_id` that
/// appears in *any* mention table, newest-first by `entry_id` (which
/// is `sessions.id`, monotonic on insert). Callers MAY short-circuit
/// when [`SearchFilter::is_empty`] returns true and use their base
/// list path instead — that's the cheaper UI path.
///
/// Ordering: descending `entry_id`. `entry_id == sessions.id` is
/// AUTOINCREMENT-monotonic, so DESC == "newest first" without
/// having to JOIN `sessions.started_at`.
pub(crate) fn search_entry_ids(conn: &Connection, filter: &SearchFilter) -> AppResult<Vec<i64>> {
    // Collect a per-axis HashSet<i64> of candidate entry_ids. Each
    // axis that's empty is skipped (does not constrain). At the end
    // we intersect everything that contributed.
    let mut axes: Vec<HashSet<i64>> = Vec::with_capacity(3);

    if !filter.entities.is_empty() {
        axes.push(entries_for_entity_ids(conn, &filter.entities)?);
    }
    if !filter.tags.is_empty() {
        axes.push(entries_for_tag_slugs(conn, &filter.tags)?);
    }
    if let Some(q) = filter.query.as_ref() {
        let q = q.trim();
        if !q.is_empty() {
            axes.push(entries_for_query(conn, q)?);
        }
    }

    let intersected: HashSet<i64> = if axes.is_empty() {
        all_mentioned_entry_ids(conn)?
    } else {
        // Across-axis AND = HashSet intersection. Start with the
        // smallest set so the intersection shrinks fast.
        axes.sort_by_key(|s| s.len());
        let mut iter = axes.into_iter();
        let mut acc = iter.next().expect("axes non-empty after check");
        for next in iter {
            acc.retain(|id| next.contains(id));
        }
        acc
    };

    let mut out: Vec<i64> = intersected.into_iter().collect();
    out.sort_unstable_by(|a, b| b.cmp(a)); // DESC = newest first
    Ok(out)
}

/// Within-axis OR over the entity ids: union of `entry_id` sets from
/// `kg_entity_mentions` for each id.
fn entries_for_entity_ids(conn: &Connection, ids: &[i64]) -> AppResult<HashSet<i64>> {
    let mut stmt = conn
        .prepare_cached("SELECT DISTINCT entry_id FROM kg_entity_mentions WHERE entity_id = ?1;")?;
    let mut acc = HashSet::new();
    for id in ids {
        let rows = stmt.query_map(params![id], |r| r.get::<_, i64>(0))?;
        for entry_id in rows {
            acc.insert(entry_id?);
        }
    }
    Ok(acc)
}

/// Within-axis OR over the tag slugs.
fn entries_for_tag_slugs(conn: &Connection, slugs: &[String]) -> AppResult<HashSet<i64>> {
    let mut stmt =
        conn.prepare_cached("SELECT DISTINCT entry_id FROM kg_tag_mentions WHERE tag_slug = ?1;")?;
    let mut acc = HashSet::new();
    for slug in slugs {
        let rows = stmt.query_map(params![slug], |r| r.get::<_, i64>(0))?;
        for entry_id in rows {
            acc.insert(entry_id?);
        }
    }
    Ok(acc)
}

/// Free-text query over entity canonical names + tag slugs.
/// Case-insensitive substring match (LIKE `%q%` with `LOWER`).
fn entries_for_query(conn: &Connection, q: &str) -> AppResult<HashSet<i64>> {
    let needle = format!("%{}%", q.to_lowercase());

    let mut acc = HashSet::new();

    // Entity-name matches.
    let mut stmt = conn.prepare_cached(
        "SELECT DISTINCT m.entry_id \
         FROM kg_entity_mentions m \
         JOIN kg_entities e ON m.entity_id = e.id \
         WHERE LOWER(e.name) LIKE ?1;",
    )?;
    let rows = stmt.query_map(params![needle], |r| r.get::<_, i64>(0))?;
    for entry_id in rows {
        acc.insert(entry_id?);
    }

    // Tag-slug matches.
    let mut stmt = conn.prepare_cached(
        "SELECT DISTINCT entry_id FROM kg_tag_mentions WHERE LOWER(tag_slug) LIKE ?1;",
    )?;
    let rows = stmt.query_map(params![needle], |r| r.get::<_, i64>(0))?;
    for entry_id in rows {
        acc.insert(entry_id?);
    }

    Ok(acc)
}

/// Every `entry_id` that has at least one mention of any kind. Used
/// as the empty-filter fallback so an unconditional call still
/// returns a sensible "all KG-touched entries" set.
fn all_mentioned_entry_ids(conn: &Connection) -> AppResult<HashSet<i64>> {
    let mut acc = HashSet::new();
    let mut stmt = conn.prepare_cached("SELECT DISTINCT entry_id FROM kg_entity_mentions;")?;
    for row in stmt.query_map([], |r| r.get::<_, i64>(0))? {
        acc.insert(row?);
    }
    let mut stmt = conn.prepare_cached("SELECT DISTINCT entry_id FROM kg_tag_mentions;")?;
    for row in stmt.query_map([], |r| r.get::<_, i64>(0))? {
        acc.insert(row?);
    }
    Ok(acc)
}

/// Autocomplete entities by name prefix. `prefix=None` returns the
/// top `limit` entities by mention count globally; passing a string
/// constrains to canonical names starting with it (case-insensitive).
///
/// Ordering is stable: `mention_count DESC` then `name ASC` (so two
/// entities with the same count surface in a predictable order, and
/// snapshot-based UI tests don't flake).
pub(crate) fn list_entities(
    conn: &Connection,
    prefix: Option<&str>,
    limit: u32,
) -> AppResult<Vec<EntitySuggestion>> {
    let prefix_pat = prefix
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .map(|p| format!("{}%", p.to_lowercase()));

    // Two query shapes (with / without prefix) instead of conditional
    // SQL — easier to read, easier to test, indexed identically.
    let mut suggestions = Vec::new();
    if let Some(pat) = prefix_pat {
        let mut stmt = conn.prepare(
            "SELECT e.id, e.name, e.entity_type, COUNT(m.id) AS cnt \
             FROM kg_entities e \
             LEFT JOIN kg_entity_mentions m ON m.entity_id = e.id \
             WHERE LOWER(e.name) LIKE ?1 \
             GROUP BY e.id \
             ORDER BY cnt DESC, e.name ASC \
             LIMIT ?2;",
        )?;
        let rows = stmt.query_map(params![pat, limit as i64], |r| {
            Ok(EntitySuggestion {
                entity_id: r.get(0)?,
                canonical_name: r.get(1)?,
                entity_type: r.get(2)?,
                mention_count: r.get(3)?,
            })
        })?;
        for s in rows {
            suggestions.push(s?);
        }
    } else {
        let mut stmt = conn.prepare(
            "SELECT e.id, e.name, e.entity_type, COUNT(m.id) AS cnt \
             FROM kg_entities e \
             LEFT JOIN kg_entity_mentions m ON m.entity_id = e.id \
             GROUP BY e.id \
             ORDER BY cnt DESC, e.name ASC \
             LIMIT ?1;",
        )?;
        let rows = stmt.query_map(params![limit as i64], |r| {
            Ok(EntitySuggestion {
                entity_id: r.get(0)?,
                canonical_name: r.get(1)?,
                entity_type: r.get(2)?,
                mention_count: r.get(3)?,
            })
        })?;
        for s in rows {
            suggestions.push(s?);
        }
    }
    Ok(suggestions)
}

/// Autocomplete tags. Same shape as [`list_entities`] but over the
/// distinct `tag_slug` values in `kg_tag_mentions`. There is no
/// `kg_tags` table in 1B (open-vocab; slugs live only on mentions).
pub(crate) fn list_tags(
    conn: &Connection,
    prefix: Option<&str>,
    limit: u32,
) -> AppResult<Vec<TagSuggestion>> {
    let prefix_pat = prefix
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .map(|p| format!("{}%", p.to_lowercase()));

    let mut suggestions = Vec::new();
    if let Some(pat) = prefix_pat {
        let mut stmt = conn.prepare(
            "SELECT tag_slug, COUNT(*) AS cnt \
             FROM kg_tag_mentions \
             WHERE LOWER(tag_slug) LIKE ?1 \
             GROUP BY tag_slug \
             ORDER BY cnt DESC, tag_slug ASC \
             LIMIT ?2;",
        )?;
        let rows = stmt.query_map(params![pat, limit as i64], |r| {
            Ok(TagSuggestion {
                tag_slug: r.get(0)?,
                mention_count: r.get(1)?,
            })
        })?;
        for s in rows {
            suggestions.push(s?);
        }
    } else {
        let mut stmt = conn.prepare(
            "SELECT tag_slug, COUNT(*) AS cnt \
             FROM kg_tag_mentions \
             GROUP BY tag_slug \
             ORDER BY cnt DESC, tag_slug ASC \
             LIMIT ?1;",
        )?;
        let rows = stmt.query_map(params![limit as i64], |r| {
            Ok(TagSuggestion {
                tag_slug: r.get(0)?,
                mention_count: r.get(1)?,
            })
        })?;
        for s in rows {
            suggestions.push(s?);
        }
    }
    Ok(suggestions)
}

/// Batch per-entry summary: for each `entry_id` in `entry_ids`,
/// return its entity refs, tag refs, and filing state in a single
/// `HashMap`. Entries with no mentions still get a row in the map
/// (`entities` + `tags` both empty) so the UI's chip rendering loop
/// never has to special-case "no key present". Filing state for
/// `entry_id`s never enqueued is [`FilingState::NotEnqueued`].
///
/// Per-row ordering of `entities` and `tags`: descending by
/// `mention_count` within this entry (most-referenced first), then
/// ASC by name / slug for determinism. The UI caps display at 5 and
/// overflows the rest into "+N more".
pub(crate) fn entries_summary(
    conn: &Connection,
    entry_ids: &[i64],
) -> AppResult<HashMap<i64, EntrySummary>> {
    let mut out: HashMap<i64, EntrySummary> = HashMap::with_capacity(entry_ids.len());
    // Pre-seed every requested id with an empty summary so the UI
    // gets a deterministic shape regardless of what's in the DB.
    for id in entry_ids {
        out.insert(
            *id,
            EntrySummary {
                entities: Vec::new(),
                tags: Vec::new(),
                filing_state: FilingState::NotEnqueued,
            },
        );
    }

    if entry_ids.is_empty() {
        return Ok(out);
    }

    // Entities per entry. Aggregate by (entry_id, entity_id) so we
    // can rank by mention count within the entry.
    let mut stmt = conn.prepare_cached(
        "SELECT m.entry_id, e.id, e.name, e.entity_type, COUNT(*) AS cnt \
         FROM kg_entity_mentions m \
         JOIN kg_entities e ON m.entity_id = e.id \
         WHERE m.entry_id = ?1 \
         GROUP BY m.entry_id, e.id \
         ORDER BY cnt DESC, e.name ASC;",
    )?;
    for entry_id in entry_ids {
        let rows = stmt.query_map(params![entry_id], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                EntityRef {
                    entity_id: r.get(1)?,
                    canonical_name: r.get(2)?,
                    entity_type: r.get(3)?,
                },
            ))
        })?;
        let bucket = out.get_mut(entry_id).expect("pre-seeded");
        for row in rows {
            let (_, eref) = row?;
            bucket.entities.push(eref);
        }
    }

    // Tags per entry.
    let mut stmt = conn.prepare_cached(
        "SELECT entry_id, tag_slug, COUNT(*) AS cnt \
         FROM kg_tag_mentions \
         WHERE entry_id = ?1 \
         GROUP BY entry_id, tag_slug \
         ORDER BY cnt DESC, tag_slug ASC;",
    )?;
    for entry_id in entry_ids {
        let rows = stmt.query_map(params![entry_id], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        })?;
        let bucket = out.get_mut(entry_id).expect("pre-seeded");
        for row in rows {
            let (_, slug) = row?;
            bucket.tags.push(TagRef { tag_slug: slug });
        }
    }

    // Filing state per entry. Single SELECT-per-entry against the
    // unique-on-entry_id queue index; trivially fast.
    let mut stmt = conn.prepare_cached("SELECT state FROM kg_filing_queue WHERE entry_id = ?1;")?;
    for entry_id in entry_ids {
        let state_str: Option<String> = stmt
            .query_row(params![entry_id], |r| r.get::<_, String>(0))
            .ok();
        let state = match state_str.as_deref() {
            Some("pending") => FilingState::Pending,
            Some("processing") => FilingState::Processing,
            Some("done") => FilingState::Done,
            Some("failed") => FilingState::Failed,
            // Unknown state strings fall through to NotEnqueued —
            // the table CHECK constraint should prevent this, but
            // we don't panic on schema drift.
            Some(_) | None => FilingState::NotEnqueued,
        };
        out.get_mut(entry_id).expect("pre-seeded").filing_state = state;
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    /// Test fixture: a single in-memory DB seeded with a tiny, hand-
    /// pickable KG dataset. Keeping the shape small + obvious means
    /// every assertion below can be read against the literal data
    /// rather than running the seed in your head.
    ///
    /// Seeded entries (== sessions == entry_ids):
    ///
    ///   entry 100: entity{Mom (person)} x2, tag{family}
    ///   entry 101: entity{Mom (person)}, entity{Acme (organization)}, tag{family}, tag{work}
    ///   entry 102: entity{Acme (organization)} x3, tag{work}
    ///   entry 103: (no mentions; seeded session row only)
    ///
    /// Queue:
    ///   entry 100 -> done
    ///   entry 101 -> failed
    ///   entry 102 -> pending
    ///   entry 103 -> (no queue row -- NotEnqueued)
    fn seed() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE sessions (id INTEGER PRIMARY KEY);
             CREATE TABLE kg_entities (
               id INTEGER PRIMARY KEY,
               name TEXT NOT NULL,
               entity_type TEXT NOT NULL,
               aliases_json TEXT NOT NULL DEFAULT '[]',
               created_at TEXT NOT NULL,
               updated_at TEXT NOT NULL,
               UNIQUE(name, entity_type)
             );
             CREATE TABLE kg_canonical_tags (id INTEGER PRIMARY KEY);
             CREATE TABLE kg_entity_mentions (
               id INTEGER PRIMARY KEY,
               entry_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
               entity_id INTEGER NOT NULL REFERENCES kg_entities(id) ON DELETE CASCADE,
               segment_idx INTEGER NOT NULL,
               surface_form TEXT NOT NULL,
               created_at TEXT NOT NULL,
               UNIQUE(entry_id, segment_idx, entity_id)
             );
             CREATE TABLE kg_tag_mentions (
               id INTEGER PRIMARY KEY,
               entry_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
               canonical_tag_id INTEGER REFERENCES kg_canonical_tags(id) ON DELETE SET NULL,
               segment_idx INTEGER NOT NULL,
               tag_slug TEXT NOT NULL,
               created_at TEXT NOT NULL,
               UNIQUE(entry_id, segment_idx, tag_slug)
             );
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

             INSERT INTO sessions (id) VALUES (100), (101), (102), (103);

             INSERT INTO kg_entities (id, name, entity_type, created_at, updated_at) VALUES
               (1, 'Mom',  'person',       't', 't'),
               (2, 'Acme', 'organization', 't', 't');

             -- entry 100: Mom x2 (segments 0,1), tag family
             INSERT INTO kg_entity_mentions (entry_id, entity_id, segment_idx, surface_form, created_at) VALUES
               (100, 1, 0, 'Mom', 't'),
               (100, 1, 1, 'Mom', 't');
             INSERT INTO kg_tag_mentions (entry_id, segment_idx, tag_slug, created_at) VALUES
               (100, 0, 'family', 't');

             -- entry 101: Mom, Acme; tags family, work
             INSERT INTO kg_entity_mentions (entry_id, entity_id, segment_idx, surface_form, created_at) VALUES
               (101, 1, 0, 'Mom',  't'),
               (101, 2, 0, 'Acme', 't');
             INSERT INTO kg_tag_mentions (entry_id, segment_idx, tag_slug, created_at) VALUES
               (101, 0, 'family', 't'),
               (101, 0, 'work',   't');

             -- entry 102: Acme x3; tag work
             INSERT INTO kg_entity_mentions (entry_id, entity_id, segment_idx, surface_form, created_at) VALUES
               (102, 2, 0, 'Acme', 't'),
               (102, 2, 1, 'Acme', 't'),
               (102, 2, 2, 'Acme', 't');
             INSERT INTO kg_tag_mentions (entry_id, segment_idx, tag_slug, created_at) VALUES
               (102, 0, 'work', 't');

             -- Queue state per the docstring above.
             INSERT INTO kg_filing_queue (entry_id, state, enqueued_at, finished_at) VALUES
               (100, 'done',    't', 't'),
               (101, 'failed',  't', 't'),
               (102, 'pending', 't', NULL);",
        )
        .unwrap();
        conn
    }

    // -------- SearchFilter::is_empty --------

    #[test]
    fn search_filter_is_empty_for_default() {
        assert!(SearchFilter::default().is_empty());
    }

    #[test]
    fn search_filter_treats_whitespace_only_query_as_empty() {
        let f = SearchFilter {
            entities: vec![],
            tags: vec![],
            query: Some("   ".into()),
        };
        assert!(f.is_empty());
    }

    #[test]
    fn search_filter_not_empty_when_any_axis_set() {
        let f = SearchFilter {
            entities: vec![1],
            tags: vec![],
            query: None,
        };
        assert!(!f.is_empty());
    }

    // -------- search_entry_ids: per-axis correctness --------

    #[test]
    fn search_empty_filter_returns_all_mentioned_entries_newest_first() {
        let c = seed();
        let ids = search_entry_ids(&c, &SearchFilter::default()).unwrap();
        // Entry 103 has no mentions -> not included. 100/101/102 ordered DESC.
        assert_eq!(ids, vec![102, 101, 100]);
    }

    #[test]
    fn search_single_entity_axis_returns_within_axis_or() {
        let c = seed();
        let ids = search_entry_ids(
            &c,
            &SearchFilter {
                entities: vec![1], // Mom
                tags: vec![],
                query: None,
            },
        )
        .unwrap();
        // Mom is in 100 and 101.
        assert_eq!(ids, vec![101, 100]);
    }

    #[test]
    fn search_multi_entity_axis_unions_within_axis() {
        let c = seed();
        let ids = search_entry_ids(
            &c,
            &SearchFilter {
                entities: vec![1, 2], // Mom OR Acme
                tags: vec![],
                query: None,
            },
        )
        .unwrap();
        assert_eq!(ids, vec![102, 101, 100]);
    }

    #[test]
    fn search_single_tag_axis_filters() {
        let c = seed();
        let ids = search_entry_ids(
            &c,
            &SearchFilter {
                entities: vec![],
                tags: vec!["work".into()],
                query: None,
            },
        )
        .unwrap();
        assert_eq!(ids, vec![102, 101]);
    }

    #[test]
    fn search_cross_axis_is_and() {
        let c = seed();
        // entity Mom AND tag work => only entry 101.
        let ids = search_entry_ids(
            &c,
            &SearchFilter {
                entities: vec![1],
                tags: vec!["work".into()],
                query: None,
            },
        )
        .unwrap();
        assert_eq!(ids, vec![101]);
    }

    #[test]
    fn search_query_matches_entity_name_substring_case_insensitive() {
        let c = seed();
        let ids = search_entry_ids(
            &c,
            &SearchFilter {
                entities: vec![],
                tags: vec![],
                query: Some("ACM".into()), // matches "Acme"
            },
        )
        .unwrap();
        assert_eq!(ids, vec![102, 101]);
    }

    #[test]
    fn search_query_matches_tag_slug_substring() {
        let c = seed();
        let ids = search_entry_ids(
            &c,
            &SearchFilter {
                entities: vec![],
                tags: vec![],
                query: Some("fam".into()),
            },
        )
        .unwrap();
        assert_eq!(ids, vec![101, 100]);
    }

    #[test]
    fn search_query_combines_with_chips_via_and() {
        let c = seed();
        // entity Acme AND query "fam" => empty (Acme appears in
        // 101+102, "fam" matches family-tagged 100+101; intersection
        // is 101).
        let ids = search_entry_ids(
            &c,
            &SearchFilter {
                entities: vec![2], // Acme
                tags: vec![],
                query: Some("fam".into()),
            },
        )
        .unwrap();
        assert_eq!(ids, vec![101]);
    }

    // -------- list_entities / list_tags --------

    #[test]
    fn list_entities_no_prefix_orders_by_mention_count_desc() {
        let c = seed();
        let s = list_entities(&c, None, 10).unwrap();
        // Acme has 4 mentions (1+3), Mom has 3 mentions (2+1).
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].canonical_name, "Acme");
        assert_eq!(s[0].mention_count, 4);
        assert_eq!(s[1].canonical_name, "Mom");
        assert_eq!(s[1].mention_count, 3);
    }

    #[test]
    fn list_entities_prefix_constrains_case_insensitive() {
        let c = seed();
        let s = list_entities(&c, Some("mo"), 10).unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].canonical_name, "Mom");
    }

    #[test]
    fn list_entities_respects_limit() {
        let c = seed();
        let s = list_entities(&c, None, 1).unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].canonical_name, "Acme");
    }

    #[test]
    fn list_entities_on_empty_db_returns_empty_vec_not_error() {
        // The NULL-on-empty SQL footgun from LESSONS Wave 1C.2
        // Finding 3 would bite here if we'd used SUM-CASE.
        // COUNT-CASE returns 0; query_map over zero rows yields []
        // cleanly.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE kg_entities (
               id INTEGER PRIMARY KEY,
               name TEXT NOT NULL,
               entity_type TEXT NOT NULL,
               aliases_json TEXT NOT NULL DEFAULT '[]',
               created_at TEXT NOT NULL,
               updated_at TEXT NOT NULL,
               UNIQUE(name, entity_type)
             );
             CREATE TABLE kg_entity_mentions (
               id INTEGER PRIMARY KEY,
               entry_id INTEGER NOT NULL,
               entity_id INTEGER NOT NULL,
               segment_idx INTEGER NOT NULL,
               surface_form TEXT NOT NULL,
               created_at TEXT NOT NULL,
               UNIQUE(entry_id, segment_idx, entity_id)
             );",
        )
        .unwrap();
        let s = list_entities(&conn, None, 10).unwrap();
        assert!(s.is_empty());
        let s = list_entities(&conn, Some("anything"), 10).unwrap();
        assert!(s.is_empty());
    }

    #[test]
    fn list_tags_no_prefix_orders_by_mention_count_desc() {
        let c = seed();
        let s = list_tags(&c, None, 10).unwrap();
        // work appears in 101+102 -> 2 mentions
        // family appears in 100+101 -> 2 mentions
        // Tie -> ASC by slug -> family first.
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].tag_slug, "family");
        assert_eq!(s[1].tag_slug, "work");
    }

    #[test]
    fn list_tags_prefix_constrains() {
        let c = seed();
        let s = list_tags(&c, Some("wo"), 10).unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].tag_slug, "work");
    }

    // -------- entries_summary --------

    #[test]
    fn summary_returns_pre_seeded_empty_for_unknown_entry_id() {
        let c = seed();
        let m = entries_summary(&c, &[999]).unwrap();
        let s = m.get(&999).unwrap();
        assert!(s.entities.is_empty());
        assert!(s.tags.is_empty());
        assert_eq!(s.filing_state, FilingState::NotEnqueued);
    }

    #[test]
    fn summary_ranks_entities_by_mention_count_within_entry() {
        let c = seed();
        let m = entries_summary(&c, &[101]).unwrap();
        let s = &m[&101];
        // 101 has Mom and Acme, both with 1 mention each -> tie
        // breaks ASC by name -> Acme, Mom.
        assert_eq!(s.entities.len(), 2);
        assert_eq!(s.entities[0].canonical_name, "Acme");
        assert_eq!(s.entities[1].canonical_name, "Mom");
    }

    #[test]
    fn summary_filing_state_maps_queue_rows() {
        let c = seed();
        let m = entries_summary(&c, &[100, 101, 102, 103]).unwrap();
        assert_eq!(m[&100].filing_state, FilingState::Done);
        assert_eq!(m[&101].filing_state, FilingState::Failed);
        assert_eq!(m[&102].filing_state, FilingState::Pending);
        assert_eq!(m[&103].filing_state, FilingState::NotEnqueued);
    }

    #[test]
    fn summary_empty_input_returns_empty_map() {
        let c = seed();
        let m = entries_summary(&c, &[]).unwrap();
        assert!(m.is_empty());
    }

    #[test]
    fn summary_includes_every_requested_id_even_with_no_data() {
        // Pre-seed guarantee: caller can iterate `entry_ids` and
        // safely `&map[&id]` without `Option`.
        let c = seed();
        let m = entries_summary(&c, &[100, 9999]).unwrap();
        assert!(m.contains_key(&100));
        assert!(m.contains_key(&9999));
    }

    // -------- DTO wire shape --------

    #[test]
    fn dtos_serialize_camel_case() {
        // The wire contract: every Rust snake_case field becomes
        // camelCase on the JS side. Pinning the contract here so
        // a typo in a `#[serde(rename_all)]` attribute surfaces
        // as a unit-test failure.
        let json = serde_json::to_string(&EntitySuggestion {
            entity_id: 1,
            canonical_name: "Mom".into(),
            entity_type: "person".into(),
            mention_count: 3,
        })
        .unwrap();
        assert!(json.contains("\"entityId\":1"));
        assert!(json.contains("\"canonicalName\":\"Mom\""));
        assert!(json.contains("\"entityType\":\"person\""));
        assert!(json.contains("\"mentionCount\":3"));

        let json = serde_json::to_string(&TagSuggestion {
            tag_slug: "family".into(),
            mention_count: 2,
        })
        .unwrap();
        assert!(json.contains("\"tagSlug\":\"family\""));

        let json = serde_json::to_string(&EntrySummary {
            entities: vec![],
            tags: vec![],
            filing_state: FilingState::NotEnqueued,
        })
        .unwrap();
        assert!(json.contains("\"filingState\":\"not_enqueued\""));
    }
}
