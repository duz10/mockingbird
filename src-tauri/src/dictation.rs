//! Dictation orchestrator — the glue that turns
//! [`crate::hotkey::state::StateAction`]s into a full
//! `audio → STT → cleanup → inject → persist` pipeline run.
//!
//! ## Architecture
//!
//! ```text
//!  State driver thread ──► Receiver<StateAction>
//!                                │
//!                                ▼
//!                     ┌─────────────────────────┐
//!                     │  DictationOrchestrator  │
//!                     │                         │
//!                     │  - audio: AudioCapture  │   (Phase 2)
//!                     │  - stt:   SpeechToText  │   (Phase 2)
//!                     │  - cleaner: Cleaner     │   (Phase 4)
//!                     │  - injector: Injector   │   (Phase 3 Wave 4)
//!                     │  - secure_guard         │   (Phase 3 Wave 2)
//!                     │  - window_ctx           │   (Phase 3 Wave 2)
//!                     │  - recording_window     │   (Phase 5 stub)
//!                     │  - db                   │   (Phase 1)
//!                     │  - fg_keydown: Option   │   ← captured on StartCapture
//!                     │  - user_overrides       │
//!                     │  - config               │
//!                     │                         │
//!                     │  run() loops on rx.iter │
//!                     └─────────────────────────┘
//! ```
//!
//! ## Pure-vs-OS split
//!
//! The complete pipeline is broken in two:
//!
//! 1. [`pipeline::Inputs`] / [`pipeline::run`] is the **pure decision
//!    layer**: given STT result + foreground snapshots + secure-input
//!    answer + cleaner output, decide what to inject (if anything)
//!    and what to persist. No OS calls, no traits, just `&str` +
//!    structs in/out. Trivially testable.
//!
//! 2. The orchestrator's `complete()` method is the **side-effect
//!    layer**: it asks the traits for their answers, hands them to
//!    [`pipeline::run`], then performs the resulting actions
//!    (`injector.inject`, `db::sessions::update_processing_complete`).
//!
//! This is the same pattern Wave 3 established for `hotkey::windows`
//! (pure `classify_keystroke` + thin OS shim).
//!
//! ## Submodules
//!
//! - [`runtime`]: Wave 4.5 spawn glue that wires the orchestrator
//!   into `lib.rs::run()` with the platform-default traits.

pub mod events;
pub mod ingest;
pub mod ingest_channel;
pub mod ingest_progress;
pub mod llm_prompts;
pub mod paste_payload;
pub mod runtime;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crossbeam_channel::{select, Receiver as CrossbeamReceiver};

use self::ingest::{headless_ingest, IngestDeps};
use self::ingest_channel::HeadlessIngestRequest;

use rusqlite::Connection;

use crate::audio::vad::VoiceActivityDetector;
use crate::audio::{trim_speech, AudioCapture, TrimConfig};
use crate::cleanup::Cleaner;
use crate::db::sessions::{
    self, CaptureKind, NewSession, ProcessingCompletion, SessionSource, SessionStatus, StartMode,
};
use crate::db::transcripts;
use crate::error::{AppError, AppResult};
use crate::hotkey::state::StateAction;
use crate::hotkey::HotkeyEvent;
use crate::injection::secure_guard::SecureInputGuard;
use crate::injection::strategy::InjectionStrategy;
use crate::injection::{InjectionOutcome, Injector};
use crate::kg;
use crate::recording_window::RecordingWindow;
use crate::settings::model::SettingKey;
use crate::settings::Settings;
use crate::stt::{SpeechToText, TranscribeRequest};
use crate::window_context::{ForegroundWindow, WindowContext};

/// Provenance + mode config the orchestrator needs at session-insert
/// time.
///
/// All FK values must point at real rows. The orchestrator does NOT
/// resolve them dynamically — Wave 5+ will reload these when the user
/// switches modes; for Wave 4 they're set once at orchestrator
/// construction.
#[derive(Debug, Clone)]
pub struct OrchestratorConfig {
    /// `modes.id` for the active mode (Normal / Fragment / Verbose).
    pub mode_id: i64,
    /// `modes.slug` — passed to [`Cleaner::clean`] to select its prompt.
    pub mode_slug: String,
    /// Active `prompts.id`. Required by NewSession.
    pub prompt_id: i64,
    /// Active `dictionary_snapshots.id`. Required by NewSession.
    pub dictionary_snapshot_id: i64,
    /// Active `example_sets.id`. Required by NewSession.
    pub example_set_id: i64,
    /// Label written to `sessions.hotkey_pressed` (e.g. `"RightAlt"`).
    pub hotkey_label: String,
}

/// The owner struct. Construct via [`DictationOrchestrator::new`],
/// then call [`Self::run`] to enter the event loop.
pub struct DictationOrchestrator {
    audio: Box<dyn AudioCapture>,
    vad: Box<dyn VoiceActivityDetector>,
    stt: Box<dyn SpeechToText>,
    cleaner: Box<dyn Cleaner>,
    injector: Box<dyn Injector>,
    window_ctx: Box<dyn WindowContext>,
    secure_guard: Box<dyn SecureInputGuard>,
    recording_window: RecordingWindow,
    db: Arc<Mutex<Connection>>,
    config: OrchestratorConfig,
    user_overrides: HashMap<String, InjectionStrategy>,

    /// Feedback channel to the hotkey driver. After every
    /// `complete()` / `discard()` (including error paths) we emit
    /// `HotkeyEvent::PipelineComplete` so the §6.1 state machine can
    /// transition `Processing → Idle`. Without this signal the
    /// machine sticks in `Processing` after one hold and ignores
    /// every subsequent KeyDown.
    hotkey_tx: Sender<HotkeyEvent>,

    /// ADR 0045 + mb-tfyp — set by [`DictationRuntime::start`] just
    /// before it injects the synthetic `KeyDown` for a programmatic
    /// session. The orchestrator swaps it back to `false` on
    /// `start_capture` so the flag never leaks past one session.
    ///
    /// Why an atomic instead of plumbing the mode through the FSM:
    /// ADR 0045 explicitly keeps the state machine + `StateAction`
    /// enum mode-agnostic (the synthetic event is indistinguishable
    /// from a real key event there). The orchestrator is the first
    /// place in the pipeline that's allowed to know — it owns the
    /// session row, the focus-drift check, and the inject decision,
    /// which are exactly the three places `start_mode` matters.
    next_start_is_programmatic: Arc<AtomicBool>,

    /// ADR 0052 + mb-0gt6 (KG Phase 1D Wave 1D.3) — set by
    /// [`DictationRuntime::start_kg_note`] just before it injects
    /// the synthetic `KeyDown` for a KG-screen audio note. Read
    /// and cleared at `start_capture` time using the same
    /// swap-and-pin pattern as `next_start_is_programmatic`. When
    /// `true`, the session is pinned to [`CaptureKind::KgNote`]
    /// for the rest of its lifetime; the dictation-tail
    /// source-gate (ADR 0052 D1) then enqueues the row into
    /// `kg_filing_queue`.
    ///
    /// Lives as a sibling atomic (not as a bit on
    /// `next_start_is_programmatic`) because the two flags are
    /// independent axes: every KG-note start is also programmatic
    /// (the UI button doesn't press a real key), but not every
    /// programmatic start is a KG note (the in-app Start button
    /// for plain dictation also goes through `start()`). Keeping
    /// them separate makes the orchestrator's swap-and-pin logic
    /// honest about which capture_kind to record.
    next_start_is_kg_note: Arc<AtomicBool>,

    /// ADR 0046 Iter 2 / mb-lvzw — vault export-job handle. Every
    /// `persist_complete` + every successful `handle_headless`
    /// trigger fires `vault.trigger(self.db.clone())` AFTER the row
    /// commit. Same additive pattern Iter 1 used for the
    /// `SessionsEventBus` recording-window field; nothing in the
    /// existing pipeline behavior changes.
    vault: Arc<crate::vault::export_job::VaultRuntime>,

    // Per-session transient state.
    state: SessionState,
}

