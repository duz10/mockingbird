//! Command Center hotkey installer — the **fourth** WH_KEYBOARD_LL
//! hook in the app, on its own message-pump thread.
//!
//! ## Why a fourth hook
//!
//! ADR 0027 already documents that multiple low-level keyboard hooks
//! can coexist iff each one calls `CallNextHookEx`. We need a chord
//! observer that fires on `Right Ctrl + Space` (configurable) and is
//! independent of the dictation push-to-talk + the meeting toggle:
//!
//! - Dictation hook (Right Alt push-to-talk) — binary down/up.
//! - Meeting hook (Right Ctrl + . tap) — chord toggle.
//! - (Wave 1B) Activity hook — deleted; activity invokes via the
//!   Command Center mode picker instead.
//! - **This hook (Right Ctrl + Space tap) — chord observer.**
//!
//! Each runs on its own thread inside its own `GetMessageW` pump.
//! Cost: still sub-microsecond per keystroke per pump.
//!
//! ## Classifier surface
//!
//! The pure classifier mirrors `meetings::hotkey_installer::
//! classify_meeting_keystroke` almost verbatim — same VK semantics,
//! same WM_KEYDOWN / WM_KEYUP set, same "never suppress" rule. The
//! orchestrator above this layer is what knows about the chord vs.
//! the tap-toggle vs. the press-and-hold UX.
//!
//! Single-tap activation is the Command Center's UX: the user taps
//! the chord, the window opens; taps again (or Esc), it closes. We
//! emit a single [`CcActivation`] event on `MainKeyDown` while the
//! modifier is held — the orchestrator does the "is this re-entry?"
//! handling via the state machine.

#![allow(missing_docs)]
// macOS port: live impl is `#[cfg(target_os = "windows")]`; these are orphaned
// on non-Windows until the cross-platform backend lands (Phase 3/4).
#![cfg_attr(not(target_os = "windows"), allow(unused_imports, unused_mut))]

use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::thread::JoinHandle;

use crate::error::{AppError, AppResult};

/// Chord configuration: a modifier VK paired with a main VK. Match the
/// meeting installer's [`ChordConfig`] shape so settings + parsers can
/// share code if a refactor ever pulls them together. (Today they
/// stay distinct because the meetings layer wants to keep its own
/// types under `meetings/` per ADR 0026.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CcChordConfig {
    pub modifier_vk: u32,
    pub main_vk: u32,
}

impl CcChordConfig {
    /// ADR 0037 §Q1 default: `Right Ctrl + Space`.
    /// `0xA3` = `VK_RCONTROL`, `0x20` = `VK_SPACE`.
    pub const fn default_right_ctrl_space() -> Self {
        Self {
            modifier_vk: 0xA3,
            main_vk: 0x20,
        }
    }
}

impl Default for CcChordConfig {
    fn default() -> Self {
        Self::default_right_ctrl_space()
    }
}

/// Single activation event — the chord fired.
///
/// Unlike dictation's push-to-talk (which needs Down + Up to bracket
/// the recording) and unlike meetings (which needs Down + Up to
/// disambiguate tap-toggle vs. press-and-hold), the Command Center
/// is a single discrete event: "the user pressed the chord; open the
/// window." Re-entrance handling lives in the state machine, not
/// here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CcActivation {
    /// Elapsed ms since hook install — useful for log + debounce
    /// on the orchestrator side.
    pub ts_ms: u64,
}

/// Owner of the chord-hook thread. `install()` plants the hook and
/// returns this struct; `stop()` / `Drop` tears it down.
// `Debug` is required by the `not(windows)` unit test that asserts
// `install()` returns a Hotkey error off-Windows (`.unwrap_err()`).
// All fields are `Debug`; no-op for the Windows build.
#[derive(Debug)]
pub struct CommandCenterHotkeyInstaller {
    chord: CcChordConfig,
    stop_signal: Arc<AtomicBool>,
    #[cfg(target_os = "windows")]
    thread_id: u32,
    #[cfg(target_os = "windows")]
    join_handle: Option<JoinHandle<()>>,
}

