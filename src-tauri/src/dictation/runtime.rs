//! Runtime wiring — installs the WH_KEYBOARD_LL hook + spawns the
//! state-driver thread + spawns the dictation orchestrator thread,
//! all wired with the platform-default deps.
//!
//! Wave 4.5 glue: turns Wave 4's building blocks into a running
//! pipeline. `lib.rs::run()` calls `DictationRuntime::spawn` from
//! its Tauri setup callback.
//!
//! ## Threading model
//!
//! ```text
//!   Main (Tauri) thread
//!     owns DictationRuntime (Drop tears down)
//!     │
//!     ├── mockingbird-hotkey thread  (WH_KEYBOARD_LL message pump)
//!     │       │
//!     │       ▼  HotkeyEvent
//!     ├── mockingbird-state thread   (20 ms tick + state machine)
//!     │       │
//!     │       ▼  StateAction
//!     └── mockingbird-dictation thread
//!             (CpalCapture, SileroVad, WhisperStt, ...
//!              all !Send deps built INSIDE this thread)
//! ```
//!
//! ## Why deps are built INSIDE the dictation thread
//!
//! `CpalCapture` is `!Send` on Windows (WASAPI handles are thread-
//! bound). Constructing it in main + moving the `Box<dyn AudioCapture>`
//! across a `thread::spawn` boundary fails the `Send` check. Building
//! it INSIDE the spawned thread sidesteps that — !Send things only
//! need to live on one thread.

// macOS port: the dictation runtime wiring is `#[cfg(target_os = "windows")]`;
// these imports + helpers (ensure_ort_dylib_set / make_default_cleaner /
// spawn_ollama_warmup) are orphaned on non-Windows until the cross-platform
// runtime lands (Phase 3/4). `needless_return` only fires on non-Windows because
// the trailing USERPROFILE fallback in ensure_ort_dylib_set is Windows-gated.
#![cfg_attr(
    not(any(target_os = "windows", target_os = "macos")),
    allow(unused_imports, dead_code)
)]
// `ensure_ort_dylib_set`'s mid-function `return` is in tail position on
// every non-Windows target (the trailing `%USERPROFILE%` fallback is
// Windows-gated), so clippy flags it as needless there.
#![cfg_attr(not(target_os = "windows"), allow(clippy::needless_return))]

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use rusqlite::Connection;

use super::ingest_channel::{self, HeadlessIngestRequest, HeadlessIngestSender};
use super::runtime_cleaner::make_default_cleaner;
use super::{DictationOrchestrator, OrchestratorConfig};
use crate::audio::vad::VoiceActivityDetector;
use crate::audio::AudioCapture;
use crate::error::{AppError, AppResult};
use crate::hotkey::driver::StateDriver;
use crate::hotkey::pause::PauseHandle;
use crate::hotkey::state::StateAction;
use crate::hotkey::HotkeyEvent;
use crate::injection::secure_guard::SecureInputGuard;
use crate::injection::strategy::InjectionStrategy;
use crate::injection::Injector;
use crate::recording_window::RecordingWindow;
use crate::stt::SpeechToText;
use crate::window_context::WindowContext;

// ADR 0063 — preserve the `dictation::runtime::*` public paths for the
// helpers relocated to `runtime_provenance` (learning + KG ingest +
// lib.rs depend on them).
pub use super::runtime_provenance::{bootstrap_provenance_rows, default_normal_config};

#[cfg(any(target_os = "windows", target_os = "macos"))]
use crate::hotkey::HotkeyListener;

/// Owned, cleanup-on-drop handle to the running dictation pipeline.
///
/// Drop order (matters; ADR 0063):
///   1. The `_hook` field drops (declaration order), invoking the
///      platform listener's teardown — `WM_QUIT` post on Windows,
///      `CFRunLoopStop` + tap-thread join on macOS — which drops the
///      listener's `Sender<HotkeyEvent>` clone.
///   2. State-driver thread sees `Disconnected`, exits, closes the
///      action channel.
///   3. The dictation thread is detached (its `JoinHandle` is dropped,
///      not joined): the orchestrator holds a `hotkey_tx` clone for
///      `PipelineComplete`, so the full channel cascade to it completes
///      at process exit. The runtime's Drop returns promptly either way.
///   4. JoinHandles drop without panic.
pub struct DictationRuntime {
    /// Public so the tray + Tauri commands can flip the paused flag.
    /// Cloneable: produces another reference to the same atomic +
    /// channel sender.
    pub pause: PauseHandle,

