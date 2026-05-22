//! Windows COM-backed [`super::Probe`] implementation.
//!
//! ## Design notes
//!
//! - **COM apartment.** UIA is single-threaded apartment (STA) on the
//!   caller side. We call `CoInitializeEx(COINIT_APARTMENTTHREADED)`
//!   on the **first** [`Probe::snapshot`] call from a given thread.
//!   The sampler thread is the only consumer; subsequent calls return
//!   `S_FALSE` which we tolerate. No `CoUninitialize` — the apartment
//!   lives for the lifetime of the sampler thread (process-wide ~1
//!   apartment, no measurable cost).
//!
//! - **Tree-walk strategy.** We use `IUIAutomationTreeWalker` with the
//!   automation's `ContentViewWalker` (skips redundant scrollbars,
//!   chrome, decorative siblings). Walks are **depth- and budget-
//!   limited**: depth ≤ [`MAX_DEPTH`], elements visited
//!   ≤ [`MAX_ELEMENTS`]. Production walks on Chrome can return
//!   thousands of nodes; we cap so a single tick doesn't stall the
//!   sampler thread for a second on a busy page.
//!
//! - **Failure mode.** Every COM call is wrapped in a `Result`; we
//!   collect partial results and surface the first failure in
//!   [`ProbeStatus::Failed`]. The probe **never panics** — if the
//!   apartment dies mid-tick, the runtime still gets a degraded
//!   payload with `app` + `title` populated.
//!
//! - **Password redaction.** When the focused element exposes
//!   `IsPassword = true`, we set `password_field_active = true` AND
//!   zero out the `value` field AND clear `visible_text_fragments`.
//!   Principle 8 / Wave 5 plan §Q5. Wave 5 will additionally drop
//!   the whole event upstream of the INSERT; Wave 2 redacts in-place.
//!
//! ## What this module does NOT do
//!
//! - Keystroke capture. There is no `WH_KEYBOARD_LL` or
//!   `GetKeyboardState` anywhere in this module — Phase 10 invariant.
//! - Persistent COM globals. Every `WindowsUiaProbe` carries its own
//!   thread-local lazy init; the runtime can construct multiple
//!   probes without state leakage (the test seam).

// `#[cfg(target_os = "windows")]` is already applied to the `pub mod windows_com`
// declaration in `super::mod`; declaring it again here would be a duplicate
// attribute (clippy::duplicated_attributes).
#![allow(missing_docs)]
#![allow(non_snake_case)]
#![allow(clippy::too_many_lines)]

use std::cell::RefCell;

use windows::Win32::Foundation::{BOOL, HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromWindow, HMONITOR, MONITORINFO, MONITORINFOEXW,
    MONITOR_DEFAULTTONEAREST,
};
// `MONITORINFOF_PRIMARY` lives under `UI::WindowsAndMessaging` in windows-rs
// 0.56 (not `Graphics::Gdi` as you'd expect from the Win32 SDK layout).
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationTreeWalker,
    IUIAutomationValuePattern, UIA_ButtonControlTypeId, UIA_DocumentControlTypeId,
    UIA_EditControlTypeId, UIA_HyperlinkControlTypeId, UIA_TextControlTypeId, UIA_ValuePatternId,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::MONITORINFOF_PRIMARY;

use super::payload::{ControlSummary, FocusedField, MonitorInfo, ProbeResult, ProbeStatus, Rect};
use super::Probe;

/// Maximum depth the tree walker descends from the root window
/// element. Empirically: 6 captures the meaningful content of every
/// Windows native, Electron, and browser page we tested in the Wave
/// 2 smoke matrix without paying for the long tail of generated DOM.
const MAX_DEPTH: u32 = 6;

/// Maximum number of UIA elements visited per snapshot. Caps tick
/// cost on busy pages. ~500 elements is enough to summarize a typical
/// browser page or IDE window (we measured 220-380 visited in the
/// Wave 2 smoke matrix; the 500 ceiling is a comfortable headroom).
const MAX_ELEMENTS: u32 = 500;

/// Per-fragment character cap for fragments harvested from UIA. The
/// payload module also enforces a cap on serialize; this is an
/// earlier prune to avoid hauling 100 KB strings through Rust before
/// throwing them away in `to_payload_json`.
const FRAGMENT_CHAR_CAP: usize = 1024;

thread_local! {
    /// The COM client lives in a thread-local so each thread that
    /// calls `snapshot` initializes its own apartment lazily. In
    /// practice this is exactly one thread (the sampler thread), so
    /// the thread-local is effectively a `OnceCell` — but using a
    /// thread-local lets the same `WindowsUiaProbe` be invoked from
    /// a fresh thread in tests without cross-thread state leakage.
    static UIA_CLIENT: RefCell<Option<UiaClient>> = const { RefCell::new(None) };
}

