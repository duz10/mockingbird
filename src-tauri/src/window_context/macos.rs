//! macOS implementation of [`super::WindowContext`].
//!
//! **Stub.** Phase 9 fills in via `NSWorkspace.frontmostApplication`
//! and AX API for the title. PLAN §12 #15.

#![cfg(target_os = "macos")]
#![allow(dead_code)]

use super::{ForegroundWindow, WindowContext};
use crate::error::{AppError, AppResult};

/// Placeholder for the future NSWorkspace + AX-based provider.
pub struct MacWindowContext;

impl WindowContext for MacWindowContext {
    fn foreground(&self) -> AppResult<ForegroundWindow> {
        Err(AppError::Other(
            "macOS window context: Phase 9 (NSWorkspace + AX)".into(),
        ))
    }
}
