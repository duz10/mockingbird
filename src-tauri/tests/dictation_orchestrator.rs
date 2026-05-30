//! End-to-end orchestrator integration tests with stub trait impls.
//!
//! These tests drive the **full** `DictationOrchestrator::run` event
//! loop — not just the pure `pipeline::decide` decision layer — by
//! supplying in-memory implementations of every trait the orchestrator
//! depends on. They give the Phase 3 Wave 5 judges (`e2e-injection`,
//! `db-provenance`, `secure-input-respected`) real teeth by asserting
//! the post-Wave-4.9 invariants the LLM judge prompts will check:
//!
//! - Injector receives `(text, strategy)` calls in the happy path AND
//!   receives ZERO calls when the secure-input guard fires.
//! - Three transcript stages (raw + cleaned + final) land in the DB
//!   for an `Ok` outcome; two stages (raw + cleaned, no final) for an
//!   `AbortedSecure` outcome.
//! - Every provenance column on `sessions` is populated (prompt_id,
//!   dictionary_snapshot_id, example_set_id, hotkey_pressed,
//!   started_at, recording_ended_at, foreground_app) — the
//!   ADR 0010 "provenance is total" rule.
//! - `injection_status` round-trips the canonical string from
//!   `InjectionOutcome::as_db_str()`.
//!
//! These run in CI (pure — no OS surfaces). They run from the
//! standalone integration-test crate, which means `mockingbird_lib`
//! is exercised through its public API exactly the way Phase 4's
//! LLM cleaner will plug in.

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use crossbeam_channel as cb;

use mockingbird_lib::audio::vad::{VadFrame, VoiceActivityDetector};
use mockingbird_lib::audio::AudioCapture;
use mockingbird_lib::cleanup::{LlmCleaner, PassthroughCleaner, StubCleanupProvider};
use mockingbird_lib::db::{sessions, transcripts, Database};
use mockingbird_lib::dictation::runtime::default_normal_config;
use mockingbird_lib::dictation::DictationOrchestrator;
use mockingbird_lib::error::{AppError, AppResult};
use mockingbird_lib::hotkey::state::{HotkeyMode, StateAction};
use mockingbird_lib::injection::secure_guard::SecureInputGuard;
use mockingbird_lib::injection::strategy::InjectionStrategy;
use mockingbird_lib::injection::{InjectionOutcome, Injector};
use mockingbird_lib::recording_window::RecordingWindow;
use mockingbird_lib::stt::{SpeechToText, TranscribeRequest, Transcript};
use mockingbird_lib::vault::export_job::VaultRuntime;
use mockingbird_lib::window_context::{ForegroundWindow, WindowContext};

/// Test fixture: a VaultRuntime that always reads disabled config so
/// `trigger()` is a fast-path no-op (no spawned worker, no I/O).
fn stub_vault(db: &Arc<Mutex<rusqlite::Connection>>) -> Arc<VaultRuntime> {
    // Stamp `mobile_sync_enabled = false` if the settings table is
    // empty/missing rows. VaultConfig::load tolerates missing rows
    // (defaults to disabled), so for these tests we just construct
    // a runtime from whatever's in the DB. The vault subsystem will
    // see `enabled = false` and short-circuit at trigger() entry.
    Arc::new(VaultRuntime::new(db).expect("VaultRuntime::new on test DB"))
}

// --------------------------------------------------------------------
// Stub trait implementations.
//
// Each stub is the minimum impl needed to drive complete() through
// to persist_complete(). No OS calls. No threads. The orchestrator's
// run loop iterates `actions.iter()` and terminates when the sender
// drops — so tests build a (tx, rx) pair, push the action sequence,
// drop tx, then call orchestrator.run(rx) inline.
// --------------------------------------------------------------------

/// Audio capture that produces an empty buffer on every `drain`.
/// The stub STT below ignores the audio anyway, so silence is fine.
struct StubAudioCapture;

impl AudioCapture for StubAudioCapture {
    fn start(&mut self) -> AppResult<()> {
        Ok(())
    }
    fn stop(&mut self) -> AppResult<()> {
        Ok(())
    }
    fn drain(&mut self, _buf: &mut Vec<i16>) -> AppResult<usize> {
        Ok(0)
    }
    fn sample_rate(&self) -> u32 {
        16_000
    }
    fn channels(&self) -> u16 {
        1
    }
}

/// VAD that always says "speech". `trim_speech` then keeps whatever
/// the audio stub produced (which is empty), and `transcribe` is
/// called with an empty slice — the stub STT returns "hello world"
/// regardless.
struct StubVad;

