//! System tray with placeholder menu items.
//!
//! Phase 1: build the tray + menu, wire stub log lines on click. Quit
//! actually exits. Phase 5 (recording lifecycle) swaps in the real
//! handlers for Open History / Pause / Settings, and adds icon-state
//! transitions tied to recording state.
//!
//! IMPORTANT: the tray is owned entirely here — do NOT also declare
//! a `trayIcon` block in `tauri.conf.json`. Doing so spawns a second,
//! handler-less tray icon with the same id (Windows then renders
//! TWO tray entries: one from config with the icon but no clicks,
//! one from Rust with handlers but no icon). The icon for THIS tray
//! is pulled from `app.default_window_icon()`, which Tauri loads from
//! the `bundle.icon` array in tauri.conf.json — so the icon source
//! of truth stays one file (the .ico/.png bundle), but only one
//! tray gets created.

use tauri::{
    menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    App, AppHandle, Manager,
};

use crate::error::{AppError, AppResult};

/// Tooltip text shown when hovering the tray icon. Matches productName
/// in tauri.conf.json on purpose — if the user has 30 tray icons,
/// "Mockingbird" is what tells them which one is us.
const TRAY_TOOLTIP: &str = "Mockingbird";

/// Label of the main window declared in `tauri.conf.json`.
/// Kept as a const so the tray wiring stays in one file.
const MAIN_WINDOW_LABEL: &str = "main";

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

    // Pull the icon from the same bundle Tauri loads for the window
    // (`bundle.icon` in tauri.conf.json — currently the MockingbirdMark
    // mark, see ADR 0023 + assets/icons/mockingbird.svg). Single source
    // of truth for both window + tray icons.
    let icon = app
        .default_window_icon()
        .ok_or_else(|| {
            AppError::Other(
                "default window icon missing — check `bundle.icon` in tauri.conf.json"
                    .into(),
            )
        })?
        .clone();

    let _tray = TrayIconBuilder::with_id("main-tray")
        .icon(icon)
        .tooltip(TRAY_TOOLTIP)
        // Left-click should toggle the window, not pop the menu —
        // matches the menuOnLeftClick=false convention from the old
        // config block and the Windows tray UX users expect.
        .show_menu_on_left_click(false)
        .menu(&menu)
        .on_menu_event(|app, event| handle_menu_event(app, event.id().as_ref()))
        .on_tray_icon_event(|tray, event| {
            // Left-click on the tray icon toggles the main window. We
            // explicitly handle Up (not Down) because Down fires while
            // the user is still pressing — visually jarring on Windows.
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_main_window(tray.app_handle());
            }
        })
        .build(app)
        .map_err(map_tauri)?;
    Ok(())
}

/// Show + focus the main window, creating-the-illusion-of-launch even
/// though the process has been running in the background since boot.
/// Logs + swallows Tauri errors — UI glitches must never panic the app.
fn show_main_window(app: &AppHandle) {
    let Some(w) = app.get_webview_window(MAIN_WINDOW_LABEL) else {
        tracing::warn!(label = MAIN_WINDOW_LABEL, "main window not found");
        return;
    };
    if let Err(e) = w.show() {
        tracing::warn!(error = ?e, "failed to show main window");
    }
    if let Err(e) = w.unminimize() {
        tracing::debug!(error = ?e, "failed to unminimize main window (ok if not minimized)");
    }
    if let Err(e) = w.set_focus() {
        tracing::warn!(error = ?e, "failed to focus main window");
    }
}

/// Show-or-hide toggle bound to tray left-click. Hides only when the
/// window is both visible AND focused — otherwise a click during
/// focus-elsewhere does the natural thing (bring it forward).
fn toggle_main_window(app: &AppHandle) {
    let Some(w) = app.get_webview_window(MAIN_WINDOW_LABEL) else {
        tracing::warn!(label = MAIN_WINDOW_LABEL, "main window not found");
        return;
    };
    let visible = w.is_visible().unwrap_or(false);
    let focused = w.is_focused().unwrap_or(false);
    if visible && focused {
        if let Err(e) = w.hide() {
            tracing::warn!(error = ?e, "failed to hide main window");
        }
    } else {
        show_main_window(app);
    }
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
        // All three navigation items just show the main window. The
        // hash-router target lives in TS — emitting an `app:navigate`
        // event would let us deep-link to History / Settings, but
        // until that lands the user lands on Insights and clicks
        // through. Cheap, predictable, no surprises.
        "open_history" | "settings" => {
            tracing::info!(id = id, "tray: opening main window");
            show_main_window(app);
        }
        "pause" => tracing::info!("tray: pause (stub, Phase 5 polish)"),
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
