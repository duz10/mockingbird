//! Migration runner.
//!
//! Migrations are embedded via `include_str!` so the shipped binary
//! contains the schema; there is no separate "migrations directory"
//! to mis-deploy. Numeric `schema_version` tracked in `schema_meta`
//! (set by each migration's trailing UPDATE) drives idempotency.

use rusqlite::Connection;

use super::prompt_loader::substitute_prompt_bodies;
use crate::error::{AppError, AppResult};

const MIGRATION_001: &str = include_str!("migrations/001_initial.sql");
const MIGRATION_002: &str = include_str!("migrations/002_audit_triggers.sql");
const MIGRATION_003: &str = include_str!("migrations/003_seed_modes.sql");
const MIGRATION_004: &str = include_str!("migrations/004_injection_status.sql");
const MIGRATION_005: &str = include_str!("migrations/005_ai_command_modes.sql");
const MIGRATION_006: &str = include_str!("migrations/006_prompt_normal_v2.sql");
const MIGRATION_007: &str = include_str!("migrations/007_prompt_normal_v3.sql");
const MIGRATION_008: &str = include_str!("migrations/008_wave2_three_modes.sql");
const MIGRATION_009: &str = include_str!("migrations/009_bump_max_tokens.sql");
const MIGRATION_010: &str = include_str!("migrations/010_adr0024_prompt_v2.sql");
// Phase MC — sibling-subsystem meeting-capture schema (ADR 0026).
// Purely additive; no prompt-body token substitution needed.
const MIGRATION_011: &str = include_str!("migrations/011_meeting_capture.sql");
// Phase 10 Wave 1B — sibling-subsystem activity-capture schema
// (ADR 0036). Purely additive; no prompt-body substitution. Four new
// tables, two immutability triggers; FTS5 deferred to Wave 3.
const MIGRATION_012: &str = include_str!("migrations/012_activity_capture.sql");
// Phase 10 Wave 3 — adds activity_blocks.label + FTS5 contentless
// shadow over (label, generated_abstract). Migration sql comments
// (ADR 0040 §Decision items 4 + 5) walk the design alternatives.
const MIGRATION_013: &str = include_str!("migrations/013_activity_blocks_fts.sql");
// Phase 10 Wave 4 — per-session audio-pipeline provenance columns on
// activity_sessions (audio_whisper_model + audio_chunk_window_ms).
// Purely additive ADD COLUMN, NULL-defaulted. ADR 0041.
const MIGRATION_014: &str = include_str!("migrations/014_activity_audio_provenance.sql");
// Phase 10 Wave 5 — Hardening. ADRs 0042 (retention cascade) + 0043
// (exclusion-rule shape). Adds `activity_exclusion_rules` with
// built-in seed defaults, plus `activity_blocks.raw_events_purged_at`
// breadcrumb column. Purely additive; no triggers; idempotent run
// via the schema_version gate.
const MIGRATION_015: &str = include_str!("migrations/015_activity_wave5_hardening.sql");
// Post-phase-10 hotfix — adds the missing `activity_blocks.primary_title`
// column that migration 012 forgot but every code path in `activity/` has
// always referenced (`blocks_persist.rs`, `blocker.rs`, `abstractor.rs`,
// `assembler.rs`, `export.rs`, `pdf_export.rs`). Schema-vs-code drift
// slipped past the Wave 6 judges because the cargo gate on this box runs
// `test --release --no-run` (LESSONS P2); the link-clean check proves
// types, not SQL. Purely additive ADD COLUMN; no triggers.
const MIGRATION_016: &str = include_str!("migrations/016_activity_blocks_primary_title.sql");
// mb-tfyp / ADR 0045 follow-up — track `start_mode` (ptt | in_app) on
// dictation sessions. Programmatic-start sessions need a distinct list-pill
// label because the ABORTED_FOCUS_CHANGED heuristic is semantically wrong
// for them (Mockingbird is the focus the whole time, there's no target
// app to lose focus to). Pure ADD COLUMN with DEFAULT 'ptt' backfill;
// audit triggers don't cover the `sessions` table, no trigger update.
const MIGRATION_017: &str = include_str!("migrations/017_dictation_start_mode.sql");
// mb-jqhw / ADR 0046: adds `sessions.source` so we can distinguish PTT /
// in-app live-mic sessions ('desktop'), "+ Audio file" desktop import
// ('desktop-import'), and the future mobile-inbox courier flow
// ('mobile-inbox'). Independent of `sessions.start_mode` (migration 017):
// start_mode says "who pressed Start", source says "where the audio came
// from". Pure ADD COLUMN with DEFAULT 'desktop' + a non-unique index for
// the Iter 2 export-job source filter.
const MIGRATION_018: &str = include_str!("migrations/018_session_source.sql");

// 019 — mb-wy9f / ADR 0047 §Wave 1.4: bump `normal` + `formal`
// dictation-mode temperatures from 0.1 to 0.2 so they match the
// already-shipped meetings LLM pass (DEFAULT_TEMPERATURE in
// `meetings/llm_pass.rs`). Pure UPDATE; no prompt bodies; no DDL.
const MIGRATION_019: &str = include_str!("migrations/019_normalize_temperatures.sql");

// 020 — mb-da5t / ADR 0047 §Wave 2.1: ship the `normal_v6_additive`
// prompt body under a dedicated `mode_slug='normal_additive'` so the
// new `DictationCleanupLevel::Medium` branch can look it up
// independently of the tone-mode prompts. Single INSERT into
// `prompts`; schema_version bump 19 -> 20. Requires prompt-body
// substitution.
const MIGRATION_020: &str = include_str!("migrations/020_dictation_cleanup_level.sql");

// 021 — mb-5a4y / ADR 0047 §Wave 2.3: repoint `casual` mode's
// `model_id` from qwen2.5:3b-instruct-q4_K_M to qwen2.5:7b-instruct-
// q4_K_M. Pure UPDATE; no prompt bodies; no DDL. Depends on the
// Wave 2A LLM-skip-on-short-utterance path (commit 7330884) being
// in place so one-liners don't pay the 7B latency.
const MIGRATION_021: &str = include_str!("migrations/021_casual_7b_model.sql");

// 022 — mb-5a4y / ADR 0047 §Wave 2.4: Q5_K_M opt-in checkpoint.
// Documentation-only migration; writes zero rows. The Q5 substitution
// runtime gate lives in `cleanup::llm_cleaner::maybe_promote_to_q5`,
// gated on `SettingKey::PreferQ5Models` (declared in settings/model.rs,
// default false). schema_version bump 21 -> 22 marks the DB as "Q5
// opt-in runtime gate present" so a future migration can branch on it.
const MIGRATION_022: &str = include_str!("migrations/022_q5_model_opt_in.sql");

