//! KG settings IPC — Phase 1C Wave 1C.1 (`mb-ucmx`, ADR 0051).
//!
//! Surface for the Settings UI's KG tab. Mirrors the
//! [`meeting_settings_get_all`] / [`meeting_settings_set`] shape from
//! `commands/settings.rs` so the UI's IPC binding layer can follow
//! the same allowlist + typed-snapshot conventions.
//!
//! [`meeting_settings_get_all`]: super::settings::meeting_settings_get_all
//! [`meeting_settings_set`]: super::settings::meeting_settings_set
//!
//! ## Shape
//!
//! - [`kg_settings_get_all`] returns a typed [`KgSettingsSnapshot`].
//!   v1 carries one field (`kg_graph_enabled`); the struct is
//!   forward-compatible — Phase 1C.3+ KG settings (filter defaults,
//!   per-mode opt-in, etc.) land here without an IPC-shape break.
//! - [`kg_settings_set`] is a key/value write gated by
//!   [`is_kg_setting_allowed_for_ui`]. v1 allows only
//!   `kg_graph_enabled`; new keys are an explicit edit to the
//!   allowlist (catches typos + accidental dictation-side writes).
//!
//! ## Wave 1C.2 additions (`mb-9ufg`, ADR 0051)
//!
//! - [`kg_list_failed_filings`] — paginated read of `state='failed'`
//!   rows for the Settings KG tab's failed-filings list.
//! - [`kg_requeue_failed`] — flips a failed row back to `pending`,
//!   resets `attempt_count`, clears `last_error`. **Idempotent** on
//!   already-pending rows (J3 invariant for ADR 0051's Wave 1C.5
//!   judge bundle); a double-click on Retry is a no-op, not an error.
//! - [`kg_queue_status`] — per-state counts + `last_done_iso` for
//!   the "Filing status" line above the failed-filings list.
//!
//! ## Wave 1C.3 additions (`mb-5ly5`, ADR 0051)
//!
//! Four read-side commands powering the Dictations page retrieval UX:
//!
//! - [`kg_search_entries`] — combinable retrieval filter
//!   (entities + tags + free-text query). **Within-axis OR /
//!   across-axis AND.** Returns the matched `entry_id` set
//!   (== `sessions.id`).
//! - [`kg_list_entities`] / [`kg_list_tags`] — prefix autocomplete
//!   for the filter-bar chip pickers, ordered by `mention_count DESC`.
//! - [`kg_entries_summary`] — **batched** per-row chip + filing-state
//!   lookup (single round-trip for a 50-element list page).
//!
//! **Category axis dropped from this wave.** The Wave 1C.3 kickoff
//! brief specified four axes including `categories`, but `category`
//! is not persisted in 1B (`kg::schema::Entry.category` is
//! in-memory only; `apply_filed_outcome` writes only entity + tag
//! mention rows). Filed as `mb-oji5` for a future wave alongside
//! the persistence change. The three axes that DO have queryable
//! data ship in this wave.
//!
//! ## Why not extend `commands/settings.rs`?
//!
//! ADR 0051's wave plan calls out `src-tauri/src/commands/kg.rs` as
//! a new file (scopes nicely to KG; keeps `settings.rs` from sprawling).
//! Future KG-side IPC for filter candidates (1C.3) and concept
//! lookups (1C.4) lands in this same module.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::commands::{into_err, lock_db, AppStateHandle};
use crate::kg::dashboard::{self, DashboardSnapshot};
use crate::kg::store::entities::{self, EntityDetail};
use crate::kg::store::queue::{self, FailedFiling, QueueStatus};
use crate::kg::store::search::{
    self, EntitySuggestion, EntrySummary, SearchFilter, TagDetail, TagSuggestion,
};
use crate::settings::{model::SettingKey, Settings};

