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
fn schema_version_is_3_after_apply() {
    let db = fresh_db();
    let v: String = db
        .conn
        .query_row(
            "SELECT value FROM schema_meta WHERE key='schema_version'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(v, "3");
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
fn trigger_count_is_14() {
    // 2 FTS5 triggers (transcripts_fts_insert/delete) + 4 tables * 3 audit = 14.
    let db = fresh_db();
    let n: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='trigger'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 14);
}

#[test]
fn seeded_modes_and_prompts_present() {
    let db = fresh_db();
    let prompts: i64 = db
        .conn
        .query_row("SELECT COUNT(*) FROM prompts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(prompts, 3);
    let modes: i64 = db
        .conn
        .query_row("SELECT COUNT(*) FROM modes", [], |r| r.get(0))
        .unwrap();
    assert_eq!(modes, 3);

    // Seed inserts MUST fire audit triggers (migration 002 ran before 003).
    let history_prompts: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM _history_prompts WHERE operation='INSERT'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(history_prompts, 3, "audit triggers should fire on seed");
    let history_modes: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM _history_modes WHERE operation='INSERT'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(history_modes, 3);
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
    // Second call must be a no-op (assertion: doesn't panic, schema_version still 3).
    mockingbird_lib::db::apply_migrations(&db.conn).expect("second apply_migrations should be Ok");
    let v: String = db
        .conn
        .query_row(
            "SELECT value FROM schema_meta WHERE key='schema_version'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(v, "3");
}
