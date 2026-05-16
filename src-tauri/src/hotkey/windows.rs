//! Windows `WH_KEYBOARD_LL` implementation of [`super::HotkeyListener`].
//!
//! **Stub in Wave 1.** The full hook install + dedicated message-pump
//! thread + watchdog lands in Wave 3 (bd `mb-k0c`). See ADR 0015 for
//! the binding design rules (no work in callback, dedicated thread,
//! RAII handle).

use std::sync::mpsc;

use super::{HotkeyEvent, HotkeyListener};
use crate::error::{AppError, AppResult};

/// Right Alt — PLAN §6.1 default. ADR 0019 may resolve this to a
/// fallback (F23, F24, Ctrl+Shift+Space) at startup.
const DEFAULT_VK_RMENU: u32 = 0xA5;

/// Stub `WH_KEYBOARD_LL` hook owner.
///
/// Wave 3 fills in:
/// - A dedicated `mockingbird-hotkey` OS thread.
/// - `SetWindowsHookEx(WH_KEYBOARD_LL, ...)` install + RAII teardown.
/// - `LowLevelKeyboardProc` that posts to the channel and returns.
/// - 5-minute watchdog log (ADR 0019).
pub struct WinKeyboardHook {
    vk: u32,
}

impl Default for WinKeyboardHook {
    /// Right Alt is the PLAN §6.1 default; ADR 0019 may resolve to a
    /// fallback (F23, F24, Ctrl+Shift+Space) at startup.
    fn default() -> Self {
        Self {
            vk: DEFAULT_VK_RMENU,
        }
    }
}

impl WinKeyboardHook {
    /// Construct a not-yet-installed hook bound to the default VK.
    pub fn new() -> AppResult<Self> {
        Ok(Self::default())
    }
}

impl HotkeyListener for WinKeyboardHook {
    fn install(&mut self, _tx: mpsc::Sender<HotkeyEvent>) -> AppResult<()> {
        Err(AppError::Hotkey(
            "WH_KEYBOARD_LL install lands in Wave 3 (bd mb-k0c)".into(),
        ))
    }

    fn uninstall(&mut self) -> AppResult<()> {
        Ok(())
    }

    fn configured_vk(&self) -> u32 {
        self.vk
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_returns_default_vk() {
        let hook = WinKeyboardHook::new().expect("construct");
        assert_eq!(hook.configured_vk(), DEFAULT_VK_RMENU);
    }

    #[test]
    fn install_is_a_clear_wave3_error() {
        let mut hook = WinKeyboardHook::new().expect("construct");
        let (tx, _rx) = mpsc::channel();
        let err = hook.install(tx).unwrap_err();
        match err {
            AppError::Hotkey(msg) => assert!(msg.contains("Wave 3")),
            other => panic!("expected Hotkey, got {other:?}"),
        }
    }

    #[test]
    fn uninstall_is_idempotent_no_op() {
        let mut hook = WinKeyboardHook::new().expect("construct");
        assert!(hook.uninstall().is_ok());
        assert!(hook.uninstall().is_ok());
    }
}
