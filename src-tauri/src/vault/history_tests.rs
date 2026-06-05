//! Tests for `vault::history` (KG Phase 1E Wave 1E.4, mb-i14b).
//!
//! Sibling file loaded via `#[cfg(test)] #[path]` to keep `history.rs`
//! itself under the 600-line cap. Same convention as
//! `markdown_serializer_tests.rs` (Wave 1E.2).

use super::*;
use crate::vault::kg_layout::bootstrap_kg_subtree;
use rusqlite::params;
use std::path::Path;
use tempfile::TempDir;

// ────────────────────────────────────────────────────────────
// Fixture builders
// ────────────────────────────────────────────────────────────

// Stable 64-char hex fixtures so the golden tests are reproducible.
const HASH_AUDIO: &str = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef";
const HASH_TEXT: &str = "cafebabecafebabecafebabecafebabecafebabecafebabecafebabecafebabe";
const HASH_SPARSE: &str = "0011223300112233001122330011223300112233001122330011223300112233";

fn kg_note_with_audio(audio_path: &Path) -> HistoryArchiveInput<'_> {
    HistoryArchiveInput {
        session_id: 42,
        session_uuid: "550e8400-e29b-41d4-a716-446655440000",
        capture_kind: "kg-note",
        captured_at: "2026-06-15T14:32:01Z",
        raw_transcript: "buy milk tomorrow",
        cleaned_transcript: "Buy milk tomorrow.",
        entry_id: "01HMVWAB7C8X9Y0Z1234567890",
        entry_filename: "2026-06-15-buy-milk__abcd1234.md",
        vault_file_hash: HASH_AUDIO,
        audio_blob_path: Some(audio_path),
    }
}

fn kg_note_text() -> HistoryArchiveInput<'static> {
    HistoryArchiveInput {
        session_id: 43,
        session_uuid: "660e8400-e29b-41d4-a716-446655440001",
        capture_kind: "kg-note-text",
        captured_at: "2026-06-15T14:32:01Z",
        raw_transcript: "Refactor injection table.",
        cleaned_transcript: "Refactor injection table.",
        entry_id: "01HMTEXT0abcdef1234567890",
        entry_filename: "2026-06-15-refactor-injection-table__01HMTEXT.md",
        vault_file_hash: HASH_TEXT,
        audio_blob_path: None,
    }
}

#[allow(dead_code)]
fn kg_note_sparse() -> HistoryArchiveInput<'static> {
    HistoryArchiveInput {
        session_id: 44,
        session_uuid: "770e8400-e29b-41d4-a716-446655440002",
        capture_kind: "kg-note",
        captured_at: "2026-01-02T03:04:05Z",
        // Sparse: empty cleaned transcript (no cleanup pass ran).
        raw_transcript: "test",
        cleaned_transcript: "",
        entry_id: "01HMSPARSE0abcdef12345678",
        entry_filename: "2026-01-02-test__01HMSPAR.md",
        vault_file_hash: HASH_SPARSE,
        audio_blob_path: None,
    }
}

// ────────────────────────────────────────────────────────────
// Pure helpers: month_bucket, sidecar_path_for, audio_archive_path_for
// ────────────────────────────────────────────────────────────

#[test]
fn month_bucket_happy_path() {
    assert_eq!(month_bucket("2026-06-15T14:32:01Z").unwrap(), "2026-06");
    assert_eq!(month_bucket("2026-01-01T00:00:00Z").unwrap(), "2026-01");
    assert_eq!(month_bucket("2026-12-31T23:59:59Z").unwrap(), "2026-12");
}

#[test]
fn month_bucket_accepts_offset_timezones_and_converts_to_utc() {
    // 2026-01-01T00:30:00+01:00 == 2025-12-31T23:30:00Z → bucket
    // must be `2025-12` (UTC), not `2026-01` (local).
    assert_eq!(
        month_bucket("2026-01-01T00:30:00+01:00").unwrap(),
        "2025-12"
    );
}

#[test]
fn month_bucket_with_milliseconds() {
    assert_eq!(month_bucket("2026-06-15T14:32:01.123Z").unwrap(), "2026-06");
}