/// All inputs `persist_complete` needs to write one fully-provenanced
/// session row + its `raw` / `cleaned` / `final` transcript stages.
///
/// Lives on its own struct (rather than 11 method params) so adding
/// new provenance fields in later waves doesn't ripple through the
/// call site as another positional argument. Borrowed lifetime `'a`
/// keeps text strings as `&str` slices — no clones on the hot path.
struct PersistCompleteParams<'a> {
    recording_ended_iso: String,
    /// Reserved for Wave 5 — currently written to `_` because
    /// `audio_duration_ms` is filled at the VAD-trim step.
    recording_duration_ms: i64,
    fg_keyup: &'a ForegroundWindow,
    outcome: InjectionOutcome,
    stt_latency_ms: Option<i64>,
    cleanup_latency_ms: Option<i64>,
    injection_latency_ms: Option<i64>,
    /// Raw STT output — IMMUTABLE per ADR 0010.
    raw_text: &'a str,
    /// Post-cleanup text. Equal to `raw_text` when the passthrough
    /// cleaner is used (Wave 4 default).
    cleaned_text: &'a str,
    /// What was actually injected. `None` when injection was aborted
    /// (secure input, user opt-out) → no `final` row.
    injected_text: Option<&'a str>,
    /// Cleanup model identifier for `transcripts.model_used`.
    cleanup_model: &'a str,
    /// ADR 0052 + mb-pxzk (Wave 1D.1) — drives the dictation-tail KG
    /// source-gate. Sourced from `SessionState::capture_kind`; lives
    /// here too so the param struct is self-contained.
    capture_kind: CaptureKind,
}

#[derive(Default)]
struct SessionState {
    /// `ForegroundWindow` snapshotted on `StartCapture`. Used by the
    /// focus-loss double-snapshot at `StopCapture` time.
    fg_keydown: Option<ForegroundWindow>,
    /// Wall-clock start, recorded on `StartCapture` for the DB row.
    started_at: Option<Instant>,
    /// ISO-8601 string for DB persistence.
    started_at_iso: Option<String>,
    /// Mode resolved at `start_capture` from the active-mode setting.
    /// Pinned for the duration of the session so a `set_active_mode`
    /// call mid-dictation can't split a session across two modes
    /// (the cleanup prompt would mismatch the DB-recorded mode_id).
    /// `None` until `start_capture`; callers fall back to
    /// `self.config` defensively.
    active_mode: Option<ResolvedMode>,
    /// ADR 0045 + mb-tfyp — snapshotted from
    /// `next_start_is_programmatic` at `start_capture`. Drives
    /// (a) the focus-drift skip, (b) the inject skip, (c) the
    /// `sessions.start_mode` DB column, (d) the `dictation:state`
    /// event payload's `startMode` field.
    start_mode: StartMode,
    /// ADR 0052 + mb-pxzk (Wave 1D.1) — pinned at `start_capture`
    /// and threaded into both the `sessions.capture_kind` column AND
    /// the dictation-tail KG source-gate. Defaults to
    /// `CaptureKind::Dictation`; Wave 1D.3 lands the path that
    /// promotes this to `CaptureKind::KgNote` for KG-screen audio
    /// captures.
    capture_kind: CaptureKind,
}

/// The mode identifiers a single dictation session uses end-to-end:
/// recording-window colour, cleanup prompt lookup, and the
/// `sessions.mode_id` / `sessions.prompt_id` FKs.
///
/// Resolved fresh at the start of each session from the
/// `dictation.active_mode_slug` setting + the `modes` table. This is
/// what makes the Modes-page active selector take effect on the
/// NEXT dictation without restart / cache-invalidation dance.
///
/// `pub(crate)` so the headless ingest path (`dictation::ingest`) can
/// share the same mode-resolution result without re-implementing the
/// fallback ladder (ADR 0046 §3 — `resolve_active_mode_from_db`).
#[derive(Debug, Clone)]
pub(crate) struct ResolvedMode {
    pub(crate) mode_id: i64,
    pub(crate) slug: String,
    pub(crate) prompt_id: i64,
}

/// Free-function mode resolver — shared by the orchestrator's
/// per-session `resolve_active_mode` AND by the headless ingest path.
///
/// ADR 0046 §3 calls this out as one of the two sanctioned `dictation.rs`
/// refactors: pull the body of `resolve_active_mode` into a free fn
/// so the headless ingest module can reuse the same fallback ladder
/// without taking a dependency on `DictationOrchestrator`.
///
/// The two graceful fallbacks (mutex poisoning, settings/modes lookup
/// failure) collapse onto the caller-supplied `fallback`, which is
/// the boot-time `OrchestratorConfig` for both production callers.
pub(crate) fn resolve_active_mode_from_db(
    db: &Arc<Mutex<Connection>>,
    fallback_mode_id: i64,
    fallback_slug: &str,
    fallback_prompt_id: i64,
) -> ResolvedMode {
    let conn = match db.lock() {
        Ok(g) => g,
        Err(_) => {
            tracing::warn!("active-mode: db mutex poisoned; using boot-time config");
            return ResolvedMode {
                mode_id: fallback_mode_id,
                slug: fallback_slug.to_string(),
                prompt_id: fallback_prompt_id,
            };
        }
    };
    resolve_active_mode_from_conn(&conn, fallback_mode_id, fallback_slug, fallback_prompt_id)
}

/// Inner mode-resolver — same fallback ladder as
/// [`resolve_active_mode_from_db`] but without the mutex-locking
/// step, for callers that already hold the connection lock. Wave
/// 1D.3 / mb-0gt6 introduced this split so the KG text-note ingest
/// path can reuse the same active-mode logic without re-locking
/// (the IPC handler holds the lock for the whole insert + enqueue
/// transaction).
pub(crate) fn resolve_active_mode_from_conn(
    conn: &Connection,
    fallback_mode_id: i64,
    fallback_slug: &str,
    fallback_prompt_id: i64,
) -> ResolvedMode {
    let fallback = || ResolvedMode {
        mode_id: fallback_mode_id,
        slug: fallback_slug.to_string(),
        prompt_id: fallback_prompt_id,
    };
    let slug: String = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            [crate::commands::active_mode::ACTIVE_MODE_KEY],
            |r| r.get(0),
        )
        .unwrap_or_else(|_| fallback_slug.to_string());
    match conn.query_row(
        "SELECT id, prompt_id FROM modes WHERE slug = ?1",
        [&slug],
        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
    ) {
        Ok((mode_id, prompt_id)) => ResolvedMode {
            mode_id,
            slug,
            prompt_id,
        },
        Err(e) => {
            tracing::warn!(error = ?e, mode = %slug,
                "active-mode: modes-table lookup failed; using boot-time config");
            fallback()
        }
    }
}

/// ADR 0050 / mb-ryq4 — Phase 1B Chunk 4. Bridge the dictation tail
/// to the Knowledge Graph filing queue.
///
/// Called from `persist_complete` after the session row + transcripts
/// have been written and (for the success paths) the edit-free-send
/// metric has been armed. Returns `()` — by contract this MUST NOT
/// propagate errors back to the dictation orchestrator (ADR 0050
/// invariant `kg-graph-failure-non-regressing`).
///
/// ## Why a free fn (and not inline)
///
/// Three guard rails sit in a small, easily-mistested arrangement
/// here: the outcome match (which subset of [`InjectionOutcome`]
/// variants are "successful enough to file"), the
/// [`SettingKey::KgGraphEnabled`] read (default-off binding per
/// ADR 0049 §6), and the ignore-error wrapper around
/// [`kg::enqueue_for_filing`]. Extracting them lets a throwaway-crate
/// harness exercise each one without instantiating the full
/// orchestrator (which transitively pulls whisper-rs/ort/cuda —
/// LESSONS P2). The call site in `persist_complete` is one line; the
/// surface area authorized by ADR 0050 §"Dictation-surface
/// authorization clause" is unchanged.
///
/// ## Three-gate cascade
///
/// As of ADR 0052 / Wave 1D.1 (`mb-pxzk`) the helper enforces THREE
/// independent gates in this order:
///
///   1. **Outcome gate** (ADR 0050 Decision B). Enqueues iff `outcome`
///      is one of [`InjectionOutcome::Ok`],
///      [`InjectionOutcome::OkClipboardNotRestored`], or
///      [`InjectionOutcome::InAppNoInject`] (ADR 0045 — text exists in
///      the Dictations list; the no-inject is intentional, not
///      defensive). The five abort/failure variants intentionally do
///      NOT enqueue.
///
///   2. **Source gate** (ADR 0052 Decision D1, NEW in 1D.1). Enqueues
///      iff `capture_kind == CaptureKind::KgNote`. Standard
///      `Dictation` sessions never enqueue regardless of the toggle
///      state — this is what reverses the Phase 1B trigger direction
///      so the user opts in per-capture rather than globally. The
///      reserved `KgNoteText` variant is rejected here too (it can't
///      reach this code path today; text notes bypass the sessions
///      table per ADR 0052 §D3, but enumerating the match exhaustively
///      makes the contract explicit).
///
///   3. **Toggle gate** (ADR 0050 Decision A). Enqueues iff
///      `SettingKey::KgGraphEnabled` reads true. Kept as the LAST
///      check so a flipped toggle alone can never re-activate stale
///      drift from the other two gates. Lives at this call site (not
///      inside [`kg::enqueue_for_filing`]) so the Phase 1E backfill
///      can reuse the same store function without a parallel path.
///
/// `pub(crate)` so the Chunk 5 graph-off invariant probe
/// (`kg::graph_off_invariant`) can exercise these gates without
/// instantiating the orchestrator. The probe asserts the
/// `kg-graph-off-untouched` principal invariant (ADR 0050 §"Invariants")
/// across all eight `InjectionOutcome` variants under the new
/// three-gate cascade.
pub(crate) fn try_enqueue_for_kg_filing(
    conn: &Connection,
    session_id: i64,
    outcome: InjectionOutcome,
    capture_kind: CaptureKind,
    captured_iso: &str,
) {
    if !matches!(
        outcome,
        InjectionOutcome::Ok
            | InjectionOutcome::OkClipboardNotRestored
            | InjectionOutcome::InAppNoInject
    ) {
        return;
    }
    // Source gate (1D.1): only KG-screen audio notes enqueue. Every
    // other capture_kind value -- including the reserved KgNoteText
    // (which bypasses sessions today, but match exhaustively for
    // defensiveness) -- is a no-op here.
    if !matches!(capture_kind, CaptureKind::KgNote) {
        return;
    }
    let kg_enabled = Settings::new(conn)
        .get::<bool>(SettingKey::KgGraphEnabled)
        .unwrap_or(false);
    if !kg_enabled {
        return;
    }
    if let Err(e) = kg::enqueue_for_filing(conn, session_id, captured_iso) {
        tracing::warn!(
            error = ?e,
            session_id,
            "KG enqueue failed; dictation tail continues (kg-graph-failure-non-regressing)"
        );
    }
}

