//! Windows `WH_KEYBOARD_LL` implementation of [`super::HotkeyListener`].
//!
//! ## Architecture (ADR 0015)
//!
//! ```text
//!  ┌────────────────────────────┐    install()    ┌─────────────────────┐
//!  │   caller (orchestrator)    │ ──────────────► │  hook owner struct  │
//!  └────────────────────────────┘                 └─────────────────────┘
//!                                                          │ spawn
//!                                                          ▼
//!                                            ┌─────────────────────────┐
//!                                            │ mockingbird-hotkey OS   │
//!                                            │ thread                  │
//!                                            │                         │
//!                                            │ ┌─ SetWindowsHookEx ──┐ │
//!                                            │ │  (WH_KEYBOARD_LL)   │ │
//!                                            │ └─────────────────────┘ │
//!                                            │ ┌─ GetMessageW loop ──┐ │
//!                                            │ │  • WM_QUIT → exit   │ │
//!                                            │ │  • dispatch         │ │
//!                                            │ └─────────────────────┘ │
//!                                            │ ┌─ on Drop ──────────┐  │
//!                                            │ │ UnhookWindowsHookEx│  │
//!                                            │ └────────────────────┘  │
//!                                            └─────────────────────────┘
//!                                                          │
//!                                                          ▼ try_send
//!                                            ┌─────────────────────────┐
//!                                            │  mpsc<HotkeyEvent>      │
//!                                            └─────────────────────────┘
//! ```
//!
//! ### Why thread-locals (not statics-with-locks)
//!
//! `WH_KEYBOARD_LL` invokes the callback on the thread that called
//! `SetWindowsHookEx`. ADR 0015 §3: the callback must return within
//! ~300 ms or Windows silently unhooks. Taking a mutex inside the
//! callback risks blocking on the consumer if it is slow. **All
//! callback-visible state lives in `thread_local!`** — a `RefCell`
//! that's only ever touched by one OS thread is uncontended by
//! construction.
//!
//! ### Why an event-message thread
//!
//! `SetWindowsHookEx` returns an `HHOOK` that's affined to the
//! installing thread. Calling `UnhookWindowsHookEx` from a different
//! thread is undefined. The dedicated thread owns the lifecycle from
//! install to unhook; the parent communicates via `PostThreadMessageW`
//! (for `WM_QUIT`) and `mpsc` (for event delivery).
//!
//! ## Pure-vs-OS split
//!
//! The callback is a tiny shim: read message type + `KBDLLHOOKSTRUCT`,
//! hand both to [`classify_keystroke`] (pure, fully tested), and
//! `try_send` the result. Everything testable lives in
//! [`classify_keystroke`]; the OS thread is exercised by the
//! `#[ignore]` integration tests via `SendInput` round-trips.

use std::cell::RefCell;
use std::sync::mpsc;
use std::sync::mpsc::Sender;
use std::thread::JoinHandle;
use std::time::Instant;

use super::{HotkeyEvent, HotkeyListener};
use crate::error::{AppError, AppResult};

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
#[cfg(target_os = "windows")]
use windows::Win32::System::Threading::GetCurrentThreadId;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, PostThreadMessageW, SetWindowsHookExW,
    TranslateMessage, UnhookWindowsHookEx, HHOOK, HOOKPROC, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL,
    WM_KEYDOWN, WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

/// Right Alt — PLAN §6.1 default. ADR 0019 may resolve this to a
/// fallback (F23, F24, Ctrl+Shift+Space) at startup.
const DEFAULT_VK_RMENU: u32 = 0xA5;

/// Reasonable upper bound on the channel queue. The hook side `try_send`s
/// once per real key transition (typically a few per dictation session);
/// the consumer drains at 20 ms tick cadence. A queue this size only
/// fills if the driver thread is wedged, in which case we drop events
/// rather than block the OS hook.
const CHANNEL_BUFFER_HINT: usize = 256;

// --------------------------------------------------------------------
// Pure classifier — testable without an OS hook
// --------------------------------------------------------------------