#[test]
fn month_bucket_rejects_garbage() {
    assert!(month_bucket("").is_err());
    assert!(month_bucket("not-a-timestamp").is_err());
    assert!(month_bucket("2026-06-15").is_err()); // date-only, no time
}

#[test]
fn sidecar_path_for_combines_history_bucket_and_uuid() {
    let td = TempDir::new().unwrap();
    let input = kg_note_text();
    let path = sidecar_path_for(&input, td.path()).unwrap();
    let suffix: Vec<_> = path
        .components()
        .rev()
        .take(4)
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    assert_eq!(suffix[0], "660e8400-e29b-41d4-a716-446655440001.json");
    assert_eq!(suffix[1], "2026-06");
    assert_eq!(suffix[2], "History");
    assert_eq!(suffix[3], "Knowledge Graph");
}

#[test]
fn audio_archive_path_for_none_when_no_audio() {
    let td = TempDir::new().unwrap();
    let input = kg_note_text();
    assert_eq!(audio_archive_path_for(&input, td.path()).unwrap(), None);
}

#[test]
fn audio_archive_path_for_preserves_extension() {
    let td = TempDir::new().unwrap();
    let src = td.path().join("recording.m4a");
    let input = kg_note_with_audio(&src);
    let p = audio_archive_path_for(&input, td.path()).unwrap().unwrap();
    assert!(p.file_name().unwrap().to_string_lossy().ends_with(".m4a"));
    assert!(p
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with("550e8400-e29b-41d4-a716-446655440000"));
}

#[test]
fn audio_archive_path_for_falls_back_to_wav_when_extension_missing() {
    let td = TempDir::new().unwrap();
    let src = td.path().join("nameless_blob");
    let input = kg_note_with_audio(&src);
    let p = audio_archive_path_for(&input, td.path()).unwrap().unwrap();
    assert!(p.file_name().unwrap().to_string_lossy().ends_with(".wav"));
}

// ────────────────────────────────────────────────────────────
// Serialization (pure)
// ────────────────────────────────────────────────────────────

#[test]
fn serialize_sidecar_field_order_is_pinned() {
    // The wire-order MUST be exactly the kickoff spec order. We
    // verify by string-searching the serialized form -- each
    // expected key must appear AFTER the previous one.
    let td = TempDir::new().unwrap();
    let src = td.path().join("rec.wav");
    let input = kg_note_with_audio(&src);
    let bytes = serialize_sidecar(&input).unwrap();
    let s = String::from_utf8(bytes).unwrap();
    let expected_order = [
        "\"session_uuid\"",
        "\"session_id\"",
        "\"capture_kind\"",
        "\"captured_at\"",
        "\"raw_transcript\"",
        "\"cleaned_transcript\"",
        "\"entry_id\"",
        "\"entry_filename\"",
        "\"vault_file_hash\"",
        "\"archive_version\"",
    ];
    let mut cursor = 0usize;
    for key in expected_order {
        match s[cursor..].find(key) {
            Some(idx) => cursor += idx + key.len(),
            None => panic!("key `{key}` missing or out of order in:\n{s}"),
        }
    }
}

#[test]
fn serialize_sidecar_uses_lf_only_with_single_trailing_newline() {
    let td = TempDir::new().unwrap();
    let src = td.path().join("rec.wav");
    let input = kg_note_with_audio(&src);
    let bytes = serialize_sidecar(&input).unwrap();
    assert!(
        !bytes.windows(2).any(|w| w == b"\r\n"),
        "no CRLF allowed in canonical sidecar"
    );
    assert!(
        !bytes.contains(&b'\r'),
        "no CR allowed in canonical sidecar"
    );
    assert_eq!(bytes.last(), Some(&b'\n'), "must end with newline");
    // Exactly ONE trailing newline.
    assert_ne!(
        bytes[bytes.len().saturating_sub(2)],
        b'\n',
        "must not have two trailing newlines"
    );
}

#[test]
fn serialize_sidecar_records_archive_version_one() {
    let input = kg_note_text();
    let bytes = serialize_sidecar(&input).unwrap();
    let s = String::from_utf8(bytes).unwrap();
    assert!(s.contains("\"archive_version\": 1"));
}

