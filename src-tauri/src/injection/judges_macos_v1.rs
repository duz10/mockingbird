//! macOS injection judges (Phase 3 dictation, Wave B2).
//!
//! Two deterministic probes, mirroring the in-tree judge pattern
//! ([`crate::secrets::judges_macos_v1`], [`crate::audio::judges_macos_v1`]):
//!
//! - [`paste_clipboard_saverestore_probe`] — `mac-p3b-paste-clipboard-saverestore`
//!   (mb-mac-v1.4.2). Sets the pasteboard to a known sentinel `X`, runs
//!   the clipboard save/restore flow with a **no-op paste closure**
//!   (the Cmd+V keypost needs Accessibility and is verified in the
//!   permission-gated `mac-p3-dictation-e2e`; the SAVE/RESTORE is the
//!   deterministic part this probe asserts), then asserts the pasteboard
//!   is back to `X`.
//! - [`secure_input_aborts_probe`] — `mac-p3c-secure-input-aborts`
//!   (mb-mac-v1.4.3). Drives [`MacSecureInputGuard`] through the
//!   mockable [`MacSecureInputProbe`] seam: a focused-element role of
//!   `AXSecureTextField` (or system-wide secure input) ⇒ guard reports
//!   secure ⇒ [`inject_secure_guarded`] aborts WITHOUT calling the
//!   injector; a normal field ⇒ the injector is called. No real
//!   Accessibility grant required.
//!
//! macOS-only; compiles to nothing elsewhere.

#![cfg(target_os = "macos")]

use std::sync::atomic::{AtomicUsize, Ordering};

use super::macos::inject_secure_guarded;
use super::paste::{self, PasteOutcome};
use super::secure_guard::{
    MacSecureInputGuard, MacSecureInputProbe, SecureInputGuard, AX_SECURE_TEXT_FIELD_ROLE,
};
use super::{InjectionOutcome, InjectionStrategy, Injector};
use crate::error::AppResult;
use crate::window_context::ForegroundWindow;

/// Outcome of the clipboard save/restore probe.
#[derive(Debug, Clone)]
pub struct PasteProbeReport {
    /// The `with_saved_clipboard` outcome (`Ok` / `OkClipboardNotRestored`).
    pub outcome: PasteOutcome,
    /// The sentinel the probe planted + verified was restored.
    pub sentinel: String,
}

/// Set pasteboard to sentinel `X`, run the save/restore flow (writes a
/// payload `Y`, no-op paste, restores `X`), assert pasteboard == `X`.
pub fn paste_clipboard_saverestore_probe() -> Result<PasteProbeReport, String> {
    use arboard::Clipboard;

    const SENTINEL: &str = "mockingbird-clip-sentinel-\u{1f512}-X";
    const PAYLOAD: &str = "mockingbird-dictation-payload-Y";

    // 1. Plant sentinel X.
    {
        let mut cb = Clipboard::new().map_err(|e| format!("open pasteboard (setup): {e}"))?;
        cb.set_text(SENTINEL.to_owned())
            .map_err(|e| format!("set sentinel: {e}"))?;
    }

    // 2. Run the save/restore flow with a no-op paste closure. This
    //    exercises the exact macOS `paste::with_saved_clipboard` path
    //    the injector uses, minus the Accessibility-gated keypost.
    let outcome = paste::with_saved_clipboard(PAYLOAD, || Ok(()))
        .map_err(|e| format!("with_saved_clipboard: {e}"))?;

    // 3. Assert the sentinel is back.
    let after = {
        let mut cb = Clipboard::new().map_err(|e| format!("open pasteboard (verify): {e}"))?;
        cb.get_text().map_err(|e| format!("get text after: {e}"))?
    };
    if after != SENTINEL {
        return Err(format!(
            "save/restore failed: clipboard after = {after:?}, expected sentinel {SENTINEL:?} \
             (outcome = {outcome:?})"
        ));
    }

    Ok(PasteProbeReport {
        outcome,
        sentinel: SENTINEL.to_string(),
    })
}

