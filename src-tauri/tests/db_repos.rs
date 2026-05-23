//! Cross-repo end-to-end integration tests.
//!
//! These exercise multiple Wave-3 modules together — the kind of
//! scenarios a production session would hit. Unit tests inside each
//! module cover module-local invariants; these cover the seams.

use mockingbird_lib::db::sessions::{NewSession, ProcessingCompletion, SessionStatus, StartMode};
use mockingbird_lib::db::{
    audit::{self, AuditedTable, Operation},
    dictionary::{self, NewDictionaryEntry},
    examples, prompts, search, sessions,
    transcripts::{self, Stage},
    Database,
};
use rusqlite::params;

fn fresh() -> Database {
    Database::open_in_memory().expect("open in-memory db")
}

#[test]
fn full_dictation_flow_end_to_end() {
    let db = fresh();
    let conn = &db.conn;

    // 1. Dictionary entry + snapshot.
    dictionary::insert(
        conn,
        &NewDictionaryEntry {
            term: "Mockingbird".into(),
            canonical: Some("Mockingbird".into()),
            source: "user".into(),
            confidence: None,
            app_context: None,
        },
    )
    .unwrap();
    let snapshot_id = dictionary::create_snapshot(conn).unwrap();

    // 2. Example set (empty is fine for Phase 1).
    let example_set_id = examples::create_example_set(conn, "normal", &[]).unwrap();

    // 3. Latest prompt for the mode.
    let prompt = prompts::get_latest_for_mode(conn, "normal")
        .unwrap()
        .expect("seeded prompt should exist");

    // 4. Insert session with full provenance.
    let new_session = NewSession {
        uuid: uuid::Uuid::new_v4().to_string(),
        mode_id: 1,
        hotkey_pressed: "Ctrl+Win".into(),
        started_at: "2026-05-15T12:00:00Z".into(),
        recording_ended_at: "2026-05-15T12:00:04Z".into(),
        status: SessionStatus::Processing,
        foreground_app: Some("vscode.exe".into()),
        foreground_window_title: Some("lib.rs — mockingbird".into()),
        audio_duration_ms: 4000,
        audio_blob_path: None,
        prompt_id: prompt.id,
        dictionary_snapshot_id: snapshot_id,
        example_set_id,
        start_mode: StartMode::Ptt,
    };
    let session_id = sessions::insert(conn, &new_session).unwrap();

    // 5-7. Three transcript stages.
    transcripts::insert_raw(conn, session_id, "hello mockingbird this is a test").unwrap();
    transcripts::insert_cleaned(
        conn,
        session_id,
        "Hello, Mockingbird — this is a test.",
        "qwen2.5:3b-instruct-q4_K_M",
    )
    .unwrap();
    transcripts::insert_final(
        conn,
        session_id,
        "Hello, Mockingbird — this is a test.",
        None,
    )
    .unwrap();

    // 8. Mark processing complete.
    sessions::update_processing_complete(
        conn,
        session_id,
        &ProcessingCompletion {
            completed_at: "2026-05-15T12:00:05Z".into(),
            status: SessionStatus::Complete,
            stt_latency_ms: Some(150),
            cleanup_latency_ms: Some(700),
            injection_latency_ms: Some(15),
            injection_status: Some("ok".into()),
        },
    )
    .unwrap();

    // 9. FTS5 hit on the raw transcript.
    let hits = search::search(conn, "mockingbird", 10).unwrap();
    assert!(!hits.is_empty(), "FTS5 should find 'mockingbird'");
    assert!(hits.iter().any(|h| h.session_id == session_id));

    // 10. Read it all back and assert provenance is intact.
    let session = sessions::get_by_id(conn, session_id).unwrap().unwrap();
    assert_eq!(session.status, SessionStatus::Complete);
    assert_eq!(session.prompt_id, Some(prompt.id));
    assert_eq!(session.dictionary_snapshot_id, Some(snapshot_id));
    assert_eq!(session.example_set_id, Some(example_set_id));
    assert_eq!(session.stt_latency_ms, Some(150));

    let all_stages = transcripts::get_by_session(conn, session_id).unwrap();
    assert_eq!(all_stages.len(), 3);
    assert_eq!(all_stages[0].stage, Stage::Raw);
    assert_eq!(all_stages[1].stage, Stage::Cleaned);
    assert_eq!(all_stages[2].stage, Stage::Final);
}