/// Default cap on [`kg_list_failed_filings`] when the UI omits the
/// `limit` argument. Matches D1 in the Wave 1C.2 binding parameters.
const DEFAULT_FAILED_FILINGS_LIMIT: u32 = 50;

/// Default cap on [`kg_list_entities`] + [`kg_list_tags`] when the
/// UI omits the `limit` argument. Matches D2 in the Wave 1C.3
/// binding parameters ("defaults limit=50").
const DEFAULT_AUTOCOMPLETE_LIMIT: u32 = 50;

/// Default cap on the `recent_entries` list returned by
/// [`kg_entity_detail`] + [`kg_tag_detail`]. Matches ADR 0051 D4
/// ("Cap visible: 50 recent").
const DEFAULT_CONCEPT_RECENT_LIMIT: u32 = 50;

/// Cap on the "Recent activity" band of [`kg_dashboard_snapshot`].
/// Matches the phase doc (`docs/phases/phase-1d.md` §"Wave 1D.2"):
/// "Last 10 filed entries with timestamps + the per-entry entity /
/// tag chip strip."
const DASHBOARD_RECENT_ACTIVITY_LIMIT: u32 = 10;

/// Cap on the "Flagged for review" band of
/// [`kg_dashboard_snapshot`]. The phase doc equates flagged with
/// `state='failed'` in v1; the band takes a smaller slice than the
/// Settings → KG tab's failed-filings list (which paginates up to
/// `DEFAULT_FAILED_FILINGS_LIMIT`) so the dashboard band stays
/// glanceable.
const DASHBOARD_FLAGGED_LIMIT: u32 = 10;

/// IPC-side wire shape for [`kg_search_entries`]'s filter argument.
/// Mirrors [`SearchFilter`] but uses serde-derive so the camelCase
/// JS payload deserializes cleanly. The store-layer type intentionally
/// stays serde-free because its callers are pure-Rust.
///
/// Tag values are open-vocab `tag_slug` strings (per 1B schema —
/// `kg_canonical_tags` is inert; the slug IS the identifier). The
/// JS side passes strings directly; no synthesized tag ids on the
/// wire.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchFilterArg {
    #[serde(default)]
    pub entities: Vec<i64>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub query: Option<String>,
}

impl From<SearchFilterArg> for SearchFilter {
    fn from(a: SearchFilterArg) -> Self {
        SearchFilter {
            entities: a.entities,
            tags: a.tags,
            query: a.query,
        }
    }
}

/// Typed snapshot of every KG-side setting the UI reads.
///
/// One field today; the struct is the forward-compat boundary so
/// later 1C waves (and 1D backfill) can add fields without
/// breaking the IPC contract.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KgSettingsSnapshot {
    /// Master KG opt-in (per ADR 0050; default `false`). When `false`
    /// the dictation tail does not enqueue and the filing worker
    /// sleeps without dequeuing (Wave 1C.1 boot-vs-poll promotion).
    pub kg_graph_enabled: bool,
}

#[tauri::command]
pub fn kg_settings_get_all(db: State<'_, AppStateHandle>) -> Result<KgSettingsSnapshot, String> {
    let conn = lock_db(&db)?;
    let s = Settings::new(&conn);
    Ok(KgSettingsSnapshot {
        kg_graph_enabled: s
            .get::<bool>(SettingKey::KgGraphEnabled)
            .map_err(into_err)?,
    })
}

#[tauri::command]
pub fn kg_settings_set(
    db: State<'_, AppStateHandle>,
    key: String,
    value: serde_json::Value,
) -> Result<(), String> {
    let setting_key = SettingKey::try_parse(&key).map_err(into_err)?;
    if !is_kg_setting_allowed_for_ui(setting_key) {
        return Err(format!(
            "setting key {key:?} cannot be written via kg_settings_set"
        ));
    }
    let conn = lock_db(&db)?;
    Settings::new(&conn)
        .set_raw(setting_key, &value)
        .map_err(into_err)
}