    /// Public so the tray can read it for status display. Cloning
    /// shares the visibility flag.
    pub recording_window: RecordingWindow,

    /// ADR 0045 + mb-tfyp — shared with the orchestrator on the
    /// dictation thread. `start()` flips it to `true` immediately
    /// before injecting the synthetic `KeyDown`; the orchestrator
    /// reads + clears it at `start_capture` time. Stays `false`
    /// for every PTT session (the real OS hook never touches it).
    next_start_is_programmatic: Arc<AtomicBool>,

    /// ADR 0052 + mb-0gt6 (KG Phase 1D Wave 1D.3) — sibling of
    /// `next_start_is_programmatic`. Flipped to `true` by
    /// [`Self::start_kg_note`] right before the synthetic
    /// `KeyDown`; the orchestrator swaps + clears it inside
    /// `start_capture` to pin the session's `CaptureKind` to
    /// `KgNote` for the source-gated KG enqueue (ADR 0052 §D1).
    /// Independent axis from the programmatic flag: a KG audio
    /// note IS programmatic AND IS a KG note (both set), but the
    /// plain in-app Start button is programmatic-only.
    next_start_is_kg_note: Arc<AtomicBool>,

    /// ADR 0046 §3.2 / mb-7vyz — producer half of the sibling
    /// `crossbeam-channel` carrying [`HeadlessIngestRequest`]s to
    /// the orchestrator. Cloneable — each IPC handler / future
    /// inbox-watcher loop grabs its own clone via Tauri managed
    /// state (`lib.rs` publishes `HeadlessIngestSender` as its own
    /// managed-state entry by cloning this field at boot).
    headless_ingest_tx: HeadlessIngestSender,

    /// Platform hotkey listener handle. Dropping it tears down the OS
    /// hook/tap via the listener's own `Drop` (`WM_QUIT` on Windows,
    /// `CFRunLoopStop` + join on macOS — ADR 0063), so the runtime needs
    /// no cfg-branched teardown of its own. Field order matters — see
    /// the struct doc.
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    _hook: Box<dyn HotkeyListener>,

    /// Dictation thread join handle — held purely so the thread
    /// isn't detached. Drop happens when the runtime drops.
    #[allow(dead_code)]
    _dictation_join: Option<JoinHandle<()>>,
}

/// Backend dependencies the dictation orchestrator consumes, built
/// INSIDE the dictation thread (several are `!Send` — `CpalCapture`'s
/// cpal `Stream` is thread-bound, so it must never cross a thread
/// boundary). Produced by an [`OrchestratorDepsFn`] (ADR 0063).
///
/// The `cleaner` is intentionally NOT here — it is built from `&db` /
/// `&config` by [`make_default_cleaner`] (cheap + passthrough-capable
/// when Ollama is absent), so the spawn/teardown judge uses the real
/// one rather than doubling it.
pub struct OrchestratorDeps {
    /// 16 kHz mono mic capture (`CpalCapture` in production).
    pub audio: Box<dyn AudioCapture>,
    /// Voice-activity detector (`SileroVad` in production).
    pub vad: Box<dyn VoiceActivityDetector>,
    /// Speech-to-text engine (`WhisperStt` in production).
    pub stt: Box<dyn SpeechToText>,
    /// Text injector (`SendInputInjector` / `MacInjector`).
    pub injector: Box<dyn Injector>,
    /// Foreground-window reader (`make_default_context`).
    pub window_ctx: Box<dyn WindowContext>,
    /// Secure-input guard (`make_default_guard`).
    pub secure_guard: Box<dyn SecureInputGuard>,
}

/// Builds [`OrchestratorDeps`] on the dictation thread. `FnOnce +
/// Send` (the closure is moved into the thread); its RETURN value need
/// not be `Send` because it is produced and consumed entirely on that
/// one thread — which is exactly why the `!Send` `CpalCapture` is OK.
pub type OrchestratorDepsFn = Box<dyn FnOnce() -> AppResult<OrchestratorDeps> + Send>;

/// The production dep builder — every backend via its `make_default_*`
/// factory. Passed by [`DictationRuntime::spawn`]; the judge passes a
/// doubling builder to [`DictationRuntime::spawn_with_deps`] instead.
#[cfg(any(target_os = "windows", target_os = "macos"))]
fn default_orchestrator_deps() -> AppResult<OrchestratorDeps> {
    Ok(OrchestratorDeps {
        audio: crate::audio::make_default_capture()?,
        vad: crate::audio::vad::make_default_vad()?,
        stt: crate::stt::make_default_stt()?,
        injector: crate::injection::make_default_injector()?,
        window_ctx: crate::window_context::make_default_context()?,
        secure_guard: crate::injection::secure_guard::make_default_guard(),
    })
}