impl VoiceActivityDetector for StubVad {
    fn process_frame(&mut self, _frame: &[i16]) -> AppResult<VadFrame> {
        Ok(VadFrame {
            is_speech: true,
            confidence: 1.0,
        })
    }
    fn reset(&mut self) {}
    fn frame_samples(&self) -> usize {
        512
    }
}

/// STT that returns a fixed transcript regardless of the audio. The
/// `model_id` is the stable identifier the Wave 5 judges look for
/// when verifying the `transcripts.model_used` column on the `raw`
/// row.
struct StubStt {
    text: String,
}

impl SpeechToText for StubStt {
    fn transcribe(&mut self, _req: TranscribeRequest<'_>) -> AppResult<Transcript> {
        Ok(Transcript {
            text: self.text.clone(),
            gpu_used: false,
            latency_ms: 1,
            model_id: "stub-stt".into(),
        })
    }
}

/// Records every `inject` call so tests can assert the injector was
/// (or was NOT) invoked. Wrapped in `Arc<Mutex<...>>` so the stub
/// can be `Box<dyn Injector>`'d into the orchestrator while the test
/// retains a handle to inspect.
#[derive(Clone, Default)]
struct RecordingInjector {
    calls: Arc<Mutex<Vec<(String, InjectionStrategy)>>>,
}

impl RecordingInjector {
    fn new() -> Self {
        Self::default()
    }

    fn calls(&self) -> Vec<(String, InjectionStrategy)> {
        self.calls
            .lock()
            .expect("recording injector mutex poisoned")
            .clone()
    }
}

impl Injector for RecordingInjector {
    fn inject(&self, text: &str, strategy: InjectionStrategy) -> AppResult<InjectionOutcome> {
        self.calls
            .lock()
            .map_err(|_| AppError::Other("recording injector mutex poisoned".into()))?
            .push((text.to_string(), strategy));
        Ok(InjectionOutcome::Ok)
    }
}

/// WindowContext returning a fixed foreground window. Sufficient for
/// these tests because we never exercise focus-change paths here —
/// the focus-change behaviour is covered by the pure `pipeline::decide`
/// unit tests in `dictation::tests`.
struct FixedWindowContext {
    fg: ForegroundWindow,
}

impl WindowContext for FixedWindowContext {
    fn foreground(&self) -> AppResult<ForegroundWindow> {
        Ok(self.fg.clone())
    }
}

/// SecureInputGuard with a const answer. Tests use `false` for the
/// happy path and `true` for the secure-input abort path.
struct ConstSecureGuard(bool);

impl SecureInputGuard for ConstSecureGuard {
    fn is_secure(&self, _fg: &ForegroundWindow) -> bool {
        self.0
    }
}

// --------------------------------------------------------------------
// Test helpers.
// --------------------------------------------------------------------

fn notepad_window() -> ForegroundWindow {
    ForegroundWindow {
        hwnd: 0x1234,
        title: "Untitled - Notepad".into(),
        process_name: "notepad.exe".into(),
        exe_path: Some("C:\\Windows\\System32\\notepad.exe".into()),
    }
}

/// Build an orchestrator wired with the stub deps + an in-memory DB.
/// Returns the orchestrator + the things tests need to assert on:
/// the Arc<Mutex<Connection>> for DB queries, the recording injector
/// for call inspection.
fn build_orchestrator(
    secure: bool,
    stt_text: &str,
) -> (
    DictationOrchestrator,
    Arc<Mutex<rusqlite::Connection>>,
    RecordingInjector,
    mpsc::Receiver<mockingbird_lib::hotkey::HotkeyEvent>,
) {
    build_orchestrator_with_cleaner(secure, stt_text, Box::new(PassthroughCleaner::new()))
}

