//! UIA (UI Automation) deep-snapshot probe.
//!
//! Phase 10 Wave 2 promotes the activity-capture sampler from
//! titles-only to **full UIA snapshots** — the focused field, a
//! flattened pass of visible text fragments, a control-type
//! summary, monitor attribution, and a password-field-active
//! redaction flag.
//!
//! ## Cross-platform boundary (Principle 5)
//!
//! The [`Probe`] trait is target-agnostic. The Windows impl lives
//! in [`windows_com`] behind a `#[cfg(target_os = "windows")]`; all
//! other targets fall through to [`StubProbe`] which returns a
//! [`ProbeStatus::Failed`] result with the app + title still populated.
//! Phase 9 will fill in macOS via the AX API.
//!
//! ## Why a separate module + trait (rather than inlining COM into the sampler)?
//!
//! - **Testability.** The Wave 2 brief documents that UIA-tree walks
//!   on real apps vary wildly (Steam = nothing, Settings = great,
//!   Electron = whatever). We want to swap in fakes that yield each
//!   shape during sampler unit tests without touching COM.
//! - **Phase-9 macOS swap-out.** A non-trait alternative would mean
//!   ifdef-ing platform code throughout `sampler.rs`. The trait keeps
//!   the platform surface inside this module.
//! - **Per-tick recovery.** If the Windows COM call fails (apartment
//!   gone, target HWND died mid-probe), we return [`ProbeStatus::Failed`]
//!   with a non-panicking payload. The runtime still records the
//!   `context_snapshot` row with degraded content.
//!
//! ## ADR decision (Wave 2 brief)
//!
//! We use raw `windows`-rs `Win32_UI_Accessibility` rather than the
//! third-party `uiautomation` crate. Rationale: no new external
//! dependency, consistent with how Phase 3 + Phase MC use the
//! `windows` crate, and the surface area of UIA we actually need
//! (CoInit + IUIAutomation + ElementFromHandle + a single
//! TreeWalker pass + a handful of property gets) is small enough
//! that a third-party wrapper doesn't earn its keep. The full
//! trade-off lives in `docs/phases/phase10-wave2-brief.md`.

pub mod payload;

#[cfg(target_os = "windows")]
pub mod windows_com;

pub use payload::{
    title_only, to_payload_json, ControlSummary, FocusedField, MonitorInfo, ProbeResult,
    ProbeStatus, Rect, MAX_PAYLOAD_BYTES,
};

/// The activity-sampler's view of a deep window snapshot.
///
/// One method, two-arg shape (HWND opaque-isize + app/title from
/// the cheap foreground probe): the platform impl decides what to do
/// with the HWND; non-Windows impls ignore it. App + title are
/// passed in because the foreground probe runs anyway (cheap) — the
/// UIA probe needs them populated even on its failure paths so the
/// JSON has the load-bearing fields.
pub trait Probe: Send {
    /// Snapshot the foreground window. Must NEVER panic; on failure,
    /// return a [`ProbeResult`] with status [`ProbeStatus::Failed`]
    /// and at least `app` + `title` populated.
    ///
    /// `hwnd_isize` is the platform-opaque foreground-window handle.
    /// On Windows it round-trips through `HWND(_)` inside the COM
    /// impl. On other platforms it's a hint (or ignored).
    fn snapshot(&mut self, hwnd_isize: isize, app: &str, title: &str) -> ProbeResult;
}

/// Construct the platform-default probe.
///
/// Returns the Windows COM-backed probe when `cfg(windows)`,
/// otherwise the stub probe. Either way the trait surface is
/// satisfied so the sampler never needs a `cfg` block.
pub fn make_default_probe() -> Box<dyn Probe> {
    #[cfg(target_os = "windows")]
    {
        Box::new(windows_com::WindowsUiaProbe::new())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Box::new(StubProbe::new(
            "UIA probe not implemented on this platform (Phase 9)",
        ))
    }
}

// --------------------------------------------------------------------
// Non-Windows stub probe
// --------------------------------------------------------------------

/// A probe that returns a [`ProbeStatus::Failed`] payload with the
/// `app` + `title` echoed. Used on non-Windows hosts.
pub struct StubProbe {
    reason: String,
}

impl StubProbe {
    /// Construct a stub probe carrying the explanatory `reason` to
    /// surface in every [`ProbeStatus::Failed`] result.
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }
}

impl Probe for StubProbe {
    fn snapshot(&mut self, _hwnd: isize, app: &str, title: &str) -> ProbeResult {
        title_only(app, title, &self.reason)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_probe_returns_failed_with_app_title() {
        let mut p = StubProbe::new("test");
        let r = p.snapshot(0, "notepad.exe", "Untitled");
        assert_eq!(r.app, "notepad.exe");
        assert_eq!(r.title, "Untitled");
        match r.status {
            ProbeStatus::Failed(reason) => assert_eq!(reason, "test"),
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn make_default_probe_returns_something() {
        // Smoke — just construct the boxed trait object.
        let _ = make_default_probe();
    }

    #[test]
    fn stub_probe_payload_serializes_with_title_only_shape() {
        let mut p = StubProbe::new("no_uia");
        let r = p.snapshot(0, "a.exe", "T");
        let (json, _) = to_payload_json(&r);
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["app"], "a.exe");
        assert_eq!(v["status"]["kind"], "failed");
    }
}