#[test]
fn serialize_sidecar_escapes_embedded_newlines_in_transcript() {
    let mut input = kg_note_text();
    input.raw_transcript = "line one\nline two";
    let bytes = serialize_sidecar(&input).unwrap();
    let s = String::from_utf8(bytes).unwrap();
    // Literal newlines in the transcript must be escaped as `\n` --
    // the output is one logical line per JSON record value.
    assert!(s.contains(r"line one\nline two"));
}

// ────────────────────────────────────────────────────────────
// archive_session_history happy paths
// ────────────────────────────────────────────────────────────

#[test]
fn archive_kg_note_with_audio_writes_json_and_moves_audio() {
    let td = TempDir::new().unwrap();
    bootstrap_kg_subtree(td.path()).unwrap();

    // Plant a source audio file in a "recordings" dir outside the
    // vault subtree -- mimics the in-app recorder's working dir.
    let recordings = td.path().join("recordings");
    fs::create_dir_all(&recordings).unwrap();
    let src_audio = recordings.join("session-42.wav");
    fs::write(&src_audio, b"FAKE WAV BYTES").unwrap();

    let input = kg_note_with_audio(&src_audio);
    let outcome = archive_session_history(&input, td.path()).unwrap();

    assert!(outcome.archived, "first call must report archived=true");
    assert!(outcome.json_path.exists(), "JSON sidecar must land on disk");
    let audio_target = outcome.audio_path.as_ref().expect("audio path");
    assert!(
        audio_target.exists(),
        "audio must land in History/<bucket>/"
    );
    assert!(
        !src_audio.exists(),
        "source audio must be moved (not copied)"
    );

    // JSON contents are the canonical bytes we'd serialize fresh.
    let on_disk = fs::read(&outcome.json_path).unwrap();
    let expected = serialize_sidecar(&input).unwrap();
    assert_eq!(on_disk, expected);

    // Target path is the expected month-bucket.
    assert!(outcome
        .json_path
        .to_string_lossy()
        .contains("Knowledge Graph"));
    assert!(outcome.json_path.to_string_lossy().contains("History"));
    assert!(outcome.json_path.to_string_lossy().contains("2026-06"));
    assert!(audio_target.to_string_lossy().contains("2026-06"));
    assert!(audio_target
        .file_name()
        .unwrap()
        .to_string_lossy()
        .ends_with(".wav"));
}

#[test]
fn archive_kg_note_text_writes_json_only_no_audio_archive() {
    let td = TempDir::new().unwrap();
    bootstrap_kg_subtree(td.path()).unwrap();

    let input = kg_note_text();
    let outcome = archive_session_history(&input, td.path()).unwrap();

    assert!(outcome.archived);
    assert!(outcome.json_path.exists());
    assert!(
        outcome.audio_path.is_none(),
        "kg-note-text must NOT produce an audio archive"
    );

    // The JSON should still record the capture_kind so a downstream
    // reader knows audio is intentionally absent.
    let s = std::fs::read_to_string(&outcome.json_path).unwrap();
    assert!(s.contains("\"capture_kind\": \"kg-note-text\""));
}

#[test]
fn archive_is_idempotent_on_resnap_with_audio_already_moved() {
    let td = TempDir::new().unwrap();
    bootstrap_kg_subtree(td.path()).unwrap();

    let recordings = td.path().join("recordings");
    fs::create_dir_all(&recordings).unwrap();
    let src_audio = recordings.join("session-42.wav");
    fs::write(&src_audio, b"FAKE WAV BYTES").unwrap();

    let input = kg_note_with_audio(&src_audio);
    let first = archive_session_history(&input, td.path()).unwrap();
    assert!(first.archived);

    // Re-call: JSON already there, audio source gone (moved on
    // first call). archive_session_history must short-circuit
    // BEFORE touching the audio source -- idempotency check is
    // on the JSON existence.
    let second = archive_session_history(&input, td.path()).unwrap();
    assert!(!second.archived, "second call must report archived=false");
    assert_eq!(first.json_path, second.json_path);
    // audio_path resolves to the existing target on disk.
    assert_eq!(second.audio_path, first.audio_path);
}

