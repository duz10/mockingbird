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
        assert_eq!(v, "11");
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
        assert_eq!(v, "11");
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