impl DictationRuntime {
    /// Construct & start the runtime with the production backends.
    ///
    /// On error nothing is left running — partial init (e.g. hook
    /// installed but threads not spawned) is rolled back via Drop on
    /// the locals built so far.
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    pub fn spawn(
        db: Arc<Mutex<Connection>>,
        config: OrchestratorConfig,
        user_overrides: HashMap<String, InjectionStrategy>,
        vault: Arc<crate::vault::export_job::VaultRuntime>,
    ) -> AppResult<Self> {
        Self::spawn_with_deps(
            db,
            config,
            user_overrides,
            vault,
            Box::new(default_orchestrator_deps),
        )
    }

    /// Construct & start the runtime with a custom backend-dep builder
    /// (ADR 0063). The public [`Self::spawn`] passes
    /// [`default_orchestrator_deps`]; the macOS spawn/teardown judge
    /// injects DOUBLED device backends (audio/vad/stt) while keeping the
    /// REAL listener + thread wiring + Drop teardown under test.
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    pub fn spawn_with_deps(
        db: Arc<Mutex<Connection>>,
        config: OrchestratorConfig,
        user_overrides: HashMap<String, InjectionStrategy>,
        vault: Arc<crate::vault::export_job::VaultRuntime>,
        deps_fn: OrchestratorDepsFn,
    ) -> AppResult<Self> {
        ensure_ort_dylib_set();

        // 1. Hotkey channel: hook → driver.
        let (hotkey_tx, hotkey_rx) = mpsc::channel::<HotkeyEvent>();

        // 2. Build + install the platform hotkey listener via the trait
        //    seam (ADR 0063): Windows `WinKeyboardHook` (WM_QUIT
        //    teardown) or macOS `MacKeyboardHook` (CGEventTap on a
        //    dedicated CFRunLoop thread; CFRunLoopStop teardown). Both
        //    encapsulate their own Drop, so the runtime stays
        //    platform-agnostic.
        let mut hook = crate::hotkey::make_default_listener()?;
        hook.install(hotkey_tx.clone())?;

        // 3. Pause handle reuses the hotkey channel: tray's
        //    `set_paused(true)` injects a `PauseToggle` event so the
        //    state machine sees the change in-stream.
        let pause = PauseHandle::new(hotkey_tx);

        // 4. State driver: hotkey events + 20 ms ticks → state actions.
        //    The handle is intentionally leaked: dropping it would
        //    join the thread immediately, but we want it to live
        //    until `hotkey_rx` closes (which happens when `hook`
        //    drops, in the right cascade).
        let (action_rx, driver_handle) = StateDriver::default().start(hotkey_rx);
        std::mem::forget(driver_handle);

        // 5. Dictation thread. !Send deps live inside.
        //
        // The dictation thread also needs a `Sender<HotkeyEvent>` so
        // the orchestrator can emit `PipelineComplete` back to the
        // driver after each session — without it, the state machine
        // sticks in `Processing` after one hold (§6.1 ignores
        // KeyDown there). Clones the same Sender the hook uses.
        let recording_window = RecordingWindow::new();
        let rw_clone = recording_window.clone();
        let pipeline_complete_tx = pause.sender_clone();
        // mb-tfyp: programmatic-start flag. Cloned once into the
        // orchestrator on the dictation thread; the runtime keeps
        // its own clone so `start()` can flip it.
        let next_start_is_programmatic = Arc::new(AtomicBool::new(false));
        let programmatic_clone = next_start_is_programmatic.clone();
        // mb-0gt6 / Wave 1D.3: sibling KG-note flag. Same lifetime
        // + clone pattern the programmatic flag uses—the
        // orchestrator gets one Arc; the runtime keeps another so
        // `start_kg_note()` can flip it before injecting the
        // synthetic KeyDown.
        let next_start_is_kg_note = Arc::new(AtomicBool::new(false));
        let kg_note_clone = next_start_is_kg_note.clone();
        // ADR 0046 §3.2: build the sibling crossbeam channel for
        // headless ingest. The runtime holds the sender so lib.rs
        // can clone it into Tauri managed state at boot; the
        // receiver is moved into the dictation thread alongside the
        // existing action stream.
        let (headless_ingest_tx, headless_ingest_rx) = ingest_channel::channel();
        let dictation_join = std::thread::Builder::new()
            .name("mockingbird-dictation".into())
            .spawn(move || {
                if let Err(e) = run_dictation_thread(
                    action_rx,
                    headless_ingest_rx,
                    rw_clone,
                    db,
                    config,
                    user_overrides,
                    pipeline_complete_tx,
                    programmatic_clone,
                    kg_note_clone,
                    vault,
                    deps_fn,
                ) {
                    tracing::error!(error = ?e, "dictation thread bailed out");
                }
            })
            .map_err(|e| AppError::Other(format!("dictation thread spawn: {e}")))?;

        tracing::info!("dictation runtime spawned");

        Ok(Self {
            pause,
            recording_window,
            next_start_is_programmatic,
            next_start_is_kg_note,
            headless_ingest_tx,
            _hook: hook,
            _dictation_join: Some(dictation_join),
        })
    }