#[test]
fn archive_creates_month_bucket_subdir_on_demand() {
    let td = TempDir::new().unwrap();
    bootstrap_kg_subtree(td.path()).unwrap();

    // The bucket dir does NOT exist yet (only History/ root from
    // bootstrap). The call must create it.
    let bucket = td
        .path()
        .join("Knowledge Graph")
        .join("History")
        .join("2026-06");
    assert!(!bucket.exists(), "precondition: bucket dir must not exist");

    let input = kg_note_text();
    archive_session_history(&input, td.path()).unwrap();

    assert!(
        bucket.exists() && bucket.is_dir(),
        "bucket dir must be created"
    );
}

#[test]
fn archive_handles_month_boundary_across_two_buckets() {
    let td = TempDir::new().unwrap();
    bootstrap_kg_subtree(td.path()).unwrap();

    let mut a = kg_note_text();
    a.captured_at = "2026-01-31T23:59:59Z";
    a.session_uuid = "aaa00000-0000-0000-0000-000000000001";
    let outcome_a = archive_session_history(&a, td.path()).unwrap();

    let mut b = kg_note_text();
    b.captured_at = "2026-02-01T00:00:01Z";
    b.session_uuid = "bbb00000-0000-0000-0000-000000000002";
    let outcome_b = archive_session_history(&b, td.path()).unwrap();

    assert!(outcome_a.json_path.to_string_lossy().contains("2026-01"));
    assert!(outcome_b.json_path.to_string_lossy().contains("2026-02"));
    assert!(outcome_a.json_path.exists());
    assert!(outcome_b.json_path.exists());
}

// ────────────────────────────────────────────────────────────
// Failure modes
// ────────────────────────────────────────────────────────────

#[test]
fn archive_returns_err_when_history_root_is_blocked_by_a_file() {
    let td = TempDir::new().unwrap();
    // Plant a regular file where the History dir would be -- forces
    // `create_dir_all` to fail with a non-directory error.
    let kg_root = td.path().join("Knowledge Graph");
    fs::create_dir_all(&kg_root).unwrap();
    fs::write(kg_root.join("History"), b"i am blocking the directory").unwrap();

    let input = kg_note_text();
    let err = archive_session_history(&input, td.path()).unwrap_err();
    match err {
        AppError::Vault(msg) => assert!(msg.contains("archive_session_history")),
        other => panic!("expected AppError::Vault, got {other:?}"),
    }
}

#[test]
fn archive_rejects_unparseable_captured_at() {
    let td = TempDir::new().unwrap();
    bootstrap_kg_subtree(td.path()).unwrap();

    let mut input = kg_note_text();
    input.captured_at = "definitely not a timestamp";
    let err = archive_session_history(&input, td.path()).unwrap_err();
    assert!(matches!(err, AppError::Vault(_)));
}

#[test]
fn archive_logs_audio_missing_at_archive_time_but_still_writes_json() {
    let td = TempDir::new().unwrap();
    bootstrap_kg_subtree(td.path()).unwrap();

    // Path points to a file that never existed.
    let phantom = td.path().join("recordings").join("ghost.wav");
    let input = kg_note_with_audio(&phantom);

    let outcome = archive_session_history(&input, td.path()).unwrap();
    assert!(outcome.archived, "JSON must still land");
    assert!(outcome.json_path.exists());
    assert!(
        outcome.audio_path.is_none(),
        "audio_path must be None when source was missing"
    );
}

// ────────────────────────────────────────────────────────────
// reconcile_history
// ────────────────────────────────────────────────────────────

fn fresh_db_with_sessions_schema() -> rusqlite::Connection {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    // Minimal schema: just the columns reconcile_history selects.
    // Avoids dragging the full migrations chain into a unit test.
    conn.execute_batch(
        "CREATE TABLE sessions (
           id INTEGER PRIMARY KEY,
           uuid TEXT NOT NULL,
           started_at TEXT NOT NULL,
           entry_id TEXT,
           vault_path TEXT,
           vault_file_hash TEXT
         );",
    )
    .unwrap();
    conn
}

fn insert_session(
    conn: &rusqlite::Connection,
    id: i64,
    uuid: &str,
    started_at: &str,
    sealed: bool,
) {
    let entry_id = if sealed {
        Some(format!("entry-{uuid}"))
    } else {
        None
    };
    let vault_path = if sealed {
        Some(format!("Knowledge Graph/Entries/{uuid}.md"))
    } else {
        None
    };
    conn.execute(
        "INSERT INTO sessions (id, uuid, started_at, entry_id, vault_path) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, uuid, started_at, entry_id, vault_path],
    )
    .unwrap();
}

