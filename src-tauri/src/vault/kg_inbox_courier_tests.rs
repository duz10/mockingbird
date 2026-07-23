//! Tests for the KG-Inbox courier (Phase 1E Wave 1E.6 / `mb-i46v`).
//!
//! These tests mirror `inbox::courier`'s test layout: stub the
//! filesystem via the [`KgFileOps`] trait, stub the orchestrator
//! via a thread that consumes one [`HeadlessIngestRequest`] and
//! replies synchronously. No real audio, no whisper-rs, no CUDA.
//!
//! Per LESSONS P2, `cargo test --release` fails to launch the test
//! binary on this Windows box. These tests are validated via
//! `cargo test --release --no-run` (link/type/trait surface) plus
//! the throwaway-crate recipe for the pure-Rust subset.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::SystemTime;

use rusqlite::Connection;

use super::*;
use crate::db::migrations;
use crate::db::sessions::{
    self as sess, CaptureKind, NewSession, SessionSource, SessionStatus, StartMode,
};
use crate::dictation::ingest_channel::HeadlessIngestRequest;
use crate::error::AppError;
use crate::vault::kg_inbox_courier_fs::{split_stem_ext, unique_failed_path, KgFileOps};

// --------------------------------------------------------------------
// FakeFs -- in-memory `KgFileOps` double. Tracks every move so tests
// can assert the quarantine routing.
// --------------------------------------------------------------------

struct FakeFs {
    size_of: StdMutex<std::collections::HashMap<PathBuf, u64>>,
    decode_result: StdMutex<AppResult<Vec<i16>>>,
    moves: StdMutex<Vec<(PathBuf, PathBuf)>>,
    now_iso: String,
}

impl FakeFs {
    fn new(_size: u64, decode_ok: bool, now_iso: &str) -> Self {
        Self {
            size_of: StdMutex::new(std::collections::HashMap::new()),
            decode_result: StdMutex::new(if decode_ok {
                Ok(vec![0i16; 16_000])
            } else {
                Err(AppError::Audio("synthetic decode failure".into()))
            }),
            moves: StdMutex::new(Vec::new()),
            now_iso: now_iso.to_string(),
        }
    }

    fn set_size(&self, path: &Path, size: u64) {
        self.size_of
            .lock()
            .unwrap()
            .insert(path.to_path_buf(), size);
    }
}

impl KgFileOps for FakeFs {
    fn move_file(&self, src: &Path, dst: &Path) -> AppResult<()> {
        self.moves
            .lock()
            .unwrap()
            .push((src.to_path_buf(), dst.to_path_buf()));
        Ok(())
    }
    fn metadata_size(&self, path: &Path) -> AppResult<u64> {
        self.size_of
            .lock()
            .unwrap()
            .get(path)
            .copied()
            .ok_or_else(|| AppError::Vault(format!("stat missing {}", path.display())))
    }
    fn decode(&self, _path: &Path) -> AppResult<Vec<i16>> {
        std::mem::replace(
            &mut *self.decode_result.lock().unwrap(),
            Err(AppError::Audio("test fixture exhausted".into())),
        )
    }
    fn now_iso(&self) -> String {
        self.now_iso.clone()
    }
}

// --------------------------------------------------------------------
// Fixture helpers
// --------------------------------------------------------------------

fn synthetic_stable(path: &str, size: u64) -> StableInboxFile {
    StableInboxFile {
        path: PathBuf::from(path),
        size,
        observed_at: SystemTime::now(),
    }
}

fn stub_orchestrator(reply: AppResult<i64>) -> HeadlessIngestSender {
    let (tx, rx) = crossbeam_channel::unbounded::<HeadlessIngestRequest>();
    std::thread::spawn(move || {
        if let Ok(req) = rx.recv() {
            let _ = req.reply_tx.send(reply);
        }
    });
    tx
}

fn empty_db() -> Arc<Mutex<Connection>> {
    let conn = Connection::open_in_memory().unwrap();
    migrations::apply_all(&conn).unwrap();
    Arc::new(Mutex::new(conn))
}