/// Per-thread COM state. Constructed on first `snapshot()` call.
struct UiaClient {
    automation: IUIAutomation,
    walker: IUIAutomationTreeWalker,
}

/// The exported probe. Cheap to clone (it's stateless beyond a
/// per-thread COM init that's stored in a `thread_local!`).
#[derive(Default)]
pub struct WindowsUiaProbe;

impl WindowsUiaProbe {
    pub fn new() -> Self {
        Self
    }
}

impl Probe for WindowsUiaProbe {
    fn snapshot(&mut self, hwnd_isize: isize, app: &str, title: &str) -> ProbeResult {
        // Seed result with the cheap fields that always succeed.
        let mut result = ProbeResult {
            app: app.to_string(),
            title: title.to_string(),
            status: ProbeStatus::Ok,
            ..Default::default()
        };

        let hwnd = HWND(hwnd_isize);
        if hwnd.0 == 0 {
            result.status = ProbeStatus::Failed("null hwnd".into());
            return result;
        }

        // Monitor attribution — always do this first; it's fast and
        // doesn't depend on the UIA apartment.
        result.monitor = read_monitor_info(hwnd).ok();

        // Lazy COM init + IUIAutomation client.
        let client_result = ensure_client();
        if let Err(e) = &client_result {
            result.status = ProbeStatus::Failed(format!("uia init: {e}"));
            return result;
        }

        // Pull the structured UIA payload while holding the thread-
        // local client borrow.
        let walked = UIA_CLIENT.with(|cell| {
            let borrow = cell.borrow();
            let client = borrow
                .as_ref()
                .expect("ensure_client succeeded but client is None");
            walk_window(client, hwnd)
        });

        match walked {
            Ok(walked) => {
                result.focused_field = walked.focused_field;
                result.visible_text_fragments = walked.fragments;
                result.control_summary = walked.summary;
                result.password_field_active = walked.password_field_active;

                if result.password_field_active {
                    // Redact in-place per Principle 8 / Wave 5 Q5.
                    if let Some(ff) = result.focused_field.as_mut() {
                        ff.value.clear();
                    }
                    result.visible_text_fragments.clear();
                }

                if walked.everything_empty {
                    result.status = ProbeStatus::NoPayload;
                }
            }
            Err(e) => {
                result.status = ProbeStatus::Failed(format!("uia walk: {e}"));
            }
        }
        result
    }
}

// --------------------------------------------------------------------
// COM apartment init
// --------------------------------------------------------------------

fn ensure_client() -> Result<(), String> {
    UIA_CLIENT.with(|cell| {
        if cell.borrow().is_some() {
            return Ok(());
        }
        // SAFETY: CoInitializeEx is idempotent per-thread; it returns
        // S_FALSE if already initialized with the same flags, RPC_E_CHANGED_MODE
        // if a different apartment was already established. We treat both
        // as success (we got an apartment we can use).
        unsafe {
            let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            // hr.is_ok() returns true for S_OK; S_FALSE comes back as Ok too
            // because windows-rs 0.56 represents both as success.
            if hr.is_err() {
                // RPC_E_CHANGED_MODE — apartment exists with a different
                // mode. The MTA in such a case is still usable for UIA
                // (UIA is apartment-aware); proceed.
                let code = hr.0;
                if code != windows::Win32::Foundation::RPC_E_CHANGED_MODE.0 {
                    return Err(format!("CoInitializeEx failed (HRESULT 0x{:08x})", code));
                }
            }
        }

        let automation: IUIAutomation = unsafe {
            CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
        }
        .map_err(|e| format!("CoCreateInstance(CUIAutomation): {e}"))?;

        let walker = unsafe { automation.ContentViewWalker() }
            .map_err(|e| format!("IUIAutomation::ContentViewWalker: {e}"))?;

        *cell.borrow_mut() = Some(UiaClient { automation, walker });
        Ok(())
    })
}

// --------------------------------------------------------------------
// Tree walk
// --------------------------------------------------------------------

struct WalkResult {
    focused_field: Option<FocusedField>,
    fragments: Vec<String>,
    summary: ControlSummary,
    password_field_active: bool,
    /// True iff focused_field, fragments, AND summary counts are all
    /// empty/zero — promotes the probe's status to `NoPayload`.
    everything_empty: bool,
}