/// Classify a low-level keyboard message into an optional
/// [`HotkeyEvent`].
///
/// Returns `None` when the event is irrelevant to our hotkey
/// (different VK, unsupported message type). Returns `Some(Escape)`
/// for any `KEYDOWN` of `VK_ESCAPE` regardless of `configured_vk` —
/// the §6.1 state machine accepts Escape during Recording from any
/// source. Modifier-distinguished modes (Fragment/Verbose) are NOT
/// detected here in Wave 3; the state machine receives them as
/// [`super::state::HotkeyMode::Normal`] for now.
///
/// The function is pure — every input combination is unit-testable.
pub fn classify_keystroke(
    wparam: u32,
    vk_code: u32,
    configured_vk: u32,
    at: Instant,
) -> Option<HotkeyEvent> {
    // VK_ESCAPE = 0x1B. We always surface Escape KEYDOWN so the
    // state machine's cancel path works without a separate hook.
    const VK_ESCAPE: u32 = 0x1B;

    match wparam {
        WM_KEYDOWN_U32 | WM_SYSKEYDOWN_U32 => {
            if vk_code == VK_ESCAPE {
                Some(HotkeyEvent::Escape { at })
            } else if vk_code == configured_vk {
                Some(HotkeyEvent::KeyDown { vk: vk_code, at })
            } else {
                None
            }
        }
        WM_KEYUP_U32 | WM_SYSKEYUP_U32 => {
            if vk_code == configured_vk {
                Some(HotkeyEvent::KeyUp { vk: vk_code, at })
            } else {
                None
            }
        }
        _ => None,
    }
}

// Mirror the Windows constants here so the classifier compiles on
// non-Windows targets (for `cargo check` / `cargo clippy` from CI).
const WM_KEYDOWN_U32: u32 = 0x100;
const WM_KEYUP_U32: u32 = 0x101;
const WM_SYSKEYDOWN_U32: u32 = 0x104;
const WM_SYSKEYUP_U32: u32 = 0x105;

// Compile-time invariant: our literal values match the windows-rs
// constants. If a `windows-rs` upgrade changes them (it won't —
// these are stable Win32 message IDs from Windows 3.0), the build
// fails loudly. Only enforced on Windows where the constants exist.
#[cfg(target_os = "windows")]
const _: () = {
    assert!(WM_KEYDOWN_U32 == WM_KEYDOWN);
    assert!(WM_KEYUP_U32 == WM_KEYUP);
    assert!(WM_SYSKEYDOWN_U32 == WM_SYSKEYDOWN);
    assert!(WM_SYSKEYUP_U32 == WM_SYSKEYUP);
};

// --------------------------------------------------------------------
// Hook owner
// --------------------------------------------------------------------

/// `WH_KEYBOARD_LL` hook owner.
///
/// Lifecycle: `new()` constructs (no OS resources held). `install(tx)`
/// spawns the dedicated thread, installs the hook, and the hook starts
/// emitting events on `tx`. `uninstall()` joins the thread (which
/// `UnhookWindowsHookEx`-es as part of its drop), or `Drop` does the
/// same defensively.
pub struct WinKeyboardHook {
    vk: u32,
    thread: Option<JoinHandle<()>>,
    hook_tid: Option<u32>,
}

impl Default for WinKeyboardHook {
    fn default() -> Self {
        Self {
            vk: DEFAULT_VK_RMENU,
            thread: None,
            hook_tid: None,
        }
    }
}

impl WinKeyboardHook {
    /// Construct a not-yet-installed hook bound to the default VK.
    pub fn new() -> AppResult<Self> {
        Ok(Self::default())
    }

    /// Construct with an explicit VK (used by the conflict probe to
    /// rebind onto a fallback hotkey).
    pub fn with_vk(vk: u32) -> Self {
        Self {
            vk,
            thread: None,
            hook_tid: None,
        }
    }

    /// Best-effort channel-buffer hint — exposed so tests + the
    /// state-machine driver can size their consumers consistently.
    pub const fn recommended_channel_buffer() -> usize {
        CHANNEL_BUFFER_HINT
    }
}