#[test]
fn reconcile_flags_session_sealed_but_no_sidecar_on_disk() {
    let td = TempDir::new().unwrap();
    bootstrap_kg_subtree(td.path()).unwrap();

    let conn = fresh_db_with_sessions_schema();
    insert_session(
        &conn,
        1,
        "abc11111-0000-0000-0000-000000000001",
        "2026-06-15T14:32:01Z",
        true,
    );

    let report = reconcile_history(&conn, td.path()).unwrap();
    assert_eq!(report.missing_sidecar_count, 1);
    assert_eq!(report.orphan_sidecar_count, 0);
}

#[test]
fn reconcile_clean_when_sidecar_matches_sealed_session() {
    let td = TempDir::new().unwrap();
    bootstrap_kg_subtree(td.path()).unwrap();

    let conn = fresh_db_with_sessions_schema();
    let uuid = "abc22222-0000-0000-0000-000000000002";
    insert_session(&conn, 2, uuid, "2026-06-15T14:32:01Z", true);

    // Write a stub sidecar at the expected path.
    let bucket = td
        .path()
        .join("Knowledge Graph")
        .join("History")
        .join("2026-06");
    fs::create_dir_all(&bucket).unwrap();
    fs::write(bucket.join(format!("{uuid}.json")), b"{}\n").unwrap();

    let report = reconcile_history(&conn, td.path()).unwrap();
    assert_eq!(report.missing_sidecar_count, 0);
    assert_eq!(report.orphan_sidecar_count, 0);
}

#[test]
fn reconcile_flags_orphan_sidecar_whose_uuid_no_session_knows() {
    let td = TempDir::new().unwrap();
    bootstrap_kg_subtree(td.path()).unwrap();

    let conn = fresh_db_with_sessions_schema();
    // No sessions at all -- any sidecar on disk is an orphan.

    let bucket = td
        .path()
        .join("Knowledge Graph")
        .join("History")
        .join("2026-06");
    fs::create_dir_all(&bucket).unwrap();
    let orphan = bucket.join("ffffffff-eeee-dddd-cccc-bbbbbbbbbbbb.json");
    fs::write(&orphan, b"{}\n").unwrap();

    let report = reconcile_history(&conn, td.path()).unwrap();
    assert_eq!(report.missing_sidecar_count, 0);
    assert_eq!(report.orphan_sidecar_count, 1);
    assert!(orphan.exists(), "reconcile must NOT delete orphan sidecars");
}

#[test]
fn reconcile_ignores_unsealed_sessions() {
    let td = TempDir::new().unwrap();
    bootstrap_kg_subtree(td.path()).unwrap();

    let conn = fresh_db_with_sessions_schema();
    // Session that hasn't reached vault projection yet -- should be
    // invisible to the history reconciler.
    insert_session(
        &conn,
        3,
        "abc33333-0000-0000-0000-000000000003",
        "2026-06-15T14:32:01Z",
        false,
    );
    let report = reconcile_history(&conn, td.path()).unwrap();
    assert_eq!(report.missing_sidecar_count, 0);
    assert_eq!(report.orphan_sidecar_count, 0);
}

#[test]
fn reconcile_handles_unparseable_started_at_without_crashing() {
    let td = TempDir::new().unwrap();
    bootstrap_kg_subtree(td.path()).unwrap();

    let conn = fresh_db_with_sessions_schema();
    insert_session(
        &conn,
        4,
        "abc44444-0000-0000-0000-000000000004",
        "definitely not a timestamp",
        true,
    );

    // Should swallow the parse error + carry on (just logs).
    let report = reconcile_history(&conn, td.path()).unwrap();
    // The bad-timestamp row contributes neither missing nor orphan
    // (it's effectively skipped).
    assert_eq!(report.missing_sidecar_count, 0);
    assert_eq!(report.orphan_sidecar_count, 0);
}

