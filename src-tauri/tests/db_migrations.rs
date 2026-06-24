//! End-to-end migration tests.
//!
//! Each test opens a fresh in-memory database via
//! `Database::open_in_memory()`, which applies migrations 001-003 from
//! scratch, runs the SQLite integrity + foreign-key checks, and returns
//! a ready-to-use connection. These tests are the canonical
//! verification that the sealed Phase-1 schema actually matches what
//! PLAN §7 + the Wave 2 brief described.

use mockingbird_lib::db::Database;

/// Fresh in-memory database fixture with all migrations applied.
fn fresh_db() -> Database {
    Database::open_in_memory().expect("migrate fresh in-memory db")
}

#[test]
fn schema_version_is_current_after_apply() {
    // Bump history: 3 → 4 (migration 004 injection_status; Phase 3
    // Wave 4) → 5 (AI command modes) → 6/7/8 (prompt iterations) → 9
    // (max_tokens bump) → 10 (ADR 0024 Wave C prompt v2) → 11 (Phase MC
    // meeting_sessions + meeting_transcripts + FTS; ADR 0026 + 0027 +
    // 0028 + 0029 + 0030) → 12 (Phase 10 Wave 1B activity-capture
    // schema) → 13 (Phase 10 Wave 3 activity_blocks FTS5 + label) → 14
    // (Phase 10 Wave 4 activity_sessions audio-pipeline provenance) →
    // 15 (Phase 10 Wave 5 hardening — exclusion-rules table +
    // activity_blocks.raw_events_purged_at; ADR 0042 + 0043) → 16
    // (post-phase-10 hotfix: add the missing
    // `activity_blocks.primary_title` column referenced by every
    // code path in `activity/`; mb-scla). → 17/18/19 (ADR 0046/0047
    // session columns + temp bumps) → 20/21/22 (ADR 0047 prompt +
    // mode + Q5 checkpoint) → 23 (edit-free-send column) → 24 (ADR
    // 0050 KG persistence) → 25 (ADR 0052 capture_kind/category) → 26
    // (ADR 0053 vault two-phase-commit columns). Bump this assert
    // when the next migration lands. (mb-mac-v1.9: assertion + fn name
    // were stale at 16 -- this integration test had never executed,
    // Windows gates `--no-run`.)
    let db = fresh_db();
    let v: String = db
        .conn
        .query_row(
            "SELECT value FROM schema_meta WHERE key='schema_version'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(v, "30");
}

#[test]
fn all_expected_tables_exist() {
    let db = fresh_db();
    let expected = [
        "schema_meta",
        "prompts",
        "modes",
        "dictionary",
        "dictionary_snapshots",
        "style_examples",
        "example_sets",
        "sessions",
        "transcripts",
        "corrections",
        "settings",
        "learning_runs",
        "_history_modes",
        "_history_prompts",
        "_history_dictionary",
        "_history_style_examples",
        // Phase MC — sibling-subsystem tables (migration 011).
        "meeting_sessions",
        "meeting_transcripts",
        // Phase 10 Wave 1B — activity-capture sibling-subsystem (migration 012).
        "activity_sessions",
        "activity_events",
        "activity_blocks",
        "activity_transcript_segments",
        // Phase 10 Wave 5 — hardening (migration 015).
        "activity_exclusion_rules",
    ];
    for t in expected {
        let n: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                [t],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1, "missing table: {t}");
    }
}

#[test]
fn trigger_count_matches_migration_set() {
    // 2 dictation FTS5 triggers (transcripts_fts_insert/delete; mig 001)
    // + 4 tables * 3 audit triggers = 12 (mig 002)
    // + 2 meeting FTS5 triggers (meeting_transcripts_fts_insert/delete; mig 011)
    // + 2 activity-events immutability triggers
    //   (activity_events_no_update / activity_events_no_delete; mig 012)
    // + 3 activity_blocks FTS5 triggers
    //   (activity_blocks_ai / activity_blocks_au / activity_blocks_ad; mig 013)
    // = 21 total through migration 015. Migration 024 (ADR 0050 KG
    // persistence) adds 2 immutability triggers → 23 total. (mb-mac-
    // v1.9: assert + fn name were stale at 21; this integration test
    // had never run -- Windows gates `--no-run`.)
    //
    // Bump this count whenever a migration adds or removes a trigger.
    // The plus side of asserting an exact number: it catches an
    // accidental "oh I'll just add another trigger" without a paired
    // ADR / plan update.
    let db = fresh_db();
    let n: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='trigger'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 23);
}

#[test]
fn seeded_modes_and_prompts_present() {
    let db = fresh_db();
    let prompts: i64 = db
        .conn
        .query_row("SELECT COUNT(*) FROM prompts", [], |r| r.get(0))
        .unwrap();
    // 17 prompt rows after all seed migrations (003 seeds 3; 005/006/
    // 007/008/010/020 append further versions; 027 appends normal_small
    // v1; 028 appends normal_small v2).
    // (mb-mac-v1.9: was 3; ADR 0065: 15 -> 16; ADR 0065 v2: 16 -> 17.)
    assert_eq!(prompts, 17);
    let modes: i64 = db
        .conn
        .query_row("SELECT COUNT(*) FROM modes", [], |r| r.get(0))
        .unwrap();
    // 8 modes: the 3 tone modes (casual/normal/formal) + the AI
    // command modes (migration 005: rewrite/expand/summarize/…).
    // (mb-mac-v1.9: was 3.)
    assert_eq!(modes, 8);

    // Seed inserts MUST fire audit triggers (migration 002 ran before 003).
    let history_prompts: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM _history_prompts WHERE operation='INSERT'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(history_prompts, 17, "audit triggers should fire on seed");
    let history_modes: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM _history_modes WHERE operation='INSERT'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(history_modes, 8);
}

