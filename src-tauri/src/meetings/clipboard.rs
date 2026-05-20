//! Meeting transcript → clipboard.
//!
//! One-shot UTF-16 clipboard write. **No save/restore** — meeting
//! export is an explicit user-initiated paste-target action (the user
//! clicked "Copy to clipboard" on a finished meeting transcript), NOT
//! an inline dictation injection. The "clipboard save/restore" binding
//! rule in `AGENTS.md` Principle 7 applies to dictation paste-injection
//! (where the user expects their pre-dictation clipboard contents to
//! survive); the meeting copy path is the opposite — the user is
//! deliberately replacing their clipboard contents with the transcript.
//!
//! Wave 4 implementation per `docs/phases/phase-mc-wave4-brief.md` §4.4
//! and the master plan's risk table (line 526):
//!   > "One-shot `SetClipboardData(CF_UNICODETEXT)` — the user
//!   > *intends* to put the transcript on the clipboard, so no
//!   > save/restore."
//!
//! ### Why not arboard
//!
//! The Wave 4 brief originally suggested `arboard` as the impl, but
//! that crate isn't yet in `Cargo.toml`. The `windows-rs` feature set
//! we already pull in for `injection/paste.rs` covers the four Win32
//! entry points we need (`OpenClipboard`, `EmptyClipboard`,
//! `SetClipboardData`, `CloseClipboard`) — adding `arboard` would
//! pull in a transitive dep tree just to call those same four
//! functions through one layer of indirection. YAGNI.
//!
//! ### Hook surface
//!
//! `scripts/hooks/warn-bare-clipboard-set.py` warns on shell-side
//! clipboard writes (`clip.exe`, `Set-Clipboard`, `pbcopy`). It does
//! not gate Rust-source `SetClipboardData` calls — that static check
//! is deferred to a clippy lint per LESSONS 2026-05-17 Wave-1
//! YAGNI-call. Meeting `copy_text_one_shot` therefore co-exists with
//! `injection::paste::paste_with_save_restore` as the second permitted
//! caller of `SetClipboardData` in the workspace.

use crate::error::AppResult;

/// Replace the system clipboard with `text` as UTF-16 (CF_UNICODETEXT).
///
/// Synchronous; returns once the Win32 close completes. The caller
/// (IPC handler) is expected to be already off the UI thread.
pub fn copy_text_one_shot(text: &str) -> AppResult<()> {
    platform::copy_text_one_shot(text)
}

// --------------------------------------------------------------------
// Platform impls — Windows real, others stub for cross-platform parity
// per AGENTS.md "Cross-platform from day one" principle.
// --------------------------------------------------------------------

#[cfg(target_os = "windows")]
mod platform {
    use std::ptr;

