//! Windows implementation of [`super::WindowContext`].
//!
//! **Stub in Wave 1.** Full impl lands in Wave 2 (bd `mb-dl2`):
//! - `GetForegroundWindow` → HWND
//! - `GetWindowTextW` → title (UTF-16 → String)
//! - `GetWindowThreadProcessId` → PID
//! - `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, ...)` +
//!   `GetModuleBaseNameW` / `QueryFullProcessImageNameW` → basename
//!   + exe path

use super::{ForegroundWindow, WindowContext};
use crate::error::{AppError, AppResult};

/// Stub Windows window-context provider.
#[derive(Default)]
pub struct WinWindowContext;

impl WinWindowContext {
    /// Construct. No OS resources are acquired. Equivalent to
    /// `<Self as Default>::default()`; both forms are kept for ergonomics.
    pub fn new() -> Self {
        Self
    }
}

impl WindowContext for WinWindowContext {
    fn foreground(&self) -> AppResult<ForegroundWindow> {
        Err(AppError::Other(
            "WindowContext::foreground lands in Wave 2 (bd mb-dl2)".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foreground_is_a_clear_wave2_error() {
        let ctx = WinWindowContext::new();
        let err = ctx.foreground().unwrap_err();
        match err {
            AppError::Other(msg) => assert!(msg.contains("Wave 2")),
            other => panic!("expected Other, got {other:?}"),
        }
    }
}