/// Allowlist for the UI-side KG settings writer.
///
/// v1 allows only `KgGraphEnabled`. Adding a new KG setting that
/// the UI should be able to flip is a one-line edit here AND in
/// the typed [`KgSettingsSnapshot`] above — both edits intentionally
/// land together so the surface stays in sync.
fn is_kg_setting_allowed_for_ui(k: SettingKey) -> bool {
    matches!(k, SettingKey::KgGraphEnabled)
}

/// List rows currently `state='failed'` in `kg_filing_queue`,
/// newest-first by enqueue time. `limit` defaults to
/// [`DEFAULT_FAILED_FILINGS_LIMIT`] when omitted.
///
/// The returned struct is the IPC DTO directly
/// ([`FailedFiling`] in `kg::store::queue`), serialized to camelCase
/// for the JS side. Wave 1C.2 / ADR 0051 D1.
#[tauri::command]
pub fn kg_list_failed_filings(
    db: State<'_, AppStateHandle>,
    limit: Option<u32>,
) -> Result<Vec<FailedFiling>, String> {
    let cap = limit.unwrap_or(DEFAULT_FAILED_FILINGS_LIMIT);
    let conn = lock_db(&db)?;
    queue::list_failed(&conn, cap).map_err(into_err)
}

/// Flip a `state='failed'` row back to `pending` for another shot.
/// Resets `attempt_count=0`, clears `last_error`. **Idempotent**:
/// calling on an already-pending (or missing) row returns `Ok(())`
/// without error -- this is the J3 invariant pinned for ADR 0051's
/// Wave 1C.5 judge bundle. Wave 1C.2 / ADR 0051 D1.
#[tauri::command]
pub fn kg_requeue_failed(db: State<'_, AppStateHandle>, queue_id: i64) -> Result<(), String> {
    let conn = lock_db(&db)?;
    queue::requeue_failed(&conn, queue_id).map_err(into_err)
}

/// Per-state queue counts + the most recent successful filing's
/// timestamp. Drives the "Filing status" line above the failed-
/// filings list. Wave 1C.2 / ADR 0051 D1.
#[tauri::command]
pub fn kg_queue_status(db: State<'_, AppStateHandle>) -> Result<QueueStatus, String> {
    let conn = lock_db(&db)?;
    queue::queue_status(&conn).map_err(into_err)
}

/// Combinable retrieval search. Returns the `entry_id`s matching
/// `filter` (within-axis OR / across-axis AND). An empty filter
/// returns every entry_id that appears in any mention table; the UI
/// should short-circuit and not call this for the no-filter case
/// (use the base `list_sessions` path instead).
///
/// Wave 1C.3 / ADR 0051 D1.
#[tauri::command]
pub fn kg_search_entries(
    db: State<'_, AppStateHandle>,
    filter: SearchFilterArg,
) -> Result<Vec<i64>, String> {
    let f: SearchFilter = filter.into();
    let conn = lock_db(&db)?;
    search::search_entry_ids(&conn, &f).map_err(into_err)
}