/// Same as `build_orchestrator` but lets the test inject a custom
/// cleaner — used by the Phase 4 `llm_cleanup_runs_in_orchestrator`
/// test to wire an `LlmCleaner` backed by `StubCleanupProvider`.
fn build_orchestrator_with_cleaner(
    secure: bool,
    stt_text: &str,
    cleaner: Box<dyn mockingbird_lib::cleanup::Cleaner>,
) -> (
    DictationOrchestrator,
    Arc<Mutex<rusqlite::Connection>>,
    RecordingInjector,
    mpsc::Receiver<mockingbird_lib::hotkey::HotkeyEvent>,
) {
    let db = Database::open_in_memory().expect("open in-memory db");
    let config = default_normal_config(&db.conn).expect("default normal config");
    let db_arc = Arc::new(Mutex::new(db.conn));

    let injector = RecordingInjector::new();
    let (hotkey_tx, hotkey_rx) = mpsc::channel();

    let orchestrator = DictationOrchestrator::new(
        Box::new(StubAudioCapture),
        Box::new(StubVad),
        Box::new(StubStt {
            text: stt_text.into(),
        }),
        cleaner,
        Box::new(injector.clone()),
        Box::new(FixedWindowContext {
            fg: notepad_window(),
        }),
        Box::new(ConstSecureGuard(secure)),
        RecordingWindow::new(),
        Arc::clone(&db_arc),
        config,
        HashMap::new(),
        hotkey_tx,
        // ADR 0045 + mb-tfyp — orchestrator now consumes a shared
        // `next_start_is_programmatic` flag set by the in-app start
        // IPC. These integration tests exercise the PTT path, so the
        // flag stays false; `start_capture` will pick StartMode::Ptt.
        Arc::new(AtomicBool::new(false)),
        // ADR 0052 + mb-0gt6 (Wave 1D.3) — sibling KG-note flag.
        // PTT integration tests never set it; `start_capture`
        // pins `CaptureKind::Dictation` so the source-gate's
        // pre-1D contract holds in these fixtures verbatim.
        Arc::new(AtomicBool::new(false)),
        // ADR 0046 Iter 2 / mb-lvzw — vault runtime, disabled by
        // default so trigger() short-circuits without touching disk.
        stub_vault(&db_arc),
    );

    (orchestrator, db_arc, injector, hotkey_rx)
}

/// Drive a complete StartCapture → StopCapture cycle through the
/// orchestrator's `run` loop. Returns after the loop terminates
/// (which happens when BOTH input channels close — ADR 0046 §3.2).
fn run_one_cycle(orchestrator: DictationOrchestrator) {
    let (action_tx, action_rx) = cb::unbounded::<StateAction>();
    let (_headless_tx, headless_rx) =
        cb::unbounded::<mockingbird_lib::dictation::ingest_channel::HeadlessIngestRequest>();
    action_tx
        .send(StateAction::StartCapture(HotkeyMode::Normal))
        .unwrap();
    action_tx.send(StateAction::StopCapture).unwrap();
    drop(action_tx); // close the actions arm
    drop(_headless_tx); // close the headless arm; both gone => run() returns Ok(())
    orchestrator
        .run(action_rx, headless_rx)
        .expect("orchestrator.run returned Err");
}

// --------------------------------------------------------------------
// Judge: e2e-injection + db-provenance (happy path)
// --------------------------------------------------------------------

#[test]
fn happy_path_injects_calls_writes_three_transcripts_and_ok_status() {
    let (orchestrator, db, injector, _hotkey_rx) = build_orchestrator(false, "hello world");
    run_one_cycle(orchestrator);

    // --- Injector was called exactly once with the cleaned text +
    //     the Paste strategy (notepad.exe → Paste per the built-in
    //     table in `injection::strategy`).
    let calls = injector.calls();
    assert_eq!(
        calls.len(),
        1,
        "injector should have been called exactly once, got {calls:?}"
    );
    assert_eq!(calls[0].0, "hello world");
    assert_eq!(calls[0].1, InjectionStrategy::Paste);

    // --- Session row exists with full provenance + injection_status = "ok".
    let conn = db.lock().unwrap();
    let recent = sessions::list_recent(&conn, 10).unwrap();
    assert_eq!(
        recent.len(),
        1,
        "expected 1 session row, got {}",
        recent.len()
    );
    let s = &recent[0];

    // Total provenance per ADR 0010: every FK + identifying field set.
    assert!(s.prompt_id.is_some(), "prompt_id must be populated");
    assert!(
        s.dictionary_snapshot_id.is_some(),
        "dictionary_snapshot_id must be populated"
    );
    assert!(
        s.example_set_id.is_some(),
        "example_set_id must be populated"
    );
    assert_eq!(s.hotkey_pressed, "RightAlt");
    assert!(!s.started_at.is_empty(), "started_at must be populated");
    assert!(
        !s.recording_ended_at.is_empty(),
        "recording_ended_at must be populated"
    );
    assert_eq!(
        s.foreground_app.as_deref(),
        Some("notepad.exe"),
        "foreground_app must reflect the FixedWindowContext"
    );
    assert_eq!(
        s.foreground_window_title.as_deref(),
        Some("Untitled - Notepad")
    );
    assert_eq!(
        s.injection_status.as_deref(),
        Some(InjectionOutcome::Ok.as_db_str()),
        "injection_status must be canonical 'ok'"
    );

    // --- Three transcript rows: raw + cleaned + final, all "hello world".
    let rows = transcripts::get_by_session(&conn, s.id).unwrap();
    assert_eq!(
        rows.len(),
        3,
        "expected 3 transcript stages (raw + cleaned + final), got {}",
        rows.len()
    );
    let stages: Vec<&str> = rows.iter().map(|r| r.stage.as_str()).collect();
    assert!(stages.contains(&"raw"));
    assert!(stages.contains(&"cleaned"));
    assert!(stages.contains(&"final"));
    for r in &rows {
        assert_eq!(
            r.text, "hello world",
            "stage {:?} should contain the STT text",
            r.stage
        );
    }
}