fn walk_window(client: &UiaClient, hwnd: HWND) -> Result<WalkResult, String> {
    // Root element for the target window.
    let root: IUIAutomationElement = unsafe { client.automation.ElementFromHandle(hwnd) }
        .map_err(|e| format!("ElementFromHandle: {e}"))?;

    let mut summary = ControlSummary::default();
    let mut fragments: Vec<String> = Vec::new();

    // BFS by hand. We don't recurse — explicit stack avoids stack-
    // overflow on pathological Electron trees.
    let mut frontier: Vec<(IUIAutomationElement, u32)> = vec![(root, 0)];
    while let Some((elem, depth)) = frontier.pop() {
        if summary.elements_visited >= MAX_ELEMENTS {
            break;
        }
        summary.elements_visited += 1;
        classify_and_collect(&elem, &mut summary, &mut fragments);

        if depth >= MAX_DEPTH {
            continue;
        }

        // Push first child + iterate siblings.
        if let Ok(first) = unsafe { client.walker.GetFirstChildElement(&elem) } {
            // ElementFromHandle returns an Option<IUIAutomationElement>-shaped
            // result in newer windows-rs but here it's Result<IUIAutomationElement>
            // with the null case mapped to E_INVALIDARG. We tolerate "no
            // children" silently — that's the leaf case.
            let mut cursor = first;
            loop {
                if summary.elements_visited >= MAX_ELEMENTS {
                    break;
                }
                let next_sibling = unsafe { client.walker.GetNextSiblingElement(&cursor) }.ok();
                frontier.push((cursor, depth + 1));
                match next_sibling {
                    Some(s) => cursor = s,
                    None => break,
                }
            }
        }
    }

    // Focused element pulled separately — UIA distinguishes "focused
    // in the foreground window's subtree" via GetFocusedElement, which
    // returns the system-wide focused element (which IS what we want
    // since we already gated on foreground).
    let (focused_field, password_field_active) =
        match unsafe { client.automation.GetFocusedElement() } {
            Ok(focused) => read_focused_field(&focused),
            Err(_) => (None, false),
        };

    let everything_empty =
        focused_field.is_none() && fragments.is_empty() && summary.elements_visited == 0;

    Ok(WalkResult {
        focused_field,
        fragments,
        summary,
        password_field_active,
        everything_empty,
    })
}

// windows-rs spells the UIA control-type ids in non-upper-case-globals form
// (`UIA_EditControlTypeId` rather than `UIA_EDIT_CONTROL_TYPE_ID`); matching
// directly against them triggers `non_upper_case_globals` warnings. The
// `#[allow]` is scoped to this function rather than module-wide so future
// genuinely-bad identifiers still get flagged.
#[allow(non_upper_case_globals)]
fn classify_and_collect(
    elem: &IUIAutomationElement,
    summary: &mut ControlSummary,
    fragments: &mut Vec<String>,
) {
    // Control type is an i32 enum. We classify into the buckets the
    // detail UI surfaces; everything else lands in `other_count`.
    let ct = unsafe { elem.CurrentControlType() }
        .unwrap_or(windows::Win32::UI::Accessibility::UIA_CONTROLTYPE_ID(0));
    match ct {
        UIA_EditControlTypeId => summary.edit_count += 1,
        UIA_ButtonControlTypeId => summary.button_count += 1,
        UIA_DocumentControlTypeId => summary.document_count += 1,
        UIA_HyperlinkControlTypeId => summary.link_count += 1,
        UIA_TextControlTypeId => summary.text_count += 1,
        _ => summary.other_count += 1,
    }

    // Name + (when available) Value text. We DON'T descend into
    // ValuePattern for non-edit controls — it just duplicates `Name`
    // for most controls and is a perf hit on huge pages.
    let name = read_bstr(unsafe { elem.CurrentName() });
    if !name.is_empty() {
        push_fragment(fragments, name);
    }
    let is_edit_or_doc = ct == UIA_EditControlTypeId || ct == UIA_DocumentControlTypeId;
    if is_edit_or_doc {
        if let Some(v) = read_value_pattern(elem) {
            if !v.is_empty() {
                push_fragment(fragments, v);
            }
        }
    }
}

fn push_fragment(out: &mut Vec<String>, s: String) {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return;
    }
    let bounded = if trimmed.chars().count() > FRAGMENT_CHAR_CAP {
        let mut t: String = trimmed.chars().take(FRAGMENT_CHAR_CAP).collect();
        t.push('…');
        t
    } else {
        trimmed.to_string()
    };
    out.push(bounded);
}

