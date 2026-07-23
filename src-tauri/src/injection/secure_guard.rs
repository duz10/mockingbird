//! Secure-input detection (ADR 0017, PLAN §12 #18 binding).
//!
//! The orchestrator in Wave 4 calls `is_secure(...)` **before** any
//! clipboard mutation or `SendInput` call. There is no path through
//! the orchestrator that reaches paste without the guard returning
//! `false` first — enforced by code structure, not by trust.
//!
//! ## Signals (post-amendment, 2026-05-17)
//!
//! The original ADR specified three OR-combined signals; the
//! 2026-05-17 amendment dropped the bogus `GUI_SECUREINPUT` flag
//! (not a real Win32 constant — see ADR 0017 "Update"). Two signals
//! remain:
//!
//! 1. **Class-name allowlist** — known secure-UI window classes.
//!    Lowercased exact match against [`SECURE_CLASSES`].
//! 2. **`ES_PASSWORD` on focused edit** — focused window per
//!    `GetGUIThreadInfo.hwndFocus`, class is `"Edit"`, and
//!    `GetWindowLongPtrW(GWL_STYLE) & ES_PASSWORD != 0`.
//!
//! UAC and other secure-desktop UIs trip the null-foreground guard in
//! [`crate::window_context::WindowContext::foreground`] and never
//! reach this code path at all.

// macOS port: these helpers are consumed only by the Windows secure-input path;
// dead on non-Windows until the cross-platform backend lands (Phase 3/4).
#![cfg_attr(not(target_os = "windows"), allow(dead_code))]

use crate::window_context::ForegroundWindow;

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::HWND;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetGUIThreadInfo, GetWindowLongPtrW, GetWindowThreadProcessId, GUITHREADINFO,
    GWL_STYLE,
};

/// Class names (lowercased) known to host secure input UI.
///
/// Match is case-insensitive against `GetClassNameW`. Anything new we
/// confirm via QA should land here (and in a LESSONS entry).
pub const SECURE_CLASSES: &[&str] = &[
    // UAC consent UI — Windows 10 / 11 modern style. Visible only
    // when UAC runs on the user's desktop (rare; usually it elevates
    // to the secure desktop, which already trips the null-foreground
    // guard upstream).
    "$$$secure uap dummy layout$$$",
    // Credential UI (network credentials, "save password" dialogs).
    "credentialdialogxamlhost",
    // Legacy UAC name (Windows 10 builds). Defensive.
    "consentui",
    // Defensive: lock-screen / Windows Hello. These typically run on
    // a different desktop too, but matching by class is a cheap
    // belt-and-braces.
    "lockapp",
];

/// Detects whether the foreground window represents a "secure input"
/// surface where injection MUST abort.
///
/// Implementations are `Send + Sync` so the orchestrator can share a
/// single guard across worker threads via `Arc`.
pub trait SecureInputGuard: Send + Sync {
    /// Returns `true` if any signal indicates a secure field is
    /// focused. On `true` the orchestrator aborts injection,
    /// persists the raw transcript with `injection_status =
    /// aborted_secure`, and emits a tray toast.
    fn is_secure(&self, fg: &ForegroundWindow) -> bool;
}

/// Conservative test-only guard — never reports secure.
///
/// Useful in unit tests where the production guard is not the
/// component under test (e.g. happy-path injection tests).
pub struct NeverSecureGuard;

impl SecureInputGuard for NeverSecureGuard {
    fn is_secure(&self, _fg: &ForegroundWindow) -> bool {
        false
    }
}

/// Construct the platform-default secure-input guard (ADR 0063 `.4.7b`).
///
/// Mirrors the other `make_default_*` factories
/// ([`crate::injection::make_default_injector`] /
/// [`crate::window_context::make_default_context`]) so the dictation
/// runtime stops hard-constructing `WinSecureInputGuard` directly.
///
/// Infallible by design — guard construction is free on both real
/// platforms — so this returns a plain `Box`, not `AppResult` (YAGNI:
/// the call-site never used `?`).
///
/// | Platform | Guard |
/// |----------|-------|
/// | Windows  | [`WinSecureInputGuard`] |
/// | macOS    | [`MacSecureInputGuard`] |
/// | other    | [`NeverSecureGuard`] (permissive null — the dictation runtime never spawns there) |
pub fn make_default_guard() -> Box<dyn SecureInputGuard> {
    #[cfg(target_os = "windows")]
    {
        Box::new(WinSecureInputGuard::new())
    }
    #[cfg(target_os = "macos")]
    {
        Box::new(MacSecureInputGuard::new())
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        Box::new(NeverSecureGuard)
    }
}

/// Production Windows guard.
///
/// Composes the two signals listed at the module head. Construction
/// is free; all the cost is in `is_secure`.
#[cfg(target_os = "windows")]
#[derive(Default)]
pub struct WinSecureInputGuard;