#[test]
fn audit_update_records_before_and_after() {
    let db = fresh_db();
    db.conn
        .execute(
            "INSERT INTO dictionary (term, canonical, source) VALUES ('foo','Foo','user')",
            [],
        )
        .unwrap();
    db.conn
        .execute("UPDATE dictionary SET canonical='FOO' WHERE term='foo'", [])
        .unwrap();
    let n: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM _history_dictionary WHERE operation='UPDATE'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 1);

    let patch: String = db
        .conn
        .query_row(
            "SELECT patch FROM _history_dictionary WHERE operation='UPDATE'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(patch.contains("\"before\""));
    assert!(patch.contains("\"after\""));
    assert!(patch.contains("FOO"));
    assert!(patch.contains("Foo"));
}

#[test]
fn fts5_round_trip_finds_inserted_transcript() {
    let db = fresh_db();

    // Need a session row first (transcripts.session_id is NOT NULL FK).
    db.conn
        .execute(
            "INSERT INTO sessions (uuid, mode_id, hotkey_pressed, started_at, recording_ended_at, status, audio_duration_ms) \
             VALUES ('test-uuid', 1, 'Ctrl+Win', '2026-05-15T00:00:00Z', '2026-05-15T00:00:05Z', 'complete', 5000)",
            [],
        )
        .unwrap();
    db.conn
        .execute(
            "INSERT INTO transcripts (session_id, stage, text) VALUES (1, 'raw', 'hello world from fts5')",
            [],
        )
        .unwrap();

    let hit: String = db
        .conn
        .query_row(
            "SELECT t.text FROM transcripts t \
             JOIN transcripts_fts f ON f.rowid = t.id \
             WHERE transcripts_fts MATCH 'hello'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(hit, "hello world from fts5");
}

#[test]
fn apply_all_is_idempotent() {
    let db = fresh_db();
    // Second call must be a no-op (assertion: doesn't panic, schema_version
    // still at the current latest). (mb-mac-v1.9: was stale at 11.)
    mockingbird_lib::db::apply_migrations(&db.conn).expect("second apply_migrations should be Ok");
    let v: String = db
        .conn
        .query_row(
            "SELECT value FROM schema_meta WHERE key='schema_version'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(v, "30");
}

/// Migration 011 ships the FTS5 mirror for meeting transcripts.
/// End-to-end through the integration test crate (mirrors the
/// existing `fts5_round_trip_finds_inserted_transcript` test for the
/// dictation side).
#[test]
fn meeting_fts5_round_trip_finds_inserted_meeting_transcript() {
    let db = fresh_db();

    db.conn
        .execute(
            "INSERT INTO meeting_sessions (uuid, started_at, ended_at, status, source, \
             total_duration_ms, hotkey_pressed, whisper_model_id, formatter_version) \
             VALUES ('mtg-fts-uuid', '2026-05-20T10:00:00Z', '2026-05-20T10:05:00Z', \
             'complete', 'both', 300000, 'VK_RCONTROL+VK_M', \
             'whisper-large-v3-turbo-q5_0', 'mc-v1')",
            [],
        )
        .unwrap();
    db.conn
        .execute(
            "INSERT INTO meeting_transcripts (meeting_session_id, channel, stage, text) \
             VALUES (1, 'merged', 'formatted', 'discussed the new pricing model in detail')",
            [],
        )
        .unwrap();

    let hit: String = db
        .conn
        .query_row(
            "SELECT t.text FROM meeting_transcripts t \
             JOIN meeting_transcripts_fts f ON f.rowid = t.id \
             WHERE meeting_transcripts_fts MATCH 'pricing'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(hit, "discussed the new pricing model in detail");
}

/// FK enforcement on the meeting cascade: deleting a meeting_sessions
/// row must delete its transcript children AND remove them from the
/// FTS shadow table (via the DELETE trigger).
#[test]
fn meeting_transcripts_cascade_delete_clears_fts() {
    let db = fresh_db();

    db.conn
        .execute(
            "INSERT INTO meeting_sessions (uuid, started_at, ended_at, status, source, \
             total_duration_ms, hotkey_pressed, whisper_model_id, formatter_version) \
             VALUES ('mtg-cascade-uuid', '2026-05-20T11:00:00Z', '2026-05-20T11:00:30Z', \
             'complete', 'mic', 30000, 'VK_RCONTROL+VK_M', \
             'whisper-large-v3-turbo-q5_0', 'mc-v1')",
            [],
        )
        .unwrap();
    db.conn
        .execute(
            "INSERT INTO meeting_transcripts (meeting_session_id, channel, stage, text) \
             VALUES (1, 'mic', 'formatted', 'unique sentinel phrase for cascade test')",
            [],
        )
        .unwrap();

    // Sanity: FTS finds it.
    let n_before: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM meeting_transcripts_fts \
             WHERE meeting_transcripts_fts MATCH 'sentinel'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n_before, 1);

    // Cascade.
    db.conn
        .execute(
            "DELETE FROM meeting_sessions WHERE uuid='mtg-cascade-uuid'",
            [],
        )
        .unwrap();

    // Both rows gone.
    let n_transcripts: i64 = db
        .conn
        .query_row("SELECT COUNT(*) FROM meeting_transcripts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n_transcripts, 0, "FK cascade did not clear transcripts");
    let n_fts: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM meeting_transcripts_fts \
             WHERE meeting_transcripts_fts MATCH 'sentinel'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n_fts, 0, "DELETE trigger did not clear FTS shadow row");
}
