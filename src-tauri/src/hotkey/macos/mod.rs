//! macOS `CGEventTap` implementation of [`super::HotkeyListener`].
//!
//! ## Architecture (ADR 0057 — the macOS analogue of ADR 0015)
//!
//! ```text
//!  ┌────────────────────────────┐   install()   ┌─────────────────────┐
//!  │   caller (orchestrator)    │ ────────────► │  tap owner struct   │
//!  └────────────────────────────┘               └─────────────────────┘
//!                                                       │ spawn
//!                                                       ▼
//!                                        ┌──────────────────────────────┐
//!                                        │ mockingbird-hotkey OS thread │
//!                                        │                              │
//!                                        │ ┌─ CGEventTapCreate ───────┐ │
//!                                        │ │  kCGEventKeyDown/Up +    │ │
//!                                        │ │  flagsChanged · ListenOnly│ │
//!                                        │ └──────────────────────────┘ │
//!                                        │ ┌─ CFRunLoop::run_current ─┐ │
//!                                        │ │  • CFRunLoopStop → exit  │ │
//!                                        │ │  • dispatch tap callback │ │
//!                                        │ └──────────────────────────┘ │
//!                                        │ ┌─ on return (Drop) ───────┐ │
//!                                        │ │ tap + source dropped →   │ │
//!                                        │ │ mach port invalidated    │ │
//!                                        │ └──────────────────────────┘ │
//!                                        └──────────────────────────────┘
//!                                                       │ send
//!                                                       ▼
//!                                        ┌──────────────────────────────┐
//!                                        │  mpsc<HotkeyEvent>           │
//!                                        └──────────────────────────────┘
//! ```
//!
//! ### Why a dedicated thread + CFRunLoop
//!
//! A `CGEventTap` delivers events through a `CFMachPort` that must be
//! pumped by a `CFRunLoop`. We do not want to commandeer the app's
//! main run loop (Tauri owns it), so — exactly as `windows.rs` runs
//! `WH_KEYBOARD_LL` on its own thread with a `GetMessageW` pump — we
//! spin a dedicated thread that owns the tap's lifecycle from create
//! to teardown. The parent stops it via `CFRunLoopStop` (one of the
//! few thread-safe CoreFoundation entry points).
//!
//! ### Why `CGEventTap` (low-level) over a hotkey crate
//!
//! ADR 0015 deliberately avoided `tauri-plugin-global-shortcut` on
//! Windows because it fires *once on press* and can't see key-**up**,
//! which the §6.1 push-to-talk state machine needs. The same logic
//! applies here: `CGEventTap` is the lowest-level public API that
//! yields both transitions, so we keep parity rather than adopt a
//! press-only abstraction. See ADR 0057.
//!
//! ### Pure-vs-OS split (mirrors `windows.rs`)
//!
//! The tap callback is a tiny shim: read the event type + keycode +
//! flags, hand them to [`classify_event`] (pure, fully unit-tested in
//! [`keymap`]), and `send` the result. The §6.1 chord/hold logic lives
//! entirely in [`super::state`]; we do **not** reimplement it here.
//!
//! ### Input Monitoring permission (graceful degradation)
//!
//! An event tap only receives events once the user grants **Input
//! Monitoring** (System Settings → Privacy & Security). If the tap
//! can't be created, we log a clear `tracing::warn!` and leave the
//! listener **inert** — `install()` still returns `Ok` so app startup
//! is never blocked. Wave C's onboarding panel / the permission-gated
//! e2e (`mac-p3-dictation-e2e`) drive the actual grant flow.

#![cfg(target_os = "macos")]
#![allow(dead_code)]

mod keymap;

pub use keymap::{
    classify_event, is_modifier_keycode, modifier_mask_for_keycode, MacEventKind, KVK_ESCAPE,
    KVK_LEFT_COMMAND, KVK_LEFT_CONTROL, KVK_LEFT_OPTION, KVK_LEFT_SHIFT, KVK_RIGHT_COMMAND,
    KVK_RIGHT_CONTROL, KVK_RIGHT_OPTION, KVK_RIGHT_SHIFT,
};