/// Insert a synthetic session row whose `audio_blob_path` points at
/// `path`. Used by the idempotency test to pre-seed the DB.
fn insert_session_with_audio_path(db: &Arc<Mutex<Connection>>, path: &Path) -> i64 {
    let conn = db.lock().unwrap();
    // Seed the FK parents the `sessions` row references. A fresh
    // `apply_all` DB seeds `modes` + `prompts` but NOT
    // `dictionary_snapshots` / `example_sets`, so the previously
    // hardcoded `id = 1` for those two violated the foreign key
    // (extended_code 787). (mb-mac-v1.9: surfaced on Mac's first
    // real test run; Windows gates `--no-run` so the insert never
    // executed.)
    conn.execute(
        "INSERT INTO dictionary_snapshots (term_ids) VALUES ('[]')",
        [],
    )
    .unwrap();
    let snapshot_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO example_sets (mode_slug, example_ids) VALUES ('normal', '[]')",
        [],
    )
    .unwrap();
    let example_set_id = conn.last_insert_rowid();
    let new = NewSession {
        uuid: uuid::Uuid::new_v4().to_string(),
        mode_id: 1,
        hotkey_pressed: "file-import:mobile-inbox".to_string(),
        started_at: "2026-06-08T12:00:00.000Z".to_string(),
        recording_ended_at: "2026-06-08T12:00:01.000Z".to_string(),
        status: SessionStatus::Complete,
        foreground_app: None,
        foreground_window_title: None,
        audio_duration_ms: 1000,
        audio_blob_path: Some(path.to_string_lossy().into_owned()),
        prompt_id: 1,
        dictionary_snapshot_id: snapshot_id,
        example_set_id,
        start_mode: StartMode::InApp,
        source: SessionSource::MobileInbox,
        capture_kind: CaptureKind::KgNote,
    };
    sess::insert(&conn, &new).unwrap()
}

// --------------------------------------------------------------------
// Happy-path: success leaves file in place (worker phase-4 archives)
// --------------------------------------------------------------------

#[test]
fn success_does_not_move_file() {
    let kg_inbox = PathBuf::from("/vault/Knowledge Graph/Inbox");
    let file = synthetic_stable("/vault/Knowledge Graph/Inbox/Memo.m4a", 12_345);
    let fs = FakeFs::new(12_345, true, "2026-06-08T18:00:00Z");
    fs.set_size(&file.path, 12_345);
    let tx = stub_orchestrator(Ok(99));
    let db = empty_db();

    let outcome = process_one(
        &kg_inbox,
        &file,
        &tx,
        &fs,
        &db,
        &ingest_progress::NoopIngestProgressBus,
    );

    match outcome {
        KgCourierOutcome::Ingested {
            session_id,
            idempotent_skip,
        } => {
            assert_eq!(session_id, 99);
            assert!(
                !idempotent_skip,
                "fresh ingest should not be flagged as idempotent skip"
            );
        }
        other => panic!("expected Ingested, got {other:?}"),
    }
    // CRUCIAL invariant: success path performs ZERO moves. The
    // worker's phase-4 archive owns the file disposition.
    assert!(
        fs.moves.lock().unwrap().is_empty(),
        "success path must not move; worker phase-4 owns the archive. moves: {:?}",
        fs.moves.lock().unwrap()
    );
}

// --------------------------------------------------------------------
// Idempotency: pre-existing session with same audio_blob_path -> skip
// --------------------------------------------------------------------

#[test]
fn idempotent_skip_when_session_already_references_path() {
    let kg_inbox = PathBuf::from("/vault/Knowledge Graph/Inbox");
    let file = synthetic_stable("/vault/Knowledge Graph/Inbox/Memo.m4a", 12_345);
    let fs = FakeFs::new(12_345, true, "2026-06-08T18:00:00Z");
    fs.set_size(&file.path, 12_345);

    let db = empty_db();
    let pre_id = insert_session_with_audio_path(&db, &file.path);

    // Orchestrator stub that PANICS if invoked -- the idempotency
    // probe should short-circuit BEFORE we send the request.
    let (tx, rx) = crossbeam_channel::unbounded::<HeadlessIngestRequest>();
    std::thread::spawn(move || {
        if let Ok(_req) = rx.recv() {
            panic!("orchestrator must not be invoked on idempotent-skip path");
        }
    });

    let outcome = process_one(
        &kg_inbox,
        &file,
        &tx,
        &fs,
        &db,
        &ingest_progress::NoopIngestProgressBus,
    );

    match outcome {
        KgCourierOutcome::Ingested {
            session_id,
            idempotent_skip,
        } => {
            assert_eq!(session_id, pre_id);
            assert!(idempotent_skip);
        }
        other => panic!("expected idempotent Ingested, got {other:?}"),
    }
    assert!(
        fs.moves.lock().unwrap().is_empty(),
        "idempotent-skip path must not move either"
    );
}

