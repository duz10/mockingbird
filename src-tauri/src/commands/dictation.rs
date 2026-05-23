//! Dictation IPC commands.
//!
//! Per ADR 0045 the Dictation kind now supports two start modes:
//! push-to-talk via the OS keyboard hook (UNCHANGED) and programmatic
//! start/stop via these two IPC commands. Both modes share one
//! `HotkeyStateMachine` — the synthetic events these commands inject
//! are indistinguishable from real key events from the FSM's POV.
//!
//! See `dictation::runtime::DictationRuntime::start` /
//! `DictationRuntime::stop` for the synthetic-event mechanism.

use tauri::{Runtime, State};

/// Start a dictation session programmatically.
///
/// Equivalent to pressing the configured push-to-talk key. The state
/// machine takes 80 ms to confirm the "hold" (matches PTT semantics)
/// before the orchestrator gets a `StartCapture` action — UI should
/// expect `dictation:state` to flip to `listening` shortly after this
/// IPC returns, not synchronously.
///
/// Returns `Ok(())` if the synthetic event was queued; the actual
/// recording start is observed via the `dictation:state` event stream.
#[tauri::command]
pub fn dictation_start<R: Runtime>(
    runtime: State<'_, crate::dictation::runtime::DictationRuntime>,
    _app: tauri::AppHandle<R>,
) -> Result<(), String> {
    runtime.start().map_err(|e| e.to_string())
}

/// Stop a dictation session programmatically.
///
/// Equivalent to releasing the PTT key. Idempotent: if no session is
/// active, this is a state-machine no-op (the FSM ignores `KeyUp` in
/// `Idle` / `Processing`). Safe to call defensively from a UI Stop
/// button.
///
/// Returns `Ok(())` if the synthetic event was queued; the actual
/// pipeline completion is observed via the `dictation:state` event
/// stream (the orchestrator finalizes the session asynchronously).
#[tauri::command]
pub fn dictation_stop<R: Runtime>(
    runtime: State<'_, crate::dictation::runtime::DictationRuntime>,
    _app: tauri::AppHandle<R>,
) -> Result<(), String> {
    runtime.stop().map_err(|e| e.to_string())
}