#[test]
fn audit_rollback_round_trip() {
    let db = fresh();
    let conn = &db.conn;

    let id = dictionary::insert(
        conn,
        &NewDictionaryEntry {
            term: "rollback_me".into(),
            canonical: Some("v1".into()),
            source: "user".into(),
            confidence: None,
            app_context: None,
        },
    )
    .unwrap();
    conn.execute(
        "UPDATE _history_dictionary SET at = '2026-05-15 10:00:00' \
         WHERE id = (SELECT MAX(id) FROM _history_dictionary)",
        [],
    )
    .unwrap();

    dictionary::update(
        conn,
        id,
        &dictionary::DictionaryEntryUpdate {
            canonical: Some(Some("v2".into())),
            ..Default::default()
        },
    )
    .unwrap();
    conn.execute(
        "UPDATE _history_dictionary SET at = '2026-05-15 10:00:05' \
         WHERE id = (SELECT MAX(id) FROM _history_dictionary)",
        [],
    )
    .unwrap();

    audit::rollback_row_to_timestamp(conn, AuditedTable::Dictionary, id, "2026-05-15 10:00:02")
        .unwrap();

    let restored = dictionary::get_by_id(conn, id).unwrap().unwrap();
    assert_eq!(restored.canonical.as_deref(), Some("v1"));

    let history = audit::list_history(conn, AuditedTable::Dictionary, id).unwrap();
    assert_eq!(history.len(), 3, "insert + update + rollback's-own-update");
    assert_eq!(history.last().unwrap().operation, Operation::Update);
}

#[test]
fn search_after_full_flow_finds_hits_in_all_stages() {
    let db = fresh();
    let conn = &db.conn;

    // Minimal session via raw SQL — provenance not the focus of this test.
    conn.execute(
        "INSERT INTO sessions (uuid, mode_id, hotkey_pressed, started_at, \
         recording_ended_at, status, audio_duration_ms) \
         VALUES ('s', 1, 'Ctrl+Win', '2026-05-15T00:00:00Z', \
         '2026-05-15T00:00:05Z', 'complete', 5000)",
        [],
    )
    .unwrap();
    let session_id = conn.last_insert_rowid();

    transcripts::insert_raw(conn, session_id, "raw smoke test text").unwrap();
    transcripts::insert_cleaned(conn, session_id, "cleaned smoke test text", "m").unwrap();
    transcripts::insert_final(conn, session_id, "final smoke test text", None).unwrap();

    let n = search::smoke_test_count(conn, "smoke").unwrap();
    assert_eq!(n, 3, "FTS5 should index all 3 stages");
}

#[test]
fn session_insert_with_nonexistent_mode_id_errors_via_fk() {
    let db = fresh();
    let conn = &db.conn;

    // Build prerequisites for a valid session except for the bad mode_id.
    let snapshot_id = dictionary::create_snapshot(conn).unwrap();
    let example_set_id = examples::create_example_set(conn, "normal", &[]).unwrap();
    let prompt_id = prompts::get_latest_for_mode(conn, "normal")
        .unwrap()
        .unwrap()
        .id;

    let bad = NewSession {
        uuid: uuid::Uuid::new_v4().to_string(),
        mode_id: 99_999,
        hotkey_pressed: "Ctrl+Win".into(),
        started_at: "2026-05-15T00:00:00Z".into(),
        recording_ended_at: "2026-05-15T00:00:05Z".into(),
        status: SessionStatus::Recording,
        foreground_app: None,
        foreground_window_title: None,
        audio_duration_ms: 5000,
        audio_blob_path: None,
        prompt_id,
        dictionary_snapshot_id: snapshot_id,
        example_set_id,
        start_mode: StartMode::Ptt,
    };
    assert!(
        sessions::insert(conn, &bad).is_err(),
        "FK violation expected"
    );
}

