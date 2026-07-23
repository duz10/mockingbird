//! macOS implementation of [`super::WindowContext`].
//!
//! ## API surface used
//!
//! The macOS foreground read is the **cheap, permission-free** path
//! (ADR 0060 / mb-mac-v1.4.5): it identifies the *app* that owns the
//! frontmost window via `NSWorkspace` + `NSRunningApplication`, none of
//! which needs an Accessibility (AX) grant.
//!
//! - `NSWorkspace.sharedWorkspace.frontmostApplication` →
//!   `NSRunningApplication?` — the app currently owning the menu bar /
//!   frontmost window. `None` only in rare transient states (login
//!   window, fast user-switch), mirroring the Windows
//!   "no foreground window" case.
//! - `NSRunningApplication.bundleIdentifier` → `"com.google.Chrome"` —
//!   the stable app identity. This is the macOS analogue of the Windows
//!   exe basename (`chrome.exe`) and is what a future per-app
//!   injection-strategy table (ADR 0016) keys off.
//! - `NSRunningApplication.localizedName` → `"Google Chrome"` — the
//!   human-readable app name (provenance / display).
//! - `NSRunningApplication.processIdentifier` → `pid_t` (i32).
//! - `NSRunningApplication.bundleURL.path` → `"/Applications/Google
//!   Chrome.app"` — the macOS analogue of the Windows full exe path.
//!
//! ## Mapping onto [`ForegroundWindow`] (documented deviation, ADR 0060)
//!
//! The shared struct was modelled on the Win32 read; macOS has no
//! per-process `HWND` and the *window title* needs AX. We map:
//!
//! | field          | macOS value                                     |
//! |----------------|-------------------------------------------------|
//! | `hwnd`         | `pid` (closest stable per-app handle macOS has) |
//! | `title`        | `""` — real window title needs AX; degrades     |
//! | `process_name` | bundle identifier (fallback: localized name)    |
//! | `exe_path`     | bundle path (`…/Foo.app`)                        |
//!
//! ### Why `title` is empty without Accessibility
//!
//! Reading the focused window's title bar requires the Accessibility
//! API (`AXUIElementCopyAttributeValue(kAXTitleAttribute)`), which is
//! TCC-gated. Per the Phase 3 plan this is a *minimal frontmost read*,
//! so we deliberately return an empty title rather than forcing an AX
//! grant for basic app-context tagging. The orchestrator's focus-loss
//! check (ADR 0016) still works: it compares the whole snapshot, and a
//! change of frontmost *app* (pid + bundle id) is the signal that
//! matters for dictation injection. Surfacing the real window title is
//! a future AX-gated leaf.

#![cfg(target_os = "macos")]

use objc2_app_kit::NSWorkspace;

use super::{ForegroundWindow, WindowContext};
use crate::error::{AppError, AppResult};

/// macOS foreground-app provider (`NSWorkspace`-based).
#[derive(Default)]
pub struct MacWindowContext;

impl MacWindowContext {
    /// Construct. Acquires no OS resources. Equivalent to
    /// `<Self as Default>::default()`; both forms kept for ergonomics
    /// (parity with `WinWindowContext::new`).
    pub fn new() -> Self {
        Self
    }
}

impl WindowContext for MacWindowContext {
    fn foreground(&self) -> AppResult<ForegroundWindow> {
        let app = read_frontmost_app()?;
        Ok(map_to_foreground(app))
    }
}

/// A permission-free snapshot of the frontmost *application* (not
/// window). Sits between the raw ObjC reads and [`ForegroundWindow`]
/// so the judge probe can assert on the raw values directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FrontmostApp {
    /// `NSRunningApplication.bundleIdentifier` — `None` for the rare
    /// process without a bundle (some daemons/helpers).
    pub bundle_id: Option<String>,
    /// `NSRunningApplication.localizedName`.
    pub localized_name: Option<String>,
    /// `NSRunningApplication.processIdentifier`.
    pub pid: i32,
    /// `NSRunningApplication.bundleURL.path` — the `…/Foo.app` path.
    pub bundle_path: Option<String>,
}