fn read_focused_field(focused: &IUIAutomationElement) -> (Option<FocusedField>, bool) {
    let name = read_bstr(unsafe { focused.CurrentName() });
    let ctype = read_bstr(unsafe { focused.CurrentLocalizedControlType() });
    let is_password = unsafe { focused.CurrentIsPassword() }
        .unwrap_or(BOOL(0))
        .as_bool();

    let value = if is_password {
        // NEVER read a password field's value. Even the act of reading
        // is policy-violating — short-circuit.
        String::new()
    } else {
        read_value_pattern(focused).unwrap_or_default()
    };

    let any = !name.is_empty() || !ctype.is_empty() || !value.is_empty();
    if !any {
        return (None, is_password);
    }

    let ff = FocusedField {
        name,
        control_type: ctype,
        value: if value.chars().count() > FRAGMENT_CHAR_CAP {
            let mut t: String = value.chars().take(FRAGMENT_CHAR_CAP).collect();
            t.push('…');
            t
        } else {
            value
        },
    };
    (Some(ff), is_password)
}

fn read_value_pattern(elem: &IUIAutomationElement) -> Option<String> {
    let pat = unsafe { elem.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) }
        .ok()?;
    let bstr = unsafe { pat.CurrentValue() }.ok()?;
    let s = bstr.to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

fn read_bstr(result: windows::core::Result<windows::core::BSTR>) -> String {
    match result {
        Ok(b) => b.to_string(),
        Err(_) => String::new(),
    }
}

// --------------------------------------------------------------------
// Monitor info
// --------------------------------------------------------------------

fn read_monitor_info(hwnd: HWND) -> Result<MonitorInfo, String> {
    let hmon: HMONITOR = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    if hmon.0 == 0 {
        return Err("MonitorFromWindow returned null".into());
    }

    let mut info: MONITORINFOEXW = unsafe { std::mem::zeroed() };
    info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
    let ok = unsafe { GetMonitorInfoW(hmon, &mut info.monitorInfo as *mut MONITORINFO as *mut _) };
    if !ok.as_bool() {
        return Err("GetMonitorInfoW returned false".into());
    }

    let name = decode_utf16_nul(&info.szDevice);
    let RECT {
        left,
        top,
        right,
        bottom,
    } = info.monitorInfo.rcMonitor;
    let is_primary = (info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY) != 0;

    let dpi_scale = unsafe { GetDpiForWindow(hwnd) };
    let dpi_scale_opt = if dpi_scale > 0 {
        Some(dpi_scale as f32 / 96.0)
    } else {
        None
    };

    Ok(MonitorInfo {
        name,
        is_primary,
        bounds: Rect {
            left,
            top,
            right,
            bottom,
        },
        dpi_scale: dpi_scale_opt,
    })
}

fn decode_utf16_nul(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    // The substantive logic in this module is COM-driven and only
    // exercises against a live desktop. Pure helpers below get unit
    // coverage; the COM walks themselves are covered by the live
    // smoke matrix documented in the wave brief.

    #[test]
    fn decode_utf16_nul_handles_nul_terminator() {
        let buf: Vec<u16> = "DISPLAY1\0\0\0".encode_utf16().collect();
        assert_eq!(decode_utf16_nul(&buf), "DISPLAY1");
    }

    #[test]
    fn decode_utf16_nul_handles_full_buffer() {
        let buf: Vec<u16> = "abc".encode_utf16().collect();
        assert_eq!(decode_utf16_nul(&buf), "abc");
    }

    #[test]
    fn decode_utf16_nul_handles_empty() {
        assert_eq!(decode_utf16_nul(&[]), "");
    }

    #[test]
    fn push_fragment_trims_and_caps() {
        let mut out = Vec::new();
        push_fragment(&mut out, "   hello   ".into());
        assert_eq!(out, vec!["hello".to_string()]);

        push_fragment(&mut out, "".into());
        assert_eq!(out.len(), 1);

        push_fragment(&mut out, "   ".into());
        assert_eq!(out.len(), 1);

        let huge: String = std::iter::repeat('x')
            .take(FRAGMENT_CHAR_CAP + 50)
            .collect();
        push_fragment(&mut out, huge);
        assert!(out.last().unwrap().ends_with('…'));
        assert_eq!(out.last().unwrap().chars().count(), FRAGMENT_CHAR_CAP + 1);
    }

    #[test]
    fn windows_uia_probe_snapshot_with_null_hwnd_returns_failed() {
        // Doesn't touch COM — short-circuits at the null check.
        let mut p = WindowsUiaProbe::new();
        let r = p.snapshot(0, "a.exe", "T");
        assert_eq!(r.app, "a.exe");
        assert_eq!(r.title, "T");
        match r.status {
            ProbeStatus::Failed(reason) => assert!(reason.contains("null hwnd")),
            other => panic!("expected Failed, got {other:?}"),
        }
    }
}