#[test]
fn create_snapshot_id_round_trips_through_session() {
    let db = fresh();
    let conn = &db.conn;

    dictionary::insert(
        conn,
        &NewDictionaryEntry {
            term: "term1".into(),
            canonical: None,
            source: "user".into(),
            confidence: None,
            app_context: None,
        },
    )
    .unwrap();
    let snapshot_id = dictionary::create_snapshot(conn).unwrap();
    let example_set_id = examples::create_example_set(conn, "normal", &[]).unwrap();
    let prompt_id = prompts::get_latest_for_mode(conn, "normal")
        .unwrap()
        .unwrap()
        .id;

    let session_id = sessions::insert(
        conn,
        &NewSession {
            uuid: uuid::Uuid::new_v4().to_string(),
            mode_id: 1,
            hotkey_pressed: "Ctrl+Win".into(),
            started_at: "2026-05-15T00:00:00Z".into(),
            recording_ended_at: "2026-05-15T00:00:05Z".into(),
            status: SessionStatus::Complete,
            foreground_app: None,
            foreground_window_title: None,
            audio_duration_ms: 5000,
            audio_blob_path: None,
            prompt_id,
            dictionary_snapshot_id: snapshot_id,
            example_set_id,
            start_mode: StartMode::Ptt,
        },
    )
    .unwrap();

    let session = sessions::get_by_id(conn, session_id).unwrap().unwrap();
    assert_eq!(session.dictionary_snapshot_id, Some(snapshot_id));

    // Read the snapshot back and verify it captures the inserted term's id.
    let term_ids_json: String = conn
        .query_row(
            "SELECT term_ids FROM dictionary_snapshots WHERE id = ?1",
            params![snapshot_id],
            |r| r.get(0),
        )
        .unwrap();
    let term_ids: Vec<i64> = serde_json::from_str(&term_ids_json).unwrap();
    assert_eq!(term_ids.len(), 1);
}

#[test]
fn audit_rollback_table_walks_every_row() {
    let db = fresh();
    let conn = &db.conn;

    let a = dictionary::insert(
        conn,
        &NewDictionaryEntry {
            term: "a".into(),
            canonical: Some("A".into()),
            source: "user".into(),
            confidence: None,
            app_context: None,
        },
    )
    .unwrap();
    let b = dictionary::insert(
        conn,
        &NewDictionaryEntry {
            term: "b".into(),
            canonical: Some("B".into()),
            source: "user".into(),
            confidence: None,
            app_context: None,
        },
    )
    .unwrap();

    conn.execute(
        "UPDATE _history_dictionary SET at = '2026-05-15 10:00:00'",
        [],
    )
    .unwrap();

    dictionary::update(
        conn,
        a,
        &dictionary::DictionaryEntryUpdate {
            canonical: Some(Some("AA".into())),
            ..Default::default()
        },
    )
    .unwrap();
    dictionary::update(
        conn,
        b,
        &dictionary::DictionaryEntryUpdate {
            canonical: Some(Some("BB".into())),
            ..Default::default()
        },
    )
    .unwrap();
    conn.execute(
        "UPDATE _history_dictionary SET at = '2026-05-15 10:00:05' \
         WHERE operation = 'UPDATE' AND at != '2026-05-15 10:00:00'",
        [],
    )
    .unwrap();

    audit::rollback_table_to_timestamp(conn, AuditedTable::Dictionary, "2026-05-15 10:00:02")
        .unwrap();

    assert_eq!(
        dictionary::get_by_id(conn, a)
            .unwrap()
            .unwrap()
            .canonical
            .as_deref(),
        Some("A")
    );
    assert_eq!(
        dictionary::get_by_id(conn, b)
            .unwrap()
            .unwrap()
            .canonical
            .as_deref(),
        Some("B")
    );
}