// ────────────────────────────────────────────────────────────
// Golden-file tests
// ────────────────────────────────────────────────────────────
//
// Lock the canonical JSON form against on-disk fixtures so any
// accidental shape change shows up as a single-test failure. Same
// MOCKINGBIRD_UPDATE_GOLDENS workflow as the Wave 1E.2 markdown
// serializer (resolve via `file!()` so the throwaway-crate harness
// writes back to the real tree per LESSONS P2).

fn golden_for(name: &str) -> &'static str {
    match name {
        "kg_note_with_audio" => {
            include_str!("../../tests/fixtures/history_golden/kg_note_with_audio.json")
        }
        "kg_note_text" => include_str!("../../tests/fixtures/history_golden/kg_note_text.json"),
        "kg_note_sparse" => {
            include_str!("../../tests/fixtures/history_golden/kg_note_sparse.json")
        }
        other => panic!("unknown golden fixture: {other}"),
    }
}

fn assert_golden_sidecar(name: &str, input: &HistoryArchiveInput<'_>) {
    let actual = String::from_utf8(serialize_sidecar(input).unwrap()).unwrap();
    if std::env::var("MOCKINGBIRD_UPDATE_GOLDENS").is_ok() {
        let here = std::path::PathBuf::from(file!());
        let path = here
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
            .expect("file!() yields a deep-enough path")
            .join("tests")
            .join("fixtures")
            .join("history_golden")
            .join(format!("{name}.json"));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &actual).unwrap();
        panic!(
            "MOCKINGBIRD_UPDATE_GOLDENS=1 -- wrote {}; re-run without the env var",
            path.display()
        );
    }
    let expected = golden_for(name);
    if actual != expected {
        panic!(
            "golden mismatch for `{name}`.\n\
             --- expected ---\n{expected}\n\
             --- actual ---\n{actual}\n\
             --- byte lengths --- expected={} actual={}",
            expected.len(),
            actual.len()
        );
    }
}

#[test]
fn golden_kg_note_with_audio() {
    // Use a stable, hard-coded source path so the golden bytes are
    // truly deterministic (the path string isn't in the JSON, but
    // using the builder helper would `leak` a fresh string each
    // run -- the build below uses constants instead).
    let input = HistoryArchiveInput {
        session_id: 42,
        session_uuid: "550e8400-e29b-41d4-a716-446655440000",
        capture_kind: "kg-note",
        captured_at: "2026-06-15T14:32:01Z",
        raw_transcript: "buy milk tomorrow",
        cleaned_transcript: "Buy milk tomorrow.",
        entry_id: "01HMVWAB7C8X9Y0Z1234567890",
        entry_filename: "2026-06-15-buy-milk__abcd1234.md",
        vault_file_hash: "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
        audio_blob_path: None, // path doesn't influence JSON bytes
    };
    assert_golden_sidecar("kg_note_with_audio", &input);
}

#[test]
fn golden_kg_note_text() {
    let input = HistoryArchiveInput {
        session_id: 43,
        session_uuid: "660e8400-e29b-41d4-a716-446655440001",
        capture_kind: "kg-note-text",
        captured_at: "2026-06-15T14:32:01Z",
        raw_transcript: "Refactor injection table.",
        cleaned_transcript: "Refactor injection table.",
        entry_id: "01HMTEXT0abcdef1234567890",
        entry_filename: "2026-06-15-refactor-injection-table__01HMTEXT.md",
        vault_file_hash: "cafebabecafebabecafebabecafebabecafebabecafebabecafebabecafebabe",
        audio_blob_path: None,
    };
    assert_golden_sidecar("kg_note_text", &input);
}

#[test]
fn golden_kg_note_sparse() {
    // Sparse: cleaned_transcript is empty (e.g. cleanup pass failed
    // mid-flight but the entry still got filed off the raw text).
    let input = HistoryArchiveInput {
        session_id: 44,
        session_uuid: "770e8400-e29b-41d4-a716-446655440002",
        capture_kind: "kg-note",
        captured_at: "2026-01-02T03:04:05Z",
        raw_transcript: "test",
        cleaned_transcript: "",
        entry_id: "01HMSPARSE0abcdef12345678",
        entry_filename: "2026-01-02-test__01HMSPAR.md",
        vault_file_hash: "0011223300112233001122330011223300112233001122330011223300112233",
        audio_blob_path: None,
    };
    assert_golden_sidecar("kg_note_sparse", &input);
}