#[cfg(test)]
mod kg_source_gate_tests {
    //! Unit coverage for the Wave 1D.1 source-gate (ADR 0052 §D1).
    //!
    //! These three tests are the explicit acceptance contract from
    //! the Wave 1D.1 kickoff: every combination of
    //! (standard dictation, kg-note) × (toggle off, toggle on) must
    //! produce the right enqueue / no-enqueue decision. The fourth
    //! cell (standard dictation + toggle off) is covered exhaustively
    //! by the existing `kg_graph_off_invariant` probe across all 8
    //! `InjectionOutcome` variants and is intentionally not duplicated
    //! here.
    //!
    //! Lives under `#[cfg(test)]` in this module so the function-under-
    //! test (`try_enqueue_for_kg_filing`, `pub(crate)`) is reachable
    //! without widening any module boundary.
    use rusqlite::Connection;

    use super::try_enqueue_for_kg_filing;
    use crate::db::migrations::apply_all;
    use crate::db::sessions::CaptureKind;
    use crate::injection::InjectionOutcome;
    use crate::settings::model::SettingKey;
    use crate::settings::Settings;

    fn fresh_db_with_session() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory");
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        apply_all(&conn).expect("apply migrations");
        conn.execute(
            "INSERT INTO sessions (id, uuid, mode_id, hotkey_pressed, started_at,
                recording_ended_at, status, audio_duration_ms)
             VALUES (1, 'source-gate-test', 1, 'RCtrl+Space',
                     '2026-06-03T08:00:00Z', '2026-06-03T08:00:01Z',
                     'complete', 1000)",
            [],
        )
        .unwrap();
        conn
    }

    fn queue_count(conn: &Connection) -> i64 {
        conn.query_row("SELECT COUNT(*) FROM kg_filing_queue", [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn standard_dictation_with_toggle_on_does_not_enqueue() {
        // The 1D.1 invariant: even with KG filing globally enabled,
        // a plain `Dictation` capture must not enqueue. This is the
        // reversal of the Phase 1B trigger direction.
        let conn = fresh_db_with_session();
        Settings::new(&conn)
            .set(SettingKey::KgGraphEnabled, &true)
            .unwrap();

        try_enqueue_for_kg_filing(
            &conn,
            1,
            InjectionOutcome::Ok,
            CaptureKind::Dictation,
            "2026-06-03T08:00:02Z",
        );

        assert_eq!(
            queue_count(&conn),
            0,
            "Dictation captures must NOT enqueue even with toggle on"
        );
    }

    #[test]
    fn kg_note_with_toggle_on_enqueues() {
        // The 1D.3 happy path proven at the gate layer: kg-note +
        // toggle on + Ok outcome must produce exactly one queue row.
        let conn = fresh_db_with_session();
        Settings::new(&conn)
            .set(SettingKey::KgGraphEnabled, &true)
            .unwrap();

        try_enqueue_for_kg_filing(
            &conn,
            1,
            InjectionOutcome::Ok,
            CaptureKind::KgNote,
            "2026-06-03T08:00:02Z",
        );

        assert_eq!(
            queue_count(&conn),
            1,
            "KgNote + toggle on + Ok outcome must enqueue exactly one row"
        );
    }

    #[test]
    fn kg_note_with_toggle_off_does_not_enqueue() {
        // The defense-in-depth invariant: even a kg-note must not
        // enqueue if the global toggle is off. (The migration 024
        // seed defaults the toggle to false; we don't flip it.)
        let conn = fresh_db_with_session();
        let toggle: bool = Settings::new(&conn)
            .get(SettingKey::KgGraphEnabled)
            .unwrap();
        assert!(!toggle, "test precondition: toggle must default to false");

        try_enqueue_for_kg_filing(
            &conn,
            1,
            InjectionOutcome::Ok,
            CaptureKind::KgNote,
            "2026-06-03T08:00:02Z",
        );

        assert_eq!(
            queue_count(&conn),
            0,
            "KgNote + toggle off must NOT enqueue (defense in depth)"
        );
    }

    #[test]
    fn kg_note_text_does_not_enqueue_through_dictation_path() {
        // ADR 0052 §D3: text notes bypass the sessions table
        // entirely. They reach kg_filing_queue via a separate
        // synthetic-entry_id path in Wave 1D.3, NOT through the
        // dictation tail. If a kg-note-text value ever reached this
        // helper (which it shouldn't), the gate must treat it as
        // non-enqueue — only KgNote enqueues here.
        let conn = fresh_db_with_session();
        Settings::new(&conn)
            .set(SettingKey::KgGraphEnabled, &true)
            .unwrap();

        try_enqueue_for_kg_filing(
            &conn,
            1,
            InjectionOutcome::Ok,
            CaptureKind::KgNoteText,
            "2026-06-03T08:00:02Z",
        );

        assert_eq!(
            queue_count(&conn),
            0,
            "KgNoteText must not reach kg_filing_queue through the dictation tail"
        );
    }
}

impl DictationOrchestrator {
    /// Construct an orchestrator from its dependencies.
    ///
    /// `user_overrides` is the per-app strategy table from settings
    /// (e.g. `{"chrome.exe": Paste, "1Password.exe": Abort}`). Empty
    /// map is acceptable — the [`crate::injection::strategy::BUILTIN_OVERRIDES`]
    /// table still applies.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        audio: Box<dyn AudioCapture>,
        vad: Box<dyn VoiceActivityDetector>,
        stt: Box<dyn SpeechToText>,
        cleaner: Box<dyn Cleaner>,
        injector: Box<dyn Injector>,
        window_ctx: Box<dyn WindowContext>,
        secure_guard: Box<dyn SecureInputGuard>,
        recording_window: RecordingWindow,
        db: Arc<Mutex<Connection>>,
        config: OrchestratorConfig,
        user_overrides: HashMap<String, InjectionStrategy>,
        hotkey_tx: Sender<HotkeyEvent>,
        next_start_is_programmatic: Arc<AtomicBool>,
        next_start_is_kg_note: Arc<AtomicBool>,
        vault: Arc<crate::vault::export_job::VaultRuntime>,
    ) -> Self {
        Self {
            audio,
            vad,
            stt,
            cleaner,
            injector,
            window_ctx,
            secure_guard,
            recording_window,
            db,
            config,
            user_overrides,
            hotkey_tx,
            next_start_is_programmatic,
            next_start_is_kg_note,
            vault,
            state: SessionState::default(),
        }
    }

    /// Emit the post-persist UI refetch signal via the
    /// [`events::SessionsEventBus`] trait — same trait the headless
    /// ingest path uses (ADR 0046 §3.1). `RecordingWindow` is the PTT
    /// path's bus impl; routing through the trait keeps PTT and
    /// headless emits going through one observable surface.
    fn emit_session_saved(&self, id: i64) {
        use crate::dictation::events::SessionsEventBus;
        <RecordingWindow as SessionsEventBus>::emit_session_saved(&self.recording_window, id);
    }

    /// Send `PipelineComplete` to the state-machine driver.
    ///
    /// Called at the end of every terminal orchestrator action
    /// (`complete()`, `discard()`, and the persistence-helper error
    /// paths invoked from inside `complete()`). The state machine
    /// uses this to transition `Processing → Idle`; without it the
    /// machine sticks in `Processing` and silently ignores all
    /// further hotkey events per §6.1.
    ///
    /// Send failure is tracing-logged but not propagated: if the
    /// driver is gone, we're shutting down anyway and the orchestrator
    /// will see its own action channel close on the next iteration.
    fn signal_pipeline_complete(&self) {
        if let Err(e) = self.hotkey_tx.send(HotkeyEvent::PipelineComplete) {
            tracing::warn!(error = ?e, "could not signal PipelineComplete — driver gone?");
        }
    }

    /// Run the event loop. Returns when **both** input channels close.
    ///
    /// ## Two-channel topology (ADR 0046 §3.2)
    ///
    /// Two independent producers feed the orchestrator:
    ///
    /// 1. **`actions`** — `StateAction`s from the hotkey FSM, bridged
    ///    into a crossbeam channel inside
    ///    [`crate::dictation::runtime::run_dictation_thread`] (the
    ///    upstream is still the unmodified `std::sync::mpsc` produced
    ///    by `StateDriver::start`; the bridge is purely a type
    ///    adapter so we can `select!` on it). PTT path.
    ///
    /// 2. **`headless_rx`** — [`HeadlessIngestRequest`]s from the
    ///    `+ Audio file` IPC (Iter 1) and the future inbox watcher
    ///    (Iter 3). Mobile / file-import path.
    ///
    /// `select!` picks whichever fires first; both arms run on this
    /// thread so the orchestrator-owned `Box<dyn VoiceActivityDetector>`
    /// / `Box<dyn SpeechToText>` / `Box<dyn Cleaner>` are reused
    /// across both paths — no duplicate whisper-rs CUDA allocations.
    ///
    /// Shutdown semantics: the loop exits when **both** receivers
    /// have disconnected. If only one disconnects, that arm is
    /// effectively dead (`select!` keeps polling the live one); the
    /// loop ends when there is nothing left to process anywhere.
    /// In practice the dictation runtime drops both senders together
    /// at shutdown, so the loop usually exits in one tick.
    pub fn run(
        mut self,
        actions: CrossbeamReceiver<StateAction>,
        headless_rx: CrossbeamReceiver<HeadlessIngestRequest>,
    ) -> AppResult<()> {
        // Track which legs are still alive so we exit cleanly once
        // both producers are gone (rather than busy-looping on a
        // closed receiver inside `select!`).
        let mut actions_open = true;
        let mut headless_open = true;
        while actions_open || headless_open {
            select! {
                recv(actions) -> msg => match msg {
                    Ok(action) => {
                        if let Err(e) = self.handle(action) {
                            // Per ADR 0010 + the "never lose provenance"
                            // rule, a pipeline error doesn't kill the
                            // orchestrator. Log and continue.
                            tracing::error!(error = ?e, "orchestrator action failed");
                        }
                    }
                    Err(_) => {
                        tracing::info!("orchestrator: state-action channel closed");
                        actions_open = false;
                    }
                },
                recv(headless_rx) -> msg => match msg {
                    Ok(req) => self.handle_headless(req),
                    Err(_) => {
                        tracing::info!("orchestrator: headless-ingest channel closed");
                        headless_open = false;
                    }
                },
            }
        }
        Ok(())
    }

    /// Process one [`HeadlessIngestRequest`] from the sibling
    /// channel. Reuses the orchestrator's existing VAD / STT /
    /// Cleaner / DB / events deps so a file import or mobile-inbox
    /// courier costs zero additional model loads.
    ///
    /// The result is sent back via the per-request reply channel.
    /// If the caller has dropped its `reply_rx` (IPC panicked,
    /// browser tab closed mid-import) we log and drop the result —
    /// the orchestrator MUST stay live for the next request either
    /// way.
    fn handle_headless(&mut self, req: HeadlessIngestRequest) {
        let HeadlessIngestRequest {
            samples,
            provenance,
            reply_tx,
        } = req;

        // Build the borrowed dep bundle for one call. All borrows
        // are released as soon as `headless_ingest` returns — the
        // orchestrator's owned `Box<dyn ...>` fields stay put.
        let deps = IngestDeps {
            vad: self.vad.as_mut(),
            stt: self.stt.as_mut(),
            cleaner: self.cleaner.as_mut(),
            db: &self.db,
            events: &self.recording_window,
            config: &self.config,
        };
        let result = headless_ingest(deps, samples, provenance);

        // ADR 0046 Iter 2 / mb-lvzw — fire the vault trigger on a
        // successful headless ingest (file-import path + future
        // mobile-inbox courier). Failures don't trigger; same
        // contract as `persist_complete` above.
        if result.is_ok() {
            self.vault.trigger(Arc::clone(&self.db));
        }

        if reply_tx.send(result).is_err() {
            tracing::warn!(
                "headless ingest reply channel closed; caller dropped \
                 receiver before result arrived (DB row is fine; toast lost)"
            );
        }
    }

    fn handle(&mut self, action: StateAction) -> AppResult<()> {
        match action {
            StateAction::None => Ok(()),
            StateAction::StartCapture(_mode) => self.start_capture(),
            // Wave 4.8: every terminal action MUST emit
            // PipelineComplete back to the state machine, even on
            // pipeline error. Centralised here (not inside
            // complete()/discard()) so the signal can't get lost in
            // an early-return error path inside the helpers.
            StateAction::StopCapture => {
                let result = self.complete();
                self.signal_pipeline_complete();
                result
            }
            StateAction::DiscardAudio => {
                let result = self.discard();
                self.signal_pipeline_complete();
                result
            }
            StateAction::ShowConfirmCancel => {
                // Wave 5 surfaces the tray toast. For now: log.
                tracing::info!("confirm-cancel UI requested (Wave 5 will render)");
                Ok(())
            }
            StateAction::HideConfirmCancel => {
                tracing::debug!("confirm-cancel UI dismissed");
                Ok(())
            }
        }
    }

    fn start_capture(&mut self) -> AppResult<()> {
        self.state.fg_keydown = self.window_ctx.foreground().ok();
        self.state.started_at = Some(Instant::now());
        self.state.started_at_iso = Some(now_iso());
        // ADR 0045 + mb-tfyp. Snapshot + RESET the programmatic flag
        // atomically. swap() guarantees the next PTT hold can't
        // accidentally inherit the bit even if this `start_capture`
        // races with another `DictationRuntime::start` call (which is
        // a state-machine no-op in `Recording` / `Processing` anyway,
        // but we want the flag to be honest).
        let start_mode = if self
            .next_start_is_programmatic
            .swap(false, Ordering::SeqCst)
        {
            StartMode::InApp
        } else {
            StartMode::Ptt
        };
        self.state.start_mode = start_mode;
        // ADR 0052 + mb-0gt6 (Wave 1D.3). Sibling swap-and-pin for
        // the KG-note source-gate flag. Same SeqCst guarantee: the
        // bit cannot leak into the next session even if a stray
        // `start_kg_note` races with a real PTT hold (the FSM no-op
        // in `Recording` / `Processing` swallows the synthetic
        // KeyDown anyway, so the misattribution surface is closed
        // at the state machine the same way the programmatic flag
        // is). Defaults to `Dictation` so the pre-1D source-gate
        // contract holds exactly as before for every non-KG path.
        if self.next_start_is_kg_note.swap(false, Ordering::SeqCst) {
            self.state.capture_kind = CaptureKind::KgNote;
        } else {
            self.state.capture_kind = CaptureKind::Dictation;
        }
        // Pin the active mode for this whole session BEFORE we open
        // the mic. Single resolution per session: recording-window
        // colour, cleanup prompt, and DB FKs all see the same mode.
        let resolved = self.resolve_active_mode();
        self.audio.start()?;
        // Tell the recording-window owner about start_mode BEFORE
        // show() emits the first `listening` event — otherwise the
        // overlay's first paint won't carry the `startMode` field
        // and the pill-overlay Stop button would briefly not render.
        self.recording_window.set_start_mode(start_mode);
        // show() handles both the OS-level window display + emitting
        // the initial `listening` state to the React overlay.
        self.recording_window.show(&resolved.slug)?;
        tracing::info!(
            fg = ?self.state.fg_keydown.as_ref().map(|f| &f.process_name),
            mode = %resolved.slug,
            start_mode = start_mode.as_db_str(),
            "dictation: start_capture"
        );
        self.state.active_mode = Some(resolved);
        Ok(())
    }

    /// Read the active transcription mode from the settings table
    /// and look up the corresponding `modes` row.
    ///
    /// This is the bridge between the Modes-page UI selector and the
    /// orchestrator pipeline. Called once per session at
    /// `start_capture`. Two graceful fallbacks for the unhappy paths:
    ///
    ///   1. DB mutex poisoned → use the boot-time `self.config`. A
    ///      poisoned mutex means a panic crossed a thread boundary,
    ///      and the user's best bet is "keep going on the last good
    ///      config" rather than "silently lose the dictation".
    ///   2. Settings row missing OR modes lookup fails → same
    ///      fallback. Covers fresh installs (settings row not yet
    ///      seeded) and corrupt-DB edge cases.
    ///
    /// Both fallbacks log at WARN so the issue is visible without
    /// killing the dictation. Cost: one indexed-PK lookup per
    /// session — negligible vs. STT/cleanup latency.
    fn resolve_active_mode(&self) -> ResolvedMode {
        // ADR 0046 §3: body extracted into the free `resolve_active_mode_from_db`
        // so the headless ingest path can share the same fallback ladder.
        resolve_active_mode_from_db(
            &self.db,
            self.config.mode_id,
            &self.config.mode_slug,
            self.config.prompt_id,
        )
    }

    /// Return the session-pinned mode, or fall back to the boot-time
    /// config when no session is active. Used by the DB-insert
    /// helpers, which may be called from error paths that bypass
    /// `start_capture`.
    fn current_mode(&self) -> ResolvedMode {
        self.state
            .active_mode
            .clone()
            .unwrap_or_else(|| ResolvedMode {
                mode_id: self.config.mode_id,
                slug: self.config.mode_slug.clone(),
                prompt_id: self.config.prompt_id,
            })
    }

    fn complete(&mut self) -> AppResult<()> {
        // Snapshot key-up state.
        let stop_at = Instant::now();
        let stop_iso = now_iso();
        self.audio.stop()?;
        // Window stays VISIBLE through the rest of the pipeline so
        // the user gets progress feedback (transcribing → cleaning →
        // pasting → done). It's hidden at the end of this fn, or by
        // the persist_failed_* helpers on early-return paths.
        //
        // **Belt + suspenders**: wire a Drop guard so that if ANY
        // step in complete() panics, early-returns through `?`, or
        // hangs long enough to be killed, the pill still vanishes.
        // Without this, a hung cleanup HTTP call (Ollama loading a
        // cold model and exceeding our 30s timeout) leaves the user
        // staring at a frozen "CLEANING" pill forever — observed in
        // Phase 5 smoketest 2026-05-17. Guard is disarmed right
        // before the explicit DONE-flash hide() at the bottom.
        let mut pill_guard = PillHideGuard::arm(self.recording_window.clone());
        // Use the session-pinned mode (resolved at start_capture).
        // Cloning is cheap — ResolvedMode is one i64 + a short String.
        let active = self.current_mode();
        let mode_slug = active.slug.clone();

        // Drain the audio.
        let mut samples: Vec<i16> = Vec::new();
        self.audio.drain(&mut samples)?;
        let recording_duration_ms = self
            .state
            .started_at
            .map(|s| stop_at.duration_since(s).as_millis() as i64)
            .unwrap_or(0);

        tracing::debug!(
            target: "audio",
            drained_samples = samples.len(),
            recording_duration_ms,
            "complete(): drained audio from ring"
        );

        // Dump the post-resample buffer to a WAV file so a human
        // can listen to it on demand. Single-slot (overwrites every
        // session). Wave 5 will gate this behind a tray-menu "debug
        // capture" toggle; for now it's always-on (cheap).
        if let Err(e) = debug_dump_wav(&samples) {
            tracing::warn!(error = ?e, "debug WAV dump failed");
        }

        // Pull key-up snapshot. None == nothing focused (e.g. between
        // desktops) — treat as focus-changed abort.
        let fg_keyup = match self.window_ctx.foreground() {
            Ok(fg) => fg,
            Err(e) => {
                tracing::warn!(error = ?e, "no foreground at key-up; aborting");
                return self.persist_failed_no_foreground(stop_iso, recording_duration_ms);
            }
        };

        // Trim with VAD. Errors here fall back to the raw audio —
        // it's better to STT the whole clip than to abort.
        //
        // Reset Silero LSTM state at the start of every utterance.
        // Each dictation is a self-contained clip — there's no
        // streaming context to preserve, and state poisoning from
        // prior captures would bias the model.
        self.vad.reset();
        let trim_start = Instant::now();
        let trimmed: Vec<i16> = trim_speech(&samples, self.vad.as_mut(), &TrimConfig::default())
            .unwrap_or_else(|e| {
                tracing::warn!(error = ?e, "VAD trim failed; falling back to raw audio");
                samples.clone()
            });
        let _vad_ms = trim_start.elapsed().as_millis() as i64;

        tracing::debug!(
            target: "audio",
            raw_samples = samples.len(),
            trimmed_samples = trimmed.len(),
            vad_ms = _vad_ms,
            "complete(): VAD trim result"
        );

        // STT.
        self.recording_window.set_state(
            crate::recording_window::state::TRANSCRIBING,
            Some(&mode_slug),
        );
        // Build the Whisper `initial_prompt` from the live
        // dictionary, biased toward terms whose `app_context`
        // matches the foreground at key-up (ADR 0047 Wave 1.3).
        // `None` on any error / empty-dict path -- Whisper then
        // transcribes bias-free, matching the prior behaviour.
        let initial_prompt = crate::stt::initial_prompt::build_from_db(
            &self.db,
            Some(fg_keyup.process_name.as_str()),
        );
        let stt_start = Instant::now();
        let stt_result = self.stt.transcribe(TranscribeRequest {
            audio: &trimmed,
            initial_prompt: initial_prompt.as_deref(),
            force_cpu: false,
        });
        let stt_latency_ms = stt_start.elapsed().as_millis() as i64;
        let raw_text = match stt_result {
            Ok(t) => t.text,
            Err(e) => {
                return self.persist_failed_stt(stop_iso, recording_duration_ms, &fg_keyup, e)
            }
        };

        // Cleanup.
        self.recording_window
            .set_state(crate::recording_window::state::CLEANING, Some(&mode_slug));
        let cleanup_start = Instant::now();
        tracing::info!(mode = %mode_slug, "dictation: cleanup begin");
        let cleaned_text = match self.cleaner.clean(&raw_text, &mode_slug) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(error = ?e, "cleaner failed; falling back to raw text");
                raw_text.clone()
            }
        };
        let cleanup_latency_ms = cleanup_start.elapsed().as_millis() as i64;
        tracing::info!(
            cleanup_latency_ms,
            cleaned_len = cleaned_text.len(),
            "dictation: cleanup end"
        );

        // ADR 0045 + mb-tfyp — programmatic sessions bypass the
        // focus-drift check AND the inject step entirely. There is
        // no target app for an in-app session (Mockingbird itself is
        // the focus the whole time), so the heuristic doesn't apply
        // and pasting would either be a no-op or land in our own UI.
        // The user clicked Start, dictated, clicked Stop — they get
        // a transcript in the list. That's the contract.
        let is_in_app = self.state.start_mode == StartMode::InApp;

        // **Post-cleanup focus-drift check** (PTT path only).
        //
        // ADR 0020 covers focus changes BETWEEN key-down and key-up
        // (permissive: inject into the key-up app). It does NOT
        // cover focus changes BETWEEN key-up and inject — which is
        // exactly what happens when cleanup hangs for 30 seconds on
        // a cold Ollama model load and the user, bored, clicks over
        // to look at Mockingbird's History window. The injector
        // happily pastes into whatever's CURRENTLY focused (the
        // History window, which eats Ctrl+V as a no-op) and reports
        // `outcome=Ok`. Result: a successful-looking session row
        // with no visible paste — the worst kind of silent failure.
        //
        // Defense: re-snapshot the foreground right before inject.
        // If the process name has changed from fg_keyup, abort with
        // `AbortedFocusChanged` (raw + cleaned still persist, only
        // the `final` stage is skipped). User can retry; better
        // than pasting into the wrong window.
        let focus_drifted = if is_in_app {
            false
        } else {
            let fg_now = self.window_ctx.foreground().ok();
            let drifted = match &fg_now {
                Some(now) => !same_process(now, &fg_keyup),
                None => true, // null foreground == drifted away from everything
            };
            if drifted {
                tracing::warn!(
                    fg_keyup = ?fg_keyup.process_name,
                    fg_now = ?fg_now.as_ref().map(|f| &f.process_name),
                    cleanup_latency_ms,
                    "focus drifted between key-up and inject (likely slow cleanup + \
                     user navigated away); aborting to avoid pasting into wrong window"
                );
            }
            drifted
        };

        // Inject (or skip per start_mode / focus drift / decision).
        self.recording_window
            .set_state(crate::recording_window::state::PASTING, Some(&mode_slug));
        let inject_start = Instant::now();
        let outcome = if is_in_app {
            // Programmatic session: nothing to paste into. Distinct
            // outcome (`in_app`) so the UI can render an `IN_APP`
            // pill without re-deriving the semantic from
            // `start_mode` AND `injection_status` in two places.
            tracing::info!(
                text_len = cleaned_text.len(),
                "dictation: inject skipped — programmatic (in-app) session"
            );
            InjectionOutcome::InAppNoInject
        } else {
            // Secure-input check + focus-loss + strategy resolution.
            // PTT path only — built lazily so the in-app branch
            // doesn't pay for an unused secure_guard probe.
            let is_secure = self.secure_guard.is_secure(&fg_keyup);
            let inputs = pipeline::Inputs {
                fg_keydown: self.state.fg_keydown.as_ref(),
                fg_keyup: &fg_keyup,
                is_secure,
                user_overrides: &self.user_overrides,
            };
            let decision = pipeline::decide(&inputs);
            tracing::info!(
                decision = ?decision,
                text_len = cleaned_text.len(),
                focus_drifted,
                "dictation: inject begin"
            );
            if focus_drifted {
                // Reuse the existing legacy variant: semantically "focus
                // changed and we declined to paste". The DB CHECK
                // constraint already allows `aborted_focus_changed`
                // (migrations/004) so no schema change needed.
                InjectionOutcome::AbortedFocusChanged
            } else {
                match decision {
                    pipeline::Decision::Proceed(strategy) => {
                        // Append a single trailing space to the *paste*
                        // payload (NOT the persisted text) so the user's
                        // next dictation flows naturally without
                        // needing a leading space. See
                        // dictation::paste_payload for the policy + tests.
                        let to_paste = paste_payload::paste_payload(&cleaned_text);
                        match self.injector.inject(&to_paste, strategy) {
                            Ok(o) => o,
                            Err(e) => {
                                tracing::warn!(error = ?e, "injector returned error");
                                InjectionOutcome::FailedSendInput
                            }
                        }
                    }
                    pipeline::Decision::Abort(o) => o,
                }
            }
        };
        let injection_latency_ms = inject_start.elapsed().as_millis() as i64;
        tracing::info!(
            injection_latency_ms,
            outcome = ?outcome,
            "dictation: inject end"
        );

        // Persist. Whether the `final` transcript stage gets a row
        // depends on whether we actually injected something: an
        // aborted-secure or aborted-user-opt-out session has NO
        // final stage (nothing was injected), but raw + cleaned
        // must always be persisted (provenance > shortcuts).
        let injected_text = match outcome {
            InjectionOutcome::Ok | InjectionOutcome::OkClipboardNotRestored => {
                Some(cleaned_text.as_str())
            }
            _ => None,
        };
        self.persist_complete(PersistCompleteParams {
            recording_ended_iso: stop_iso,
            recording_duration_ms,
            fg_keyup: &fg_keyup,
            outcome,
            stt_latency_ms: Some(stt_latency_ms),
            cleanup_latency_ms: Some(cleanup_latency_ms),
            injection_latency_ms: Some(injection_latency_ms),
            raw_text: &raw_text,
            cleaned_text: &cleaned_text,
            injected_text,
            cleanup_model: self.cleaner.model_name(),
            capture_kind: self.state.capture_kind,
        })?;

        // Brief "done" flash before the window vanishes. The 200ms
        // sleep is on the dedicated dictation thread; the next
        // hotkey can't fire until the state machine receives
        // PipelineComplete (sent by the caller in handle()), so the
        // brief pause never delays the user.
        self.recording_window
            .set_state(crate::recording_window::state::DONE, Some(&mode_slug));
        std::thread::sleep(std::time::Duration::from_millis(200));
        // Disarm the guard — we're about to hide explicitly with the
        // proper transition; we don't want a double-hide log.
        pill_guard.disarm();
        self.recording_window.hide()?;

        // Reset transient state.
        self.state = SessionState::default();
        Ok(())
    }

    fn discard(&mut self) -> AppResult<()> {
        self.audio.stop()?;
        let mut samples: Vec<i16> = Vec::new();
        let _ = self.audio.drain(&mut samples); // discard
        self.recording_window.hide()?;
        tracing::info!("dictation: discard");
        self.state = SessionState::default();
        Ok(())
    }

    // ----- persistence helpers -----

    fn persist_complete(&self, p: PersistCompleteParams<'_>) -> AppResult<()> {
        let _ = p.recording_duration_ms; // Wave 5 fills `audio_duration_ms`.
        let conn = self
            .db
            .lock()
            .map_err(|_| AppError::Other("orchestrator: db mutex poisoned".into()))?;
        let id = self.insert_session_row(&conn, &p.recording_ended_iso, p.fg_keyup)?;

        // Transcripts: ALWAYS write raw + cleaned (provenance > shortcuts).
        // Write final only if something was actually injected. Errors
        // are non-fatal — log + continue so the session row at least
        // gets its terminal status update.
        if let Err(e) = transcripts::insert_raw(&conn, id, p.raw_text) {
            tracing::warn!(error = ?e, session_id = id, "persist raw transcript failed");
        }
        if let Err(e) = transcripts::insert_cleaned(&conn, id, p.cleaned_text, p.cleanup_model) {
            tracing::warn!(error = ?e, session_id = id, "persist cleaned transcript failed");
        }
        if let Some(injected) = p.injected_text {
            if let Err(e) = transcripts::insert_final(&conn, id, injected, Some(p.cleanup_model)) {
                tracing::warn!(error = ?e, session_id = id, "persist final transcript failed");
            }
        }

        sessions::update_processing_complete(
            &conn,
            id,
            &ProcessingCompletion {
                completed_at: now_iso(),
                status: SessionStatus::Complete,
                stt_latency_ms: p.stt_latency_ms,
                cleanup_latency_ms: p.cleanup_latency_ms,
                injection_latency_ms: p.injection_latency_ms,
                injection_status: Some(p.outcome.as_db_str().to_string()),
            },
        )?;

        // mb-v2fa / ADR 0047 §Wave 2.5 -- arm the edit-free-send
        // metric ONLY when the paste actually landed. Abort /
        // failure / in-app variants stay NULL, which the Insights
        // aggregation reads as "excluded from the population". This
        // is the only place success-inject is observed centrally;
        // keeping the eligibility match here means the SQL helper
        // doesn't need to know about every InjectionOutcome variant.
        if matches!(
            p.outcome,
            InjectionOutcome::Ok | InjectionOutcome::OkClipboardNotRestored
        ) {
            if let Err(e) = sessions::mark_injected_for_edit_metric(&conn, id) {
                tracing::warn!(
                    error = ?e,
                    session_id = id,
                    "persist edit-free-send metric arm failed"
                );
            }
        }

        // ADR 0050 / mb-ryq4 — Phase 1B Chunk 4. Hook the dictation
        // tail to the KG filing queue (default-off; gated by
        // SettingKey::KgGraphEnabled). All gating logic + the
        // ignore-error contract live in the helper — see its docs.
        try_enqueue_for_kg_filing(&conn, id, p.outcome, p.capture_kind, &p.recording_ended_iso);

        // Drop the DB lock before emitting so the frontend's
        // refetch (which goes through `list_sessions` -> DB) doesn't
        // race against our still-held connection.
        drop(conn);
        self.emit_session_saved(id);
        // ADR 0046 Iter 2 / mb-lvzw — vault export trigger fires
        // ONLY on the success path. Error-status sessions don't get
        // projected (the query in `vault::export_job::query_dictations`
        // filters by `status = 'complete'`).
        self.vault.trigger(Arc::clone(&self.db));
        Ok(())
    }

    fn persist_failed_stt(
        &self,
        recording_ended_iso: String,
        _recording_duration_ms: i64,
        fg_keyup: &ForegroundWindow,
        err: AppError,
    ) -> AppResult<()> {
        // Surface to the overlay BEFORE the DB write so the user sees
        // an error indicator immediately even if persistence stalls.
        let msg = format!("stt failed: {err}");
        self.recording_window.set_error(&msg);
        self.recording_window.hide()?;
        let conn = self
            .db
            .lock()
            .map_err(|_| AppError::Other("orchestrator: db mutex poisoned".into()))?;
        let id = self.insert_session_row(&conn, &recording_ended_iso, fg_keyup)?;
        sessions::update_status_error(&conn, id, &msg)?;
        drop(conn);
        self.emit_session_saved(id);
        Ok(())
    }

    fn persist_failed_no_foreground(
        &self,
        recording_ended_iso: String,
        _recording_duration_ms: i64,
    ) -> AppResult<()> {
        self.recording_window
            .set_error("no foreground window at key-up");
        self.recording_window.hide()?;
        let conn = self
            .db
            .lock()
            .map_err(|_| AppError::Other("orchestrator: db mutex poisoned".into()))?;
        // No fg_keyup means we can't fill foreground_app — leave NULL.
        let id = self.insert_session_row_no_fg(&conn, &recording_ended_iso)?;
        sessions::update_status_error(&conn, id, "no foreground window at key-up")?;
        drop(conn);
        self.emit_session_saved(id);
        Ok(())
    }

    fn insert_session_row(
        &self,
        conn: &Connection,
        recording_ended_iso: &str,
        fg_keyup: &ForegroundWindow,
    ) -> AppResult<i64> {
        let started_at = self.state.started_at_iso.clone().unwrap_or_else(now_iso);
        let active = self.current_mode();
        let new = NewSession {
            uuid: new_uuid(),
            mode_id: active.mode_id,
            hotkey_pressed: self.config.hotkey_label.clone(),
            started_at,
            recording_ended_at: recording_ended_iso.to_string(),
            status: SessionStatus::Processing,
            foreground_app: Some(fg_keyup.process_name.clone()),
            foreground_window_title: Some(fg_keyup.title.clone()),
            audio_duration_ms: 0, // Wave 5 fills from VAD-trimmed length
            audio_blob_path: None,
            prompt_id: active.prompt_id,
            dictionary_snapshot_id: self.config.dictionary_snapshot_id,
            example_set_id: self.config.example_set_id,
            start_mode: self.state.start_mode,
            // ADR 0046 / mb-jqhw: PTT + in-app live-mic sessions all
            // originate from this desktop's mic. 'desktop-import' and
            // 'mobile-inbox' are written by the headless ingest path
            // (dictation/ingest.rs), never by the orchestrator.
            source: SessionSource::Desktop,
            // ADR 0052 / mb-pxzk: pinned at start_capture. Default
            // `Dictation` for PTT + the in-app Dictations Start
            // button; Wave 1D.3 wires the KG audio-note button to
            // promote this to `KgNote` before start_capture runs.
            capture_kind: self.state.capture_kind,
        };
        sessions::insert(conn, &new)
    }

    fn insert_session_row_no_fg(
        &self,
        conn: &Connection,
        recording_ended_iso: &str,
    ) -> AppResult<i64> {
        let started_at = self.state.started_at_iso.clone().unwrap_or_else(now_iso);
        let active = self.current_mode();
        let new = NewSession {
            uuid: new_uuid(),
            mode_id: active.mode_id,
            hotkey_pressed: self.config.hotkey_label.clone(),
            started_at,
            recording_ended_at: recording_ended_iso.to_string(),
            status: SessionStatus::Processing,
            foreground_app: None,
            foreground_window_title: None,
            audio_duration_ms: 0,
            audio_blob_path: None,
            prompt_id: active.prompt_id,
            dictionary_snapshot_id: self.config.dictionary_snapshot_id,
            example_set_id: self.config.example_set_id,
            start_mode: self.state.start_mode,
            // ADR 0046 / mb-jqhw: same rationale as insert_session_row.
            source: SessionSource::Desktop,
            // ADR 0052 / mb-pxzk: same rationale as insert_session_row.
            capture_kind: self.state.capture_kind,
        };
        sessions::insert(conn, &new)
    }
}

