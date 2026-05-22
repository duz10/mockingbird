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
        assert_eq!(v, "15");
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
        assert_eq!(v, "15");
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