#[cfg(target_os = "windows")]
impl WinSecureInputGuard {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(target_os = "windows")]
impl SecureInputGuard for WinSecureInputGuard {
    fn is_secure(&self, fg: &ForegroundWindow) -> bool {
        // HWND wraps isize in windows-rs 0.56 (matches the underlying
        // Win32 typedef). ForegroundWindow stores it as `isize` for
        // cross-thread portability; cast back at the OS boundary.
        let hwnd = HWND(fg.hwnd);

        // Signal 1 — class-name allowlist on the foreground window.
        if let Some(class) = read_class_name(hwnd) {
            if class_in_allowlist(&class) {
                return true;
            }
        }

        // Signal 2 — focused child window is a password Edit.
        if focused_edit_is_password(hwnd) {
            return true;
        }

        false
    }
}

// --------------------------------------------------------------------
// Pure helpers — testable without an HWND
// --------------------------------------------------------------------

/// Lowercased-exact match against [`SECURE_CLASSES`].
///
/// Pure function. Tests cover it without needing a real window.
pub(crate) fn class_in_allowlist(class_name: &str) -> bool {
    let needle = class_name.to_ascii_lowercase();
    SECURE_CLASSES.iter().any(|c| *c == needle)
}

/// Does this `GWL_STYLE` value have the `ES_PASSWORD` bit set?
///
/// `ES_PASSWORD` is `0x20`. Bit-test only; no HWND required.
pub(crate) fn style_has_es_password(style: isize) -> bool {
    const ES_PASSWORD_BIT: isize = 0x20; // mirrors winuser.h
    (style & ES_PASSWORD_BIT) != 0
}

// --------------------------------------------------------------------
// Windows-only helpers — touch the OS
// --------------------------------------------------------------------

#[cfg(target_os = "windows")]
fn read_class_name(hwnd: HWND) -> Option<String> {
    // 256 chars is plenty — even Microsoft's longest documented
    // class names (e.g. WorkerW-style internals) are under 64.
    let mut buf = [0u16; 256];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) } as usize;
    if n == 0 {
        return None;
    }
    let end = n.min(buf.len());
    Some(String::from_utf16_lossy(&buf[..end]))
}

