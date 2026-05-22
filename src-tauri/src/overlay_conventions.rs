//! Shared overlay-window conventions.
//!
//! Closes ADR 0026's YAGNI debt: with the Command Center landing as
//! overlay window #3 (alongside the dictation recording pip and the
//! meeting overlay), we now have the rule of three required to
//! extract the common math + Win32 fixup into one neutral module
//! instead of duplicating it three times.
//!
//! **Neutral ground.** This file deliberately lives at the top of
//! `src-tauri/src/` rather than under `meetings/`, `dictation/`, or
//! `command_center/` — owning it from any one subsystem would make
//! the other two reach across module boundaries, which the
//! pre-commit `block-cross-module-coupling` hook would (rightly)
//! flag as smell. Per ADR 0037 §Decision item 2.
//!
//! ## What lives here
//!
//! - [`bottom_center_rect`] — pure math: given a monitor work area
//!   and a desired (width, height), return the (x, y, w, h) that
//!   centers the overlay horizontally and floats it `MARGIN_BOTTOM_PX`
//!   above the taskbar. Throwaway-crate testable.
//! - [`apply_noactivate_layered`] — Win32 fixup that flips
//!   `WS_EX_NOACTIVATE | WS_EX_LAYERED | WS_EX_TOPMOST` on an HWND
//!   after Tauri creates it. The pip + meeting overlay each used to
//!   carry their own copy of this; now they share one.
//! - [`MARGIN_BOTTOM_PX`] — the gap between the overlay and the
//!   bottom edge of the monitor work area. Const, tuned in the Phase
//!   MC overlay polish iteration.
//!
//! All non-Windows builds get no-ops for the Win32 fixup; the math
//! helpers are cross-platform.

#![allow(clippy::module_name_repetitions)]
#![allow(missing_docs)] // Field-level docs would just repeat the module-level prose.

/// Pixels of clearance between the overlay's bottom edge and the
/// monitor work-area bottom (i.e. above the taskbar on Windows).
/// 16 px matches the Phase MC overlay's settled position and is the
/// same value the dictation pip's Tauri-level `center: true` plus
/// `position` config produced empirically.
pub const MARGIN_BOTTOM_PX: i32 = 16;

/// A monitor's usable work area in device pixels (post-DPI). Caller
/// fetches this from `tauri::Monitor::size` / `position` or the
/// `MonitorFromWindow` + `GetMonitorInfoW` Win32 pair; this struct
/// is the input contract for [`bottom_center_rect`].
///
/// **`x` / `y` are the work-area ORIGIN** (top-left) on the virtual
/// desktop — important for multi-monitor setups where secondary
/// monitors have non-zero origins. `width` / `height` are the
/// usable area (excluding the taskbar / dock).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkArea {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// The placement an overlay window should adopt: top-left corner plus
/// final (width, height). The width/height fields just echo the input
/// — they're carried in the struct so callers can pass it straight to
/// Tauri's `set_size` / `set_position` pair without re-binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverlayPlacement {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Compute the bottom-center placement for an overlay of size
/// `(w, h)` on a monitor whose work area is `area`. Centers
/// horizontally, floats above the taskbar by [`MARGIN_BOTTOM_PX`].
///
/// Clamps to the work area: if the overlay is bigger than the monitor
/// (pathological, but possible on a 800x600 secondary), the result is
/// pinned to the work-area origin so at least the top-left corner is
/// visible. Better than coordinates outside the desktop.
///
/// Pure function. No OS calls; safe to unit-test via throwaway-crate.
pub fn bottom_center_rect(area: WorkArea, width: i32, height: i32) -> OverlayPlacement {
    // Clamp negative / zero sizes to 1 so the math doesn't divide
    // by something weird. A 1-px overlay is wrong-but-not-broken.
    let w = width.max(1);
    let h = height.max(1);

    // Horizontal center.
    let x = area.x + (area.width - w) / 2;
    // Float above the work-area bottom by the margin.
    let y = area.y + area.height - h - MARGIN_BOTTOM_PX;

    // Clamp to work-area origin if the overlay overflows.
    let x = x.max(area.x);
    let y = y.max(area.y);

    OverlayPlacement {
        x,
        y,
        width: w,
        height: h,
    }
}

