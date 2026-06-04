//! System-level commands the UI calls — "open this folder in
//! Explorer", "tell me my data/logs/models paths".
//!
//! Paths are computed from environment variables (same source the
//! Tauri runtime uses) rather than `AppHandle::path()` so we don't
//! have to thread a runtime generic through the command signature.
//! See https://docs.rs/tauri/2/tauri/path/struct.PathResolver.html
//! for the equivalent if we ever need per-app overrides.

use std::path::PathBuf;

use crate::cleanup::OllamaProvider;
use crate::commands::types::AppPaths;

/// Open a path with the OS default handler. On Windows this lands in
/// Explorer for directories, the associated app for files. Errors if
/// the path doesn't exist — avoids the silent no-op `explorer.exe`
/// produces on bad inputs.
#[tauri::command]
pub fn open_path(path: String) -> Result<(), String> {
    let p = PathBuf::from(&path);
    if !p.exists() {
        return Err(format!("path does not exist: {path}"));
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer.exe")
            .arg(&p)
            .spawn()
            .map_err(|e| format!("explorer spawn failed: {e}"))?;
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("open_path is currently Windows-only".to_string())
    }
}

/// Report the canonical paths the user might want to inspect. Matches
/// the layout `lib.rs::run` + `logging::init` create:
///
/// - data:   `%APPDATA%/Mockingbird`
/// - logs:   `%APPDATA%/Mockingbird/logs`
/// - models: `%USERPROFILE%/mockingbird_models`  (override via env)
#[tauri::command]
pub fn app_paths() -> Result<AppPaths, String> {
    let appdata = std::env::var("APPDATA").map_err(|_| "APPDATA env var not set".to_string())?;
    let data_dir = PathBuf::from(&appdata).join("Mockingbird");
    let logs_dir = data_dir.join("logs");

    let models_dir = std::env::var("MOCKINGBIRD_MODELS_DIR")
        .ok()
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var("USERPROFILE")
                .ok()
                .map(|home| PathBuf::from(home).join("mockingbird_models"))
        })
        .unwrap_or_else(|| PathBuf::from("(unset)"));

    Ok(AppPaths {
        data_dir: data_dir.to_string_lossy().into_owned(),
        logs_dir: logs_dir.to_string_lossy().into_owned(),
        models_dir: models_dir.to_string_lossy().into_owned(),
    })
}

/// mb-1z0m (Round 3) — fire-and-forget JS→Rust mirror so IPC outcomes
/// (success AND failure) land in `mockingbird.log` instead of only in
/// DevTools' `console.warn`. Deliberately takes NO `State<>` parameter
/// so it dispatches even when the AppState-managed-state slot is the
/// broken thing we're trying to diagnose.
///
/// Used by:
///   * `ui/src/lib/bootApp.ts` — mirrors each of the 5 boot IPC outcomes.
///   * `ui/src/pages/Insights.tsx` — mirrors the `insights_snapshot` outcome.
///
/// Labels are free-form short ASCII strings (IPC name); reasons are the
/// stringified rejection (or `None` on success). `tracing::info!` on
/// success keeps the boot trail readable; `tracing::warn!` on failure
/// so the line is greppable as a problem.
///
/// Once Round 3's diagnosis lands and the bug is fixed, this command
/// stays (it's load-bearing observability per the recent-context P3
/// papercut: IPC failures should not be DevTools-only).
#[tauri::command]
pub fn report_ipc_status(label: String, ok: bool, reason: Option<String>) {
    if ok {
        tracing::info!(target: "ipc::report", %label, "boot/IPC fulfilled");
    } else {
        tracing::warn!(
            target: "ipc::report",
            %label,
            reason = reason.as_deref().unwrap_or("<no reason>"),
            "boot/IPC rejected"
        );
    }
}

/// List the local Ollama tags via `GET /api/tags`. Used by the Modes
/// editor's per-mode model dropdown — UI populates the `<select>`
/// options with whatever the user actually has installed, so we
/// can't recommend a default the box can't run.
///
/// Returns the empty Vec when Ollama isn't reachable so the UI can
/// fall back to a free-text input instead of blocking the editor.
/// (The cleaner already handles missing-model gracefully at
/// dictation time.)
#[tauri::command]
pub fn list_installed_models() -> Result<Vec<String>, String> {
    let provider = OllamaProvider::new();
    match provider.list_models() {
        Ok(models) => Ok(models),
        Err(e) => {
            // Log but don't propagate — the UI falls back to text input.
            tracing::warn!(error = %e, "list_installed_models: ollama unreachable");
            Ok(Vec::new())
        }
    }
}