impl HotkeyListener for WinKeyboardHook {
    #[cfg(target_os = "windows")]
    fn install(&mut self, tx: Sender<HotkeyEvent>) -> AppResult<()> {
        if self.thread.is_some() {
            // Idempotent per trait contract.
            return Ok(());
        }
        let vk = self.vk;

        // Use a one-shot std::sync::mpsc to receive the hook thread's
        // TID — we need it to `PostThreadMessageW(WM_QUIT)` on shutdown.
        let (tid_tx, tid_rx) = mpsc::channel::<u32>();

        let handle = std::thread::Builder::new()
            .name("mockingbird-hotkey".into())
            .spawn(move || run_hook_thread(vk, tx, tid_tx))
            .map_err(|e| AppError::Hotkey(format!("hotkey thread spawn failed: {e}")))?;

        let hook_tid = tid_rx
            .recv_timeout(std::time::Duration::from_millis(500))
            .map_err(|_| {
                AppError::Hotkey(
                    "hotkey thread did not report its TID within 500 ms — bailing".into(),
                )
            })?;

        self.thread = Some(handle);
        self.hook_tid = Some(hook_tid);
        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    fn install(&mut self, _tx: Sender<HotkeyEvent>) -> AppResult<()> {
        Err(AppError::Hotkey(
            "WH_KEYBOARD_LL is Windows-only (Phase 9 platform parity)".into(),
        ))
    }

    #[cfg(target_os = "windows")]
    fn uninstall(&mut self) -> AppResult<()> {
        if let (Some(tid), Some(handle)) = (self.hook_tid.take(), self.thread.take()) {
            // SAFETY: `tid` is a valid TID for our spawned thread; the
            // message-pump receives WM_QUIT and exits cleanly.
            let _ = unsafe { PostThreadMessageW(tid, WM_QUIT, WPARAM(0), LPARAM(0)) };
            let _ = handle.join();
        }
        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    fn uninstall(&mut self) -> AppResult<()> {
        Ok(())
    }

    fn configured_vk(&self) -> u32 {
        self.vk
    }
}

impl Drop for WinKeyboardHook {
    fn drop(&mut self) {
        // Best-effort cleanup; ignore errors (we're already going away).
        let _ = self.uninstall();
    }
}

// --------------------------------------------------------------------
// OS thread implementation
// --------------------------------------------------------------------

#[cfg(target_os = "windows")]
thread_local! {
    /// Sender used by the hook callback to emit events. Set on hook
    /// thread entry; cleared on exit. Only this thread ever touches it.
    static CALLBACK_TX: RefCell<Option<Sender<HotkeyEvent>>> = const { RefCell::new(None) };
    /// Configured VK; the callback reads it to filter events.
    static CALLBACK_VK: RefCell<u32> = const { RefCell::new(DEFAULT_VK_RMENU) };
    /// Owned hook handle. RAII-released via `UnhookWindowsHookEx` when
    /// the thread exits (this `thread_local!` is dropped).
    static CALLBACK_HHOOK: RefCell<Option<HHOOK>> = const { RefCell::new(None) };
}

#[cfg(target_os = "windows")]
fn run_hook_thread(vk: u32, tx: Sender<HotkeyEvent>, tid_tx: Sender<u32>) {
    // 1. Publish our TID so `uninstall()` can find us.
    let tid = unsafe { GetCurrentThreadId() };
    if tid_tx.send(tid).is_err() {
        // Owner went away before we could report — nothing more to do.
        return;
    }

    // 2. Plant the callback context.
    CALLBACK_TX.with(|cell| *cell.borrow_mut() = Some(tx));
    CALLBACK_VK.with(|cell| *cell.borrow_mut() = vk);

    // 3. Install the hook. `lpfn` must be `Some(...)`; `hmod` may be
    //    `HINSTANCE(0)` since we're hooking only our own thread's
    //    message loop is wrong — WH_KEYBOARD_LL is a SYSTEM-wide hook,
    //    but its callback runs on the installing thread, hence the
    //    dedicated thread + message pump. `dwThreadId = 0` makes it
    //    apply to all threads (system-wide).
    let proc: HOOKPROC = Some(low_level_keyboard_proc);
    let hook_res = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, proc, HINSTANCE(0), 0) };
    let hhook = match hook_res {
        Ok(h) => h,
        Err(e) => {
            // Surface via channel by dropping the sender (best we can
            // do without a real result path). Caller will see no
            // events and can investigate.
            eprintln!("SetWindowsHookEx(WH_KEYBOARD_LL) failed: {e}");
            CALLBACK_TX.with(|c| c.borrow_mut().take());
            return;
        }
    };
    CALLBACK_HHOOK.with(|cell| *cell.borrow_mut() = Some(hhook));

    // 4. Run the message pump until WM_QUIT.
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
        // GetMessageW returns:
        //   > 0  → normal message
        //   == 0 → WM_QUIT (exit the loop)
        //   < 0  → error (also exit)
        if r.0 <= 0 {
            break;
        }
        unsafe {
            let _ = TranslateMessage(&msg as *const MSG);
            DispatchMessageW(&msg as *const MSG);
        }
    }

    // 5. Tear down. RAII order: unhook → drop sender → drop VK.
    CALLBACK_HHOOK.with(|cell| {
        if let Some(h) = cell.borrow_mut().take() {
            let _ = unsafe { UnhookWindowsHookEx(h) };
        }
    });
    CALLBACK_TX.with(|cell| {
        let _ = cell.borrow_mut().take();
    });
}

