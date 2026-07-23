//! Tauri command surface for the macOS permissions onboarding panel
//! (ADR 0061 / mb-mac-v1.4.6).
//!
//! Two commands the UI calls:
//!
//!   - [`mac_permission_statuses`] — returns all four grant states in one
//!     payload. The UI polls this on mount and on window-focus regain
//!     (so the panel updates after the user grants in System Settings and
//!     tabs back). On non-macOS it returns all `Unsupported` rather than
//!     erroring, so callers stay branch-free.
//!   - [`mac_open_settings_pane`] — deep-links System Settings straight
//!     to the pane for the named permission.
//!
//! The grant states reflect *real* system TCC state, so they're not
//! deterministically unit-testable here — the pure deep-link/mapping
//! logic in `crate::permissions` carries the unit tests, and the live
//! panel rendering + deep-link opening fold into the human e2e
//! (`mac-p3f-permissions-onboarding-renders`, a MANUAL judge).

use crate::permissions::PermissionStatuses;

/// Return the current grant state of all four macOS permissions.
///
/// macOS: real silent preflight reads. Other platforms: all
/// `Unsupported` (the UI gates the whole panel on `host_os == "macos"`,
/// so this is belt-and-suspenders).
#[tauri::command]
pub fn mac_permission_statuses() -> Result<PermissionStatuses, String> {
    #[cfg(target_os = "macos")]
    {
        Ok(crate::permissions::macos::query_all())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok(PermissionStatuses::unsupported())
    }
}

/// Open System Settings to the Privacy pane for `permission`.
///
/// `permission` is one of the UI keys (`"microphone"`,
/// `"inputMonitoring"`, `"accessibility"`, `"screenRecording"`); an
/// unknown key is a hard error so a typo surfaces instead of silently
/// opening nothing.
#[tauri::command]
pub fn mac_open_settings_pane(permission: String) -> Result<(), String> {
    let perm = crate::permissions::Permission::from_key(&permission)
        .ok_or_else(|| format!("unknown permission key: {permission:?}"))?;
    let url = perm.settings_pane_url();

    #[cfg(target_os = "macos")]
    {
        // `open <url>` hands the x-apple.systempreferences: scheme to
        // System Settings, which jumps to the pane. Fire-and-forget:
        // we don't wait for Settings to exit.
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map_err(|e| format!("failed to open System Settings ({url}): {e}"))?;
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = url;
        Err("mac_open_settings_pane is macOS-only".to_string())
    }
}