// --------------------------------------------------------------------
// Judge: secure-input-respected + db-provenance (abort path)
// --------------------------------------------------------------------

#[test]
fn secure_input_aborts_injector_unused_two_transcripts_aborted_status() {
    let (orchestrator, db, injector, _hotkey_rx) = build_orchestrator(true, "hello world");
    run_one_cycle(orchestrator);

    // --- Injector MUST NOT have been called.
    let calls = injector.calls();
    assert!(
        calls.is_empty(),
        "secure-input guard must block injection; got calls = {calls:?}"
    );

    // --- Session row with injection_status = "aborted_secure".
    let conn = db.lock().unwrap();
    let recent = sessions::list_recent(&conn, 10).unwrap();
    assert_eq!(recent.len(), 1);
    let s = &recent[0];
    assert_eq!(
        s.injection_status.as_deref(),
        Some(InjectionOutcome::AbortedSecure.as_db_str()),
        "injection_status must round-trip 'aborted_secure'"
    );
    // Provenance is total even on abort — abort is not an excuse to
    // drop FKs (ADR 0010).
    assert!(s.prompt_id.is_some());
    assert!(s.dictionary_snapshot_id.is_some());
    assert!(s.example_set_id.is_some());
    assert_eq!(s.foreground_app.as_deref(), Some("notepad.exe"));

    // --- Two transcript rows: raw + cleaned. NO final, because
    //     nothing was injected.
    let rows = transcripts::get_by_session(&conn, s.id).unwrap();
    assert_eq!(
        rows.len(),
        2,
        "aborted-secure should persist raw + cleaned, no final; got {}",
        rows.len()
    );
    let stages: Vec<&str> = rows.iter().map(|r| r.stage.as_str()).collect();
    assert!(stages.contains(&"raw"));
    assert!(stages.contains(&"cleaned"));
    assert!(
        !stages.contains(&"final"),
        "aborted-secure must NOT write a `final` transcript row"
    );
}

// --------------------------------------------------------------------
// Judge: e2e-injection (text fidelity — preserves whatever the STT
// produced through the cleaner and into the injector unchanged)
// --------------------------------------------------------------------

#[test]
fn cleaned_text_round_trips_through_injector_call_verbatim() {
    let weird = "Hello, world! — Mockingbird's clipboard 🎙";
    let (orchestrator, _db, injector, _hotkey_rx) = build_orchestrator(false, weird);
    run_one_cycle(orchestrator);

    let calls = injector.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].0, weird,
        "passthrough cleaner must preserve the text byte-for-byte"
    );
}

// --------------------------------------------------------------------
// Phase 4: `LlmCleaner` (backed by `StubCleanupProvider`) actually runs
// inside the orchestrator and produces text that differs from raw.
// This is the Phase 4 mb-modes-differ analogue at the orchestrator
// level — proves the cleanup layer is wired up end-to-end.
// --------------------------------------------------------------------

