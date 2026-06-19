//! Meetings hotkey installer — the SECOND `WH_KEYBOARD_LL` hook on a
//! dedicated message-pump thread (Phase MC Wave 3, brief §3.4).
//!
//! ## Binding constraints
//!
//! 1. **Do NOT touch `hotkey/state.rs`, `hotkey/windows.rs`, or
//!    `hotkey/driver.rs`.** The dictation hook is sealed. This module
//!    installs an INDEPENDENT, SECOND hook on its OWN thread. The two
//!    hooks share zero state.
//! 2. **Always `CallNextHookEx`.** Dictation installed first at app
//!    boot; meetings installs second. The meetings hook MUST let every
//!    keystroke flow downstream so dictation still sees them.
//! 3. **No global mutable state at file scope.** Per-thread state
//!    lives in `thread_local!`, the same pattern dictation uses.
//!
//! ## What this gives you
//!
//! - [`MeetingHotkeyInstaller`] — owns the hook thread; `install()`
//!   spawns + plants, `stop()` posts WM_QUIT + joins.
//! - [`ChordConfig`] — `(modifier_vk, main_vk)` carried by value.
//! - [`classify_meeting_keystroke`] — **pure** function that turns
//!   `(WM_*, vk_code, modifier_vk, main_vk, ts_ms)` into an optional
//!   [`ActivationEvent`]. This is the entire testable surface of the
//!   hook proc; the proc itself is a thin OS adapter.
//!
//! ## Testability
//!
//! The pure classifier is fully covered by unit tests in this file.
//! The OS install path is exercised by `#[ignore]`'d integration
//! tests gated `#[cfg(target_os = "windows")]` — synthetic
//! `keybd_event` injection works on a desktop but is unreliable on
//! headless CI runners (no input desktop), so we don't run them by
//! default. The activation state machine + the runtime wiring exercise
//! the path end-to-end in Wave 4.

// macOS port: live impl is `#[cfg(target_os = "windows")]`; these imports/fields
// are orphaned on non-Windows until the cross-platform backend lands (Phase 3/4).
#![cfg_attr(
    not(target_os = "windows"),
    allow(unused_imports, dead_code, unused_mut)
)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use crate::error::{AppError, AppResult};
use crate::meetings::activation::ActivationEvent;

// ---------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------

/// VK codes for the meeting chord. Default is `VK_RCONTROL +
/// VK_OEM_PERIOD` (the `.>` key) as of the mb-fc1 hotfix — the prior
/// `VK_M` default collided with Microsoft 365 Copilot on Windows 11.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChordConfig {
    /// Modifier VK. Defaults to `0xA3` (VK_RCONTROL).
    pub modifier_vk: u32,
    /// Main-key VK. Defaults to `0xBE` (VK_OEM_PERIOD).
    pub main_vk: u32,
}

impl Default for ChordConfig {
    fn default() -> Self {
        Self {
            modifier_vk: 0xA3, // VK_RCONTROL
            main_vk: 0xBE,     // VK_OEM_PERIOD (`.>`)
        }
    }
}

/// Owner of the meetings hook thread. `install()` plants the second
/// `WH_KEYBOARD_LL` hook on a dedicated thread; `stop()` posts WM_QUIT
/// and joins.
// `Debug` is required by the `not(windows)` unit test that asserts
// `install()` returns a Phase-9 error off-Windows (`.unwrap_err()`).
// All fields are `Debug`; no-op for the Windows build.
#[derive(Debug)]
pub struct MeetingHotkeyInstaller {
    chord: ChordConfig,
    thread: Option<JoinHandle<()>>,
    hook_tid: Option<u32>,
    /// Set by `stop()` to signal the message-pump thread (paired with
    /// the WM_QUIT post — the pump exits on either condition).
    stop_signal: Arc<AtomicBool>,
}

