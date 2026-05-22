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
    menu::{CheckMenuItemBuilder, MenuBuilder, MenuItemBuilder, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    App, AppHandle, Manager,
};

use crate::error::{AppError, AppResult};
#[cfg(target_os = "windows")]
use crate::meetings::runtime::MeetingCaptureRuntime;

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
    // Phase 10 Wave 1A (ADR 0037): tray-entry for the Command
    // Center, so users without the chord configured (or whose chord
    // failed conflict-probe) still have a discoverable path.
    let open_command_center =
        MenuItemBuilder::with_id("open_command_center", "Open Command Center")
            .build(app)
            .map_err(map_tauri)?;
    let pause = MenuItemBuilder::with_id("pause", "Pause")
        .build(app)
        .map_err(map_tauri)?;
    // Phase MC Wave 5 — "Pause Meeting Hotkey" check item. Its
    // initial checked state mirrors `MeetingCaptureRuntime::
    // is_meeting_hotkey_paused()` (which was hydrated from settings
    // at spawn). The on-click handler toggles, and the resulting
    // re-render is handled by tracking the cached handle (we just
    // call `.set_checked()` on it inside the menu-event closure).
    //
    // On non-Windows builds the meetings runtime doesn't exist; the
    // menu item is still rendered for UI parity but is checked=false
    // and the click handler is a no-op log line.
    #[cfg(target_os = "windows")]
    let initial_meeting_paused = app
        .try_state::<MeetingCaptureRuntime>()
        .map(|s| s.is_meeting_hotkey_paused())
        .unwrap_or(false);
    #[cfg(not(target_os = "windows"))]
    let initial_meeting_paused = false;
    let pause_meeting = CheckMenuItemBuilder::with_id("pause_meeting", "Pause Meeting Hotkey")
        .checked(initial_meeting_paused)
        .build(app)
        .map_err(map_tauri)?;
    let settings = MenuItemBuilder::with_id("settings", "Settings…")
        .build(app)
        .map_err(map_tauri)?;
    let separator = PredefinedMenuItem::separator(app).map_err(map_tauri)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit Mockingbird")
        .build(app)
        .map_err(map_tauri)?;

    // Stash the pause_meeting handle in a clone-friendly Arc so the
    // on_menu_event closure can call `.set_checked(...)` after the
    // user clicks. The handle itself is `Clone` (it's a thin wrapper
    // around a Tauri-managed Resource id) so the .clone() into the
    // closure is cheap.
    let pause_meeting_for_handler = pause_meeting.clone();

    let menu = MenuBuilder::new(app)
        .items(&[
            &open_command_center,
            &open_history,
            &pause,
            &pause_meeting,
            &settings,
            &separator,
            &quit,
        ])
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
                "default window icon missing — check `bundle.icon` in tauri.conf.json".into(),
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
        .on_menu_event(move |app, event| {
            let id = event.id().as_ref();
            // "pause_meeting" is special: it needs to flip the cached
            // check-item handle's state in addition to the side
            // effects. Other ids route through the pure helper.
            if id == "pause_meeting" {
                handle_pause_meeting_click(app, &pause_meeting_for_handler);
            } else {
                handle_menu_event(app, id);
            }
        })
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
    matches!(
        id,
        "open_history" | "open_command_center" | "pause" | "pause_meeting" | "settings" | "quit"
    )
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
        "open_command_center" => {
            tracing::info!("tray: open command center");
            if let Some(cc) = app.try_state::<crate::command_center::CommandCenter>() {
                cc.open_via_tray();
            } else {
                tracing::warn!("tray: command center not registered");
            }
        }
        "pause" => tracing::info!("tray: pause (stub, Phase 5 polish)"),
        "quit" => {
            tracing::info!("tray: quit — exiting");
            app.exit(0);
        }
        other => tracing::warn!(id = %other, "tray: unknown menu id"),
    }
}

/// Toggle the "Pause Meeting Hotkey" check item. Reads the current
/// state from the meeting-capture runtime, flips it, and writes the
/// new state back (which persists to settings AND injects a
/// PauseToggle event into the activation thread). Updates the
/// menu-item's checkmark to match.
///
/// Tracing-only on non-Windows builds where the runtime is absent.
fn handle_pause_meeting_click(app: &AppHandle, item: &tauri::menu::CheckMenuItem<tauri::Wry>) {
    #[cfg(target_os = "windows")]
    {
        let Some(state) = app.try_state::<MeetingCaptureRuntime>() else {
            tracing::warn!("tray: pause_meeting clicked but no meeting runtime");
            // Best-effort: keep the checkbox visually consistent.
            let _ = item.set_checked(false);
            return;
        };
        let next = !state.is_meeting_hotkey_paused();
        if let Err(e) = state.set_meeting_hotkey_paused(next) {
            tracing::warn!(error = %e, "tray: set_meeting_hotkey_paused failed");
            // Don't update the checkbox — the runtime state is the
            // source of truth and we couldn't change it.
            return;
        }
        if let Err(e) = item.set_checked(next) {
            tracing::warn!(error = ?e, "tray: set_checked on pause_meeting failed");
        }
        tracing::info!(paused = next, "tray: pause_meeting toggled");
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (app, item);
        tracing::info!("tray: pause_meeting click ignored on non-Windows build");
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

    #[test]
    fn handle_menu_event_pure_recognizes_pause_meeting_id() {
        // Phase MC Wave 5 — the new tray check-item.
        assert!(handle_menu_event_pure("pause_meeting"));
    }

    #[test]
    fn handle_menu_event_pure_recognizes_open_command_center() {
        // Phase 10 Wave 1A — ADR 0037 tray entry.
        assert!(handle_menu_event_pure("open_command_center"));
    }
}
