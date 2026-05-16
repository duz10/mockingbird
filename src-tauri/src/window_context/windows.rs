//! Windows implementation of [`super::WindowContext`].
//!
//! ## API surface used
//!
//! - `GetForegroundWindow` → `HWND`
//! - `GetWindowTextW(hwnd, &mut [u16; 512])` → title length
//! - `GetWindowThreadProcessId(hwnd, &mut pid)` → thread id (unused for now)
//! - `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid)` → handle
//!   - `PROCESS_QUERY_LIMITED_INFORMATION` (not the older
//!     `PROCESS_QUERY_INFORMATION`) is required to open protected
//!     processes (svchost, csrss). Regular query returns
//!     `ERROR_ACCESS_DENIED`. Limited-info has been the recommended
//!     access right for non-debugging callers since Windows Vista.
//! - `K32GetModuleBaseNameW(hproc, None, &mut [u16; 256])` → process
//!   basename (e.g. `"notepad.exe"`).
//! - `QueryFullProcessImageNameW(hproc, PROCESS_NAME_WIN32, ...)` → full
//!   exe path. Falls back to `None` on failure (rare; protected
//!   processes again).
//!
//! Handles are wrapped in [`OwnedHandle`] so they're closed
//! deterministically on `Drop`, including on the error/panic paths.
//!
//! ## What this module deliberately does NOT do
//!
//! - Class-name probes (deferred to `injection/secure_guard.rs` —
//!   that's where they're consumed).
//! - HWND→appcontainer / UWP detection. Not needed for the §3 strategy
//!   resolver; basename + path are enough.
//! - Caching. The orchestrator snapshots foreground twice per
//!   dictation (key-down + key-up), so the overhead of two syscall
//!   chains is negligible. Premature caching also breaks the
//!   focus-loss detection ADR 0016 §7 requires.

use windows::Win32::Foundation::{CloseHandle, FALSE, HANDLE, HWND};
use windows::Win32::System::ProcessStatus::K32GetModuleBaseNameW;
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
};

use super::{ForegroundWindow, WindowContext};
use crate::error::{AppError, AppResult};

/// Windows foreground-window provider.
#[derive(Default)]
pub struct WinWindowContext;

impl WinWindowContext {
    /// Construct. No OS resources are acquired. Equivalent to
    /// `<Self as Default>::default()`; both forms kept for ergonomics.
    pub fn new() -> Self {
        Self
    }
}

impl WindowContext for WinWindowContext {
    fn foreground(&self) -> AppResult<ForegroundWindow> {
        let hwnd = unsafe { GetForegroundWindow() };
        // HWND in windows-rs 0.56 wraps an `isize`. NULL HWND is
        // represented as 0 (matches Win32's `HWND_DESKTOP` sentinel).
        if hwnd.0 == 0 {
            return Err(AppError::Other(
                "no foreground window (transient — try again)".into(),
            ));
        }

        let title = read_window_text(hwnd);
        let pid = read_pid(hwnd).ok_or_else(|| {
            AppError::Other("GetWindowThreadProcessId returned tid=0 (no process)".into())
        })?;

        let hproc = OwnedHandle::open_for_query(pid).map_err(|e| {
            AppError::Other(format!(
                "OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, pid={pid}) failed: {e}"
            ))
        })?;

        let process_name = read_module_base_name(hproc.0).unwrap_or_default();
        let exe_path = read_full_image_name(hproc.0);

        Ok(ForegroundWindow {
            hwnd: hwnd.0,
            title,
            process_name,
            exe_path,
        })
    }
}

// --------------------------------------------------------------------
// HANDLE RAII
// --------------------------------------------------------------------

/// A `HANDLE` that closes itself on drop.
///
/// Why this exists: the orchestrator probes foreground state twice per
/// dictation session under realistic load. Forgetting `CloseHandle`
/// on any error branch is a slow handle leak that only manifests in
/// production. RAII eliminates the entire class of leak.
struct OwnedHandle(HANDLE);

impl OwnedHandle {
    fn open_for_query(pid: u32) -> windows::core::Result<Self> {
        let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid) }?;
        Ok(Self(h))
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: HANDLE was obtained from OpenProcess and is non-null
        // by construction (OpenProcess returns Err for the null case).
        // CloseHandle's Result is ignored — we're in Drop, can't
        // recover; a leak on shutdown is a non-issue.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

// --------------------------------------------------------------------
// UTF-16 helpers
// --------------------------------------------------------------------

/// Decode a UTF-16 buffer up to the first NUL terminator.
///
/// Falls back to `String::from_utf16_lossy` on malformed sequences
/// (replacement char `U+FFFD` in titles is much better than dropping
/// the snapshot — provenance > pedantry).
fn decode_utf16_to_nul(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

fn read_window_text(hwnd: HWND) -> String {
    // 512 chars is enough for any sensible window title. Real titles
    // average ~30; the rare HUGE titles (paste a novel into Notepad's
    // title via WM_SETTEXT) get truncated, which is fine.
    let mut buf = [0u16; 512];
    let len = unsafe { GetWindowTextW(hwnd, &mut buf) } as usize;
    if len == 0 {
        return String::new();
    }
    decode_utf16_to_nul(&buf[..len.min(buf.len())])
}

fn read_pid(hwnd: HWND) -> Option<u32> {
    let mut pid: u32 = 0;
    let tid = unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid as *mut u32)) };
    if tid == 0 {
        None
    } else {
        Some(pid)
    }
}