// --------------------------------------------------------------------
// Failure -> quarantine to <KG Inbox>/_failed/
// --------------------------------------------------------------------

#[test]
fn ingest_failure_quarantines_to_kg_inbox_failed_dir() {
    let kg_inbox = PathBuf::from("/vault/Knowledge Graph/Inbox");
    let file = synthetic_stable("/vault/Knowledge Graph/Inbox/Broken.m4a", 100);
    let fs = FakeFs::new(100, true, "2026-06-08T18:00:00Z");
    fs.set_size(&file.path, 100);
    let tx = stub_orchestrator(Err(AppError::Other("synthetic ingest failure".into())));
    let db = empty_db();

    let outcome = process_one(
        &kg_inbox,
        &file,
        &tx,
        &fs,
        &db,
        &ingest_progress::NoopIngestProgressBus,
    );

    match outcome {
        KgCourierOutcome::Quarantined { failed_to, .. } => {
            assert_eq!(
                failed_to,
                PathBuf::from("/vault/Knowledge Graph/Inbox/_failed/Broken.m4a")
            );
        }
        other => panic!("expected Quarantined, got {other:?}"),
    }
    let moves = fs.moves.lock().unwrap();
    assert_eq!(moves.len(), 1, "expected exactly one move (the quarantine)");
}

#[test]
fn zero_byte_file_quarantines_without_decoding() {
    let kg_inbox = PathBuf::from("/vault/Knowledge Graph/Inbox");
    let file = synthetic_stable("/vault/Knowledge Graph/Inbox/Empty.m4a", 0);
    // Decode would panic with "test fixture exhausted" on a second
    // call, but the validate step should short-circuit BEFORE decode.
    let fs = FakeFs::new(0, false, "2026-06-08T18:00:00Z");
    fs.set_size(&file.path, 0);
    let (tx, _rx) = crossbeam_channel::unbounded::<HeadlessIngestRequest>();
    let db = empty_db();

    let outcome = process_one(
        &kg_inbox,
        &file,
        &tx,
        &fs,
        &db,
        &ingest_progress::NoopIngestProgressBus,
    );

    match outcome {
        KgCourierOutcome::Quarantined {
            reason: KgCourierFailure::Empty,
            ..
        } => {}
        other => panic!("expected Quarantined(Empty), got {other:?}"),
    }
}

#[test]
fn wrong_extension_quarantines() {
    let kg_inbox = PathBuf::from("/vault/Knowledge Graph/Inbox");
    let file = synthetic_stable("/vault/Knowledge Graph/Inbox/Notes.txt", 50);
    let fs = FakeFs::new(50, true, "2026-06-08T18:00:00Z");
    fs.set_size(&file.path, 50);
    let (tx, _rx) = crossbeam_channel::unbounded::<HeadlessIngestRequest>();
    let db = empty_db();

    let outcome = process_one(
        &kg_inbox,
        &file,
        &tx,
        &fs,
        &db,
        &ingest_progress::NoopIngestProgressBus,
    );

    match outcome {
        KgCourierOutcome::Quarantined {
            reason: KgCourierFailure::UnsupportedExtension(_),
            ..
        } => {}
        other => panic!("expected Quarantined(UnsupportedExtension), got {other:?}"),
    }
}

