#![allow(missing_docs)] // Trait + factory; method-level docs are the API.

//! Global hotkey listener — fires on **both** key-down and key-up.
//!
//! Cross-platform via the [`HotkeyListener`] trait. Windows impl uses
//! `SetWindowsHookEx(WH_KEYBOARD_LL)` per ADR 0015; macOS / Linux
//! impls are `todo!()` stubs per PLAN §12 #15 — the trait shape
//! locks the contract so Phase 9 is "fill the stubs", not "rewrite
//! the layer".
//!
//! ## Two-event model
//!
//! Unlike `tauri-plugin-global-shortcut` (which fires once on press),
//! a [`HotkeyListener`] yields **both** [`HotkeyEvent::KeyDown`] and
//! [`HotkeyEvent::KeyUp`] for the configured key. The §6.1 state
//! machine in [`state`] distinguishes a tap (<80 ms — pass through to
//! OS) from a hold (≥80 ms — start recording). See ADR 0015 for the
//! rationale.

pub mod driver;
pub mod pause;
pub mod probe;
pub mod state;

#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(not(target_os = "windows"))]
use crate::error::AppError;
use crate::error::AppResult;

use std::sync::mpsc;
use std::time::Instant;

/// Events a [`HotkeyListener`] emits to the state machine.
///
/// Carrying `Instant` timestamps lets the state machine reason about
/// hold duration deterministically without re-reading the clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyEvent {
    /// The configured hotkey transitioned to pressed.
    KeyDown { vk: u32, at: Instant },
    /// The configured hotkey transitioned to released.
    KeyUp { vk: u32, at: Instant },
    /// Escape was pressed while a session was active (cancel signal).
    Escape { at: Instant },
    /// Periodic tick from the state-machine driver — used for the
    /// 300 s auto-stop, the 80 ms hold discriminator, and the
    /// confirm-cancel toast timeout.
    Tick { at: Instant },
    /// The tray "Pause dictation" toggle changed state.
    PauseToggle { paused: bool },
    /// The orchestrator's `complete()` / `discard()` finished. Drives
    /// the §6.1 `Processing → Idle` transition, which the state
    /// machine can't make on its own (the orchestrator owns the
    /// pipeline completion signal). Without this, the machine is
    /// stuck in `Processing` after the first hold and silently
    /// drops every subsequent KeyDown per §6.1.
    PipelineComplete,
}

/// Listens for the configured global hotkey and yields key-down +
/// key-up events on a channel.
///
/// **Implementations must not perform work in the OS callback.** ADR
/// 0015 codifies the rule: read the keystroke, build a [`HotkeyEvent`],
/// `try_send` on the channel, return immediately. Anything else
/// risks the Windows low-level-hook timeout and a silent unhook.
pub trait HotkeyListener: Send {
    /// Install the OS-level hook and begin emitting events on the
    /// provided sender. Idempotent: calling on an already-installed
    /// listener is a no-op.
    fn install(&mut self, tx: mpsc::Sender<HotkeyEvent>) -> AppResult<()>;

    /// Tear down the OS-level hook. Idempotent.
    fn uninstall(&mut self) -> AppResult<()>;

    /// Currently-configured virtual key (Windows VK code on Windows;
    /// platform-equivalent elsewhere). Used for the conflict probe
    /// (ADR 0019) and the watchdog log line.
    fn configured_vk(&self) -> u32;
}

/// Construct the platform-default listener.
///
/// Returns an error on non-Windows for Phase 3 — the macOS / Linux
/// scaffolds exist to lock the trait contract; Phase 9 fills them.
pub fn make_default_listener() -> AppResult<Box<dyn HotkeyListener>> {
    #[cfg(target_os = "windows")]
    {
        Ok(Box::new(windows::WinKeyboardHook::new()?))
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err(AppError::Hotkey(
            "hotkey listener not implemented for this platform (Phase 9 macOS/Linux)".into(),
        ))
    }
}