#[test]
fn llm_cleanup_runs_in_orchestrator_and_injects_cleaned_text() {
    // Build an LlmCleaner backed by the stub provider. We need to
    // share the DB with both the cleaner (for prompt + dict lookups)
    // and the orchestrator (for session persistence). Easiest: build
    // the orchestrator with the cleaner already wired, sharing a DB
    // Arc.
    let db = Database::open_in_memory().expect("open in-memory db");
    let config = default_normal_config(&db.conn).expect("default normal config");
    let db_arc = Arc::new(Mutex::new(db.conn));

    // LlmCleaner needs model_id/temp/max_tokens — pull from the mode row.
    let (model_id, temperature, max_tokens) = {
        let conn = db_arc.lock().unwrap();
        conn.query_row(
            "SELECT model_id, temperature, max_tokens FROM modes WHERE slug='normal'",
            [],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, f64>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            },
        )
        .unwrap()
    };
    let cleaner = Box::new(LlmCleaner::new(
        Box::new(StubCleanupProvider::new()),
        Arc::clone(&db_arc),
        model_id,
        temperature as f32,
        max_tokens as u32,
    ));

    let injector = RecordingInjector::new();
    let (hotkey_tx, _hotkey_rx) = mpsc::channel();
    let orchestrator = DictationOrchestrator::new(
        Box::new(StubAudioCapture),
        Box::new(StubVad),
        Box::new(StubStt {
            text: "hello world".into(),
        }),
        cleaner,
        Box::new(injector.clone()),
        Box::new(FixedWindowContext {
            fg: notepad_window(),
        }),
        Box::new(ConstSecureGuard(false)),
        RecordingWindow::new(),
        Arc::clone(&db_arc),
        config,
        HashMap::new(),
        hotkey_tx,
        // PTT path; programmatic-start flag stays false (mb-tfyp).
        Arc::new(AtomicBool::new(false)),
        // KG-note path inactive in this test (Wave 1D.3 / mb-0gt6).
        Arc::new(AtomicBool::new(false)),
        // ADR 0046 Iter 2 / mb-lvzw — vault disabled in tests.
        stub_vault(&db_arc),
    );
    run_one_cycle(orchestrator);

    // The stub provider's `normal` mode capitalises + adds a period.
    let calls = injector.calls();
    assert_eq!(calls.len(), 1, "got {calls:?}");
    assert_eq!(
        calls[0].0, "Hello world.",
        "LlmCleaner output should be the stub's normal-mode transform, not raw"
    );

    // And the DB should have raw="hello world", cleaned="Hello world.",
    // final="Hello world.".
    let conn = db_arc.lock().unwrap();
    let session = sessions::list_recent(&conn, 1).unwrap().pop().unwrap();
    let rows = transcripts::get_by_session(&conn, session.id).unwrap();
    assert_eq!(rows.len(), 3);
    let raw_row = rows.iter().find(|r| r.stage.as_str() == "raw").unwrap();
    let cleaned_row = rows.iter().find(|r| r.stage.as_str() == "cleaned").unwrap();
    let final_row = rows.iter().find(|r| r.stage.as_str() == "final").unwrap();
    assert_eq!(raw_row.text, "hello world");
    assert_eq!(cleaned_row.text, "Hello world.");
    assert_eq!(final_row.text, "Hello world.");
    // Stub provider returns `stub-{mode_slug}`; LlmCleaner forwards
    // that into `last_model_used`, and the orchestrator persists it.
    assert_eq!(
        cleaned_row.model_used.as_deref(),
        Some("stub-normal"),
        "cleaned-row model_used should reflect the actual model the provider used"
    );
}

// --------------------------------------------------------------------
// ADR 0046 §3.2 — topology: orchestrator multi-select between
// StateAction and HeadlessIngestRequest
// --------------------------------------------------------------------

/// Smoke test for the §3.2 multi-channel `run` loop: with BOTH channels
/// closed the loop must exit cleanly. The previous single-channel
/// shape would exit on EITHER close; this asserts the new
/// `actions_open || headless_open` predicate doesn't accidentally hang
/// when one arm closes before the other.
#[test]
fn run_loop_exits_when_both_channels_closed() {
    let (orchestrator, _db, _injector, _hotkey_rx) = build_orchestrator(false, "topology smoke");

    let (action_tx, action_rx) = cb::unbounded::<StateAction>();
    let (headless_tx, headless_rx) =
        cb::unbounded::<mockingbird_lib::dictation::ingest_channel::HeadlessIngestRequest>();

    // Close actions first, headless second — verifies the loop
    // survives the in-between state where one arm is dead and the
    // other is still empty (the select! must keep polling until
    // BOTH disconnect).
    drop(action_tx);
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(50));
        drop(headless_tx);
    });

    // Bounded by std::time::Duration on the test runner; if `run`
    // hangs the harness aborts via its own timeout. We just need
    // it to return Ok within a few ms after both senders drop.
    orchestrator
        .run(action_rx, headless_rx)
        .expect("orchestrator.run returned Err");
}