impl MeetingHotkeyInstaller {
    /// Install the meetings hook on a fresh thread. Returns once the
    /// thread has reported its TID (needed for the WM_QUIT post on
    /// stop). Errors if the thread can't spawn or doesn't report its
    /// TID within 500 ms (mirrors the dictation hook's contract).
    pub fn install(chord: ChordConfig, sender: Sender<ActivationEvent>) -> AppResult<Self> {
        #[cfg(target_os = "windows")]
        {
            install_windows(chord, sender)
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = (chord, sender);
            Err(AppError::Hotkey(
                "meetings hotkey is Windows-only (Phase 9 platform parity)".into(),
            ))
        }
    }

    /// Tear down: post `WM_QUIT` to the hook thread, join. Idempotent.
    pub fn stop(mut self) -> AppResult<()> {
        self.stop_signal.store(true, Ordering::Relaxed);
        #[cfg(target_os = "windows")]
        {
            stop_windows(&mut self)?;
        }
        Ok(())
    }

    /// The chord config the installer was built with (used by tests +
    /// the runtime's structured logging).
    pub fn chord(&self) -> ChordConfig {
        self.chord
    }
}

impl Drop for MeetingHotkeyInstaller {
    fn drop(&mut self) {
        // Best-effort: if the caller forgot to call stop(), still
        // tear down so we don't leak the hook handle.
        self.stop_signal.store(true, Ordering::Relaxed);
        #[cfg(target_os = "windows")]
        {
            if let Err(e) = stop_windows(self) {
                tracing::warn!(
                    target: "meetings",
                    error = %e,
                    "MeetingHotkeyInstaller::drop saw stop error"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------
// Pure classifier (the entire testable surface)
// ---------------------------------------------------------------------

// Mirror the dictation hook's local copies of WM_* — they're carried
// here as `pub(crate)` so the proc + the tests can share them, and the
// const_assert below pins them to the live `windows` crate constants
// on Windows builds.
pub(crate) const WM_KEYDOWN_U32: u32 = 0x100;
pub(crate) const WM_KEYUP_U32: u32 = 0x101;
pub(crate) const WM_SYSKEYDOWN_U32: u32 = 0x104;
pub(crate) const WM_SYSKEYUP_U32: u32 = 0x105;

#[cfg(target_os = "windows")]
#[allow(dead_code)]
const _WM_ASSERTIONS: () = {
    use windows::Win32::UI::WindowsAndMessaging::{
        WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
    };
    assert!(WM_KEYDOWN_U32 == WM_KEYDOWN);
    assert!(WM_KEYUP_U32 == WM_KEYUP);
    assert!(WM_SYSKEYDOWN_U32 == WM_SYSKEYDOWN);
    assert!(WM_SYSKEYUP_U32 == WM_SYSKEYUP);
};

/// Classify a low-level keystroke into an [`ActivationEvent`].
///
/// Returns `None` for keystrokes the activation state machine doesn't
/// care about (anything that isn't our `modifier_vk` or `main_vk`).
/// Modifier alias families (e.g. Ctrl is delivered as VK_CONTROL by
/// SendInput sometimes, but as VK_RCONTROL/VK_LCONTROL by hardware
/// keyboards) are explicitly NOT folded together — the caller is
/// expected to pass the EXACT VK they configured. Left vs right
/// disambiguation is the user's job at settings time.
///
/// Pure function. No OS calls.
pub fn classify_meeting_keystroke(
    wm: u32,
    vk_code: u32,
    modifier_vk: u32,
    main_vk: u32,
    ts_ms: u64,
) -> Option<ActivationEvent> {
    let is_down = matches!(wm, WM_KEYDOWN_U32 | WM_SYSKEYDOWN_U32);
    let is_up = matches!(wm, WM_KEYUP_U32 | WM_SYSKEYUP_U32);
    if !is_down && !is_up {
        return None;
    }
    if vk_code == modifier_vk {
        Some(if is_down {
            ActivationEvent::ModifierDown { ts_ms }
        } else {
            ActivationEvent::ModifierUp { ts_ms }
        })
    } else if vk_code == main_vk {
        Some(if is_down {
            ActivationEvent::MainKeyDown { ts_ms }
        } else {
            ActivationEvent::MainKeyUp { ts_ms }
        })
    } else {
        None
    }
}

// ---------------------------------------------------------------------
// Windows-only OS thread + hook proc
// ---------------------------------------------------------------------

#[cfg(target_os = "windows")]
mod win {
    use super::*;
    use std::cell::RefCell;
    use std::time::Instant;
    use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetMessageW, PostThreadMessageW, SetWindowsHookExW,
        TranslateMessage, UnhookWindowsHookEx, HHOOK, HOOKPROC, KBDLLHOOKSTRUCT, MSG,
        WH_KEYBOARD_LL, WM_QUIT,
    };

    // Per-thread storage for the meetings hook. Independent of the
    // dictation hook's thread_locals — different thread, different
    // RefCell, no shared state.
    thread_local! {
        pub(super) static MEETING_TX: RefCell<Option<Sender<ActivationEvent>>> =
            const { RefCell::new(None) };
        pub(super) static MEETING_CHORD: RefCell<ChordConfig> =
            const { RefCell::new(ChordConfig { modifier_vk: 0xA3, main_vk: 0xBE }) };
        pub(super) static MEETING_HHOOK: RefCell<Option<HHOOK>> =
            const { RefCell::new(None) };
        /// Anchor for elapsed-millis timestamps on `ActivationEvent`.
        pub(super) static MEETING_ANCHOR: RefCell<Option<Instant>> =
            const { RefCell::new(None) };
    }

    pub(super) fn run_thread(chord: ChordConfig, tx: Sender<ActivationEvent>, tid_tx: Sender<u32>) {
        let tid = unsafe { GetCurrentThreadId() };
        if tid_tx.send(tid).is_err() {
            return;
        }
        MEETING_TX.with(|c| *c.borrow_mut() = Some(tx));
        MEETING_CHORD.with(|c| *c.borrow_mut() = chord);
        MEETING_ANCHOR.with(|c| *c.borrow_mut() = Some(Instant::now()));

        let proc: HOOKPROC = Some(low_level_keyboard_proc);
        let hook_res = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, proc, HINSTANCE(0), 0) };
        let hhook = match hook_res {
            Ok(h) => h,
            Err(e) => {
                tracing::error!(
                    target: "meetings",
                    error = %e,
                    "meeting SetWindowsHookEx(WH_KEYBOARD_LL) failed"
                );
                MEETING_TX.with(|c| c.borrow_mut().take());
                return;
            }
        };
        MEETING_HHOOK.with(|c| *c.borrow_mut() = Some(hhook));

        // Message pump until WM_QUIT.
        let mut msg = MSG::default();
        loop {
            let r = unsafe {
                GetMessageW(
                    &mut msg as *mut MSG,
                    windows::Win32::Foundation::HWND(0),
                    0,
                    0,
                )
            };
            if r.0 <= 0 {
                break;
            }
            unsafe {
                let _ = TranslateMessage(&msg as *const MSG);
                DispatchMessageW(&msg as *const MSG);
            }
        }

        MEETING_HHOOK.with(|c| {
            if let Some(h) = c.borrow_mut().take() {
                let _ = unsafe { UnhookWindowsHookEx(h) };
            }
        });
        MEETING_TX.with(|c| {
            let _ = c.borrow_mut().take();
        });
    }

    /// The hook proc. **Always `CallNextHookEx`** — dictation
    /// downstream must still see every keystroke. We only observe
    /// (never suppress).
    pub(super) unsafe extern "system" fn low_level_keyboard_proc(
        code: i32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if code < 0 {
            return CallNextHookEx(None, code, wparam, lparam);
        }
        let kbd = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        let chord = MEETING_CHORD.with(|c| *c.borrow());
        let ts_ms = MEETING_ANCHOR.with(|c| {
            c.borrow()
                .map(|anchor| anchor.elapsed().as_millis() as u64)
                .unwrap_or(0)
        });
        if let Some(ev) = classify_meeting_keystroke(
            wparam.0 as u32,
            kbd.vkCode,
            chord.modifier_vk,
            chord.main_vk,
            ts_ms,
        ) {
            MEETING_TX.with(|c| {
                if let Some(tx) = c.borrow().as_ref() {
                    let _ = tx.send(ev);
                }
            });
        }
        // Never suppress: dictation hook downstream must still fire.
        CallNextHookEx(None, code, wparam, lparam)
    }

    pub(super) fn post_quit(tid: u32) {
        let _ = unsafe { PostThreadMessageW(tid, WM_QUIT, WPARAM(0), LPARAM(0)) };
    }
}

#[cfg(target_os = "windows")]
fn install_windows(
    chord: ChordConfig,
    sender: Sender<ActivationEvent>,
) -> AppResult<MeetingHotkeyInstaller> {
    let (tid_tx, tid_rx) = mpsc::channel::<u32>();
    let chord_for_thread = chord;
    let handle = std::thread::Builder::new()
        .name("mockingbird-meeting-hotkey".into())
        .spawn(move || win::run_thread(chord_for_thread, sender, tid_tx))
        .map_err(|e| AppError::Hotkey(format!("meeting hotkey thread spawn: {e}")))?;
    let hook_tid = tid_rx
        .recv_timeout(Duration::from_millis(500))
        .map_err(|_| {
            AppError::Hotkey("meeting hotkey thread did not report its TID within 500 ms".into())
        })?;
    Ok(MeetingHotkeyInstaller {
        chord,
        thread: Some(handle),
        hook_tid: Some(hook_tid),
        stop_signal: Arc::new(AtomicBool::new(false)),
    })
}

#[cfg(target_os = "windows")]
fn stop_windows(this: &mut MeetingHotkeyInstaller) -> AppResult<()> {
    if let (Some(tid), Some(handle)) = (this.hook_tid.take(), this.thread.take()) {
        win::post_quit(tid);
        let _ = handle.join();
    }
    Ok(())
}

// =====================================================================
// Tests
// =====================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn classify(wm: u32, vk: u32) -> Option<ActivationEvent> {
        // Default chord: VK_RCONTROL (0xA3) + VK_M (0x4D)
        classify_meeting_keystroke(wm, vk, 0xA3, 0x4D, 42)
    }

