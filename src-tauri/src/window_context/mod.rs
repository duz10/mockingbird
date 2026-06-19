#![allow(missing_docs)] // Trait + factory; method-level docs are the API.

//! Foreground-window snapshotting.
//!
//! The injection orchestrator (Wave 4) takes a snapshot at key-down
//! (stored in the session row for provenance) and again at key-up
//! (compared against the first to detect focus loss — ADR 0016 risk).
//! The strategy resolver (`injection/strategy::resolve`) maps
//! [`ForegroundWindow::process_name`] to an
//! [`crate::injection::InjectionStrategy`].
//!
//! Cross-platform via the [`WindowContext`] trait; Windows impl uses
//! `GetForegroundWindow` + `GetWindowTextW` + `GetModuleBaseNameW`.
//! macOS uses `NSWorkspace.frontmostApplication` (permission-free app
//! identity; window title needs Accessibility and is deferred — ADR
//! 0060). Linux is a `todo!()` stub (PLAN §12 #15).

#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "macos")]
pub mod macos;

// mb-mac-v1.4.5 — macOS frontmost-app judge (mac-p3e-frontmost-app-readable),
// mirroring the `injection::judges_macos_v1` in-tree pattern.
#[cfg(target_os = "macos")]
pub mod judges_macos_v1;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
use crate::error::AppError;
use crate::error::AppResult;

/// A snapshot of "what window has keyboard focus right now".
///
/// Captured by [`WindowContext::foreground`] at well-known moments
/// (key-down, key-up — the orchestrator owns the timing). All fields
/// are owned `String` / numeric values so the snapshot can be sent
/// across threads and persisted to the DB without HWND lifetime
/// concerns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForegroundWindow {
    /// Window handle as the OS's native integer. Treated as opaque
    /// outside `windows.rs`. Zero on platforms without a stable HWND
    /// notion or when the foreground is genuinely null.
    pub hwnd: isize,
    /// Window title via `GetWindowTextW` (Windows) / equivalent.
    /// May be empty for windows without titles.
    pub title: String,
    /// Process basename (e.g. `"chrome.exe"`, `"WindowsTerminal.exe"`)
    /// via `GetModuleBaseNameW`. Used as the lookup key into the
    /// injection-strategy table (ADR 0016). Lowercased at lookup
    /// time, NOT here — preserve original casing for provenance.
    pub process_name: String,
    /// Full executable path, when obtainable. Useful for richer
    /// app fingerprinting in future ADRs; not required for v1
    /// strategy resolution. `None` when `QueryFullProcessImageNameW`
    /// fails (rare; protected processes).
    pub exe_path: Option<String>,
}

/// Snapshots the foreground window.
pub trait WindowContext: Send + Sync {
    /// Returns the current foreground window, or
    /// [`AppError::Other`] if there is none (rare transient state —
    /// e.g. between desktop-switching animations).
    fn foreground(&self) -> AppResult<ForegroundWindow>;
}

/// Construct the platform-default context provider.
pub fn make_default_context() -> AppResult<Box<dyn WindowContext>> {
    #[cfg(target_os = "windows")]
    {
        Ok(Box::new(windows::WinWindowContext::new()))
    }
    #[cfg(target_os = "macos")]
    {
        Ok(Box::new(macos::MacWindowContext::new()))
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        Err(AppError::Other(
            "window context not implemented for this platform (Linux deferred — PLAN §12 #15)"
                .into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foreground_window_is_clonable_and_eq() {
        // The orchestrator clones the key-down snapshot into the
        // session row builder, then compares it to the key-up
        // snapshot for focus-loss detection. Both operations require
        // Clone + PartialEq on the struct.
        let a = ForegroundWindow {
            hwnd: 0x1234,
            title: "Untitled — Notepad".into(),
            process_name: "notepad.exe".into(),
            exe_path: Some(r"C:\Windows\System32\notepad.exe".into()),
        };
        let b = a.clone();
        assert_eq!(a, b);
    }
}