// --------------------------------------------------------------------
// Pure pipeline — testable without traits or OS calls
// --------------------------------------------------------------------

pub mod pipeline {
    //! Pure pipeline-decision functions.
    //!
    //! These run inside [`super::DictationOrchestrator::complete`]
    //! and are the "what should we do?" logic. The orchestrator's
    //! side-effect layer hands them values + acts on the decision.

    use std::collections::HashMap;

    use crate::injection::strategy::InjectionStrategy;
    use crate::injection::strategy_wiring::{decide_injection, InjectionDecision};
    use crate::injection::InjectionOutcome;
    use crate::window_context::ForegroundWindow;

    /// All inputs the decision layer needs.
    #[derive(Debug)]
    pub struct Inputs<'a> {
        /// Foreground at key-down, if we captured it (rare miss).
        pub fg_keydown: Option<&'a ForegroundWindow>,
        /// Foreground at key-up — never `None` here (caller already
        /// handled the no-foreground case).
        pub fg_keyup: &'a ForegroundWindow,
        /// Did the secure-input guard flag this target?
        pub is_secure: bool,
        /// User-defined per-app strategy overrides.
        pub user_overrides: &'a HashMap<String, InjectionStrategy>,
    }

    /// Resolved decision.
    #[derive(Debug, PartialEq, Eq)]
    pub enum Decision {
        /// Proceed with the given strategy.
        Proceed(InjectionStrategy),
        /// Abort with the given outcome (for DB persistence).
        Abort(InjectionOutcome),
    }

    /// Decide what to do. Pure.
    ///
    /// Precedence (top wins):
    ///   1. Secure-input on `fg_keyup` → `Abort(AbortedSecure)`.
    ///   2. Strategy resolution on `fg_keyup` → either `Proceed(...)`
    ///      or `Abort(AbortedUserOptOut)`.
    ///
    /// **No focus-change abort.** Per ADR 0020 (Wave 4.9), focus
    /// change between key-down and key-up is permissive: injection
    /// proceeds into whatever is focused at key-up, with full
    /// provenance recorded. The secure-input guard above runs on
    /// `fg_keyup` so password fields are still independently caught
    /// regardless of which app held focus at key-down.
    pub fn decide(inputs: &Inputs<'_>) -> Decision {
        if inputs.is_secure {
            return Decision::Abort(InjectionOutcome::AbortedSecure);
        }
        match decide_injection(inputs.fg_keydown, inputs.fg_keyup, inputs.user_overrides) {
            InjectionDecision::Proceed(s) => Decision::Proceed(s),
            InjectionDecision::AbortUserOptOut => {
                Decision::Abort(InjectionOutcome::AbortedUserOptOut)
            }
        }
    }
}