    #[test]
    fn chord_config_default_matches_adr_0019() {
        let c = ChordConfig::default();
        assert_eq!(c.modifier_vk, 0xA3); // VK_RCONTROL
        assert_eq!(c.main_vk, 0x4D); // 'M'
    }

    #[test]
    fn modifier_down_emits_modifier_down() {
        let ev = classify(WM_KEYDOWN_U32, 0xA3).unwrap();
        assert_eq!(ev, ActivationEvent::ModifierDown { ts_ms: 42 });
    }

    #[test]
    fn modifier_up_emits_modifier_up() {
        let ev = classify(WM_KEYUP_U32, 0xA3).unwrap();
        assert_eq!(ev, ActivationEvent::ModifierUp { ts_ms: 42 });
    }

    #[test]
    fn main_key_down_emits_main_key_down() {
        let ev = classify(WM_KEYDOWN_U32, 0x4D).unwrap();
        assert_eq!(ev, ActivationEvent::MainKeyDown { ts_ms: 42 });
    }

    #[test]
    fn main_key_up_emits_main_key_up() {
        let ev = classify(WM_KEYUP_U32, 0x4D).unwrap();
        assert_eq!(ev, ActivationEvent::MainKeyUp { ts_ms: 42 });
    }

    #[test]
    fn syskeydown_path_also_classifies() {
        // Modifiers paired with menus arrive as WM_SYSKEYDOWN even if
        // they're our configured modifier alone — must still classify.
        let ev = classify(WM_SYSKEYDOWN_U32, 0xA3).unwrap();
        assert_eq!(ev, ActivationEvent::ModifierDown { ts_ms: 42 });
        let ev = classify(WM_SYSKEYUP_U32, 0x4D).unwrap();
        assert_eq!(ev, ActivationEvent::MainKeyUp { ts_ms: 42 });
    }