    /// Unsupported-platform stub — returns an error so callers fail
    /// loudly rather than silently doing nothing. (Windows + macOS now
    /// share the real spawn above; this is the Phase 9 Linux gap.)
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    pub fn spawn(
        _db: Arc<Mutex<Connection>>,
        _config: OrchestratorConfig,
        _user_overrides: HashMap<String, InjectionStrategy>,
        _vault: Arc<crate::vault::export_job::VaultRuntime>,
    ) -> AppResult<Self> {
        Err(AppError::Other(
            "dictation runtime not implemented for this platform (Phase 9 Linux)".into(),
        ))
    }

    /// Clone the headless-ingest sender so lib.rs can publish it as
    /// its own Tauri managed-state entry. Cheap (`Arc` clone under
    /// the hood) — call once at boot.
    pub fn headless_ingest_sender(&self) -> HeadlessIngestSender {
        self.headless_ingest_tx.clone()
    }

    /// Programmatic start (ADR 0045 mode (b)).
    ///
    /// Injects a synthetic [`HotkeyEvent::KeyDown`] on the same
    /// channel the OS hook feeds the state-driver. The FSM cannot
    /// distinguish synthetic from real key events: it transitions
    /// `Idle → PendingHold` and (after the 80 ms hold threshold)
    /// promotes to `Recording`, emitting `StartCapture` to the
    /// orchestrator. From there the path is identical to PTT.
    ///
    /// **VK choice.** Uses a sentinel VK (`0x07`, reserved by
    /// Microsoft and never emitted by the LL hook for real input)
    /// so a stray Right Alt release while a programmatic session
    /// is active can't accidentally stop it via the `vk == held_vk`
    /// guard in `HotkeyState::Recording`. Pair-matching only — the
    /// state machine doesn't validate VK against any allow-list.
    ///
    /// Idempotent at the FSM level: a second `start()` while a
    /// session is in flight lands a `KeyDown` in `Recording` /
    /// `Processing`, both of which §6.1 ignores — so the call is a
    /// no-op, not a corruption.
    pub fn start(&self) -> AppResult<()> {
        // mb-tfyp: tell the orchestrator this session is
        // programmatic BEFORE the synthetic KeyDown lands on the
        // hotkey channel. Otherwise there's a tiny race where the
        // FSM could process KeyDown + emit StartCapture + the
        // orchestrator's `start_capture` could read the flag —
        // all before we'd flipped it. The other direction is
        // safe: setting the flag, then NOT sending KeyDown (because
        // the channel closed), leaves a stale `true` that the next
        // programmatic call would consume — harmless because the
        // flag is gated on the synthetic KeyDown actually producing
        // a `StartCapture`, which it won't if the channel is dead.
        // Worst case: the very next REAL PTT hold gets tagged as
        // in-app. That requires the hotkey channel to be both
        // closed AND magically reopen — i.e. the runtime is
        // being torn down, so the misattribution is moot.
        self.next_start_is_programmatic
            .store(true, std::sync::atomic::Ordering::SeqCst);
        self.pause
            .sender_clone()
            .send(HotkeyEvent::KeyDown {
                vk: PROGRAMMATIC_VK,
                at: std::time::Instant::now(),
            })
            .map_err(|e| {
                AppError::Hotkey(format!(
                    "DictationRuntime::start: hotkey channel closed: {e}"
                ))
            })
    }

