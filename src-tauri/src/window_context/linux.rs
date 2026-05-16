//! Linux implementation of [`super::WindowContext`].
//!
//! **Stub.** Phase 9 fills this in — Wayland makes "foreground
//! window" surprisingly nuanced (only the compositor knows for sure,
//! and most compositors don't expose it without protocols like
//! `wlr-foreign-toplevel-management` or `ext-foreign-toplevel-list`).
//! X11 via `_NET_ACTIVE_WINDOW` is straightforward. PLAN §12 #15.

#![cfg(target_os = "linux")]
#![allow(dead_code)]

use super::{ForegroundWindow, WindowContext};
use crate::error::{AppError, AppResult};

/// Placeholder for the future X11 / Wayland provider.
pub struct LinuxWindowContext;

impl WindowContext for LinuxWindowContext {
    fn foreground(&self) -> AppResult<ForegroundWindow> {
        Err(AppError::Other(
            "Linux window context: Phase 9 (X11 _NET_ACTIVE_WINDOW or Wayland protocol)".into(),
        ))
    }
}
