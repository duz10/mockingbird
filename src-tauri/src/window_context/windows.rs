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
//! - `QueryFullProcessImageNameW(hproc, PROCESS_NAME_WIN32, ...)` → full
//!   exe path. Falls back to `None` on failure (rare; protected
//!   processes again).
//! - `process_name` (e.g. `"notepad.exe"`) is derived from the full
//!   exe path's file-name component. **Important:** the obvious-looking
//!   `K32GetModuleBaseNameW` is NOT used — it requires
//!   `PROCESS_QUERY_INFORMATION + PROCESS_VM_READ` access, while we
//!   open with the lighter `PROCESS_QUERY_LIMITED_INFORMATION`
//!   (needed for protected processes). `K32GetModuleBaseNameW`
//!   silently returns 0 on insufficient access → empty string → all
//!   downstream strategy resolution / per-app overrides / judges
//!   silently break. Wave 4.9 bug-fix; see `docs/LESSONS.md`.
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

        let exe_path = read_full_image_name(hproc.0);
        let process_name = exe_path
            .as_deref()
            .and_then(basename_from_path)
            .unwrap_or_default();

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

/// Extract the file-name component of a full Win32 path.
///
/// Pure helper — no OS calls. Returns `None` on empty input or paths
/// that consist only of directory separators (`\\\\?\\C:\\`). Uses
/// `std::path::Path::file_name` semantics, which handle both `\` and
/// `/` separators correctly on Windows.
fn basename_from_path(path: &str) -> Option<String> {
    std::path::Path::new(path)
        .file_name()
        .map(|os| os.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
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
    // basename_from_path — pure, deterministic
    // -----------------------------------------------------------------

    #[test]
    fn basename_extracts_exe_from_typical_win32_path() {
        assert_eq!(
            basename_from_path(r"C:\Windows\System32\notepad.exe").as_deref(),
            Some("notepad.exe")
        );
        assert_eq!(
            basename_from_path(r"C:\Program Files\Microsoft VS Code\Code.exe").as_deref(),
            Some("Code.exe")
        );
    }

    #[test]
    fn basename_handles_long_path_prefix() {
        // Windows long-path prefix `\\?\`. file_name() ignores it.
        assert_eq!(
            basename_from_path(r"\\?\C:\very\deep\path\app.exe").as_deref(),
            Some("app.exe")
        );
    }

    #[test]
    fn basename_returns_none_for_empty_or_root_only() {
        assert_eq!(basename_from_path(""), None);
        assert_eq!(basename_from_path(r"C:\"), None);
    }

    #[test]
    fn basename_handles_forward_slashes() {
        // Some Win32 APIs return paths with forward slashes (mixed
        // mode). Path::file_name handles both on Windows.
        assert_eq!(
            basename_from_path("C:/Users/dustin/app.exe").as_deref(),
            Some("app.exe")
        );
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
                // process_name is derived from exe_path.file_name().
                // For protected processes both may be None/empty —
                // we only assert internal consistency: if exe_path
                // is Some(p), process_name must equal p's basename.
                if let Some(exe) = &fg.exe_path {
                    let expected = basename_from_path(exe).unwrap_or_default();
                    assert_eq!(
                        fg.process_name, expected,
                        "process_name must equal basename(exe_path)"
                    );
                }
                let _ = &fg.title;
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
