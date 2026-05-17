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

use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use rusqlite::Connection;

use super::{DictationOrchestrator, OrchestratorConfig};
use crate::cleanup::{Cleaner, LlmCleaner, OllamaProvider, PassthroughCleaner};
use crate::error::{AppError, AppResult};
use crate::hotkey::driver::StateDriver;
use crate::hotkey::pause::PauseHandle;
use crate::hotkey::HotkeyEvent;
use crate::injection::strategy::InjectionStrategy;
use crate::recording_window::RecordingWindow;

#[cfg(target_os = "windows")]
use crate::hotkey::windows::WinKeyboardHook;
#[cfg(target_os = "windows")]
use crate::hotkey::HotkeyListener;
#[cfg(target_os = "windows")]
use crate::injection::secure_guard::WinSecureInputGuard;

/// Owned, cleanup-on-drop handle to the running dictation pipeline.
///
/// Drop order (matters):
///   1. Drop `hook` first — it sends `WM_QUIT` to the hotkey thread,
///      which closes the hotkey channel.
///   2. State-driver thread sees `Disconnected`, exits, closes the
///      action channel.
///   3. Dictation thread sees `Disconnected` and exits cleanly.
///   4. JoinHandles drop without panic.
pub struct DictationRuntime {
    /// Public so the tray + Tauri commands can flip the paused flag.
    /// Cloneable: produces another reference to the same atomic +
    /// channel sender.
    pub pause: PauseHandle,

    /// Public so the tray can read it for status display. Cloning
    /// shares the visibility flag.
    pub recording_window: RecordingWindow,

    /// Hook handle — Drop posts WM_QUIT to the hotkey thread.
    #[cfg(target_os = "windows")]
    _hook: WinKeyboardHook,

    /// Dictation thread join handle — held purely so the thread
    /// isn't detached. Drop happens when the runtime drops.
    #[allow(dead_code)]
    _dictation_join: Option<JoinHandle<()>>,
}

impl DictationRuntime {
    /// Construct & start the runtime.
    ///
    /// On error nothing is left running — partial init (e.g. hook
    /// installed but threads not spawned) is rolled back via Drop on
    /// the locals built so far.
    #[cfg(target_os = "windows")]
    pub fn spawn(
        db: Arc<Mutex<Connection>>,
        config: OrchestratorConfig,
        user_overrides: HashMap<String, InjectionStrategy>,
    ) -> AppResult<Self> {
        ensure_ort_dylib_set();

        // 1. Hotkey channel: hook → driver.
        let (hotkey_tx, hotkey_rx) = mpsc::channel::<HotkeyEvent>();

        // 2. Install the low-level keyboard hook.
        let mut hook = WinKeyboardHook::new()?;
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
        let dictation_join = std::thread::Builder::new()
            .name("mockingbird-dictation".into())
            .spawn(move || {
                if let Err(e) = run_dictation_thread(
                    action_rx,
                    rw_clone,
                    db,
                    config,
                    user_overrides,
                    pipeline_complete_tx,
                ) {
                    tracing::error!(error = ?e, "dictation thread bailed out");
                }
            })
            .map_err(|e| AppError::Other(format!("dictation thread spawn: {e}")))?;

        tracing::info!("dictation runtime spawned");

        Ok(Self {
            pause,
            recording_window,
            _hook: hook,
            _dictation_join: Some(dictation_join),
        })
    }

    /// Non-Windows stub — returns an error so callers fail loudly
    /// rather than silently doing nothing.
    #[cfg(not(target_os = "windows"))]
    pub fn spawn(
        _db: Arc<Mutex<Connection>>,
        _config: OrchestratorConfig,
        _user_overrides: HashMap<String, InjectionStrategy>,
    ) -> AppResult<Self> {
        Err(AppError::Other(
            "dictation runtime is Windows-only (Phase 9 platform parity)".into(),
        ))
    }
}

#[cfg(target_os = "windows")]
fn run_dictation_thread(
    actions: std::sync::mpsc::Receiver<crate::hotkey::state::StateAction>,
    recording_window: RecordingWindow,
    db: Arc<Mutex<Connection>>,
    config: OrchestratorConfig,
    user_overrides: HashMap<String, InjectionStrategy>,
    hotkey_tx: std::sync::mpsc::Sender<crate::hotkey::HotkeyEvent>,
) -> AppResult<()> {
    // Build !Send deps here on this thread. None cross thread boundaries.
    let audio = crate::audio::make_default_capture()?;
    let vad = crate::audio::vad::make_default_vad()?;
    let stt = crate::stt::make_default_stt()?;
    let cleaner = make_default_cleaner(&db, &config);
    let injector = crate::injection::make_default_injector()?;
    let window_ctx = crate::window_context::make_default_context()?;
    let secure_guard = Box::new(WinSecureInputGuard::new());

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
    );

    orchestrator.run(actions)
}

