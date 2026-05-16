//! Linux implementation of [`super::HotkeyListener`].
//!
//! **Stub.** Phase 9 fills this in (likely via `evdev` on Wayland,
//! `XGrabKey` on X11). PLAN §12 #15 mandates the trait + stub exist
//! from day one.

#![cfg(target_os = "linux")]
#![allow(dead_code)]

use std::sync::mpsc;

use super::{HotkeyEvent, HotkeyListener};
use crate::error::{AppError, AppResult};

/// Placeholder for the future evdev / X11 listener.
pub struct LinuxKeyboardHook;

impl LinuxKeyboardHook {
    /// Phase 9 fills this in.
    pub fn new() -> AppResult<Self> {
        Err(AppError::Hotkey(
            "Linux hotkey listener: Phase 9 (evdev / XGrabKey)".into(),
        ))
    }
}

impl HotkeyListener for LinuxKeyboardHook {
    fn install(&mut self, _tx: mpsc::Sender<HotkeyEvent>) -> AppResult<()> {
        todo!("Phase 9 — Linux evdev / XGrabKey install")
    }

    fn uninstall(&mut self) -> AppResult<()> {
        todo!("Phase 9 — Linux evdev / XGrabKey uninstall")
    }

    fn configured_vk(&self) -> u32 {
        todo!("Phase 9 — Linux key code (evdev KEY_* / X11 KeySym)")
    }
}