    /// Programmatic start for a KG-screen audio note (ADR 0052 §D1,
    /// mb-0gt6 / Wave 1D.3).
    ///
    /// Identical to [`Self::start`] EXCEPT it also flips the
    /// `next_start_is_kg_note` flag so the orchestrator's
    /// `start_capture` pins the session's `CaptureKind` to `KgNote`
    /// for the dictation-tail source-gate. The session still goes
    /// through the full mic / VAD / STT / cleanup / persist pipeline
    /// like any other in-app live-mic session — the only material
    /// difference is the capture_kind, which (a) records intent in
    /// the DB, (b) opts the row into KG filing, and (c) lets the UI
    /// distinguish KG-originated rows in future surfaces.
    ///
    /// Both flags are set BEFORE the synthetic KeyDown for the same
    /// race-avoidance rationale documented on [`Self::start`].
    pub fn start_kg_note(&self) -> AppResult<()> {
        self.next_start_is_programmatic
            .store(true, std::sync::atomic::Ordering::SeqCst);
        self.next_start_is_kg_note
            .store(true, std::sync::atomic::Ordering::SeqCst);
        self.pause
            .sender_clone()
            .send(HotkeyEvent::KeyDown {
                vk: PROGRAMMATIC_VK,
                at: std::time::Instant::now(),
            })
            .map_err(|e| {
                AppError::Hotkey(format!(
                    "DictationRuntime::start_kg_note: hotkey channel closed: {e}"
                ))
            })
    }

    /// Programmatic stop (ADR 0045 mode (b)).
    ///
    /// Injects a synthetic [`HotkeyEvent::KeyUp`] on the hotkey
    /// channel. If a programmatic session is live (FSM in
    /// `Recording`), this transitions to `Processing` and emits
    /// `StopCapture` exactly like releasing the PTT key.
    ///
    /// Idempotent against an idle FSM: `KeyUp` in `Idle` /
    /// `PendingHold` (with non-matching VK) / `Processing` are all
    /// state-machine no-ops per §6.1. The most common cause of a
    /// no-op call is the UI clicking Stop after the recording was
    /// already finalized via Esc-cancel.
    pub fn stop(&self) -> AppResult<()> {
        self.pause
            .sender_clone()
            .send(HotkeyEvent::KeyUp {
                vk: PROGRAMMATIC_VK,
                at: std::time::Instant::now(),
            })
            .map_err(|e| {
                AppError::Hotkey(format!(
                    "DictationRuntime::stop: hotkey channel closed: {e}"
                ))
            })
    }
}

/// Sentinel VK used for programmatic dictation start/stop. Picked
/// from the Microsoft "reserved" range (`0x07`) so it can never
/// collide with a real PTT key the user might configure. See
/// `DictationRuntime::start` for the full rationale.
const PROGRAMMATIC_VK: u32 = 0x07;