/// Ensure `ORT_DYLIB_PATH` is set so `ort` can `dlopen` the runtime.
///
/// Discovery order (mirrors `models_dir`):
///   1. Existing `ORT_DYLIB_PATH` (caller wins).
///   2. `<models_dir>\onnxruntime.dll`.
///   3. `%USERPROFILE%\mockingbird_models\onnxruntime.dll`.
///
/// On miss we don't error — VAD construction will fail later with a
/// clearer message. Setting the env early just saves a setup step.
fn ensure_ort_dylib_set() {
    if std::env::var("ORT_DYLIB_PATH").is_ok() {
        return;
    }
    if let Ok(dir) = crate::stt::models_dir() {
        let candidate = dir.join("onnxruntime.dll");
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

/// Build the cleaner the dictation thread will use.
///
/// Strategy: try to construct an [`LlmCleaner`] wired to a local
/// Ollama via the mode's configured model. If Ollama is unreachable
/// (no service running, wrong port, network blocked), log a `WARN`
/// and fall back to [`PassthroughCleaner`] — the user still gets
/// their raw transcript injected, with the cleanup phase a no-op.
///
/// Phase 7 will replace this with a settings-driven dispatcher that
/// picks Ollama vs Claude per-mode. For Phase 4 we hard-default to
/// the Normal mode's `ollama` provider — that's the PLAN §8 default.
fn make_default_cleaner(
    db: &Arc<Mutex<Connection>>,
    config: &OrchestratorConfig,
) -> Box<dyn Cleaner> {
    let lookup = {
        let conn = match db.lock() {
            Ok(g) => g,
            Err(_) => {
                tracing::warn!("cleaner: db mutex poisoned at boot; using passthrough");
                return Box::new(PassthroughCleaner::new());
            }
        };
        conn.query_row(
            "SELECT model_id, temperature, max_tokens FROM modes WHERE slug = ?1",
            [&config.mode_slug],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, f64>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            },
        )
    };
    let (model_id, temperature, max_tokens) = match lookup {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(
                error = ?e,
                mode = %config.mode_slug,
                "cleaner: mode lookup failed; using passthrough"
            );
            return Box::new(PassthroughCleaner::new());
        }
    };

    let provider = OllamaProvider::new();
    match provider.health_check() {
        Ok(_) => {
            tracing::info!(
                model = %model_id,
                temperature,
                max_tokens,
                "cleaner: Ollama reachable; using LlmCleaner"
            );
            Box::new(LlmCleaner::new(
                Box::new(provider),
                Arc::clone(db),
                model_id,
                temperature as f32,
                max_tokens as u32,
            ))
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                "cleaner: Ollama health check failed; falling back to passthrough \
                 (start Ollama + pull the model to enable LLM cleanup)"
            );
            Box::new(PassthroughCleaner::new())
        }
    }
}

/// Bootstrap default provenance rows (dictionary_snapshot + example_set)
/// if the DB doesn't have any yet.
///
/// The orchestrator's `NewSession` requires non-null FKs for both.
/// Phase 1's seed migration only populates `prompts` + `modes`; this
/// fills the gap on first launch.
///
/// Returns `(dictionary_snapshot_id, example_set_id)`.
pub fn bootstrap_provenance_rows(conn: &Connection) -> AppResult<(i64, i64)> {
    let dict_id: i64 = match conn
        .query_row(
            "SELECT id FROM dictionary_snapshots ORDER BY id ASC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .ok()
    {
        Some(id) => id,
        None => {
            conn.execute(
                "INSERT INTO dictionary_snapshots (term_ids) VALUES ('[]')",
                [],
            )?;
            conn.last_insert_rowid()
        }
    };
    let example_id: i64 = match conn
        .query_row(
            "SELECT id FROM example_sets ORDER BY id ASC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .ok()
    {
        Some(id) => id,
        None => {
            conn.execute(
                "INSERT INTO example_sets (mode_slug, example_ids) VALUES ('normal', '[]')",
                [],
            )?;
            conn.last_insert_rowid()
        }
    };
    Ok((dict_id, example_id))
}

/// Build the default Wave 4.5 [`OrchestratorConfig`] for Normal mode.
///
/// Phase 5 will swap this for settings-driven config that respects
/// the user's active mode.
pub fn default_normal_config(conn: &Connection) -> AppResult<OrchestratorConfig> {
    let (dict_id, example_id) = bootstrap_provenance_rows(conn)?;

    // Modes table: id=1 normal, id=2 verbose, id=3 fragment.
    let prompt_id: i64 = conn
        .query_row("SELECT prompt_id FROM modes WHERE slug='normal'", [], |r| {
            r.get(0)
        })
        .map_err(|e| AppError::Other(format!("lookup normal-mode prompt_id: {e}")))?;

    Ok(OrchestratorConfig {
        mode_id: 1,
        mode_slug: "normal".into(),
        prompt_id,
        dictionary_snapshot_id: dict_id,
        example_set_id: example_id,
        hotkey_label: "RightAlt".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    #[test]
    fn bootstrap_creates_rows_on_empty_db() {
        let db = Database::open_in_memory().unwrap();
        let (dict, ex) = bootstrap_provenance_rows(&db.conn).unwrap();
        assert!(dict > 0);
        assert!(ex > 0);
    }

    #[test]
    fn bootstrap_is_idempotent() {
        let db = Database::open_in_memory().unwrap();
        let (d1, e1) = bootstrap_provenance_rows(&db.conn).unwrap();
        let (d2, e2) = bootstrap_provenance_rows(&db.conn).unwrap();
        assert_eq!(d1, d2);
        assert_eq!(e1, e2);
    }

    #[test]
    fn default_normal_config_resolves_prompt_id() {
        let db = Database::open_in_memory().unwrap();
        let cfg = default_normal_config(&db.conn).unwrap();
        assert_eq!(cfg.mode_id, 1);
        assert_eq!(cfg.mode_slug, "normal");
        assert!(cfg.prompt_id > 0);
        assert_eq!(cfg.hotkey_label, "RightAlt");
    }
}