    #[test]
    fn unrelated_keys_classify_to_none() {
        // 'A' = 0x41, neither modifier nor main.
        assert!(classify(WM_KEYDOWN_U32, 0x41).is_none());
        // VK_F12 = 0x7B.
        assert!(classify(WM_KEYUP_U32, 0x7B).is_none());
    }

    #[test]
    fn unrelated_wm_message_classifies_to_none() {
        // WM_MOUSEMOVE (0x200) → not a key event.
        assert!(classify(0x200, 0xA3).is_none());
        // WM_CHAR (0x102) → not a key down/up.
        assert!(classify(0x102, 0xA3).is_none());
    }

    #[test]
    fn classifier_with_custom_chord() {
        // VK_LSHIFT (0xA0) + VK_F13 (0x7C) as a fallback chord.
        let ev = classify_meeting_keystroke(WM_KEYDOWN_U32, 0xA0, 0xA0, 0x7C, 7).unwrap();
        assert_eq!(ev, ActivationEvent::ModifierDown { ts_ms: 7 });
        let ev = classify_meeting_keystroke(WM_KEYUP_U32, 0x7C, 0xA0, 0x7C, 7).unwrap();
        assert_eq!(ev, ActivationEvent::MainKeyUp { ts_ms: 7 });
        // Default VK_RCONTROL must NOT fire when the chord is rebound.
        assert!(classify_meeting_keystroke(WM_KEYDOWN_U32, 0xA3, 0xA0, 0x7C, 7).is_none());
    }