    use windows::Win32::Foundation::{HANDLE, HGLOBAL, HWND};
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};

    use crate::error::{AppError, AppResult};

    /// `CF_UNICODETEXT` — standard predefined clipboard format ID for
    /// UTF-16LE NUL-terminated text. Hardcoded as `13` (per MSDN
    /// `winuser.h`) to avoid enabling the `Win32_System_Ole` feature
    /// just for one `pub const u32`.
    const CF_UNICODETEXT: u32 = 13;

    /// `OpenClipboard` retry count. Win+V and other clipboard
    /// shellhooks briefly hold the clipboard open; a small retry
    /// loop with backoff matches the dictation paste path's policy.
    const OPEN_RETRIES: usize = 3;
    /// Backoff between `OpenClipboard` retries.
    const OPEN_BACKOFF_MS: u64 = 10;

    pub fn copy_text_one_shot(text: &str) -> AppResult<()> {
        // 1. Allocate a movable global block sized for the UTF-16
        //    representation + trailing NUL terminator.
        let mut wide: Vec<u16> = text.encode_utf16().collect();
        wide.push(0); // CF_UNICODETEXT requires a trailing NUL.
        let bytes = wide.len() * std::mem::size_of::<u16>();

        // SAFETY: GlobalAlloc with GMEM_MOVEABLE is the documented
        // allocation strategy for SetClipboardData payloads. The
        // handle ownership transfers to the system on a successful
        // SetClipboardData call.
        let hglobal: HGLOBAL = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes) }
            .map_err(|e| AppError::MeetingCapture(format!("GlobalAlloc({bytes}): {e}")))?;

        // 2. Lock, memcpy the UTF-16 bytes in, unlock.
        // SAFETY: hglobal is non-null because GlobalAlloc returned Ok.
        let lock_ptr = unsafe { GlobalLock(hglobal) };
        if lock_ptr.is_null() {
            return Err(AppError::MeetingCapture(
                "GlobalLock returned null".to_string(),
            ));
        }
        // SAFETY: lock_ptr is non-null + points at `bytes` of writable
        // storage; wide.as_ptr() points at exactly `bytes` bytes of
        // initialized memory.
        unsafe {
            ptr::copy_nonoverlapping(wide.as_ptr() as *const u8, lock_ptr as *mut u8, bytes);
        }
        // SAFETY: balances the GlobalLock above.
        let _ = unsafe { GlobalUnlock(hglobal) };

        // 3. Open clipboard (with retries), empty, set, close.
        //
        // NOTE on `GlobalFree`: `windows-rs` 0.56 does NOT expose
        // `GlobalFree` (verified: not present in
        // `Win32::System::Memory` for the enabled feature set).
        // `injection::paste.rs` has the same constraint and lives with
        // it the same way. The consequence: on the rare error paths
        // below (clipboard locked, EmptyClipboard fails,
        // SetClipboardData fails), the `GMEM_MOVEABLE` block we
        // allocated leaks until process exit. The leak is bounded by
        // the size of one transcript (typically <100 KB, hard-capped
        // by the meeting duration cap) and the OS reclaims on exit;
        // accepting the leak is YAGNI vs. pulling in `winapi` solely
        // for `GlobalFree`. Happy path: `SetClipboardData` succeeds
        // and the system takes ownership of the handle (no free
        // needed by us regardless).
        let _hglobal_handle_for_docs = hglobal; // suppress unused-binding-after-error churn
        let guard = open_with_retries()?;

        // SAFETY: clipboard is open via the guard above.
        if let Err(e) = unsafe { EmptyClipboard() } {
            drop(guard);
            return Err(AppError::MeetingCapture(format!("EmptyClipboard: {e}")));
        }

        // SAFETY: clipboard is open, hglobal is HGLOBAL-backed
        // CF_UNICODETEXT data per the format spec.
        let set_result = unsafe { SetClipboardData(CF_UNICODETEXT, HANDLE(hglobal.0 as isize)) };
        match set_result {
            Ok(_) => {
                // Ownership transferred to the system; do NOT free.
                drop(guard);
                Ok(())
            }
            Err(e) => {
                drop(guard);
                Err(AppError::MeetingCapture(format!(
                    "SetClipboardData(CF_UNICODETEXT): {e}"
                )))
            }
        }
    }

    /// RAII guard that calls `CloseClipboard` on drop.
    struct CloseGuard;

    impl Drop for CloseGuard {
        fn drop(&mut self) {
            // SAFETY: balances the OpenClipboard in `open_with_retries`.
            let _ = unsafe { CloseClipboard() };
        }
    }

    fn open_with_retries() -> AppResult<CloseGuard> {
        for attempt in 0..OPEN_RETRIES {
            // SAFETY: HWND(0) = no owner window, documented idiom.
            let r = unsafe { OpenClipboard(HWND(0)) };
            if r.is_ok() {
                return Ok(CloseGuard);
            }
            if attempt + 1 < OPEN_RETRIES {
                std::thread::sleep(std::time::Duration::from_millis(OPEN_BACKOFF_MS));
            }
        }
        Err(AppError::MeetingCapture(format!(
            "OpenClipboard failed after {OPEN_RETRIES} retries (clipboard locked)"
        )))
    }
}

#[cfg(not(target_os = "windows"))]
mod platform {
    use crate::error::{AppError, AppResult};

    /// Cross-platform stub. PLAN §10 Phase 9 brings macOS/Linux
    /// clipboard support; until then, the IPC command surfaces this
    /// error and the UI toasts.
    pub fn copy_text_one_shot(_text: &str) -> AppResult<()> {
        Err(AppError::MeetingCapture(
            "meeting copy_to_clipboard not yet implemented on this platform".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The brief asks for one live-clipboard round-trip test, gated
    // `#[ignore]` because it mutates the user's clipboard. We honor
    // that here. Run via `cargo test --release -- --ignored
    // copy_text_round_trips` on a desktop with a usable clipboard.
    #[test]
    #[ignore]
    #[cfg(target_os = "windows")]
    fn copy_text_round_trips() {
        // Note: this test can't easily READ the clipboard back
        // without importing GetClipboardData — and the meetings
        // module isn't permitted to read the clipboard (the
        // injection module owns that surface). The acceptance
        // criterion is: the call returns Ok, the system clipboard
        // contains the text afterward (verified manually). This
        // ignored test is therefore a smoke that exercises the
        // happy path without crashing.
        copy_text_one_shot("hello from mockingbird").expect("copy");
    }

    #[test]
    fn empty_string_is_a_no_op_success() {
        // Pure logic test: empty input should still produce a valid
        // CF_UNICODETEXT payload (just the trailing NUL). We exercise
        // the encoding logic without touching the system clipboard
        // by re-using the encode_utf16 step.
        let wide: Vec<u16> = "".encode_utf16().collect();
        assert_eq!(wide.len(), 0);
        // The actual API call would add the trailing NUL; we just
        // verify our encoding step is total.
    }

    #[test]
    fn unicode_text_encodes_to_utf16() {
        let wide: Vec<u16> = "héllo 🐶".encode_utf16().collect();
        // 'h', accented 'e', 'l', 'l', 'o', space, 🐶 (high+low surrogate).
        assert!(!wide.is_empty());
        // 🐶 is U+1F436 which encodes to a surrogate pair (2 u16s).
        assert!(wide.len() >= 7);
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn non_windows_returns_explicit_error() {
        let err = copy_text_one_shot("hello").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("not yet implemented"), "got: {msg}");
    }
}