// --------------------------------------------------------------------
// Helpers
// --------------------------------------------------------------------

/// RAII guard that hides the recording pill on drop unless explicitly
/// disarmed. Used by [`DictationOrchestrator::complete`] so that any
/// early return (`?`), panic, or task abort still tears down the
/// overlay — a stuck pill is the worst possible failure mode because
/// it covers the user's screen with stale UI and they can't tell if
/// the app is hung or just slow.
///
/// `Clone` on [`RecordingWindow`] is cheap (Arc<AtomicBool> +
/// Arc<Mutex<Option<AppHandle>>>) so we hold a clone rather than a
/// borrow — keeps the borrow checker happy when complete() still
/// needs mutable access to other parts of `self`.
struct PillHideGuard {
    window: RecordingWindow,
    armed: bool,
}

impl PillHideGuard {
    fn arm(window: RecordingWindow) -> Self {
        Self {
            window,
            armed: true,
        }
    }

    /// Disarm the guard — call this on the normal success path right
    /// before doing the explicit `hide()` with a state transition
    /// (e.g. the DONE flash). Without disarming, the explicit hide()
    /// would run, then Drop would call hide() AGAIN, producing two
    /// log lines and two beeps.
    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for PillHideGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        // If the window is already hidden, one of the persist_failed_*
        // helpers already did its explicit hide() with the proper
        // logging. Don't log a noisy redundant warning in the happy
        // "early-return-that-already-cleaned-up" case.
        if !self.window.is_visible() {
            return;
        }
        tracing::warn!(
            "PillHideGuard fired — complete() exited with pill still visible; \
             hiding defensively (likely panic or hung step that no other \
             cleanup path reached)"
        );
        // Best-effort: hide() returns AppResult but we're in Drop so
        // we can't propagate; tracing-log on failure.
        if let Err(e) = self.window.hide() {
            tracing::warn!(error = ?e, "PillHideGuard: hide() failed");
        }
    }
}

