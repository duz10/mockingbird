//! macOS window-context judge (Phase 3 dictation, Wave C).
//!
//! One probe, mirroring the in-tree judge pattern
//! ([`crate::injection::judges_macos_v1`], [`crate::secrets::judges_macos_v1`]):
//!
//! - [`frontmost_app_readable_probe`] — `mac-p3e-frontmost-app-readable`
//!   (mb-mac-v1.4.5). Asserts that `NSWorkspace.frontmostApplication`
//!   returns a **non-empty bundle identifier AND a non-empty localized
//!   app name** for whatever app is frontmost when the probe runs (the
//!   test runner's own frontmost GUI app — typically the terminal — is
//!   fine), with no panic. This is the permission-free path: it needs
//!   NO Accessibility grant. The window *title* (which would need AX) is
//!   deliberately NOT asserted — see [`super::macos`] / ADR 0060.
//!
//! macOS-only; compiles to nothing elsewhere.

#![cfg(target_os = "macos")]

use super::macos::read_frontmost_app;

/// What the frontmost-app probe observed.
#[derive(Debug, Clone)]
pub struct FrontmostProbeReport {
    /// The bundle identifier read (asserted non-empty).
    pub bundle_id: String,
    /// The localized app name read (asserted non-empty).
    pub localized_name: String,
    /// The pid of the frontmost app.
    pub pid: i32,
    /// The bundle path, if the app exposed one.
    pub bundle_path: Option<String>,
}

/// Read the frontmost app and assert a non-empty bundle id + name.
///
/// Best-effort about the *which* app — it's whatever owns the frontmost
/// window when the probe runs. The assertion is only that the
/// permission-free identity fields come back populated and nothing
/// panics.
pub fn frontmost_app_readable_probe() -> Result<FrontmostProbeReport, String> {
    let app = read_frontmost_app().map_err(|e| format!("read frontmost app: {e}"))?;

    let bundle_id = app
        .bundle_id
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "frontmost app reported an empty/None bundle identifier".to_string())?;

    let localized_name = app
        .localized_name
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "frontmost app reported an empty/None localized name".to_string())?;

    Ok(FrontmostProbeReport {
        bundle_id,
        localized_name,
        pid: app.pid,
        bundle_path: app.bundle_path,
    })
}