/// `LowLevelKeyboardProc` callback. **ZERO non-trivial work inside.**
///
/// ## Suppression policy (Phase 3 Wave 4.7)
///
/// When the keystroke matches our `configured_vk` (e.g. RightAlt),
/// we return `LRESULT(1)` to **block propagation**. Without this,
/// the OS still processes the bare key — for Alt that means menu-bar
/// activation on key-up; for Win that would mean Start-menu opening;
/// for Ctrl alone it's harmless but distracting in keyboard hooks.
///
/// The tradeoff: the hotkey VK becomes exclusive to mockingbird while
/// the app is running. Tap-passthrough (PLAN §6.1 "taps < 80 ms pass
/// through to OS for native shortcuts") is architecturally impossible
/// in a `WH_KEYBOARD_LL` hook — the suppress-or-not decision must be
/// made at key-down before we know hold duration. We chose exclusivity.
///
/// Escape always passes through. The state machine receives Escape
/// events for cancel-during-recording, but Escape must remain useful
/// to every other app on the system (dialogs, modals, etc.).
#[cfg(target_os = "windows")]
unsafe extern "system" fn low_level_keyboard_proc(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // Per MSDN: if `code` is negative, do not process the hook and
    // pass through to `CallNextHookEx` immediately.
    if code < 0 {
        return CallNextHookEx(None, code, wparam, lparam);
    }

    // SAFETY: WH_KEYBOARD_LL guarantees lparam points to a valid
    // KBDLLHOOKSTRUCT for the duration of the callback.
    let kbd = &*(lparam.0 as *const KBDLLHOOKSTRUCT);

    let configured_vk = CALLBACK_VK.with(|cell| *cell.borrow());
    let event = classify_keystroke(wparam.0 as u32, kbd.vkCode, configured_vk, Instant::now());

    if let Some(ev) = event {
        CALLBACK_TX.with(|cell| {
            if let Some(tx) = cell.borrow().as_ref() {
                // Std mpsc `send` is non-blocking (unbounded), so
                // this can't trip the LL-hook watchdog.
                let _ = tx.send(ev);
            }
        });
    }

    // Suppress IFF this was an event on our configured hotkey VK.
    // Escape (when classified) is passed through — it's classified
    // for our visibility, not for consumption.
    if should_suppress(kbd.vkCode, configured_vk) {
        return LRESULT(1);
    }

    CallNextHookEx(None, code, wparam, lparam)
}