// 023 -- mb-v2fa / ADR 0047 §Wave 2.5: edit-free-send metric. Adds
// `sessions.edit_free_within_5min INTEGER` (nullable tri-state:
// NULL = not observed, 1 = injected and edit-free within 5 min,
// 0 = injected and user took an edit-equivalent action within 5
// min). Pure ADD COLUMN. Schema header documents the state
// machine + the call sites that flip it.
const MIGRATION_023: &str = include_str!("migrations/023_session_edit_free_send.sql");

// 024 -- mb-geds / ADR 0050 (KG Phase 1B Chunk 2). Adds the KG
// persistence half: kg_entities, kg_canonical_tags (v1.1 inert),
// kg_entity_mentions + kg_tag_mentions (per-segment provenance, D1),
// kg_filing_queue (async filing queue, D3), two concept-page VIEWs
// (D7), immutability triggers on the two mention tables (Principle
// 2 analog), and the seed row for kg_graph_enabled = false (D4).
// FK target: sessions(id) ON DELETE CASCADE. No prompt bodies;
// substituter pass for the leftover-token guard only.
const MIGRATION_024: &str = include_str!("migrations/024_kg_phase_1b.sql");

/// Apply every migration with a version strictly greater than the
/// current `schema_version`. Idempotent — returns Ok early if up-to-date.
///
/// Migration 003 contains `__PROMPT_*_BODY__` tokens that are
/// substituted via [`substitute_prompt_bodies`] before execution. 001
/// and 002 are emitted verbatim.
pub fn apply_all(conn: &Connection) -> AppResult<()> {
    let current = read_current_version(conn)?;

    if current < 1 {
        conn.execute_batch(MIGRATION_001)?;
    }
    if current < 2 {
        conn.execute_batch(MIGRATION_002)?;
    }
    if current < 3 {
        let prepared = substitute_prompt_bodies(MIGRATION_003);
        conn.execute_batch(&prepared)?;
    }
    if current < 4 {
        conn.execute_batch(MIGRATION_004)?;
    }
    if current < 5 {
        let prepared = substitute_prompt_bodies(MIGRATION_005);
        conn.execute_batch(&prepared)?;
    }
    if current < 6 {
        let prepared = substitute_prompt_bodies(MIGRATION_006);
        conn.execute_batch(&prepared)?;
    }
    if current < 7 {
        let prepared = substitute_prompt_bodies(MIGRATION_007);
        conn.execute_batch(&prepared)?;
    }
    if current < 8 {
        let prepared = substitute_prompt_bodies(MIGRATION_008);
        conn.execute_batch(&prepared)?;
    }
    if current < 9 {
        // No prompt-body substitution needed for 009 — pure UPDATE.
        // Run through the substituter anyway so the leftover-token
        // guard catches accidental `__PROMPT_*_BODY__` leaks.
        let prepared = substitute_prompt_bodies(MIGRATION_009);
        conn.execute_batch(&prepared)?;
    }
    if current < 10 {
        // ADR 0024 Wave C: casual_v2 / normal_v5 / formal_v2 prompt
        // bumps. Token substitution required for the three new prompt
        // bodies.
        let prepared = substitute_prompt_bodies(MIGRATION_010);
        conn.execute_batch(&prepared)?;
    }
    if current < 11 {
        // Phase MC: meeting_sessions + meeting_transcripts + FTS5
        // shadow table + insert/delete triggers. No prompt bodies; meeting
        // LLM prompts live as markdown files under
        // `src-tauri/src/meetings/prompts/` and are loaded at call-time
        // by `meetings/llm_pass.rs`, NOT seeded into the DB. Run through
        // the substituter anyway so the leftover-token guard catches
        // accidental `__PROMPT_*_BODY__` leaks in the file.
        let prepared = substitute_prompt_bodies(MIGRATION_011);
        conn.execute_batch(&prepared)?;
    }
    if current < 12 {
        // Phase 10 Wave 1B: activity_sessions + activity_events +
        // activity_blocks + activity_transcript_segments + two
        // immutability triggers on activity_events (Principle 1).
        // No prompt bodies; activity LLM prompts will live as
        // markdown files under `src-tauri/src/activity/prompts/`
        // when Wave 3 lands the abstractor. Run through the
        // substituter for the leftover-token-guard safety net.
        let prepared = substitute_prompt_bodies(MIGRATION_012);
        conn.execute_batch(&prepared)?;
    }
    if current < 13 {
        // Phase 10 Wave 3: adds activity_blocks.label + FTS5
        // contentless shadow over (label, generated_abstract). ADR
        // 0040 §Decision items 4 + 5. Substituter pass for the
        // leftover-token guard.
        let prepared = substitute_prompt_bodies(MIGRATION_013);
        conn.execute_batch(&prepared)?;
    }
    if current < 14 {
        // Phase 10 Wave 4: adds activity_sessions.audio_whisper_model
        // + audio_chunk_window_ms for per-session audio-pipeline
        // provenance (ADR 0041, Principle 2). Pure ADD COLUMN; the
        // substituter pass catches accidental prompt-token leaks.
        let prepared = substitute_prompt_bodies(MIGRATION_014);
        conn.execute_batch(&prepared)?;
    }
    if current < 15 {
        // Phase 10 Wave 5: hardening schema — exclusion rules table
        // (with built-in seed defaults) + activity_blocks.raw_events_
        // purged_at column for retention cascade (ADR 0042 + 0043).
        // Substituter pass for the leftover-token guard.
        let prepared = substitute_prompt_bodies(MIGRATION_015);
        conn.execute_batch(&prepared)?;
    }
    if current < 16 {
        // Post-phase-10 hotfix: adds the missing
        // `activity_blocks.primary_title` column referenced by every
        // code path in `activity/`. Migration 012 forgot it; the
        // schema-vs-code drift went unnoticed because the cargo gate
        // on this box is `test --release --no-run` (LESSONS P2). The
        // live-fire UI surfaced `no such column: primary_title` on
        // the activity session detail page (mb-scla). Substituter
        // pass for the leftover-token guard.
        let prepared = substitute_prompt_bodies(MIGRATION_016);
        conn.execute_batch(&prepared)?;
    }
    if current < 17 {
        // mb-tfyp / ADR 0045 follow-up: adds `sessions.start_mode` so
        // the UI can render IN_APP for programmatic-start sessions
        // instead of the (semantically wrong) ABORTED_FOCUS_CHANGED
        // that the legacy focus-drift abort path produced. Pure ADD
        // COLUMN with DEFAULT 'ptt'. Substituter pass for the
        // leftover-token guard.
        let prepared = substitute_prompt_bodies(MIGRATION_017);
        conn.execute_batch(&prepared)?;
    }
    if current < 18 {
        // mb-jqhw / ADR 0046: adds `sessions.source` ('desktop' default,
        // 'desktop-import' for the + Audio file button, 'mobile-inbox' for
        // the Iter 3 courier flow). Pure ADD COLUMN + CREATE INDEX.
        // Substituter pass for the leftover-token guard.
        let prepared = substitute_prompt_bodies(MIGRATION_018);
        conn.execute_batch(&prepared)?;
    }
    if current < 19 {
        // mb-wy9f / ADR 0047 §Wave 1.4: bump normal + formal dictation
        // temperatures from 0.1 to 0.2 so they match the already-
        // shipped meetings LLM pass (DEFAULT_TEMPERATURE = 0.2 in
        // meetings/llm_pass.rs). Aligns the over-cooled modes with
        // casual's 0.2 baseline (set in migration 010 / ADR 0024 Wave C).
        // Pure UPDATE -- no prompt bodies, no DDL. Substituter pass
        // for the leftover-token guard.
        let prepared = substitute_prompt_bodies(MIGRATION_019);
        conn.execute_batch(&prepared)?;
    }
    if current < 20 {
        // mb-da5t / ADR 0047 §Wave 2.1: ship the normal_v6_additive
        // prompt body under mode_slug='normal_additive', version=1
        // for the DictationCleanupLevel::Medium branch. Append-only
        // INSERT (ADR 0008 compliant). Substitution required for the
        // __PROMPT_NORMAL_V6_ADDITIVE_BODY__ token.
        let prepared = substitute_prompt_bodies(MIGRATION_020);
        conn.execute_batch(&prepared)?;
    }
    if current < 21 {
        // mb-5a4y / ADR 0047 §Wave 2.3: repoint `casual` mode at
        // qwen2.5:7b-instruct-q4_K_M (same model already backing
        // `normal` + `formal`). Wave 2A's LLM-skip path keeps
        // one-liners on the preprocessor-only fast path, so the 7B
        // tax only applies to long-form casual dictations -- which
        // is the population where 3B's over-consolidation symptom
        // bit hardest. Pure UPDATE; no prompt bodies. Substituter
        // pass for the leftover-token guard.
        let prepared = substitute_prompt_bodies(MIGRATION_021);
        conn.execute_batch(&prepared)?;
    }
    if current < 22 {
        // mb-5a4y / ADR 0047 §Wave 2.4: Q5_K_M opt-in checkpoint.
        // Documentation-only migration -- the actual Q5 substitution
        // happens at request time in cleanup/llm_cleaner.rs gated on
        // SettingKey::PreferQ5Models (default false). Bumping
        // schema_version to 22 marks the DB as having the runtime
        // gate present. Substituter pass for the leftover-token
        // guard.
        let prepared = substitute_prompt_bodies(MIGRATION_022);
        conn.execute_batch(&prepared)?;
    }
    if current < 23 {
        // mb-v2fa / ADR 0047 §Wave 2.5: edit-free-send metric.
        // Pure ADD COLUMN -- no prompt bodies, no triggers, no
        // FK changes. Substituter pass for the leftover-token
        // guard only.
        let prepared = substitute_prompt_bodies(MIGRATION_023);
        conn.execute_batch(&prepared)?;
    }
    if current < 24 {
        // mb-geds / ADR 0050 (KG Phase 1B Chunk 2): the KG
        // persistence half. Five tables + two VIEWs + two
        // immutability triggers + one seed row. Substituter pass
        // for the leftover-token guard only -- no prompt bodies.
        let prepared = substitute_prompt_bodies(MIGRATION_024);
        conn.execute_batch(&prepared)?;
    }
    Ok(())
}

