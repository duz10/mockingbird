//! System tray with placeholder menu items.
//!
//! Phase 1: build the tray + menu, wire stub log lines on click. Quit
//! actually exits. Phase 5 (recording lifecycle) swaps in the real
//! handlers for Open History / Pause / Settings, and adds icon-state
//! transitions tied to recording state.

use tauri::{
    menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem},
    tray::TrayIconBuilder,
    App, AppHandle,
};

use crate::error::{AppError, AppResult};

/// Build and register the tray. Idempotent in the sense that only one
/// caller should run it during `.setup()`.
pub fn register(app: &mut App) -> AppResult<()> {
    let open_history = MenuItemBuilder::with_id("open_history", "Open History")
        .build(app)
        .map_err(map_tauri)?;
    let pause = MenuItemBuilder::with_id("pause", "Pause")
        .build(app)
        .map_err(map_tauri)?;
    let settings = MenuItemBuilder::with_id("settings", "Settings…")
        .build(app)
        .map_err(map_tauri)?;
    let separator = PredefinedMenuItem::separator(app).map_err(map_tauri)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit Mockingbird")
        .build(app)
        .map_err(map_tauri)?;

    let menu = MenuBuilder::new(app)
        .items(&[&open_history, &pause, &settings, &separator, &quit])
        .build()
        .map_err(map_tauri)?;

    let _tray = TrayIconBuilder::with_id("main-tray")
        .menu(&menu)
        .on_menu_event(|app, event| handle_menu_event(app, event.id().as_ref()))
        .build(app)
        .map_err(map_tauri)?;
    Ok(())
}

fn map_tauri(e: tauri::Error) -> AppError {
    AppError::Tauri(e)
}

/// Pure function so tests can poke at it without a real `AppHandle`.
/// Returns `true` if the id was recognized (covers "should we log a
/// warn?" branch).
pub fn handle_menu_event_pure(id: &str) -> bool {
    matches!(id, "open_history" | "pause" | "settings" | "quit")
}

fn handle_menu_event(app: &AppHandle, id: &str) {
    match id {
        "open_history" => tracing::info!("tray: open_history (stub, Phase 5)"),
        "pause" => tracing::info!("tray: pause (stub, Phase 5)"),
        "settings" => tracing::info!("tray: settings (stub, Phase 5)"),
        "quit" => {
            tracing::info!("tray: quit — exiting");
            app.exit(0);
        }
        other => tracing::warn!(id = %other, "tray: unknown menu id"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handle_menu_event_pure_recognizes_known_ids() {
        for id in ["open_history", "pause", "settings", "quit"] {
            assert!(handle_menu_event_pure(id), "should recognize {id}");
        }
    }

    #[test]
    fn handle_menu_event_pure_rejects_unknown_ids() {
        assert!(!handle_menu_event_pure("garbage"));
        assert!(!handle_menu_event_pure(""));
    }
}