fn new_uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Case-insensitive `process_name` equality for two foreground
/// snapshots. Used by the post-cleanup focus-drift check in
/// [`DictationOrchestrator::complete`] to decide whether the user
/// has navigated away from their dictation target during a slow
/// cleanup (typically a cold-load Ollama call).
///
/// We match on process basename only — not HWND or title — because:
///   - HWND changes when an app cycles between document windows but
///     the user's mental model is "I'm still in Notepad".
///   - Title changes constantly (`*The quick brown fox - Notepad`
///     vs `The quick brown fox - Notepad`) and would trip false
///     drift alarms on every keystroke.
fn same_process(a: &ForegroundWindow, b: &ForegroundWindow) -> bool {
    a.process_name.eq_ignore_ascii_case(&b.process_name)
}

/// Dump the post-resample drained audio to a WAV file at
/// `%APPDATA%/com.dustin.mockingbird/last_capture.wav`.
///
/// Single-slot (overwrites every session). Useful for debugging
/// audio-pipeline issues — open the file in any audio player to
/// verify what we're handing to Silero + Whisper. Wave 5 will gate
/// behind a tray-menu toggle.
fn debug_dump_wav(samples: &[i16]) -> AppResult<()> {
    // Resolve %APPDATA%\com.dustin.mockingbird directly — this is
    // diagnostic-only code so we accept the cross-cutting env lookup
    // rather than threading an app_data_dir down through the
    // orchestrator's constructor (Wave 5 will gate this behind a CLI
    // flag and revisit the path-resolution story).
    let app_data =
        std::env::var("APPDATA").map_err(|e| AppError::Audio(format!("APPDATA env: {e}")))?;
    let dir = std::path::PathBuf::from(app_data).join("com.dustin.mockingbird");
    let path = dir.join("last_capture.wav");
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(&path, spec)
        .map_err(|e| AppError::Audio(format!("WAV create: {e}")))?;
    for &s in samples {
        writer
            .write_sample(s)
            .map_err(|e| AppError::Audio(format!("WAV write_sample: {e}")))?;
    }
    writer
        .finalize()
        .map_err(|e| AppError::Audio(format!("WAV finalize: {e}")))?;
    tracing::info!(target: "audio", path = %path.display(), "debug WAV dumped");
    Ok(())
}