/// Apply the non-activating extended window styles to a Tauri webview
/// window's underlying HWND. Idempotent: calling it twice on the
/// same window is harmless (just redundantly sets the same bits).
///
/// Windows: flips on `WS_EX_NOACTIVATE | WS_EX_LAYERED | WS_EX_TOPMOST`.
/// This is what makes the overlay refuse focus when clicked (so the
/// user's target app keeps focus during dictation / meeting capture),
/// participate in per-pixel alpha (transparent backgrounds), and stay
/// above other non-topmost windows.
///
/// Non-Windows: no-op. Other platforms manage these affordances
/// through Tauri's high-level `focus: false` + `transparent: true`
/// config; the Win32 raw-styles trick is needed because Tauri's
/// `focus: false` on Windows isn't a hard guarantee against
/// `WM_ACTIVATE` from a mouse click in the overlay area.
///
/// Errors are logged + swallowed: the overlay still renders if the
/// fixup fails, it just might steal focus on click. That's a UX
/// degradation, not a correctness break.
#[cfg(target_os = "windows")]
pub fn apply_noactivate_layered<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_LAYERED, WS_EX_NOACTIVATE,
        WS_EX_TOPMOST,
    };

    // `window.hwnd()` returns a raw `*mut c_void` HWND across the
    // Tauri 2 API; the `windows-rs` 0.56 `HWND` is `pub struct
    // HWND(pub isize)`. Convert via `isize as` so the same call
    // site keeps working if the underlying types drift again.
    let raw = match window.hwnd() {
        Ok(h) => h.0 as isize,
        Err(e) => {
            tracing::warn!(
                error = ?e,
                label = window.label(),
                "overlay_conventions: hwnd() failed; skipping NOACTIVATE fixup"
            );
            return;
        }
    };
    let hwnd = HWND(raw);
    // SAFETY: HWND comes from Tauri's own webview; GetWindowLongPtrW /
    // SetWindowLongPtrW with GWL_EXSTYLE is a stable user32 ABI. We
    // only OR-in bits we own (NOACTIVATE / LAYERED / TOPMOST) and
    // never clear bits Tauri set.
    unsafe {
        let prev = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
        let new = prev
            | (WS_EX_NOACTIVATE.0 as isize)
            | (WS_EX_LAYERED.0 as isize)
            | (WS_EX_TOPMOST.0 as isize);
        if new != prev {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new);
        }
    }
}

/// Non-Windows no-op so `lib.rs` + the subsystem modules can call
/// this unconditionally without `#[cfg(target_os)]` at every call
/// site.
#[cfg(not(target_os = "windows"))]
pub fn apply_noactivate_layered<R: tauri::Runtime>(_window: &tauri::WebviewWindow<R>) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(x: i32, y: i32, w: i32, h: i32) -> WorkArea {
        WorkArea {
            x,
            y,
            width: w,
            height: h,
        }
    }

    #[test]
    fn centers_on_primary_monitor() {
        // 1920x1080 work area at origin, 440x64 overlay.
        let p = bottom_center_rect(area(0, 0, 1920, 1080), 440, 64);
        assert_eq!(p.x, (1920 - 440) / 2);
        // Bottom edge of overlay sits MARGIN_BOTTOM_PX above 1080.
        assert_eq!(p.y, 1080 - 64 - MARGIN_BOTTOM_PX);
        assert_eq!(p.width, 440);
        assert_eq!(p.height, 64);
    }

    #[test]
    fn respects_nonzero_work_area_origin() {
        // Secondary monitor at (1920, 0), 1280x720.
        let p = bottom_center_rect(area(1920, 0, 1280, 720), 440, 64);
        assert_eq!(p.x, 1920 + (1280 - 440) / 2);
        assert_eq!(p.y, 0 + 720 - 64 - MARGIN_BOTTOM_PX);
    }

    #[test]
    fn handles_negative_origin_for_left_secondary_monitor() {
        // Windows allows monitors at negative virtual-desktop coords
        // (a secondary to the LEFT of the primary). Math must hold.
        let p = bottom_center_rect(area(-1920, 0, 1920, 1080), 440, 64);
        assert_eq!(p.x, -1920 + (1920 - 440) / 2);
        assert_eq!(p.y, 1080 - 64 - MARGIN_BOTTOM_PX);
    }

    #[test]
    fn clamps_when_overlay_is_wider_than_monitor() {
        // 800x600 monitor, 1200x64 overlay (oversized). Should pin
        // x to the work-area origin rather than emit a negative x.
        let p = bottom_center_rect(area(0, 0, 800, 600), 1200, 64);
        assert_eq!(p.x, 0);
        assert_eq!(p.y, 600 - 64 - MARGIN_BOTTOM_PX);
    }

    #[test]
    fn clamps_when_overlay_is_taller_than_monitor() {
        // Pathological tall overlay on a small screen.
        let p = bottom_center_rect(area(0, 0, 1920, 200), 440, 600);
        // y would be 200 - 600 - 16 = -416; clamp to area.y (0).
        assert_eq!(p.y, 0);
    }

    #[test]
    fn coerces_zero_or_negative_size_inputs() {
        // Defensive against a caller passing 0 or negative dims.
        let p = bottom_center_rect(area(0, 0, 1920, 1080), 0, -10);
        assert_eq!(p.width, 1);
        assert_eq!(p.height, 1);
    }

    #[test]
    fn margin_constant_is_what_we_expect() {
        // Tests downstream assume this exact value. If it changes,
        // the Phase MC overlay-position snapshot tests need to update.
        assert_eq!(MARGIN_BOTTOM_PX, 16);
    }

    #[test]
    fn placement_is_deterministic_for_same_inputs() {
        // Pure-function contract.
        let a = bottom_center_rect(area(0, 0, 1920, 1080), 440, 64);
        let b = bottom_center_rect(area(0, 0, 1920, 1080), 440, 64);
        assert_eq!(a, b);
    }
}