fn read_module_base_name(hproc: HANDLE) -> Option<String> {
    let mut buf = [0u16; 256];
    // K32GetModuleBaseNameW returns the number of TCHARs copied,
    // not including the trailing NUL. Zero indicates failure.
    let n = unsafe { K32GetModuleBaseNameW(hproc, None, &mut buf) } as usize;
    if n == 0 {
        return None;
    }
    Some(decode_utf16_to_nul(&buf[..n.min(buf.len())]))
}

fn read_full_image_name(hproc: HANDLE) -> Option<String> {
    let mut buf = [0u16; 1024]; // MAX_PATH (260) is the floor; many apps now exceed it.
    let mut sz: u32 = buf.len() as u32;
    let res = unsafe {
        QueryFullProcessImageNameW(
            hproc,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut sz as *mut u32,
        )
    };
    res.ok()?;
    // sz is updated to the actual length on success.
    Some(decode_utf16_to_nul(&buf[..(sz as usize).min(buf.len())]))
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------
    // UTF-16 helpers — pure, deterministic, no OS calls
    // -----------------------------------------------------------------

    #[test]
    fn decode_handles_nul_mid_buffer() {
        // Buffer ends early via NUL — typical for GetWindowTextW's
        // overallocated array.
        let mut buf = [0u16; 8];
        let s: Vec<u16> = "hi".encode_utf16().collect();
        buf[..s.len()].copy_from_slice(&s);
        // buf[2] is already 0 (NUL terminator).
        assert_eq!(decode_utf16_to_nul(&buf), "hi");
    }

    #[test]
    fn decode_handles_full_buffer_without_nul() {
        // Edge: a window title that exactly fills the buffer with no
        // room for NUL. Should still decode all bytes.
        let buf: Vec<u16> = "abcdef".encode_utf16().collect();
        assert_eq!(decode_utf16_to_nul(&buf), "abcdef");
    }

    #[test]
    fn decode_handles_non_ascii_round_trip() {
        // Mockingbird's clientele includes em-dashes, accented
        // characters, and emoji in window titles (Notepad, browsers,
        // chat apps). Round-trip must preserve them.
        let original = "héllo 🐦 world";
        let buf: Vec<u16> = original
            .encode_utf16()
            .chain(std::iter::once(0u16))
            .collect();
        assert_eq!(decode_utf16_to_nul(&buf), original);
    }

    #[test]
    fn decode_handles_lone_surrogate_via_lossy() {
        // Construct a deliberately-broken UTF-16 sequence — a lone
        // high surrogate with no matching low. `from_utf16_lossy`
        // replaces with U+FFFD; we want that, not a panic.
        let buf: Vec<u16> = vec![0xD800, 0x0041, 0]; // bad, 'A', NUL
        let decoded = decode_utf16_to_nul(&buf);
        assert!(
            decoded.contains('\u{FFFD}') || decoded.contains('A'),
            "expected replacement char or 'A' in lossy decode, got {decoded:?}"
        );
    }

    #[test]
    fn decode_empty_buffer_yields_empty_string() {
        assert_eq!(decode_utf16_to_nul(&[]), "");
    }

    #[test]
    fn decode_buffer_starting_with_nul_yields_empty_string() {
        // GetWindowTextW returns 0 for windows with empty titles;
        // we should still produce a clean empty String.
        let buf = [0u16; 8];
        assert_eq!(decode_utf16_to_nul(&buf), "");
    }

    // -----------------------------------------------------------------
    // Live foreground snapshot — runs against the host's actual desktop
    // -----------------------------------------------------------------
    //
    // This test is best-effort: in a headless CI runner there may be
    // NO foreground window at all, in which case the call legitimately
    // returns AppError::Other("no foreground window..."). We accept
    // either a populated snapshot OR that specific error.
    //
    // Marked `#[ignore]` so `cargo test` runs it only on explicit
    // request — keeps the default test run deterministic.

    #[test]
    #[ignore = "best-effort live foreground probe; run with `cargo test -- --ignored`"]
    fn live_foreground_snapshot_is_either_populated_or_no_window_error() {
        let ctx = WinWindowContext::new();
        match ctx.foreground() {
            Ok(fg) => {
                assert!(fg.hwnd != 0, "hwnd should be non-zero when Ok");
                // process_name may be empty on protected processes
                // even when we got past OpenProcess (rare). We only
                // assert it doesn't panic.
                let _ = (&fg.title, &fg.process_name, &fg.exe_path);
            }
            Err(AppError::Other(msg)) => {
                // Acceptable error on headless / between-window
                // animations / locked screens.
                assert!(
                    msg.contains("no foreground window")
                        || msg.contains("OpenProcess")
                        || msg.contains("tid=0"),
                    "unexpected error message: {msg}"
                );
            }
            Err(other) => panic!("unexpected error variant: {other:?}"),
        }
    }
}