fn now_iso() -> String {
    // Phase 1 chose chrono-free time strings; format manually to keep
    // the dep surface minimal. Pattern: 2026-05-17T03:14:15Z.
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format_secs_as_iso(secs)
}

/// Format a Unix timestamp (seconds) as an ISO-8601 string in UTC.
/// Pure helper — testable without clock access.
pub fn format_secs_as_iso(secs: u64) -> String {
    // Naive but correct UTC seconds → broken-down time. Matches what
    // SQLite's `datetime('now')` produces.
    let (days, time_of_day) = (secs / 86_400, secs % 86_400);
    let (hours, rest) = (time_of_day / 3600, time_of_day % 3600);
    let (minutes, seconds) = (rest / 60, rest % 60);
    let (year, month, day) = civil_from_days(days as i64);
    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
}

/// Howard Hinnant's days→Y/M/D algorithm (public domain). Used here
/// so we don't pull in chrono just for one ISO string.
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let y = (y + if m <= 2 { 1 } else { 0 }) as i32;
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::injection::InjectionOutcome;

    fn fg(name: &str) -> ForegroundWindow {
        ForegroundWindow {
            hwnd: 0x100,
            title: "Title".into(),
            process_name: name.into(),
            exe_path: None,
        }
    }

    // -----------------------------------------------------------------
    // pipeline::decide — exhaustive cases
    // -----------------------------------------------------------------

    #[test]
    fn happy_path_proceeds_with_paste() {
        let prev = fg("notepad.exe");
        let now = fg("notepad.exe");
        let overrides = HashMap::new();
        let d = pipeline::decide(&pipeline::Inputs {
            fg_keydown: Some(&prev),
            fg_keyup: &now,
            is_secure: false,
            user_overrides: &overrides,
        });
        assert_eq!(d, pipeline::Decision::Proceed(InjectionStrategy::Paste));
    }

    #[test]
    fn secure_input_aborts_even_if_focus_matches() {
        let prev = fg("notepad.exe");
        let now = fg("notepad.exe");
        let overrides = HashMap::new();
        let d = pipeline::decide(&pipeline::Inputs {
            fg_keydown: Some(&prev),
            fg_keyup: &now,
            is_secure: true,
            user_overrides: &overrides,
        });
        assert_eq!(
            d,
            pipeline::Decision::Abort(InjectionOutcome::AbortedSecure)
        );
    }

    #[test]
    fn focus_change_proceeds_into_keyup_app_per_adr_0020() {
        // ADR 0020 permissive policy: user dictates into notepad,
        // alt-tabs to chrome before releasing the hotkey, text
        // lands in chrome. chrome.exe → Paste strategy.
        let prev = fg("notepad.exe");
        let now = fg("chrome.exe");
        let overrides = HashMap::new();
        let d = pipeline::decide(&pipeline::Inputs {
            fg_keydown: Some(&prev),
            fg_keyup: &now,
            is_secure: false,
            user_overrides: &overrides,
        });
        assert_eq!(d, pipeline::Decision::Proceed(InjectionStrategy::Paste));
    }

    #[test]
    fn focus_change_into_secure_input_still_aborts() {
        // Secure-input guard runs on fg_keyup regardless of where
        // focus was at key-down — belt-and-suspenders for the
        // permissive focus-change policy.
        let prev = fg("notepad.exe");
        let now = fg("chrome.exe");
        let overrides = HashMap::new();
        let d = pipeline::decide(&pipeline::Inputs {
            fg_keydown: Some(&prev),
            fg_keyup: &now,
            is_secure: true,
            user_overrides: &overrides,
        });
        assert_eq!(
            d,
            pipeline::Decision::Abort(InjectionOutcome::AbortedSecure)
        );
    }

    #[test]
    fn user_opt_out_aborts_via_password_manager() {
        let prev = fg("1Password.exe");
        let now = fg("1Password.exe");
        let overrides = HashMap::new();
        let d = pipeline::decide(&pipeline::Inputs {
            fg_keydown: Some(&prev),
            fg_keyup: &now,
            is_secure: false,
            user_overrides: &overrides,
        });
        assert_eq!(
            d,
            pipeline::Decision::Abort(InjectionOutcome::AbortedUserOptOut)
        );
    }

    #[test]
    fn terminal_uses_keystroke_strategy() {
        let prev = fg("WindowsTerminal.exe");
        let now = fg("WindowsTerminal.exe");
        let overrides = HashMap::new();
        let d = pipeline::decide(&pipeline::Inputs {
            fg_keydown: Some(&prev),
            fg_keyup: &now,
            is_secure: false,
            user_overrides: &overrides,
        });
        assert_eq!(d, pipeline::Decision::Proceed(InjectionStrategy::Keystroke));
    }

    #[test]
    fn missing_keydown_does_not_block_injection() {
        let now = fg("notepad.exe");
        let overrides = HashMap::new();
        let d = pipeline::decide(&pipeline::Inputs {
            fg_keydown: None,
            fg_keyup: &now,
            is_secure: false,
            user_overrides: &overrides,
        });
        // No fg_keydown means no focus-loss check; happy path.
        assert_eq!(d, pipeline::Decision::Proceed(InjectionStrategy::Paste));
    }

    #[test]
    fn user_override_to_keystroke_wins_over_builtin_paste() {
        let prev = fg("notepad.exe");
        let now = fg("notepad.exe");
        let mut overrides = HashMap::new();
        overrides.insert("notepad.exe".into(), InjectionStrategy::Keystroke);
        let d = pipeline::decide(&pipeline::Inputs {
            fg_keydown: Some(&prev),
            fg_keyup: &now,
            is_secure: false,
            user_overrides: &overrides,
        });
        assert_eq!(d, pipeline::Decision::Proceed(InjectionStrategy::Keystroke));
    }

    // -----------------------------------------------------------------
    // format_secs_as_iso — pure date math
    // -----------------------------------------------------------------

    #[test]
    fn iso_at_unix_epoch_is_1970_01_01() {
        assert_eq!(format_secs_as_iso(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn iso_handles_known_timestamp() {
        // 2026-05-17T00:00:00Z = 1_778_976_000 seconds since epoch.
        // Derived from 2024-01-01 (known: 1_704_067_200) + 867 days
        // (731 across 2024-2025 + 136 days Jan 1 → May 17 in 2026).
        assert_eq!(format_secs_as_iso(1_778_976_000), "2026-05-17T00:00:00Z");
    }

    #[test]
    fn iso_handles_leap_year_feb_29() {
        // 2024-02-29T00:00:00Z = 1_709_164_800 seconds since epoch.
        assert_eq!(format_secs_as_iso(1_709_164_800), "2024-02-29T00:00:00Z");
    }

    #[test]
    fn iso_handles_time_of_day() {
        // 1970-01-01T12:34:56Z = 45_296 seconds.
        assert_eq!(format_secs_as_iso(45_296), "1970-01-01T12:34:56Z");
    }
}