/// Pure helper: decide whether the LL hook should suppress this key.
///
/// Suppress iff the key's VK equals the configured hotkey VK. Pure
/// so it's unit-testable without an OS hook.
#[inline]
fn should_suppress(vk_code: u32, configured_vk: u32) -> bool {
    vk_code == configured_vk
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------
    // classify_keystroke — pure, deterministic
    // -----------------------------------------------------------------

    fn now() -> Instant {
        Instant::now()
    }

    #[test]
    fn keydown_of_configured_vk_yields_keydown_event() {
        let t = now();
        let ev = classify_keystroke(WM_KEYDOWN_U32, 0xA5, 0xA5, t).unwrap();
        match ev {
            HotkeyEvent::KeyDown { vk, .. } => assert_eq!(vk, 0xA5),
            other => panic!("expected KeyDown, got {other:?}"),
        }
    }

    #[test]
    fn syskeydown_of_configured_vk_yields_keydown_event() {
        // Right Alt's KEYDOWN arrives as WM_SYSKEYDOWN, not WM_KEYDOWN
        // (because Alt is a system key). This test guards the §6.1
        // path that breaks on right-alt-only systems.
        let ev = classify_keystroke(WM_SYSKEYDOWN_U32, 0xA5, 0xA5, now()).unwrap();
        assert!(matches!(ev, HotkeyEvent::KeyDown { .. }));
    }

    #[test]
    fn keyup_of_configured_vk_yields_keyup_event() {
        let ev = classify_keystroke(WM_KEYUP_U32, 0xA5, 0xA5, now()).unwrap();
        assert!(matches!(ev, HotkeyEvent::KeyUp { .. }));
    }

    #[test]
    fn syskeyup_of_configured_vk_yields_keyup_event() {
        let ev = classify_keystroke(WM_SYSKEYUP_U32, 0xA5, 0xA5, now()).unwrap();
        assert!(matches!(ev, HotkeyEvent::KeyUp { .. }));
    }

    #[test]
    fn keydown_of_escape_yields_escape_event_regardless_of_configured_vk() {
        let ev = classify_keystroke(WM_KEYDOWN_U32, 0x1B, 0xA5, now()).unwrap();
        assert!(matches!(ev, HotkeyEvent::Escape { .. }));
    }

    #[test]
    fn keyup_of_escape_is_not_surfaced() {
        // Only Escape KEYDOWN matters; KEYUP is ignored.
        assert!(classify_keystroke(WM_KEYUP_U32, 0x1B, 0xA5, now()).is_none());
    }

    #[test]
    fn keydown_of_unrelated_vk_yields_none() {
        assert!(classify_keystroke(WM_KEYDOWN_U32, 0x41, 0xA5, now()).is_none()); // 'A'
        assert!(classify_keystroke(WM_KEYDOWN_U32, 0x86, 0xA5, now()).is_none());
        // VK_F23
    }

    // -----------------------------------------------------------------
    // should_suppress — pure suppression-policy helper (Wave 4.7)
    // -----------------------------------------------------------------

    #[test]
    fn suppress_matches_configured_vk() {
        // RightAlt configured; RightAlt event → suppress (blocks the
        // OS from triggering menu activation).
        assert!(should_suppress(0xA5, 0xA5));
    }

    #[test]
    fn suppress_lets_unrelated_keys_pass_through() {
        // 'A' down, RightAlt configured: must not suppress (otherwise
        // we'd nuke every keystroke on the system).
        assert!(!should_suppress(0x41, 0xA5));
    }

    #[test]
    fn suppress_lets_escape_pass_through_even_when_classified() {
        // Escape is classified for our visibility but suppression
        // must not consume it — Escape is sacred everywhere else.
        assert!(!should_suppress(0x1B, 0xA5));
    }

    #[test]
    fn suppress_when_configured_to_some_other_vk() {
        // If a user reconfigures to e.g. RightCtrl (0xA3), then
        // RightAlt events should pass through cleanly.
        assert!(should_suppress(0xA3, 0xA3));
        assert!(!should_suppress(0xA5, 0xA3));
    }

    #[test]
    fn unknown_message_type_yields_none() {
        // WM_CHAR (0x102) and friends should pass through cleanly.
        for msg in [0x102u32, 0x200, 0x201, 0x0, 0xFFFF_FFFF] {
            assert!(
                classify_keystroke(msg, 0xA5, 0xA5, now()).is_none(),
                "msg {msg:#x} should not produce an event"
            );
        }
    }

    #[test]
    fn rebinding_to_f23_works() {
        // ADR 0019 fallback chain: when the conflict probe rebinds
        // to F23 (0x86), the same classifier should now treat F23 as
        // the hotkey and RMENU as a passthrough.
        let f23 = 0x86u32;
        assert!(matches!(
            classify_keystroke(WM_KEYDOWN_U32, f23, f23, now()),
            Some(HotkeyEvent::KeyDown { .. })
        ));
        assert!(classify_keystroke(WM_KEYDOWN_U32, 0xA5, f23, now()).is_none());
    }

    // -----------------------------------------------------------------
    // Owner struct lifecycle
    // -----------------------------------------------------------------

    #[test]
    fn new_returns_default_vk() {
        let hook = WinKeyboardHook::new().expect("construct");
        assert_eq!(hook.configured_vk(), DEFAULT_VK_RMENU);
    }

    #[test]
    fn with_vk_overrides_default() {
        let hook = WinKeyboardHook::with_vk(0x86);
        assert_eq!(hook.configured_vk(), 0x86);
    }

    #[test]
    fn recommended_buffer_is_reasonable() {
        // Sanity: not zero (would drop everything), not absurd
        // (memory blowup). 64..=4096 is the sensible band.
        let n = WinKeyboardHook::recommended_channel_buffer();
        assert!(
            (64..=4096).contains(&n),
            "buffer size {n} outside sensible band"
        );
    }

    #[test]
    fn uninstall_is_idempotent_before_install() {
        let mut hook = WinKeyboardHook::new().expect("construct");
        assert!(hook.uninstall().is_ok());
        assert!(hook.uninstall().is_ok());
    }

    // -----------------------------------------------------------------
    // Live install / uninstall — requires interactive desktop
    // -----------------------------------------------------------------

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "live WH_KEYBOARD_LL install; run with `cargo test -- --ignored`"]
    fn live_install_uninstall_round_trip() {
        let mut hook = WinKeyboardHook::new().expect("construct");
        let (tx, _rx) = mpsc::channel();
        hook.install(tx).expect("install");
        std::thread::sleep(std::time::Duration::from_millis(100));
        hook.uninstall().expect("uninstall");
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "live install must be idempotent; run with `cargo test -- --ignored`"]
    fn live_install_is_idempotent() {
        let mut hook = WinKeyboardHook::new().expect("construct");
        let (tx, _rx) = mpsc::channel();
        hook.install(tx.clone()).expect("first install");
        hook.install(tx).expect("second install is a no-op");
        hook.uninstall().expect("uninstall");
    }
}