#[cfg(target_os = "windows")]
fn focused_edit_is_password(foreground: HWND) -> bool {
    let mut pid: u32 = 0;
    let tid = unsafe { GetWindowThreadProcessId(foreground, Some(&mut pid as *mut u32)) };
    if tid == 0 {
        return false;
    }

    let mut gti = GUITHREADINFO {
        cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    let res = unsafe { GetGUIThreadInfo(tid, &mut gti as *mut GUITHREADINFO) };
    if res.is_err() {
        return false;
    }
    let focused = gti.hwndFocus;
    if focused.0 == 0 {
        return false;
    }

    // Must be an Edit-class control to carry ES_PASSWORD meaningfully.
    let class = match read_class_name(focused) {
        Some(c) => c,
        None => return false,
    };
    if !class.eq_ignore_ascii_case("Edit") {
        return false;
    }

    let style = unsafe { GetWindowLongPtrW(focused, GWL_STYLE) };
    style_has_es_password(style)
}

// --------------------------------------------------------------------
// macOS branch (ADR 0059, mb-mac-v1.4.3)
// --------------------------------------------------------------------
//
// Plugs into the SAME `SecureInputGuard` seam as the Windows guard.
// Detection signals, OR-combined:
//
//   1. **Per-field AX role** (PRIMARY) — the system-wide focused UI
//      element's `AXRole` equals `AXSecureTextField`. This is the macOS
//      twin of the Windows per-element `ES_PASSWORD` check. Needs the
//      Accessibility grant to read the AX tree.
//   2. **`IsSecureEventInputEnabled()`** (belt-and-suspenders) — a
//      coarse, system-wide Carbon flag set whenever any app has secure
//      keyboard entry on. Readable WITHOUT any permission.
//
// ## Fail-safe stance (Accessibility not granted)
//
// The Windows guard fails *permissive* when a signal can't be read
// (returns `false`), relying on the upstream null-foreground guard to
// catch the truly-secure system surfaces. macOS mirrors that for the AX
// signal: an unreadable AX tree yields `focused_role() == None`, i.e.
// "not a secure field by this signal". But macOS is MORE conservative
// than Windows overall, because signal #2 (`IsSecureEventInputEnabled`)
// works with no grant at all and surfaces system-wide secure input even
// when AX is blind. And note the natural backstop: the Cmd+V keypost in
// `macos.rs` is gated on the SAME Accessibility grant, so a denied grant
// can never silently inject into a password field — the keypost is a
// no-op too.
//
// The AX query sits behind the [`MacSecureInputProbe`] seam so the
// `mac-p3c-secure-input-aborts` judge can mock it deterministically
// without a real Accessibility grant.

/// macOS AX role string for a secure (password) text field. Parity with
/// the Windows `ES_PASSWORD` per-element check.
#[cfg(target_os = "macos")]
pub const AX_SECURE_TEXT_FIELD_ROLE: &str = "AXSecureTextField";

/// Mockable seam over the two macOS secure-input signals. The judge
/// substitutes a fake; production uses [`AxSecureInputProbe`].
#[cfg(target_os = "macos")]
pub trait MacSecureInputProbe: Send + Sync {
    /// `AXRole` of the system-wide focused UI element, or `None` when it
    /// cannot be determined (no Accessibility grant / no focused
    /// element).
    fn focused_role(&self) -> Option<String>;
    /// The system-wide `IsSecureEventInputEnabled()` Carbon flag.
    fn secure_event_input_enabled(&self) -> bool;
}

/// Pure classifier — secure iff system-wide secure input is on OR the
/// focused element is a secure text field. Testable without any FFI.
#[cfg(target_os = "macos")]
pub fn classify_mac_secure(focused_role: Option<&str>, secure_event_input: bool) -> bool {
    secure_event_input || matches!(focused_role, Some(r) if r == AX_SECURE_TEXT_FIELD_ROLE)
}

/// Production macOS guard. Composes the two signals via the AX/Carbon
/// FFI probe. Construction is free; all cost is in `is_secure`.
#[cfg(target_os = "macos")]
pub struct MacSecureInputGuard {
    probe: Box<dyn MacSecureInputProbe>,
}

#[cfg(target_os = "macos")]
impl MacSecureInputGuard {
    /// Production guard backed by the real AX + Carbon FFI probe.
    pub fn new() -> Self {
        Self {
            probe: Box::new(AxSecureInputProbe),
        }
    }

    /// Inject a custom probe — used by the `mac-p3c` judge to feed a
    /// mocked focused-element role deterministically.
    pub fn with_probe(probe: Box<dyn MacSecureInputProbe>) -> Self {
        Self { probe }
    }
}

#[cfg(target_os = "macos")]
impl Default for MacSecureInputGuard {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "macos")]
impl SecureInputGuard for MacSecureInputGuard {
    fn is_secure(&self, _fg: &ForegroundWindow) -> bool {
        // The macOS focused element is read system-wide, not per-window,
        // so `_fg` is unused (kept for trait parity with Windows).
        classify_mac_secure(
            self.probe.focused_role().as_deref(),
            self.probe.secure_event_input_enabled(),
        )
    }
}

/// Real probe: reads the AX tree + the Carbon secure-input flag.
#[cfg(target_os = "macos")]
struct AxSecureInputProbe;

#[cfg(target_os = "macos")]
impl MacSecureInputProbe for AxSecureInputProbe {
    fn focused_role(&self) -> Option<String> {
        // SAFETY: the AX calls below pass only valid, owned references
        // and balance every Copy-rule retain with a release.
        unsafe { ax_focused_role() }
    }

    fn secure_event_input_enabled(&self) -> bool {
        // SAFETY: `IsSecureEventInputEnabled` has no preconditions.
        unsafe { IsSecureEventInputEnabled() != 0 }
    }
}

#[cfg(target_os = "macos")]
#[link(name = "Carbon", kind = "framework")]
extern "C" {
    /// Carbon system-wide secure keyboard-entry flag. Returns a Carbon
    /// `Boolean` (`unsigned char`); readable without any grant.
    fn IsSecureEventInputEnabled() -> std::os::raw::c_uchar;
}