impl CommandCenterHotkeyInstaller {
    /// Install the WH_KEYBOARD_LL hook on a new thread and return.
    ///
    /// Errors with [`AppError::Phase9`] on non-Windows; the calling
    /// site is `#[cfg(target_os = "windows")]` in production but the
    /// non-Windows branch keeps the cross-platform compile clean.
    #[allow(unused_variables)]
    pub fn install(chord: CcChordConfig, sender: Sender<CcActivation>) -> AppResult<Self> {
        #[cfg(target_os = "windows")]
        {
            install_windows(chord, sender)
        }
        #[cfg(not(target_os = "windows"))]
        {
            // Cross-platform tooling: macOS / Linux ports land in
            // Phase 9. For now the installer reports a hotkey error
            // and the orchestrator falls back to tray-only entry.
            Err(AppError::Hotkey(
                "command_center::hotkey is Windows-only in v1".into(),
            ))
        }
    }

    /// Stop the hook + join the thread. Idempotent.
    pub fn stop(mut self) -> AppResult<()> {
        #[cfg(target_os = "windows")]
        {
            stop_windows(&mut self)?;
        }
        Ok(())
    }

    /// The chord this installer is currently observing.
    pub fn chord(&self) -> CcChordConfig {
        self.chord
    }
}

impl Drop for CommandCenterHotkeyInstaller {
    fn drop(&mut self) {
        // Best-effort: never panic out of Drop. Errors logged.
        self.stop_signal
            .store(true, std::sync::atomic::Ordering::Relaxed);
        #[cfg(target_os = "windows")]
        {
            if let Err(e) = stop_windows(self) {
                tracing::warn!(
                    target: "command_center",
                    error = %e,
                    "CommandCenterHotkeyInstaller::drop saw stop error"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------
// Pure classifier (the entire testable surface)
// ---------------------------------------------------------------------

// Mirror the meetings hook's local copies of WM_* — they're carried
// here as `pub(crate)` so the proc + the tests can share them.
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

/// What we track between keystrokes — just whether the modifier is
/// down right now. Cheap copy; lives in a `RefCell` per-thread on
/// Windows builds.
#[derive(Debug, Clone, Copy, Default)]
pub struct CcChordTracker {
    modifier_down: bool,
}

impl CcChordTracker {
    /// Reset to "no chord state observed yet". Useful at install time.
    pub fn reset(&mut self) {
        self.modifier_down = false;
    }
}

/// Result of feeding one keystroke into the chord tracker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CcKeystrokeOutcome {
    /// No interesting transition; carry on.
    Ignore,
    /// The chord fired (`main` pressed while `modifier` was held).
    /// Emit a [`CcActivation`].
    ChordFired { ts_ms: u64 },
}

/// Feed one low-level keystroke into the tracker.
///
/// Returns [`CcKeystrokeOutcome::ChordFired`] iff the keystroke is a
/// `WM_KEYDOWN` (or `WM_SYSKEYDOWN`) for `main_vk` AND the modifier
/// is currently held. Modifier transitions update tracker state in
/// place; everything else is `Ignore`.
///
/// Pure function. No OS calls. Throwaway-crate testable.
pub fn feed_chord_keystroke(
    tracker: &mut CcChordTracker,
    wm: u32,
    vk_code: u32,
    chord: CcChordConfig,
    ts_ms: u64,
) -> CcKeystrokeOutcome {
    let is_down = matches!(wm, WM_KEYDOWN_U32 | WM_SYSKEYDOWN_U32);
    let is_up = matches!(wm, WM_KEYUP_U32 | WM_SYSKEYUP_U32);
    if !is_down && !is_up {
        return CcKeystrokeOutcome::Ignore;
    }

    if vk_code == chord.modifier_vk {
        // Modifier transition.
        tracker.modifier_down = is_down;
        return CcKeystrokeOutcome::Ignore;
    }

    if vk_code == chord.main_vk && is_down && tracker.modifier_down {
        return CcKeystrokeOutcome::ChordFired { ts_ms };
    }

    CcKeystrokeOutcome::Ignore
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

    // Per-thread storage. Independent of the dictation + meetings
    // hooks' thread_locals — different thread, different RefCell.
    thread_local! {
        pub(super) static CC_TX: RefCell<Option<Sender<CcActivation>>> =
            const { RefCell::new(None) };
        pub(super) static CC_CHORD: RefCell<CcChordConfig> = const {
            RefCell::new(CcChordConfig {
                modifier_vk: 0xA3, main_vk: 0x20,
            })
        };
        pub(super) static CC_TRACKER: RefCell<CcChordTracker> =
            const { RefCell::new(CcChordTracker { modifier_down: false }) };
        pub(super) static CC_HHOOK: RefCell<Option<HHOOK>> =
            const { RefCell::new(None) };
        pub(super) static CC_ANCHOR: RefCell<Option<Instant>> =
            const { RefCell::new(None) };
    }

    pub(super) fn run_thread(chord: CcChordConfig, tx: Sender<CcActivation>, tid_tx: Sender<u32>) {
        let tid = unsafe { GetCurrentThreadId() };
        if tid_tx.send(tid).is_err() {
            return;
        }
        CC_TX.with(|c| *c.borrow_mut() = Some(tx));
        CC_CHORD.with(|c| *c.borrow_mut() = chord);
        CC_TRACKER.with(|c| c.borrow_mut().reset());
        CC_ANCHOR.with(|c| *c.borrow_mut() = Some(Instant::now()));

        let proc: HOOKPROC = Some(low_level_keyboard_proc);
        let hook_res = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, proc, HINSTANCE(0), 0) };
        let hhook = match hook_res {
            Ok(h) => h,
            Err(e) => {
                tracing::error!(
                    target: "command_center",
                    error = %e,
                    "command_center SetWindowsHookEx(WH_KEYBOARD_LL) failed"
                );
                CC_TX.with(|c| c.borrow_mut().take());
                return;
            }
        };
        CC_HHOOK.with(|c| *c.borrow_mut() = Some(hhook));

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

        CC_HHOOK.with(|c| {
            if let Some(h) = c.borrow_mut().take() {
                let _ = unsafe { UnhookWindowsHookEx(h) };
            }
        });
        CC_TX.with(|c| {
            let _ = c.borrow_mut().take();
        });
    }

    /// The hook proc. **Always `CallNextHookEx`** — dictation +
    /// meetings hooks downstream must still see every keystroke. We
    /// only observe (never suppress).
    pub(super) unsafe extern "system" fn low_level_keyboard_proc(
        code: i32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if code < 0 {
            return CallNextHookEx(None, code, wparam, lparam);
        }
        let kbd = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        let chord = CC_CHORD.with(|c| *c.borrow());
        let ts_ms = CC_ANCHOR.with(|c| {
            c.borrow()
                .map(|anchor| anchor.elapsed().as_millis() as u64)
                .unwrap_or(0)
        });
        let outcome = CC_TRACKER.with(|t| {
            let mut tracker = t.borrow_mut();
            feed_chord_keystroke(&mut tracker, wparam.0 as u32, kbd.vkCode, chord, ts_ms)
        });
        if let CcKeystrokeOutcome::ChordFired { ts_ms } = outcome {
            CC_TX.with(|c| {
                if let Some(tx) = c.borrow().as_ref() {
                    let _ = tx.send(CcActivation { ts_ms });
                }
            });
        }
        CallNextHookEx(None, code, wparam, lparam)
    }

    pub(super) fn post_quit(tid: u32) {
        // Best-effort — the thread might already have exited.
        unsafe {
            let _ = PostThreadMessageW(tid, WM_QUIT, WPARAM(0), LPARAM(0));
        }
    }
}

#[cfg(target_os = "windows")]
fn install_windows(
    chord: CcChordConfig,
    sender: Sender<CcActivation>,
) -> AppResult<CommandCenterHotkeyInstaller> {
    let stop_signal = Arc::new(AtomicBool::new(false));
    let (tid_tx, tid_rx) = std::sync::mpsc::channel::<u32>();
    let join_handle = std::thread::Builder::new()
        .name("cc-hotkey".into())
        .spawn(move || win::run_thread(chord, sender, tid_tx))
        .map_err(|e| AppError::Other(format!("spawn command_center hotkey thread: {e}")))?;
    let thread_id = tid_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .map_err(|e| AppError::Other(format!("command_center hotkey TID handshake: {e}")))?;
    tracing::info!(
        target: "command_center",
        thread_id,
        modifier_vk = format!("0x{:02X}", chord.modifier_vk),
        main_vk = format!("0x{:02X}", chord.main_vk),
        "command_center WH_KEYBOARD_LL hook installed"
    );
    Ok(CommandCenterHotkeyInstaller {
        chord,
        stop_signal,
        thread_id,
        join_handle: Some(join_handle),
    })
}

#[cfg(target_os = "windows")]
fn stop_windows(this: &mut CommandCenterHotkeyInstaller) -> AppResult<()> {
    win::post_quit(this.thread_id);
    if let Some(h) = this.join_handle.take() {
        let _ = h.join();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> CcChordTracker {
        CcChordTracker::default()
    }

    const CHORD: CcChordConfig = CcChordConfig {
        modifier_vk: 0xA3,
        main_vk: 0x20,
    };

    #[test]
    fn default_chord_is_right_ctrl_space() {
        let c = CcChordConfig::default();
        assert_eq!(c.modifier_vk, 0xA3);
        assert_eq!(c.main_vk, 0x20);
    }

    #[test]
    fn modifier_down_alone_does_not_fire() {
        let mut t = fresh();
        let r = feed_chord_keystroke(&mut t, WM_KEYDOWN_U32, 0xA3, CHORD, 0);
        assert_eq!(r, CcKeystrokeOutcome::Ignore);
        assert!(t.modifier_down);
    }

    #[test]
    fn main_key_alone_does_not_fire() {
        let mut t = fresh();
        let r = feed_chord_keystroke(&mut t, WM_KEYDOWN_U32, 0x20, CHORD, 100);
        assert_eq!(r, CcKeystrokeOutcome::Ignore);
    }

    #[test]
    fn modifier_then_main_fires() {
        let mut t = fresh();
        feed_chord_keystroke(&mut t, WM_KEYDOWN_U32, 0xA3, CHORD, 0);
        let r = feed_chord_keystroke(&mut t, WM_KEYDOWN_U32, 0x20, CHORD, 50);
        assert_eq!(r, CcKeystrokeOutcome::ChordFired { ts_ms: 50 });
    }

    #[test]
    fn main_up_does_not_fire() {
        let mut t = fresh();
        feed_chord_keystroke(&mut t, WM_KEYDOWN_U32, 0xA3, CHORD, 0);
        let r = feed_chord_keystroke(&mut t, WM_KEYUP_U32, 0x20, CHORD, 50);
        assert_eq!(r, CcKeystrokeOutcome::Ignore);
    }

    #[test]
    fn modifier_release_clears_tracker() {
        let mut t = fresh();
        feed_chord_keystroke(&mut t, WM_KEYDOWN_U32, 0xA3, CHORD, 0);
        feed_chord_keystroke(&mut t, WM_KEYUP_U32, 0xA3, CHORD, 50);
        assert!(!t.modifier_down);
        let r = feed_chord_keystroke(&mut t, WM_KEYDOWN_U32, 0x20, CHORD, 100);
        assert_eq!(r, CcKeystrokeOutcome::Ignore);
    }

    #[test]
    fn syskey_variants_classify_too() {
        // Alt-modified system keys come through WM_SYSKEYDOWN.
        let mut t = fresh();
        feed_chord_keystroke(&mut t, WM_SYSKEYDOWN_U32, 0xA3, CHORD, 0);
        let r = feed_chord_keystroke(&mut t, WM_SYSKEYDOWN_U32, 0x20, CHORD, 10);
        assert_eq!(r, CcKeystrokeOutcome::ChordFired { ts_ms: 10 });
    }

    #[test]
    fn unrelated_vk_is_ignored() {
        let mut t = fresh();
        feed_chord_keystroke(&mut t, WM_KEYDOWN_U32, 0xA3, CHORD, 0);
        let r = feed_chord_keystroke(&mut t, WM_KEYDOWN_U32, 0x41, CHORD, 50); // 'A'
        assert_eq!(r, CcKeystrokeOutcome::Ignore);
    }

    #[test]
    fn unrelated_wm_message_is_ignored() {
        let mut t = fresh();
        // Anything that's not KEYDOWN/KEYUP/SYSKEYDOWN/SYSKEYUP.
        let r = feed_chord_keystroke(&mut t, 0x200 /* WM_MOUSEMOVE */, 0xA3, CHORD, 0);
        assert_eq!(r, CcKeystrokeOutcome::Ignore);
        assert!(!t.modifier_down);
    }

    #[test]
    fn chord_can_be_remapped() {
        // RShift + F13.
        let chord = CcChordConfig {
            modifier_vk: 0xA1,
            main_vk: 0x7C,
        };
        let mut t = fresh();
        feed_chord_keystroke(&mut t, WM_KEYDOWN_U32, 0xA1, chord, 0);
        let r = feed_chord_keystroke(&mut t, WM_KEYDOWN_U32, 0x7C, chord, 50);
        assert_eq!(r, CcKeystrokeOutcome::ChordFired { ts_ms: 50 });
    }

    #[test]
    fn repeat_chord_fires_each_time() {
        // Holding modifier + tapping main repeatedly should fire on
        // each tap (the chord-fired event is *down*; up is ignored).
        let mut t = fresh();
        feed_chord_keystroke(&mut t, WM_KEYDOWN_U32, 0xA3, CHORD, 0);
        let r1 = feed_chord_keystroke(&mut t, WM_KEYDOWN_U32, 0x20, CHORD, 50);
        let _ = feed_chord_keystroke(&mut t, WM_KEYUP_U32, 0x20, CHORD, 70);
        let r2 = feed_chord_keystroke(&mut t, WM_KEYDOWN_U32, 0x20, CHORD, 100);
        assert_eq!(r1, CcKeystrokeOutcome::ChordFired { ts_ms: 50 });
        assert_eq!(r2, CcKeystrokeOutcome::ChordFired { ts_ms: 100 });
    }

    #[test]
    fn tracker_reset_clears_modifier_state() {
        let mut t = fresh();
        feed_chord_keystroke(&mut t, WM_KEYDOWN_U32, 0xA3, CHORD, 0);
        t.reset();
        let r = feed_chord_keystroke(&mut t, WM_KEYDOWN_U32, 0x20, CHORD, 50);
        assert_eq!(r, CcKeystrokeOutcome::Ignore);
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn install_returns_hotkey_error_on_non_windows() {
        let (tx, _rx) = std::sync::mpsc::channel();
        let err = CommandCenterHotkeyInstaller::install(CcChordConfig::default(), tx).unwrap_err();
        match err {
            AppError::Hotkey(_) => {}
            other => panic!("expected Hotkey error, got {other:?}"),
        }
    }
}