/// Outcome of the secure-input abort probe.
#[derive(Debug, Clone)]
pub struct SecureProbeReport {
    /// The AX role the probe used to trigger the secure path.
    pub secure_role: String,
}

/// Mockable probe feeding fixed signals to [`MacSecureInputGuard`].
struct FakeSecureProbe {
    role: Option<String>,
    secure_event_input: bool,
}

impl MacSecureInputProbe for FakeSecureProbe {
    fn focused_role(&self) -> Option<String> {
        self.role.clone()
    }
    fn secure_event_input_enabled(&self) -> bool {
        self.secure_event_input
    }
}

/// Spy injector that records how many times `inject` was called and
/// otherwise reports success. Used to prove the secure path never calls
/// the injector (no paste / no keypost).
struct RecordingInjector {
    calls: AtomicUsize,
}

impl RecordingInjector {
    fn new() -> Self {
        Self {
            calls: AtomicUsize::new(0),
        }
    }
    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl Injector for RecordingInjector {
    fn inject(&self, _text: &str, _strategy: InjectionStrategy) -> AppResult<InjectionOutcome> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(InjectionOutcome::Ok)
    }
}

fn dummy_fg() -> ForegroundWindow {
    ForegroundWindow {
        hwnd: 0,
        title: "probe".into(),
        process_name: "probe".into(),
        exe_path: None,
    }
}

/// Assert: secure field ⇒ guard secure + injection aborts (no paste);
/// system-wide secure input ⇒ secure; normal field ⇒ injection proceeds.
pub fn secure_input_aborts_probe() -> Result<SecureProbeReport, String> {
    let fg = dummy_fg();

    // --- Secure case: focused AXSecureTextField ⇒ abort, no paste. ---
    let secure_guard = MacSecureInputGuard::with_probe(Box::new(FakeSecureProbe {
        role: Some(AX_SECURE_TEXT_FIELD_ROLE.to_string()),
        secure_event_input: false,
    }));
    if !secure_guard.is_secure(&fg) {
        return Err("guard did not report secure for AXSecureTextField role".into());
    }
    let injector = RecordingInjector::new();
    let outcome = inject_secure_guarded(
        &injector,
        &secure_guard,
        &fg,
        "secret",
        InjectionStrategy::Paste,
    )
    .map_err(|e| format!("guarded inject (secure): {e}"))?;
    if outcome != InjectionOutcome::AbortedSecure {
        return Err(format!("expected AbortedSecure, got {outcome:?}"));
    }
    if injector.calls() != 0 {
        return Err(format!(
            "injector was called {} time(s) on a secure field; must be 0 (no paste)",
            injector.calls()
        ));
    }

    // --- Secure via system-wide secure event input alone. ---
    let sei_guard = MacSecureInputGuard::with_probe(Box::new(FakeSecureProbe {
        role: Some("AXTextField".to_string()),
        secure_event_input: true,
    }));
    if !sei_guard.is_secure(&fg) {
        return Err("guard did not report secure when IsSecureEventInputEnabled() is true".into());
    }

    // --- Non-secure case: normal field ⇒ injection proceeds. ---
    let open_guard = MacSecureInputGuard::with_probe(Box::new(FakeSecureProbe {
        role: Some("AXTextField".to_string()),
        secure_event_input: false,
    }));
    if open_guard.is_secure(&fg) {
        return Err("guard reported secure for a normal AXTextField".into());
    }
    let injector2 = RecordingInjector::new();
    let _ = inject_secure_guarded(
        &injector2,
        &open_guard,
        &fg,
        "hello",
        InjectionStrategy::Paste,
    )
    .map_err(|e| format!("guarded inject (open): {e}"))?;
    if injector2.calls() != 1 {
        return Err(format!(
            "injector should have been called once for a normal field; got {}",
            injector2.calls()
        ));
    }

    Ok(SecureProbeReport {
        secure_role: AX_SECURE_TEXT_FIELD_ROLE.to_string(),
    })
}