use std::sync::mpsc::{self, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::{HotkeyEvent, HotkeyListener};
use crate::error::{AppError, AppResult};
use keymap::DEFAULT_VK;

/// Reasonable upper bound on consumer queue depth — exposed so the
/// driver / tests size consumers consistently (parity with the
/// Windows hook). The tap `send`s once per real transition; the
/// driver drains at the 20 ms tick cadence.
const CHANNEL_BUFFER_HINT: usize = 256;

// --------------------------------------------------------------------
// Tap owner
// --------------------------------------------------------------------

/// Status the tap thread reports back to `install()` before it blocks
/// on the run loop.
enum TapBootStatus {
    /// Tap is live; `runloop` is the `CFRunLoopRef` (as `usize`, since
    /// the ref is not `Send`) the owner uses to `CFRunLoopStop`.
    Ready { runloop: usize },
    /// Tap could not be created/enabled — almost always missing Input
    /// Monitoring permission. The listener is inert; startup proceeds.
    Failed,
}

/// `CGEventTap`-based global hotkey listener.
///
/// Lifecycle: `new()` holds no OS resources. `install(tx)` spawns the
/// dedicated thread, creates + enables the tap, and starts emitting on
/// `tx`. `uninstall()` stops the run loop and joins the thread (which
/// drops the tap, tearing down the mach port). `Drop` does the same
/// defensively.
pub struct MacKeyboardHook {
    vk: u32,
    thread: Option<JoinHandle<()>>,
    /// `CFRunLoopRef` of the tap thread, stored as `usize` (the ref is
    /// `!Send`). `None` when no live tap (not installed, or the
    /// permission-denied inert case).
    runloop: Option<usize>,
}

impl Default for MacKeyboardHook {
    fn default() -> Self {
        Self {
            vk: DEFAULT_VK,
            thread: None,
            runloop: None,
        }
    }
}

impl MacKeyboardHook {
    /// Construct a not-yet-installed hook bound to the default key
    /// (Right Option). Holds no OS resources.
    pub fn new() -> AppResult<Self> {
        Ok(Self::default())
    }

    /// Construct with an explicit macOS virtual keycode (used by the
    /// conflict probe to rebind onto a fallback hotkey).
    pub fn with_vk(vk: u32) -> Self {
        Self {
            vk,
            thread: None,
            runloop: None,
        }
    }

    /// Best-effort channel-buffer hint — parity with the Windows hook.
    pub const fn recommended_channel_buffer() -> usize {
        CHANNEL_BUFFER_HINT
    }
}

impl HotkeyListener for MacKeyboardHook {
    fn install(&mut self, tx: Sender<HotkeyEvent>) -> AppResult<()> {
        if self.thread.is_some() {
            // Idempotent per trait contract.
            return Ok(());
        }
        let vk = self.vk;

        let (boot_tx, boot_rx) = mpsc::channel::<TapBootStatus>();
        let handle = std::thread::Builder::new()
            .name("mockingbird-hotkey".into())
            .spawn(move || run_tap_thread(vk, tx, boot_tx))
            .map_err(|e| AppError::Hotkey(format!("hotkey thread spawn failed: {e}")))?;

        // The thread reports readiness BEFORE entering the run loop, so
        // this resolves quickly. A 2 s ceiling guards against a wedged
        // thread without hanging startup.
        match boot_rx.recv_timeout(Duration::from_secs(2)) {
            Ok(TapBootStatus::Ready { runloop }) => {
                self.thread = Some(handle);
                self.runloop = Some(runloop);
                Ok(())
            }
            Ok(TapBootStatus::Failed) => {
                // Permission not granted (or tap create failed). The
                // thread has already exited; join it and stay inert.
                // Startup is NOT blocked — return Ok.
                let _ = handle.join();
                Ok(())
            }
            Err(_) => {
                // Thread didn't report in time — treat as inert and
                // detach rather than risk a hang-on-drop (we have no
                // run-loop ref to stop it). Should be unreachable in
                // practice: tap creation is synchronous and fast.
                tracing::warn!(
                    target: "hotkey",
                    "hotkey tap thread did not report readiness within 2s; \
                     treating the global hotkey as inactive this session"
                );
                std::mem::forget(handle);
                Ok(())
            }
        }
    }

    fn uninstall(&mut self) -> AppResult<()> {
        if let Some(runloop) = self.runloop.take() {
            // CFRunLoopStop is thread-safe and wakes the blocked
            // run loop so `run_current()` returns and the thread tears
            // the tap down on its way out.
            unsafe { CFRunLoopStop(runloop as CFRunLoopRef) };
        }
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
        Ok(())
    }

    fn configured_vk(&self) -> u32 {
        self.vk
    }
}

impl Drop for MacKeyboardHook {
    fn drop(&mut self) {
        // Best-effort cleanup; ignore errors (we're going away).
        let _ = self.uninstall();
    }
}

// --------------------------------------------------------------------
// OS thread implementation
// --------------------------------------------------------------------

// `CFRunLoopRef` + `CFRunLoopStop` come from the CoreFoundation
// framework that the `core-foundation` crate already links. We bind
// the type and the one thread-safe entry point we need directly.
use core_foundation::runloop::CFRunLoopRef;

extern "C" {
    /// Force `rl` to stop running and return from `CFRunLoopRun`.
    /// Thread-safe (one of the few CF functions that is).
    fn CFRunLoopStop(rl: CFRunLoopRef);
}

/// Tap thread body: create the event tap, wire it into this thread's
/// run loop, report readiness, then pump until `CFRunLoopStop`.
fn run_tap_thread(vk: u32, tx: Sender<HotkeyEvent>, boot_tx: Sender<TapBootStatus>) {
    use core_foundation::base::TCFType;
    use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop};
    use core_graphics::event::{
        CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType,
        EventField,
    };

    let current = CFRunLoop::get_current();

    // ListenOnly: we observe events, never mutate/suppress them. This
    // is the conservative Wave B1 choice — suppressing a *modifier*
    // (Right Option) system-wide is a separate decision (cf. the
    // Windows Wave 4.7 suppression ADR) deferred to a later wave.
    let tap = CGEventTap::new(
        CGEventTapLocation::HID,
        CGEventTapPlacement::HeadInsertEventTap,
        CGEventTapOptions::ListenOnly,
        vec![
            CGEventType::KeyDown,
            CGEventType::KeyUp,
            CGEventType::FlagsChanged,
        ],
        move |_proxy, event_type, event| {
            let keycode = event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE) as u32;
            let flags = event.get_flags().bits();
            let kind = match event_type {
                CGEventType::KeyDown => MacEventKind::KeyDown,
                CGEventType::KeyUp => MacEventKind::KeyUp,
                CGEventType::FlagsChanged => MacEventKind::FlagsChanged,
                // TapDisabledByTimeout / TapDisabledByUserInput / any
                // other type: nothing to translate. (ListenOnly taps
                // are rarely disabled; re-enable hardening is noted in
                // ADR 0057 as future work.)
                _ => return None,
            };
            // Minimal, non-blocking: translate + send, nothing heavy —
            // same discipline as a WH_KEYBOARD_LL callback. `send` on
            // an unbounded std mpsc never blocks.
            if let Some(ev) = classify_event(kind, keycode, flags, vk, Instant::now()) {
                let _ = tx.send(ev);
            }
            // ListenOnly: the return value is ignored by the OS. We
            // return `None` (no event replacement) for clarity.
            None
        },
    );

    let tap = match tap {
        Ok(t) => t,
        Err(()) => {
            tracing::warn!(
                target: "hotkey",
                "Input Monitoring permission required: CGEventTapCreate failed. \
                 The global dictation hotkey is INACTIVE until access is granted \
                 under System Settings → Privacy & Security → Input Monitoring. \
                 App startup continues."
            );
            let _ = boot_tx.send(TapBootStatus::Failed);
            return;
        }
    };

    let source = match tap.mach_port.create_runloop_source(0) {
        Ok(s) => s,
        Err(()) => {
            tracing::warn!(
                target: "hotkey",
                "failed to create CFRunLoop source for the event tap; hotkey inactive"
            );
            let _ = boot_tx.send(TapBootStatus::Failed);
            return;
        }
    };

    unsafe {
        current.add_source(&source, kCFRunLoopCommonModes);
    }
    tap.enable();

    // Publish our run-loop pointer so `uninstall()` can stop us, THEN
    // block. Sending before `run_current()` guarantees the owner can
    // always stop the loop.
    let runloop_ptr = current.as_concrete_TypeRef() as usize;
    if boot_tx
        .send(TapBootStatus::Ready {
            runloop: runloop_ptr,
        })
        .is_err()
    {
        // Owner went away before install completed — nothing to pump.
        return;
    }

    // Pump until `CFRunLoopStop`. On return, `tap` + `source` drop:
    // the mach port is invalidated and the source removed — the OS
    // hook is fully torn down.
    CFRunLoop::run_current();
}

// --------------------------------------------------------------------
// Tests — owner struct lifecycle (no OS resources)
// --------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_returns_default_vk() {
        let hook = MacKeyboardHook::new().expect("construct");
        assert_eq!(hook.configured_vk(), DEFAULT_VK);
    }

    #[test]
    fn with_vk_overrides_default() {
        let hook = MacKeyboardHook::with_vk(KVK_RIGHT_COMMAND);
        assert_eq!(hook.configured_vk(), KVK_RIGHT_COMMAND);
    }

    #[test]
    fn recommended_buffer_is_reasonable() {
        let n = MacKeyboardHook::recommended_channel_buffer();
        assert!((64..=4096).contains(&n), "buffer {n} outside sensible band");
    }

    #[test]
    fn uninstall_is_idempotent_before_install() {
        let mut hook = MacKeyboardHook::new().expect("construct");
        assert!(hook.uninstall().is_ok());
        assert!(hook.uninstall().is_ok());
    }
}