    #[test]
    fn classifier_treats_modifier_eq_main_vk_as_modifier_first() {
        // Pathological: settings somehow have modifier == main. The
        // `if modifier == vk` branch fires first, so the event is
        // a Modifier{Down,Up}. This isn't a "supported" config but
        // documenting the deterministic fallthrough avoids confusion.
        let ev = classify_meeting_keystroke(WM_KEYDOWN_U32, 0xA3, 0xA3, 0xA3, 1).unwrap();
        assert_eq!(ev, ActivationEvent::ModifierDown { ts_ms: 1 });
    }

    // ----- Live OS install path (ignored by default) -----

    /// Spawn the hook thread, wait briefly, stop. Verifies the
    /// install/stop lifecycle doesn't panic and doesn't leak the hook
    /// handle. Ignored because LL hook installs can interact with
    /// other tests sharing the same input desktop; run manually with
    /// `cargo test -- --ignored hotkey_install_and_stop_cleanly`.
    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "live WH_KEYBOARD_LL install; run with `cargo test -- --ignored`"]
    fn hotkey_install_and_stop_cleanly() {
        let (tx, _rx) = mpsc::channel::<ActivationEvent>();
        let installer =
            MeetingHotkeyInstaller::install(ChordConfig::default(), tx).expect("install");
        // Let the message pump live briefly so the install is visible
        // in spy tooling.
        std::thread::sleep(Duration::from_millis(100));
        installer.stop().expect("stop");
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn install_returns_phase_9_error_on_non_windows() {
        let (tx, _rx) = mpsc::channel::<ActivationEvent>();
        let err = MeetingHotkeyInstaller::install(ChordConfig::default(), tx).unwrap_err();
        assert!(format!("{err}").contains("Phase 9"));
    }
}
