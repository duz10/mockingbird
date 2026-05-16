//! macOS implementation of [`super::HotkeyListener`].
//!
//! **Stub.** Phase 9 fills this in (likely via `CGEventTap`). PLAN
//! §12 #15 mandates the trait + stub exist from day one so the
//! later port is "fill the stubs", not "rewrite the layer".

#![cfg(target_os = "macos")]
#![allow(dead_code)]

use std::sync::mpsc;

use super::{HotkeyEvent, HotkeyListener};
use crate::error::{AppError, AppResult};

/// Placeholder for the future `CGEventTap`-based listener.
pub struct MacKeyboardHook;

impl MacKeyboardHook {
    /// Phase 9 fills this in.
    pub fn new() -> AppResult<Self> {
        Err(AppError::Hotkey(
            "macOS hotkey listener: Phase 9 (CGEventTap)".into(),
        ))
    }
}

impl HotkeyListener for MacKeyboardHook {
    fn install(&mut self, _tx: mpsc::Sender<HotkeyEvent>) -> AppResult<()> {
        todo!("Phase 9 — macOS CGEventTap install")
    }

    fn uninstall(&mut self) -> AppResult<()> {
        todo!("Phase 9 — macOS CGEventTap uninstall")
    }

    fn configured_vk(&self) -> u32 {
        todo!("Phase 9 — macOS key code (kVK_* family)")
    }
}