#[cfg(any(target_os = "windows", target_os = "macos"))]
#[allow(clippy::too_many_arguments)]
fn run_dictation_thread(
    actions: std::sync::mpsc::Receiver<StateAction>,
    headless_rx: crossbeam_channel::Receiver<HeadlessIngestRequest>,
    recording_window: RecordingWindow,
    db: Arc<Mutex<Connection>>,
    config: OrchestratorConfig,
    user_overrides: HashMap<String, InjectionStrategy>,
    hotkey_tx: std::sync::mpsc::Sender<crate::hotkey::HotkeyEvent>,
    next_start_is_programmatic: Arc<AtomicBool>,
    next_start_is_kg_note: Arc<AtomicBool>,
    vault: Arc<crate::vault::export_job::VaultRuntime>,
    deps_fn: OrchestratorDepsFn,
) -> AppResult<()> {
    // Build the backend deps here on this thread (several are !Send;
    // none cross thread boundaries — ADR 0063). The default builder
    // uses the `make_default_*` factories; the judge injects doubles.
    let OrchestratorDeps {
        audio,
        vad,
        stt,
        injector,
        window_ctx,
        secure_guard,
    } = deps_fn()?;
    // The cleaner is built from db/config (passthrough when Ollama is
    // absent), not part of the injected dep bundle.
    let cleaner = make_default_cleaner(&db, &config);

    let orchestrator = DictationOrchestrator::new(
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
    )
    // mb-58i — production cleaner only: enable the lazy Ollama self-heal
    // at dictation boundaries (the injected-double path in tests never
    // opts in).
    .with_cleaner_self_heal();

    // ADR 0046 §3.2 — bridge the std::sync::mpsc StateAction stream
    // into a crossbeam channel so the orchestrator's `run` loop can
    // `select!` it alongside `headless_rx`. The upstream
    // `StateDriver::start` and `HotkeyStateMachine` are untouched
    // (their channel type is fixed by `hotkey/driver.rs`, which is
    // out of boundary per ADR 0046 §3); this bridge is a pure
    // type-adapter that lives entirely inside the dictation runtime.
    //
    // Lifecycle: when `actions` closes (state-driver thread exits),
    // the recv loop falls through, `actions_tx_cb` drops, and the
    // orchestrator's recv arm closes on the next tick. Symmetric
    // shutdown — no extra signal needed.
    let (actions_tx_cb, actions_rx_cb) = crossbeam_channel::unbounded::<StateAction>();
    let bridge = std::thread::Builder::new()
        .name("mockingbird-dictation-bridge".into())
        .spawn(move || {
            for action in actions.iter() {
                if actions_tx_cb.send(action).is_err() {
                    // Orchestrator went away. Drain + exit so the
                    // upstream channel can shut down cleanly.
                    tracing::info!(
                        "dictation bridge: orchestrator dropped action receiver; exiting"
                    );
                    break;
                }
            }
            tracing::info!("dictation bridge: upstream StateAction channel closed");
        })
        .map_err(|e| AppError::Other(format!("dictation bridge spawn: {e}")))?;

    let result = orchestrator.run(actions_rx_cb, headless_rx);

    // Join the bridge so the thread name doesn't leak as zombie.
    // It exits as soon as the upstream `actions` channel closes
    // (driver thread gone) OR we drop `actions_rx_cb`, whichever
    // comes first. Best-effort: a panic in the bridge is a non-fatal
    // shutdown anomaly.
    if let Err(e) = bridge.join() {
        tracing::warn!(?e, "dictation bridge thread panicked during shutdown");
    }
    result
}

/// Ensure `ORT_DYLIB_PATH` is set so `ort` can `dlopen` the runtime.
///
/// Discovery order (mirrors `models_dir`):
///   1. Existing `ORT_DYLIB_PATH` (caller wins — on macOS this is what
///      `scripts/dev/cargo-mac.sh` exports in dev).
///   2. `<models_dir>/<platform dylib>` — `onnxruntime.dll` on Windows,
///      `libonnxruntime.dylib` on macOS, `libonnxruntime.so` elsewhere.
///      `models_dir` resolves `MODEL_PATH` first (also set by
///      cargo-mac.sh), so the dev `models/` dir is found here too.
///   3. `%USERPROFILE%\mockingbird_models\onnxruntime.dll` (Windows dev).
///
/// macOS note: for the packaged `.app`, discovery from the bundle
/// Resources dir is a later item (ADR 0062) — the dev `ORT_DYLIB_PATH`
/// env + the repo `models/` dir cover .4.7a.
///
/// On miss we don't error — VAD construction will fail later with a
/// clearer message. Setting the env early just saves a setup step.
fn ensure_ort_dylib_set() {
    if std::env::var("ORT_DYLIB_PATH").is_ok() {
        return;
    }

    // Platform filename for the onnxruntime shared library.
    #[cfg(target_os = "windows")]
    const ORT_DYLIB_NAME: &str = "onnxruntime.dll";
    #[cfg(target_os = "macos")]
    const ORT_DYLIB_NAME: &str = "libonnxruntime.dylib";
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    const ORT_DYLIB_NAME: &str = "libonnxruntime.so";

    if let Ok(dir) = crate::stt::models_dir() {
        let candidate = dir.join(ORT_DYLIB_NAME);
        if candidate.is_file() {
            tracing::info!(path = %candidate.display(), "setting ORT_DYLIB_PATH");
            // SAFETY: process-startup env mutation; no other thread
            // is yet reading ORT_DYLIB_PATH.
            std::env::set_var("ORT_DYLIB_PATH", candidate);
            return;
        }
    }
    #[cfg(target_os = "windows")]
    if let Ok(profile) = std::env::var("USERPROFILE") {
        let candidate = std::path::PathBuf::from(profile)
            .join("mockingbird_models")
            .join("onnxruntime.dll");
        if candidate.is_file() {
            tracing::info!(path = %candidate.display(), "setting ORT_DYLIB_PATH");
            std::env::set_var("ORT_DYLIB_PATH", candidate);
        }
    }
}