/// Read the `AXRole` of the system-wide focused UI element.
///
/// Returns `None` when Accessibility is not granted, when there is no
/// focused element, or on any AX error (the conservative "this signal
/// says nothing" outcome).
///
/// # Safety
/// Calls Apple AX FFI. Each `…Copy…` result is released exactly once
/// (Copy rule = caller owns +1).
#[cfg(target_os = "macos")]
unsafe fn ax_focused_role() -> Option<String> {
    use accessibility_sys::{
        kAXErrorSuccess, kAXFocusedUIElementAttribute, kAXRoleAttribute,
        AXUIElementCopyAttributeValue, AXUIElementCreateSystemWide, AXUIElementRef,
    };
    use core_foundation::base::{CFRelease, CFTypeRef, TCFType};
    use core_foundation::string::{CFString, CFStringRef};

    let system = AXUIElementCreateSystemWide();
    if system.is_null() {
        return None;
    }

    // 1. Focused UI element.
    let focused_attr = CFString::from_static_string(kAXFocusedUIElementAttribute);
    let mut focused: CFTypeRef = std::ptr::null();
    let err =
        AXUIElementCopyAttributeValue(system, focused_attr.as_concrete_TypeRef(), &mut focused);
    CFRelease(system as CFTypeRef);
    if err != kAXErrorSuccess || focused.is_null() {
        return None;
    }

    // 2. `AXRole` of the focused element.
    let role_attr = CFString::from_static_string(kAXRoleAttribute);
    let mut role_val: CFTypeRef = std::ptr::null();
    let err = AXUIElementCopyAttributeValue(
        focused as AXUIElementRef,
        role_attr.as_concrete_TypeRef(),
        &mut role_val,
    );
    CFRelease(focused);
    if err != kAXErrorSuccess || role_val.is_null() {
        return None;
    }

    // `role_val` is a CFString owned by us (+1) — wrap_under_create_rule
    // takes ownership and releases on drop.
    let role = CFString::wrap_under_create_rule(role_val as CFStringRef);
    Some(role.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window_context::ForegroundWindow;

    fn fg(class: &str) -> ForegroundWindow {
        // For tests that only exercise pure helpers we don't need a
        // real HWND. The class string is plumbed through the test
        // helpers directly, not through the OS.
        ForegroundWindow {
            hwnd: 0,
            title: class.into(),
            process_name: "test.exe".into(),
            exe_path: None,
        }
    }

    // -----------------------------------------------------------------
    // NeverSecureGuard
    // -----------------------------------------------------------------

    #[test]
    fn never_secure_guard_returns_false() {
        let f = fg("anything");
        assert!(!NeverSecureGuard.is_secure(&f));
    }

    // -----------------------------------------------------------------
    // Class-name allowlist (pure)
    // -----------------------------------------------------------------

    #[test]
    fn allowlist_match_is_case_insensitive() {
        for spelling in [
            "$$$secure uap dummy layout$$$",
            "$$$SECURE UAP DUMMY LAYOUT$$$",
            "$$$Secure UAP Dummy Layout$$$",
            "CredentialDialogXamlHost",
            "credentialDialogXAMLHost",
            "ConsentUI",
            "consentui",
            "LockApp",
        ] {
            assert!(
                class_in_allowlist(spelling),
                "class '{spelling}' should match the allowlist"
            );
        }
    }

    #[test]
    fn allowlist_miss_returns_false() {
        for class in [
            "Notepad",
            "Chrome_WidgetWin_1",
            "WindowsTerminalWindow",
            "ApplicationFrameWindow",
            "",
            "credential", // partial match should NOT count
            "$$$secure uap$$$",
        ] {
            assert!(
                !class_in_allowlist(class),
                "class '{class}' should NOT match the allowlist"
            );
        }
    }

    // -----------------------------------------------------------------
    // ES_PASSWORD style (pure)
    // -----------------------------------------------------------------

    #[test]
    fn es_password_bit_detected() {
        // 0x20 is ES_PASSWORD in winuser.h. Real edit-control styles
        // OR many bits together; we only care about this one.
        assert!(style_has_es_password(0x20));
        assert!(style_has_es_password(0x0080_0020)); // WS_VISIBLE | ES_PASSWORD
        assert!(style_has_es_password(0xFFFF_FFFF_u32 as i32 as isize));
    }

    #[test]
    fn es_password_bit_absent_when_unset() {
        assert!(!style_has_es_password(0));
        assert!(!style_has_es_password(0x0080_0000)); // WS_VISIBLE only
        assert!(!style_has_es_password(0x10)); // ES_MULTILINE
        assert!(!style_has_es_password(0x40)); // ES_AUTOVSCROLL
    }

    // -----------------------------------------------------------------
    // WinSecureInputGuard — class-only path (we don't have a focused
    // HWND in unit tests, so the ES_PASSWORD path is exercised via the
    // pure helper above)
    // -----------------------------------------------------------------

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "live foreground probe; run with `cargo test -- --ignored`"]
    fn live_guard_on_current_foreground_does_not_panic() {
        // Best-effort: snapshot the host's foreground window, run
        // the guard, accept any boolean.
        use crate::window_context::make_default_context;
        let ctx = make_default_context().expect("Windows ctx ok");
        if let Ok(fg) = ctx.foreground() {
            let guard = WinSecureInputGuard::new();
            let _ = guard.is_secure(&fg);
        }
    }

    #[test]
    fn allowlist_documented_classes_are_all_lowercase() {
        // The allowlist must be lowercased for the eq-on-lowered
        // comparison in `class_in_allowlist` to work. This test
        // guards against accidental mixed-case entries.
        for class in SECURE_CLASSES {
            assert_eq!(
                *class,
                class.to_ascii_lowercase(),
                "SECURE_CLASSES entry '{class}' must be lowercase"
            );
        }
    }
}