#[test]
fn oversized_file_quarantines_without_decoding() {
    let kg_inbox = PathBuf::from("/vault/Knowledge Graph/Inbox");
    let file = synthetic_stable("/vault/Knowledge Graph/Inbox/Huge.m4a", 51 * 1024 * 1024);
    let fs = FakeFs::new(51 * 1024 * 1024, false, "2026-06-08T18:00:00Z");
    fs.set_size(&file.path, 51 * 1024 * 1024);
    let (tx, _rx) = crossbeam_channel::unbounded::<HeadlessIngestRequest>();
    let db = empty_db();

    let outcome = process_one(
        &kg_inbox,
        &file,
        &tx,
        &fs,
        &db,
        &ingest_progress::NoopIngestProgressBus,
    );

    match outcome {
        KgCourierOutcome::Quarantined {
            reason: KgCourierFailure::TooLarge(_),
            ..
        } => {}
        other => panic!("expected Quarantined(TooLarge), got {other:?}"),
    }
}

#[test]
fn decode_failure_quarantines() {
    let kg_inbox = PathBuf::from("/vault/Knowledge Graph/Inbox");
    let file = synthetic_stable("/vault/Knowledge Graph/Inbox/Garbage.m4a", 100);
    let fs = FakeFs::new(100, false, "2026-06-08T18:00:00Z");
    fs.set_size(&file.path, 100);
    let (tx, _rx) = crossbeam_channel::unbounded::<HeadlessIngestRequest>();
    let db = empty_db();

    let outcome = process_one(
        &kg_inbox,
        &file,
        &tx,
        &fs,
        &db,
        &ingest_progress::NoopIngestProgressBus,
    );

    match outcome {
        KgCourierOutcome::Quarantined {
            reason: KgCourierFailure::DecodeFailed(_),
            ..
        } => {}
        other => panic!("expected Quarantined(DecodeFailed), got {other:?}"),
    }
}

#[test]
fn orchestrator_unavailable_quarantines() {
    let kg_inbox = PathBuf::from("/vault/Knowledge Graph/Inbox");
    let file = synthetic_stable("/vault/Knowledge Graph/Inbox/Memo.m4a", 100);
    let fs = FakeFs::new(100, true, "2026-06-08T18:00:00Z");
    fs.set_size(&file.path, 100);
    // Construct a sender whose receiver is immediately dropped --
    // any send attempt returns Err.
    let (tx, rx) = crossbeam_channel::unbounded::<HeadlessIngestRequest>();
    drop(rx);
    let db = empty_db();

    let outcome = process_one(
        &kg_inbox,
        &file,
        &tx,
        &fs,
        &db,
        &ingest_progress::NoopIngestProgressBus,
    );

    match outcome {
        KgCourierOutcome::Quarantined {
            reason: KgCourierFailure::OrchestratorUnavailable,
            ..
        } => {}
        other => panic!("expected Quarantined(OrchestratorUnavailable), got {other:?}"),
    }
}

// --------------------------------------------------------------------
// Quarantine collision: same filename twice -> -1 suffix
// --------------------------------------------------------------------

#[test]
fn unique_failed_path_appends_suffix_on_collision() {
    let tmp = tempfile::tempdir().unwrap();
    let kg_inbox = tmp.path();
    let failed_dir = kg_inbox.join("_failed");
    std::fs::create_dir_all(&failed_dir).unwrap();
    std::fs::write(failed_dir.join("Memo.m4a"), b"already-failed").unwrap();

    let dst = unique_failed_path(kg_inbox, "Memo.m4a");
    assert_eq!(dst.file_name().and_then(|s| s.to_str()), Some("Memo-1.m4a"));

    // Two collisions -> -2
    std::fs::write(failed_dir.join("Memo-1.m4a"), b"also-failed").unwrap();
    let dst2 = unique_failed_path(kg_inbox, "Memo.m4a");
    assert_eq!(
        dst2.file_name().and_then(|s| s.to_str()),
        Some("Memo-2.m4a")
    );
}

#[test]
fn split_stem_ext_handles_dotfiles_and_no_extension() {
    assert_eq!(split_stem_ext("foo.m4a"), ("foo", Some("m4a")));
    assert_eq!(split_stem_ext("foo"), ("foo", None));
    // Dotfile -- whole name is the stem; no extension.
    assert_eq!(split_stem_ext(".hidden"), (".hidden", None));
    assert_eq!(split_stem_ext("foo.bar.baz"), ("foo.bar", Some("baz")));
}