/// Read `schema_meta.schema_version`. Returns 0 if `schema_meta`
/// doesn't exist yet (a fresh DB).
fn read_current_version(conn: &Connection) -> AppResult<u32> {
    let exists: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master \
         WHERE type='table' AND name='schema_meta';",
        [],
        |row| row.get(0),
    )?;
    if exists == 0 {
        return Ok(0);
    }

    let raw: String = conn
        .query_row(
            "SELECT value FROM schema_meta WHERE key='schema_version';",
            [],
            |row| row.get(0),
        )
        .unwrap_or_else(|_| "0".to_string());

    raw.parse::<u32>()
        .map_err(|e| AppError::Other(format!("schema_version not parseable: {raw} ({e})")))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `read_current_version` on a brand-new in-memory connection
    /// (no `schema_meta` yet) must return 0, not error.
    #[test]
    fn read_current_version_on_empty_db_returns_zero() {
        let conn = Connection::open_in_memory().expect("open in-memory");
        assert_eq!(read_current_version(&conn).expect("read version"), 0);
    }

    /// Applying all migrations against a fresh in-memory DB must
    /// leave `schema_version` at the current latest. Bump the
    /// expected string here when you add a migration; the migration
    /// `if current < N` block in `apply_all` plus this assert are the
    /// two coupled spots.
    #[test]
    fn apply_all_brings_fresh_db_to_latest_version() {
        let conn = Connection::open_in_memory().expect("open in-memory");
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .expect("pragma fk");
        apply_all(&conn).expect("apply_all");
        let v: String = conn
            .query_row(
                "SELECT value FROM schema_meta WHERE key='schema_version'",
                [],
                |r| r.get(0),
            )
            .expect("read schema_version");
        assert_eq!(v, "24");
    }

    /// Second `apply_all` call against a fully-migrated DB is a no-op.
    /// Bump the expected version string whenever a new migration lands.
    #[test]
    fn apply_all_is_idempotent() {
        let conn = Connection::open_in_memory().expect("open in-memory");
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .expect("pragma fk");
        apply_all(&conn).expect("first apply_all");
        apply_all(&conn).expect("second apply_all should be a no-op");
        let v: String = conn
            .query_row(
                "SELECT value FROM schema_meta WHERE key='schema_version'",
                [],
                |r| r.get(0),
            )
            .expect("read schema_version");
        assert_eq!(v, "24");
    }

    /// Migration 024 (mb-geds / ADR 0050 KG Phase 1B Chunk 2) ships
    /// the KG persistence half. Verify each invariant the rest of
    /// the chunk (and Chunks 3/4) depends on:
    ///   * Five tables exist with the right columns + UNIQUE constraints.
    ///   * Two VIEWs exist and are SELECT-able (empty result is fine).
    ///   * Immutability triggers fire on UPDATE against mention tables.
    ///   * Seed row 'kg_graph_enabled' = 'false' lands in settings.
    ///   * FK target = sessions(id) ON DELETE CASCADE (verified via
    ///     PRAGMA foreign_key_list on each mention/queue table).
    #[test]
    fn migration_024_ships_kg_phase_1b_schema() {
        let conn = Connection::open_in_memory().expect("open in-memory");
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        apply_all(&conn).unwrap();

        // ── Tables exist. ─────────────────────────────────────────
        let table_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master \
                 WHERE type='table' AND name IN \
                   ('kg_entities','kg_canonical_tags','kg_entity_mentions',\
                    'kg_tag_mentions','kg_filing_queue');",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            table_count, 5,
            "all five KG tables must exist after migration 024"
        );

        // ── Views exist. ──────────────────────────────────────────
        let view_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master \
                 WHERE type='view' AND name IN \
                   ('kg_concept_entities_view','kg_concept_tags_view');",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(view_count, 2, "both concept-page VIEWs must exist");
        // Views must be SELECT-able (empty result is fine on a fresh DB).
        conn.query_row("SELECT COUNT(*) FROM kg_concept_entities_view;", [], |r| {
            r.get::<_, i64>(0)
        })
        .expect("kg_concept_entities_view must be queryable");
        conn.query_row("SELECT COUNT(*) FROM kg_concept_tags_view;", [], |r| {
            r.get::<_, i64>(0)
        })
        .expect("kg_concept_tags_view must be queryable");

        // ── Immutability triggers fire on UPDATE. ─────────────────
        // Seed a session row (modes are already seeded by migration 003;
        // we just need to use one of the existing mode IDs). Sessions
        // columns per migration 001: uuid + hotkey_pressed + started_at
        // + recording_ended_at + status + audio_duration_ms are NOT NULL.
        conn.execute_batch(
            "INSERT INTO sessions (id, uuid, mode_id, hotkey_pressed, started_at, \
                 recording_ended_at, status, audio_duration_ms) \
               VALUES (1, 'test-uuid-1', 1, 'RCtrl+Space', \
                       '2026-05-30T00:00:00Z', '2026-05-30T00:00:01Z', \
                       'completed', 1000); \
             INSERT INTO kg_entities (id, name, entity_type, created_at, updated_at) \
               VALUES (1, 'Mom', 'person', '2026-05-30T00:00:00Z', '2026-05-30T00:00:00Z'); \
             INSERT INTO kg_entity_mentions (entry_id, entity_id, segment_idx, \
                 surface_form, created_at) \
               VALUES (1, 1, 0, 'Mom', '2026-05-30T00:00:00Z'); \
             INSERT INTO kg_tag_mentions (entry_id, segment_idx, tag_slug, created_at) \
               VALUES (1, 0, 'family', '2026-05-30T00:00:00Z');",
        )
        .expect("seed rows must apply");

        let entity_update = conn.execute(
            "UPDATE kg_entity_mentions SET surface_form = 'mom' WHERE id = 1;",
            [],
        );
        assert!(
            entity_update.is_err(),
            "kg_entity_mentions UPDATE must be blocked by the no_update trigger"
        );

        let tag_update = conn.execute(
            "UPDATE kg_tag_mentions SET tag_slug = 'kids' WHERE id = 1;",
            [],
        );
        assert!(
            tag_update.is_err(),
            "kg_tag_mentions UPDATE must be blocked by the no_update trigger"
        );

        // ── UNIQUE constraint on mentions enforces idempotency. ──
        let dup_mention = conn.execute(
            "INSERT INTO kg_entity_mentions (entry_id, entity_id, segment_idx, \
                 surface_form, created_at) \
               VALUES (1, 1, 0, 'Mom', '2026-05-30T00:00:00Z');",
            [],
        );
        assert!(
            dup_mention.is_err(),
            "duplicate (entry_id, segment_idx, entity_id) must be rejected"
        );

        // ── FK CASCADE: deleting the session row wipes mentions. ──
        conn.execute("DELETE FROM sessions WHERE id = 1;", [])
            .expect("session delete must succeed");
        let remaining_entity_mentions: i64 = conn
            .query_row("SELECT COUNT(*) FROM kg_entity_mentions;", [], |r| r.get(0))
            .unwrap();
        let remaining_tag_mentions: i64 = conn
            .query_row("SELECT COUNT(*) FROM kg_tag_mentions;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            remaining_entity_mentions, 0,
            "session DELETE must CASCADE into kg_entity_mentions"
        );
        assert_eq!(
            remaining_tag_mentions, 0,
            "session DELETE must CASCADE into kg_tag_mentions"
        );

        // ── Seed row: kg_graph_enabled = 'false'. ─────────────────
        let seed: String = conn
            .query_row(
                "SELECT value FROM settings WHERE key='kg_graph_enabled';",
                [],
                |r| r.get(0),
            )
            .expect("kg_graph_enabled seed row must land in settings");
        assert_eq!(seed, "false");
    }

    /// Migration 023 (mb-v2fa / ADR 0047 §Wave 2.5) adds the
    /// `sessions.edit_free_within_5min` nullable INTEGER column.
    /// Verify shape + default. The state-machine semantics are
    /// covered by integration tests in `db::sessions`.
    #[test]
    fn migration_023_ships_edit_free_within_5min_column() {
        let conn = Connection::open_in_memory().expect("open in-memory");
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        apply_all(&conn).unwrap();

        let mut stmt = conn
            .prepare("PRAGMA table_info(sessions)")
            .expect("prepare table_info");
        let found = stmt
            .query_map([], |row| {
                let name: String = row.get(1)?;
                let ty: String = row.get(2)?;
                let notnull: i64 = row.get(3)?;
                let dflt: Option<String> = row.get(4)?;
                Ok((name, ty, notnull, dflt))
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .find(|(n, _, _, _)| n == "edit_free_within_5min")
            .expect("edit_free_within_5min column should exist after migration 023");
        assert_eq!(found.1, "INTEGER");
        assert_eq!(
            found.2, 0,
            "edit_free_within_5min must be nullable (NULL = not-yet-observed)"
        );
        assert!(
            found.3.is_none(),
            "edit_free_within_5min must have no default; legacy + in-flight rows must read as NULL"
        );
    }

    /// Migration 022 (mb-5a4y / ADR 0047 §Wave 2.4) is a documentation
    /// checkpoint -- writes zero rows, bumps schema_version to 22 to
    /// mark the DB as having the Q5 opt-in runtime gate present. The
    /// actual Q5 substitution happens in `cleanup::llm_cleaner` gated
    /// on `SettingKey::PreferQ5Models`. Verify the schema_version bump
    /// and that no `modes.model_id` was silently changed.
    #[test]
    fn migration_022_bumps_schema_version_without_touching_modes() {
        let conn = Connection::open_in_memory().expect("open in-memory");
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        apply_all(&conn).unwrap();

        // schema_version is at 22 after the full apply_all run.
        let v: String = conn
            .query_row(
                "SELECT value FROM schema_meta WHERE key='schema_version'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(v, "22");

        // Migration 022 must not have moved any modes.model_id. The
        // three tone modes should still all be on qwen2.5:7b-instruct-
        // q4_K_M (set by migrations 008 + 021); the Q5 path is purely
        // a runtime substitution.
        for slug in ["casual", "normal", "formal"] {
            let model: String = conn
                .query_row("SELECT model_id FROM modes WHERE slug = ?1", [slug], |r| {
                    r.get(0)
                })
                .unwrap_or_else(|_| panic!("mode row for slug={slug} should exist"));
            assert_eq!(
                model, "qwen2.5:7b-instruct-q4_K_M",
                "migration 022 must not have changed modes.model_id; slug={slug} got {model}"
            );
        }

        // No row should have been INSERTed into `settings` either --
        // PreferQ5Models's default lives in the typed registry.
        let q5_row_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM settings WHERE key = 'prefer_q5_models'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            q5_row_count, 0,
            "migration 022 must not pre-INSERT the PreferQ5Models row"
        );
    }

    /// Migration 021 (mb-5a4y / ADR 0047 §Wave 2.3) repoints the
    /// `casual` mode at qwen2.5:7b-instruct-q4_K_M so it matches the
    /// model already backing `normal` and `formal`. The 3B compromise
    /// is no longer needed because Wave 2A's LLM-skip-on-short-
    /// utterance path keeps casual one-liners off the LLM entirely
    /// (so the 7B latency only applies to long-form casual
    /// dictations -- the population where 3B's over-consolidation
    /// symptom actually bit).
    #[test]
    fn migration_021_repoints_casual_to_7b_q4_k_m() {
        let conn = Connection::open_in_memory().expect("open in-memory");
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        apply_all(&conn).unwrap();

        let casual_model: String = conn
            .query_row(
                "SELECT model_id FROM modes WHERE slug = 'casual'",
                [],
                |r| r.get(0),
            )
            .expect("casual mode row should exist after migration 021");
        assert_eq!(
            casual_model, "qwen2.5:7b-instruct-q4_K_M",
            "casual should be repointed at the 7B Q4_K_M model"
        );

        // Sibling sanity check -- all three tone modes should now
        // share the same model_id (was the goal of 021). Drift here
        // signals an accidental UPDATE that escaped the WHERE clause.
        for slug in ["casual", "normal", "formal"] {
            let model: String = conn
                .query_row("SELECT model_id FROM modes WHERE slug = ?1", [slug], |r| {
                    r.get(0)
                })
                .unwrap_or_else(|_| panic!("mode row for slug={slug} should exist"));
            assert_eq!(
                model, "qwen2.5:7b-instruct-q4_K_M",
                "slug={slug} expected the 7B Q4_K_M model, got {model}"
            );
        }
    }

    /// Migration 020 (mb-da5t / ADR 0047 §Wave 2.1) ships the
    /// additive-only prompt body under a dedicated mode_slug so the
    /// Medium cleanup-level branch can resolve it without colliding
    /// with the existing casual / normal / formal tone prompts.
    #[test]
    fn migration_020_ships_normal_additive_prompt() {
        let conn = Connection::open_in_memory().expect("open in-memory");
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        apply_all(&conn).unwrap();

        let body: String = conn
            .query_row(
                "SELECT body FROM prompts WHERE mode_slug='normal_additive' AND version=1",
                [],
                |r| r.get(0),
            )
            .expect("normal_additive v1 prompt row should exist after migration 020");
        assert!(
            !body.is_empty(),
            "normal_additive v1 prompt body should not be empty"
        );
        assert!(
            body.to_lowercase().contains("additive"),
            "prompt body should reference its additive nature; got: {body}"
        );

        // Existing normal v5 is the canonical Normal tone prompt and
        // must STILL be the latest version for `mode_slug='normal'`.
        // The additive prompt deliberately uses a separate slug to
        // avoid colliding with the tone-mode latest-version chain.
        let normal_latest_version: i64 = conn
            .query_row(
                "SELECT MAX(version) FROM prompts WHERE mode_slug='normal'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            normal_latest_version, 5,
            "migration 020 must not bump `normal`'s latest version chain"
        );
    }

    /// Migration 018 (mb-jqhw / ADR 0046) adds the `sessions.source`
    /// column with DEFAULT 'desktop' plus a non-unique index for the
    /// future export-job source filter. Verify both shapes land.
    #[test]
    fn migration_018_ships_sessions_source_column_and_index() {
        let conn = Connection::open_in_memory().expect("open in-memory");
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        apply_all(&conn).unwrap();

        // Column exists with the expected type + default.
        let mut stmt = conn
            .prepare("PRAGMA table_info(sessions)")
            .expect("prepare table_info");
        let found = stmt
            .query_map([], |row| {
                let name: String = row.get(1)?;
                let ty: String = row.get(2)?;
                let notnull: i64 = row.get(3)?;
                let dflt: Option<String> = row.get(4)?;
                Ok((name, ty, notnull, dflt))
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .find(|(n, _, _, _)| n == "source")
            .expect("source column should exist after migration 018");
        assert_eq!(found.1, "TEXT");
        assert_eq!(found.2, 1, "source should be NOT NULL");
        assert_eq!(
            found.3.as_deref(),
            Some("'desktop'"),
            "source should default to 'desktop'"
        );

        // Index exists.
        let idx: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master \
                 WHERE type='index' AND name='idx_sessions_source'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            idx, 1,
            "idx_sessions_source should exist after migration 018"
        );
    }

    /// Migration 019 (mb-wy9f / ADR 0047 Wave 1.4) bumps the `normal`
    /// and `formal` mode temperatures from 0.1 to 0.2 so they match
    /// the meetings LLM-pass precedent. `casual` was already 0.2
    /// (migration 010 / ADR 0024 Wave C); we assert all three end up
    /// at the same value so future drift jumps out in this test.
    #[test]
    fn migration_019_normalizes_normal_and_formal_temperatures_to_0_2() {
        let conn = Connection::open_in_memory().expect("open in-memory");
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        apply_all(&conn).unwrap();

        for slug in ["casual", "normal", "formal"] {
            let temp: f64 = conn
                .query_row(
                    "SELECT temperature FROM modes WHERE slug = ?1",
                    [slug],
                    |r| r.get(0),
                )
                .unwrap_or_else(|_| panic!("row for slug={slug} should exist"));
            // f64 equality is fine here -- the value was written
            // verbatim by the migration; no arithmetic in between.
            assert!(
                (temp - 0.2).abs() < f64::EPSILON,
                "slug={slug} expected temperature=0.2, got {temp}"
            );
        }
    }

    /// Migration 017 (mb-tfyp / ADR 0045 follow-up) adds the
    /// `sessions.start_mode` column with DEFAULT 'ptt'. Verify the
    /// column lands and the default backfills legacy rows.
    #[test]
    fn migration_017_ships_sessions_start_mode_column() {
        let conn = Connection::open_in_memory().expect("open in-memory");
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        apply_all(&conn).unwrap();

        // Column exists with the expected type + default.
        let mut stmt = conn
            .prepare("PRAGMA table_info(sessions)")
            .expect("prepare table_info");
        let found = stmt
            .query_map([], |row| {
                let name: String = row.get(1)?;
                let ty: String = row.get(2)?;
                let notnull: i64 = row.get(3)?;
                let dflt: Option<String> = row.get(4)?;
                Ok((name, ty, notnull, dflt))
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .find(|(n, _, _, _)| n == "start_mode")
            .expect("start_mode column should exist after migration 017");
        assert_eq!(found.1, "TEXT");
        assert_eq!(found.2, 1, "start_mode should be NOT NULL");
        assert_eq!(
            found.3.as_deref(),
            Some("'ptt'"),
            "start_mode should default to 'ptt'"
        );
    }

    /// Migration 015 ships the exclusion-rules table + raw_events_
    /// purged_at column. ADR 0042 + 0043; Wave 5. Verify both shapes
    /// land.
    #[test]
    fn migration_015_ships_exclusion_rules_and_block_purged_column() {
        let conn = Connection::open_in_memory().expect("open in-memory");
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        apply_all(&conn).unwrap();

        // Table exists with built-ins seeded.
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM activity_exclusion_rules WHERE is_builtin = 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            count >= 8,
            "expected at least 8 built-in exclusion rules; got {count}"
        );

        // raw_events_purged_at column exists on activity_blocks.
        let mut stmt = conn.prepare("PRAGMA table_info(activity_blocks)").unwrap();
        let cols: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        assert!(
            cols.iter().any(|c| c == "raw_events_purged_at"),
            "activity_blocks missing raw_events_purged_at column; got {cols:?}"
        );
    }

    /// Migration 016 ships the missing `activity_blocks.primary_title`
    /// column that migration 012 forgot. Post-phase-10 hotfix for
    /// mb-scla. Without this column, every read path through
    /// `blocks_persist::list_blocks` raises
    /// `no such column: primary_title` and the activity session
    /// detail UI explodes.
    ///
    /// This test would have flagged the original drift had
    /// `cargo test --release` run live on this box; it stays as a
    /// guard against future schema-vs-code drift on the
    /// `activity_blocks` table.
    #[test]
    fn migration_016_ships_activity_blocks_primary_title_column() {
        let conn = Connection::open_in_memory().expect("open in-memory");
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        apply_all(&conn).unwrap();

        // Column exists.
        let mut stmt = conn.prepare("PRAGMA table_info(activity_blocks)").unwrap();
        let cols: Vec<(String, String, i64)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(1)?, // name
                    row.get::<_, String>(2)?, // type
                    row.get::<_, i64>(3)?,    // notnull
                ))
            })
            .unwrap()
            .map(Result::unwrap)
            .collect();
        let pt = cols
            .iter()
            .find(|(n, _, _)| n == "primary_title")
            .expect("primary_title column should exist after migration 016");
        assert_eq!(pt.1.to_uppercase(), "TEXT");
        assert_eq!(pt.2, 1, "primary_title should be NOT NULL");

        // Round-trip: insert a Block with primary_title, read it back.
        // Proves the SELECT path in `blocks_persist::list_blocks`
        // will succeed.
        conn.execute(
            "INSERT INTO activity_sessions (id, started_at, status, audio_enabled, \
             screenshot_enabled, created_at, updated_at) \
             VALUES ('s-pt', 1000, 'completed', 0, 0, 1000, 1000)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO activity_blocks (id, session_id, started_at, ended_at, \
             primary_app, primary_title, generated_abstract, source_event_ids, \
             prompt_version_sha, user_edited, label, created_at, updated_at) \
             VALUES ('b-pt', 's-pt', 1000, 2000, 'code.exe', \
             'editing migrations.rs', NULL, '[]', 'sha-x', 0, NULL, 1000, 1000)",
            [],
        )
        .unwrap();
        let title: String = conn
            .query_row(
                "SELECT primary_title FROM activity_blocks WHERE id = 'b-pt'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(title, "editing migrations.rs");
    }

    /// Migration 014 ships per-session audio-pipeline provenance
    /// columns. ADR 0041; Wave 4 writes these when `audio_enabled = 1`.
    /// Pre-Wave-4 sessions stay NULL forever (no audio = no provenance).
    #[test]
    fn migration_014_ships_activity_audio_provenance_columns() {
        let conn = Connection::open_in_memory().expect("open in-memory");
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        apply_all(&conn).unwrap();

        let mut stmt = conn
            .prepare("PRAGMA table_info(activity_sessions)")
            .unwrap();
        let cols: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        for c in ["audio_whisper_model", "audio_chunk_window_ms"] {
            assert!(
                cols.iter().any(|x| x == c),
                "missing activity_sessions.{c}; cols: {cols:?}"
            );
        }

        // Round-trip: insert a session with the new columns populated,
        // read it back. Proves the columns are writable and the
        // types line up.
        conn.execute(
            "INSERT INTO activity_sessions (id, started_at, status, audio_enabled, \
             screenshot_enabled, audio_whisper_model, audio_chunk_window_ms, \
             created_at, updated_at) \
             VALUES ('s-audio', 1000, 'in_progress', 1, 0, \
             'whisper-large-v3-turbo-q5_0', 30000, 1000, 1000)",
            [],
        )
        .unwrap();
        let (model, win): (String, i64) = conn
            .query_row(
                "SELECT audio_whisper_model, audio_chunk_window_ms \
                 FROM activity_sessions WHERE id = 's-audio'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(model, "whisper-large-v3-turbo-q5_0");
        assert_eq!(win, 30_000);
    }

    /// Migration 014 must stay in its sibling-subsystem lane.
    #[test]
    fn migration_014_does_not_touch_dictation_or_meeting_tables() {
        let sql = MIGRATION_014.to_lowercase();
        for forbidden in [
            "alter table sessions",
            "alter table transcripts",
            "alter table modes",
            "alter table meeting_sessions",
            "alter table meeting_transcripts",
            "drop table",
        ] {
            assert!(
                !sql.contains(forbidden),
                "migration 014 must stay in its sibling-subsystem lane: forbidden snippet found: {forbidden}"
            );
        }
    }

    /// Migration 013 ships the activity-blocks FTS5 surface + the
    /// `label` column. ADR 0040 §Decision items 4 + 5.
    #[test]
    fn migration_013_ships_activity_blocks_fts_and_label_column() {
        let conn = Connection::open_in_memory().expect("open in-memory");
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        apply_all(&conn).unwrap();

        // `label` column exists.
        let mut stmt = conn.prepare("PRAGMA table_info(activity_blocks)").unwrap();
        let cols: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert!(
            cols.iter().any(|c| c == "label"),
            "activity_blocks.label missing; cols: {cols:?}"
        );

        // FTS5 virtual table exists.
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master \
                 WHERE name = 'activity_blocks_fts'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(n >= 1, "activity_blocks_fts virtual table missing");

        // End-to-end: insert a session + a block, MATCH via FTS.
        conn.execute(
            "INSERT INTO activity_sessions (id, started_at, status, audio_enabled, \
             screenshot_enabled, created_at, updated_at) \
             VALUES ('s1', 1000, 'completed', 0, 0, 1000, 1000)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO activity_blocks (id, session_id, started_at, ended_at, \
             primary_app, primary_title, generated_abstract, source_event_ids, \
             prompt_version_sha, user_edited, label, created_at, updated_at) \
             VALUES ('b1', 's1', 1000, 2000, 'code.exe', 'main.rs', \
             'The user edited the dictation source file.', '[]', \
             'abstract_v1-00000000', 0, 'rust review', 1000, 1000)",
            [],
        )
        .unwrap();
        let hit: String = conn
            .query_row(
                "SELECT t.generated_abstract FROM activity_blocks t \
                 JOIN activity_blocks_fts f ON f.rowid = t.rowid \
                 WHERE activity_blocks_fts MATCH 'dictation'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(hit.contains("dictation"));
    }

    /// Migration 012 ships the activity-capture schema (ADR 0036).
    /// Verifies tables, immutability triggers, the CASCADE delete
    /// path, and the no-UPDATE invariant on activity_events
    /// (Principle 1).
    #[test]
    fn migration_012_ships_activity_capture_schema() {
        let conn = Connection::open_in_memory().expect("open in-memory");
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        apply_all(&conn).unwrap();

        // Tables present.
        for t in [
            "activity_sessions",
            "activity_events",
            "activity_blocks",
            "activity_transcript_segments",
        ] {
            let n: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master \
                     WHERE name=?1 AND type='table'",
                    [t],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 1, "missing activity table: {t}");
        }

        // Immutability triggers present.
        for trig in ["activity_events_no_update", "activity_events_no_delete"] {
            let n: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master \
                     WHERE type='trigger' AND name=?1",
                    [trig],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 1, "missing activity immutability trigger: {trig}");
        }

        // Seed a session + event, then prove the immutability triggers fire.
        conn.execute(
            "INSERT INTO activity_sessions (id, started_at, status, \
             audio_enabled, screenshot_enabled, created_at, updated_at) \
             VALUES ('sess-01', 1000, 'in_progress', 0, 0, 1000, 1000)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO activity_events (id, session_id, ts, kind, \
             app_name, window_title, created_at) \
             VALUES ('evt-01', 'sess-01', 1100, 'app_switch', \
             'notepad.exe', 'Untitled', 1100)",
            [],
        )
        .unwrap();

        // UPDATE must be rejected (Principle 1).
        let upd = conn.execute(
            "UPDATE activity_events SET window_title = 'tampered' WHERE id = 'evt-01'",
            [],
        );
        assert!(upd.is_err(), "UPDATE on activity_events must be rejected");
        let msg = upd.unwrap_err().to_string();
        assert!(
            msg.contains("immutable"),
            "trigger should mention immutability; got: {msg}"
        );

        // Direct DELETE on an in_progress session's event IS allowed
        // (the WHEN-clause guards only completed/crashed sessions);
        // bump the session to 'completed' first, then verify the
        // trigger fires.
        conn.execute(
            "UPDATE activity_sessions SET status = 'completed', ended_at = 2000, \
             updated_at = 2000 WHERE id = 'sess-01'",
            [],
        )
        .unwrap();
        let del = conn.execute("DELETE FROM activity_events WHERE id = 'evt-01'", []);
        assert!(
            del.is_err(),
            "direct DELETE on a completed session's event must be rejected"
        );

        // CASCADE via session-row delete IS the sanctioned path.
        conn.execute("DELETE FROM activity_sessions WHERE id = 'sess-01'", [])
            .unwrap();
        let remaining: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM activity_events WHERE session_id = 'sess-01'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            remaining, 0,
            "CASCADE delete should clear events when the session row goes"
        );
    }

    /// Migration 012 must not touch dictation or meeting tables.
    /// Static side of the sibling-subsystem boundary (ADR 0036 §1-2).
    #[test]
    fn migration_012_does_not_touch_dictation_or_meeting_tables() {
        let sql = MIGRATION_012.to_lowercase();
        for forbidden in [
            "alter table sessions",
            "alter table transcripts",
            "alter table modes",
            "alter table meeting_sessions",
            "alter table meeting_transcripts",
            "drop table sessions",
            "drop table transcripts",
            "drop table meeting_sessions",
            "drop table meeting_transcripts",
            "insert into modes",
        ] {
            assert!(
                !sql.contains(forbidden),
                "migration 012 must stay in its sibling-subsystem lane: forbidden snippet found: {forbidden}"
            );
        }
    }

    /// Migration 011 ships the meeting-capture schema (ADR 0026).
    /// Verifies all expected tables, the FTS5 virtual table, and a
    /// minimal write→read round trip through the FTS shadow trigger.
    #[test]
    fn migration_011_ships_meeting_capture_schema() {
        let conn = Connection::open_in_memory().expect("open in-memory");
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        apply_all(&conn).unwrap();

        // Tables present.
        for t in [
            "meeting_sessions",
            "meeting_transcripts",
            "meeting_transcripts_fts",
        ] {
            let n: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master \
                     WHERE name=?1 AND (type='table' OR type='virtual table' OR type='view')",
                    [t],
                    |r| r.get(0),
                )
                .unwrap();
            assert!(n >= 1, "missing meeting table/virtual-table: {t}");
        }

        // Triggers present.
        for trig in [
            "meeting_transcripts_fts_insert",
            "meeting_transcripts_fts_delete",
        ] {
            let n: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='trigger' AND name=?1",
                    [trig],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 1, "missing meeting FTS trigger: {trig}");
        }

        // End-to-end: insert a meeting + a formatted transcript,
        // round-trip the text through the FTS shadow table.
        conn.execute(
            "INSERT INTO meeting_sessions (uuid, started_at, ended_at, status, source, \
             total_duration_ms, hotkey_pressed, whisper_model_id, formatter_version) \
             VALUES ('mtg-test-uuid', '2026-05-20T00:00:00Z', '2026-05-20T00:01:00Z', \
             'complete', 'mic', 60000, 'VK_RCONTROL+VK_M', \
             'whisper-large-v3-turbo-q5_0', 'mc-v1')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO meeting_transcripts (meeting_session_id, channel, stage, text) \
             VALUES (1, 'mic', 'formatted', 'kickoff notes for the q3 planning meeting')",
            [],
        )
        .unwrap();
        let hit: String = conn
            .query_row(
                "SELECT t.text FROM meeting_transcripts t \
                 JOIN meeting_transcripts_fts f ON f.rowid = t.id \
                 WHERE meeting_transcripts_fts MATCH 'kickoff'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(hit.contains("kickoff notes"), "FTS hit text: {hit}");
    }

    /// The meeting schema must be append-only sibling to the existing
    /// dictation schema. None of migration 011's CREATE statements
    /// should reference the dictation tables (sessions, transcripts).
    /// This is the static side of the `mc-dictation-untouched` judge.
    #[test]
    fn migration_011_does_not_touch_dictation_tables() {
        let sql = MIGRATION_011.to_lowercase();
        for forbidden in [
            "alter table sessions",
            "alter table transcripts",
            "drop table sessions",
            "drop table transcripts",
            "alter table modes",
            "insert into modes",
        ] {
            assert!(
                !sql.contains(forbidden),
                "migration 011 must not touch dictation surface: forbidden snippet found: {forbidden}"
            );
        }
    }

    /// Migration 010 (ADR 0024 Wave C) inserts the v2 prompts AND
    /// repoints each mode at the new latest version. Verifies the
    /// full chain — markdown file → include_str! → token sub →
    /// migration apply → mode-row repoint — works end-to-end.
    #[test]
    fn migration_010_ships_v2_prompts_and_repoints_modes() {
        let conn = Connection::open_in_memory().expect("open in-memory");
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        apply_all(&conn).unwrap();

        for (slug, expected_version, body_must_contain) in [
            // casual_v2's anti-substitution rule is the canary.
            ("casual", 2, "NEVER SUBSTITUTE THE INPUT WITH AN EXAMPLE"),
            // normal_v5 also carries the anti-substitution rule.
            ("normal", 5, "NEVER SUBSTITUTE THE INPUT WITH AN EXAMPLE"),
            // formal_v2 rule 1 — added in iter-2 — is the canary.
            ("formal", 2, "ALWAYS CLEAN, NEVER REFUSE"),
        ] {
            let (version, body): (i64, String) = conn
                .query_row(
                    "SELECT p.version, p.body FROM modes m
                     JOIN prompts p ON m.prompt_id = p.id
                     WHERE m.slug = ?1",
                    [slug],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap_or_else(|e| panic!("resolve {slug}: {e}"));
            assert_eq!(
                version, expected_version,
                "mode {slug} did not repoint to v{expected_version}"
            );
            assert!(
                body.contains(body_must_contain),
                "mode {slug} v{expected_version} body missing canary phrase {body_must_contain:?}; \
                 first 200 chars: {:.200}",
                body
            );
        }

        // ADR 0008 append-only: the v1 / v4 rows must still be present
        // for historical session provenance.
        for (slug, version) in [("casual", 1), ("normal", 4), ("formal", 1)] {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM prompts WHERE mode_slug=?1 AND version=?2",
                    rusqlite::params![slug, version],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(
                count, 1,
                "ADR 0008 violation: {slug}@v{version} row should be preserved"
            );
        }

        // Casual temperature drop 0.4 → 0.2 (defensive against
        // attention-anchor drift on the 3B model).
        let casual_temp: f64 = conn
            .query_row(
                "SELECT temperature FROM modes WHERE slug = 'casual'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            (casual_temp - 0.2).abs() < 0.001,
            "casual temperature should be 0.2 post-010, got {casual_temp}"
        );
    }

    /// Migration 005 seeds the AI command modes (disabled by default).
    #[test]
    fn migration_005_seeds_ai_command_modes() {
        let conn = Connection::open_in_memory().expect("open in-memory");
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        apply_all(&conn).unwrap();
        for slug in ["rewrite", "expand", "summarize"] {
            let count: i64 = conn
                .query_row("SELECT COUNT(*) FROM modes WHERE slug = ?1", [slug], |r| {
                    r.get(0)
                })
                .unwrap();
            assert_eq!(count, 1, "missing mode row for {slug}");
            let enabled: i64 = conn
                .query_row("SELECT enabled FROM modes WHERE slug = ?1", [slug], |r| {
                    r.get(0)
                })
                .unwrap();
            assert_eq!(enabled, 0, "{slug} should ship disabled by default");
        }
    }

    /// Migration 004 actually adds the column it claims to add.
    #[test]
    fn migration_004_adds_injection_status_column() {
        let conn = Connection::open_in_memory().expect("open in-memory");
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        apply_all(&conn).unwrap();
        // `PRAGMA table_info` enumerates columns; check the injection_status one is there.
        let mut stmt = conn.prepare("PRAGMA table_info(sessions)").unwrap();
        let names: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .map(|n| n.unwrap())
            .collect();
        assert!(
            names.iter().any(|n| n == "injection_status"),
            "injection_status column missing; got {names:?}"
        );
    }
}