/// Entity-chip autocomplete. `prefix=None` returns the global top
/// entities ranked by mention count; passing a string constrains to
/// canonical names starting with it (case-insensitive). `limit`
/// defaults to [`DEFAULT_AUTOCOMPLETE_LIMIT`] when omitted.
///
/// Wave 1C.3 / ADR 0051 D2.
#[tauri::command]
pub fn kg_list_entities(
    db: State<'_, AppStateHandle>,
    prefix: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<EntitySuggestion>, String> {
    let cap = limit.unwrap_or(DEFAULT_AUTOCOMPLETE_LIMIT);
    let conn = lock_db(&db)?;
    search::list_entities(&conn, prefix.as_deref(), cap).map_err(into_err)
}

/// Tag-chip autocomplete. Same shape as [`kg_list_entities`] over
/// the distinct `tag_slug` values in `kg_tag_mentions`. Wave 1C.3 /
/// ADR 0051 D2.
#[tauri::command]
pub fn kg_list_tags(
    db: State<'_, AppStateHandle>,
    prefix: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<TagSuggestion>, String> {
    let cap = limit.unwrap_or(DEFAULT_AUTOCOMPLETE_LIMIT);
    let conn = lock_db(&db)?;
    search::list_tags(&conn, prefix.as_deref(), cap).map_err(into_err)
}

/// Batched per-row chip + filing-state lookup. Single round-trip
/// for the Dictations list page's per-row KG strip — calling per
/// row would 50x the IPC load on a typical page render.
///
/// Wave 1C.3 / ADR 0051 D2 ("batch for per-row display").
#[tauri::command]
pub fn kg_entries_summary(
    db: State<'_, AppStateHandle>,
    entry_ids: Vec<i64>,
) -> Result<HashMap<i64, EntrySummary>, String> {
    let conn = lock_db(&db)?;
    search::entries_summary(&conn, &entry_ids).map_err(into_err)
}

/// Drill-down payload for the concept modal's entity mode
/// (Wave 1C.4 / ADR 0051 D4). Returns header (canonical name,
/// entity_type, aliases), counters (mention_count, total_entries),
/// and the most-recent N entries (cap `recent_limit`, default 50).
///
/// **Error semantics:** an unknown `entity_id` is an error, not an
/// empty payload — the modal should surface a clear toast ("Entity
/// not found") rather than render a blank panel. The store layer
/// returns `AppError::Other("entity not found: <id>")`; the
/// `into_err` shim converts it to a string the sonner toast can
/// display verbatim.
#[tauri::command]
pub fn kg_entity_detail(
    db: State<'_, AppStateHandle>,
    entity_id: i64,
    recent_limit: Option<u32>,
) -> Result<EntityDetail, String> {
    let cap = recent_limit.unwrap_or(DEFAULT_CONCEPT_RECENT_LIMIT);
    let conn = lock_db(&db)?;
    entities::entity_detail(&conn, entity_id, cap).map_err(into_err)
}

/// Composite read-only dashboard snapshot for `/knowledge-graph`
/// (Wave 1D.2 / ADR 0052 §D2). One round-trip per render: counts +
/// queue status + recent activity + flagged-for-review +
/// upcoming-due (empty until Phase 1E populates it).
///
/// **Graph-off contract:** when `KgGraphEnabled = false`, returns an
/// empty snapshot WITHOUT reading the DB. Mirrors the existing
/// `kg_*` IPC pattern; protects the graph-off-UI invariant (J1 from
/// ADR 0051 / extended in 1D.2 to cover the `/knowledge-graph`
/// route) when the route guard fires the IPC by accident (e.g.
/// from a stale URL after the toggle flipped off).
#[tauri::command]
pub fn kg_dashboard_snapshot(db: State<'_, AppStateHandle>) -> Result<DashboardSnapshot, String> {
    let conn = lock_db(&db)?;
    let kg_on = Settings::new(&conn)
        .get::<bool>(SettingKey::KgGraphEnabled)
        .map_err(into_err)?;
    if !kg_on {
        return Ok(empty_dashboard_snapshot());
    }
    dashboard::dashboard_snapshot(
        &conn,
        DASHBOARD_RECENT_ACTIVITY_LIMIT,
        DASHBOARD_FLAGGED_LIMIT,
    )
    .map_err(into_err)
}

/// Empty snapshot returned when the graph is off. Factored out so
/// the test below can pin the exact shape independently of the
/// store-layer composition logic.
fn empty_dashboard_snapshot() -> DashboardSnapshot {
    DashboardSnapshot {
        counts: crate::kg::dashboard::DashboardCounts {
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
    }
}

/// Drill-down payload for the concept modal's tag mode
/// (Wave 1C.4 / ADR 0051 D4).
///
/// **Deviates from the Wave 1C.4 kickoff spec on the argument key:**
/// kickoff prescribed `tag_id: i64` but `kg_canonical_tags` is
/// inert in 1B (LESSONS P11). The slug IS the wire identifier for
/// tags across 1C ([`SearchFilter::tags`], [`TagSuggestion::tag_slug`]);
/// using `tag_slug: String` here keeps the IPC surface internally
/// consistent and side-steps a synthetic-id translation layer that
/// would never round-trip back through the wire. See `TagDetail`'s
/// docstring for the rationale + the v1.1 forward-compat note.
///
/// **Open-vocab semantics:** an unknown slug returns zero counts +
/// empty `recent_entries`, **not** an error. Opening the modal for
/// a slug that the user just typed into the filter bar (before any
/// dictation has been filed against it) is a legitimate state.
#[tauri::command]
pub fn kg_tag_detail(
    db: State<'_, AppStateHandle>,
    tag_slug: String,
    recent_limit: Option<u32>,
) -> Result<TagDetail, String> {
    let cap = recent_limit.unwrap_or(DEFAULT_CONCEPT_RECENT_LIMIT);
    let conn = lock_db(&db)?;
    search::tag_detail(&conn, &tag_slug, cap).map_err(into_err)
}

/// Phase 1D Wave 1D.3 (`mb-0gt6`, ADR 0052) -- KG text-note ingest.
///
/// Synchronous: the React text-note input awaits the inserted
/// `sessions.id`, then re-renders its status pill. The actual KG
/// filing job runs out-of-band in the existing background worker;
/// observers should listen for the standard `history:session-saved`
/// event (fired here) plus the dashboard refetch path.
///
/// Returns the inserted `sessions.id` so the UI can scroll-to-row
/// in the KG dashboard's recent-activity band without a separate
/// lookup round-trip.
///
/// Errors round-trip as `Err(String)` (Tauri convention); the
/// failure modes are limited to (a) empty input (UI should pre-
/// validate but defensive here), (b) DB mutex poisoning (process-
/// fatal in practice -- bubbles up to a toast), or (c) a rare
/// transcript write failure that gets logged and surfaced.
#[tauri::command]
pub fn kg_ingest_text_note(
    db: State<'_, AppStateHandle>,
    config: State<'_, std::sync::Arc<crate::dictation::OrchestratorConfig>>,
    runtime: State<'_, crate::dictation::runtime::DictationRuntime>,
    text: String,
) -> Result<i64, String> {
    crate::kg::ingest_text_note(&db.db, &runtime.recording_window, config.as_ref(), &text)
        .map_err(into_err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowlist_accepts_kg_graph_enabled() {
        assert!(is_kg_setting_allowed_for_ui(SettingKey::KgGraphEnabled));
    }

    #[test]
    fn allowlist_rejects_unrelated_keys() {
        // Dictation-side / meeting-side keys must not be writable
        // through the KG IPC — that's a typo guard, mirrors the
        // meeting_settings allowlist test's posture.
        assert!(!is_kg_setting_allowed_for_ui(SettingKey::Theme));
        assert!(!is_kg_setting_allowed_for_ui(SettingKey::LearningEnabled));
        assert!(!is_kg_setting_allowed_for_ui(SettingKey::MeetingHotkeyKey));
        assert!(!is_kg_setting_allowed_for_ui(
            SettingKey::CommandCenterChord
        ));
    }

    // End-to-end snapshot / set / set-allowlist tests live in the
    // throwaway-crate runner ((LESSONS P2 — `cargo test --release`
    // is broken on this box). The pure-Rust unit tests above cover
    // the allowlist contract; the wired-in-Tauri-State tests would
    // require a managed-state harness that's heavier than the
    // surface warrants and is exercised through the existing
    // `kg_graph_off_invariant` probe end-to-end.
    //
    // Wave 1C.2: the store-layer surface
    // (`list_failed` / `requeue_failed` / `queue_status`) is
    // exhaustively tested in `kg::store::queue::tests` -- IPC
    // command wrappers are 3-line `lock_db` + `map_err` proxies.
    // The wire-shape camelCase contract is tested in queue.rs's
    // `dtos_serialize_camel_case` test.

    #[test]
    fn default_failed_filings_limit_matches_brief() {
        // Brief D1: "defaults limit=50". Pinning the constant here so
        // a typo in the default surfaces as a unit-test failure rather
        // than a quiet UX regression where the UI shows the wrong
        // number of rows.
        assert_eq!(DEFAULT_FAILED_FILINGS_LIMIT, 50);
    }

    #[test]
    fn default_autocomplete_limit_matches_1c3_brief() {
        // Wave 1C.3 D2: "defaults limit=50".
        assert_eq!(DEFAULT_AUTOCOMPLETE_LIMIT, 50);
    }

    #[test]
    fn search_filter_arg_deserializes_camel_case_with_defaults() {
        // Pin the wire contract. The JS side sends camelCase keys;
        // missing fields default to empty / None so the UI can omit
        // axes it isn't using.
        let json = r#"{"entities": [1, 2], "tags": ["family"], "query": "foo"}"#;
        let arg: SearchFilterArg = serde_json::from_str(json).unwrap();
        assert_eq!(arg.entities, vec![1, 2]);
        assert_eq!(arg.tags, vec!["family".to_string()]);
        assert_eq!(arg.query.as_deref(), Some("foo"));

        let arg: SearchFilterArg = serde_json::from_str("{}").unwrap();
        assert!(arg.entities.is_empty());
        assert!(arg.tags.is_empty());
        assert!(arg.query.is_none());
    }

    #[test]
    fn search_filter_arg_into_search_filter_round_trips() {
        let arg = SearchFilterArg {
            entities: vec![10, 20],
            tags: vec!["work".into()],
            query: Some("hi".into()),
        };
        let f: SearchFilter = arg.into();
        assert_eq!(f.entities, vec![10, 20]);
        assert_eq!(f.tags, vec!["work".to_string()]);
        assert_eq!(f.query.as_deref(), Some("hi"));
    }

    #[test]
    fn dashboard_caps_match_phase_1d_brief() {
        // Phase doc §"Wave 1D.2": "Last 10 filed entries with
        // timestamps". Pinning the constants so a UX tweak that
        // edits the cap doesn't silently diverge from the doc.
        assert_eq!(DASHBOARD_RECENT_ACTIVITY_LIMIT, 10);
        assert_eq!(DASHBOARD_FLAGGED_LIMIT, 10);
    }

    #[test]
    fn empty_dashboard_snapshot_is_well_formed() {
        // The graph-off branch of `kg_dashboard_snapshot` returns
        // this without touching the DB. Pin every band so a future
        // change that adds a band to DashboardSnapshot has to think
        // about the off-mode shape too.
        let s = empty_dashboard_snapshot();
        assert_eq!(s.counts.total_entities, 0);
        assert_eq!(s.counts.total_entries, 0);
        assert!(s.counts.entities_by_type.is_empty());
        assert_eq!(s.queue_status.pending, 0);
        assert_eq!(s.queue_status.processing, 0);
        assert_eq!(s.queue_status.failed, 0);
        assert_eq!(s.queue_status.done, 0);
        assert!(s.queue_status.last_done_iso.is_none());
        assert!(s.recent_activity.is_empty());
        assert!(s.flagged_for_review.is_empty());
        assert!(s.upcoming_due.is_empty());
    }

    #[test]
    fn default_concept_recent_limit_matches_1c4_brief() {
        // Wave 1C.4 / ADR 0051 D4: "Cap visible: 50 recent". Pinning
        // the constant so a typo surfaces as a unit-test failure
        // rather than the modal silently truncating to the wrong
        // count.
        assert_eq!(DEFAULT_CONCEPT_RECENT_LIMIT, 50);
    }
}