/// Read the frontmost application via `NSWorkspace`.
///
/// Returns [`AppError::Other`] when there is genuinely no frontmost app
/// (login window, between space-switch animations) — the macOS twin of
/// the Windows "no foreground window (transient — try again)" path.
pub(crate) fn read_frontmost_app() -> AppResult<FrontmostApp> {
    // `sharedWorkspace` returns the process-wide singleton;
    // `frontmostApplication` is a documented, thread-safe read that does
    // not mutate UI state. objc2 marks these accessors safe, and the
    // returned objects are managed by `Retained` (ARC) — no manual
    // release needed.
    let workspace = NSWorkspace::sharedWorkspace();
    let running = workspace.frontmostApplication().ok_or_else(|| {
        AppError::Other("no frontmost application (transient — try again)".into())
    })?;

    let bundle_id = running.bundleIdentifier().map(|s| s.to_string());
    let localized_name = running.localizedName().map(|s| s.to_string());
    let pid = running.processIdentifier();
    let bundle_path = running
        .bundleURL()
        .and_then(|url| url.path())
        .map(|s| s.to_string());

    Ok(FrontmostApp {
        bundle_id,
        localized_name,
        pid,
        bundle_path,
    })
}

/// Pure mapping from a [`FrontmostApp`] onto the cross-platform
/// [`ForegroundWindow`]. Split out so it's unit-testable without an
/// ObjC runtime (the `NSWorkspace` read is exercised by the probe).
pub(crate) fn map_to_foreground(app: FrontmostApp) -> ForegroundWindow {
    // process_name = bundle id (strategy key), falling back to the
    // localized name when an app has no bundle id, then to empty —
    // never panic on a missing field.
    let process_name = app
        .bundle_id
        .clone()
        .or_else(|| app.localized_name.clone())
        .unwrap_or_default();

    ForegroundWindow {
        // No stable cross-process HWND on macOS; the pid is the closest
        // per-app handle and is useful for provenance. (i32 → isize is
        // always lossless.)
        hwnd: app.pid as isize,
        // Window title needs Accessibility; degrade to empty.
        title: String::new(),
        process_name,
        exe_path: app.bundle_path,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(bundle: Option<&str>, name: Option<&str>) -> FrontmostApp {
        FrontmostApp {
            bundle_id: bundle.map(str::to_owned),
            localized_name: name.map(str::to_owned),
            pid: 4242,
            bundle_path: Some("/Applications/Google Chrome.app".into()),
        }
    }

    #[test]
    fn maps_bundle_id_to_process_name() {
        let fg = map_to_foreground(sample(Some("com.google.Chrome"), Some("Google Chrome")));
        assert_eq!(fg.process_name, "com.google.Chrome");
        assert_eq!(fg.hwnd, 4242);
        assert_eq!(fg.title, "");
        assert_eq!(
            fg.exe_path.as_deref(),
            Some("/Applications/Google Chrome.app")
        );
    }

    #[test]
    fn falls_back_to_localized_name_when_no_bundle_id() {
        let fg = map_to_foreground(sample(None, Some("Some Helper")));
        assert_eq!(fg.process_name, "Some Helper");
    }

    #[test]
    fn process_name_empty_when_no_identity_at_all() {
        let fg = map_to_foreground(sample(None, None));
        assert_eq!(fg.process_name, "");
        // Still no panic and pid is preserved for provenance.
        assert_eq!(fg.hwnd, 4242);
    }

    #[test]
    fn title_is_always_empty_without_accessibility() {
        // Whatever the app, the minimal read never fabricates a title.
        let fg = map_to_foreground(sample(Some("com.apple.Terminal"), Some("Terminal")));
        assert!(fg.title.is_empty());
    }
}
