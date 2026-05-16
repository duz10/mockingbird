//! Windows `SendInput` implementation of [`super::Injector`].
//!
//! Three strategies (per [`InjectionStrategy`]):
//!
//! - **Paste**: `paste::with_saved_clipboard(text, send_ctrl_v)` —
//!   clipboard is saved + payload written + Ctrl+V via `SendInput` +
//!   clipboard restored. All clipboard-touching code lives in
//!   `paste.rs` per PLAN §12 #17.
//! - **Keystroke**: Each `char` of `text` is sent as a synthetic
//!   `KEYEVENTF_UNICODE` event. Surrogate pairs are handled by
//!   `str::encode_utf16()` — emitting two `INPUT` entries for chars
//!   beyond the BMP.
//! - **Abort**: `InjectionOutcome::AbortedUserOptOut`. No OS calls.
//!
//! ## Pure-vs-OS split
//!
//! [`build_unicode_input_events`] converts a `&str` into a `Vec<INPUT>`
//! of keystroke events (each char = down + up; surrogate pairs = 4
//! events). It's pure — covered by unit tests with synthesised
//! input. The `SendInput` call is a one-line shim that hands the
//! vector to the OS.

use super::{InjectionOutcome, InjectionStrategy, Injector};
use crate::error::{AppError, AppResult};

#[cfg(target_os = "windows")]
use super::paste::{self, PasteOutcome};

/// Delay between the clipboard write and `SendInput(Ctrl+V)`.
/// ADR 0018 §4 mandates a non-zero guard so the target app has time
/// to notice the new clipboard contents before we synthesise the
/// paste keystroke. Defaults to 30 ms; the orchestrator may make this
/// configurable in Wave 5 if QA discovers slow apps.
#[cfg(target_os = "windows")]
pub const PASTE_GUARD: std::time::Duration = std::time::Duration::from_millis(30);

/// `SendInput`-based injector. Stateless — every `inject()` call is
/// independent.
#[derive(Default)]
pub struct SendInputInjector;

impl SendInputInjector {
    /// Construct an injector. No OS resources are acquired.
    pub fn new() -> AppResult<Self> {
        Ok(Self)
    }
}

impl Injector for SendInputInjector {
    fn inject(&self, text: &str, strategy: InjectionStrategy) -> AppResult<InjectionOutcome> {
        match strategy {
            InjectionStrategy::Abort => Ok(InjectionOutcome::AbortedUserOptOut),
            InjectionStrategy::Paste => paste_path(text),
            InjectionStrategy::Keystroke => keystroke_path(text),
        }
    }
}

// --------------------------------------------------------------------
// Strategy paths
// --------------------------------------------------------------------

#[cfg(target_os = "windows")]
fn paste_path(text: &str) -> AppResult<InjectionOutcome> {
    let outcome = paste::with_saved_clipboard(text, || {
        std::thread::sleep(PASTE_GUARD);
        send_ctrl_v()
    })?;
    Ok(match outcome {
        PasteOutcome::Ok => InjectionOutcome::Ok,
        PasteOutcome::OkClipboardNotRestored => InjectionOutcome::OkClipboardNotRestored,
    })
}

#[cfg(not(target_os = "windows"))]
fn paste_path(_text: &str) -> AppResult<InjectionOutcome> {
    Err(AppError::Injection(
        "paste injection is Windows-only (Phase 9 platform parity)".into(),
    ))
}

#[cfg(target_os = "windows")]
fn keystroke_path(text: &str) -> AppResult<InjectionOutcome> {
    let events = build_unicode_input_events(text);
    if events.is_empty() {
        return Ok(InjectionOutcome::Ok); // empty text is trivially "delivered"
    }
    send_inputs(&events)
}

#[cfg(not(target_os = "windows"))]
fn keystroke_path(_text: &str) -> AppResult<InjectionOutcome> {
    Err(AppError::Injection(
        "keystroke injection is Windows-only (Phase 9 platform parity)".into(),
    ))
}

// --------------------------------------------------------------------
// Pure: build_unicode_input_events
// --------------------------------------------------------------------

/// Description of a single synthesised keystroke event. The Wave-4
/// keystroke path emits down + up for each UTF-16 code unit; surrogate
/// pairs naturally yield 4 events.
///
/// Kept as a plain struct (not the Win32 `INPUT` union) so the helper
/// is portable and trivially testable on non-Windows targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeystrokeEvent {
    /// UTF-16 code unit. For surrogate pairs the high surrogate comes
    /// first, then the low surrogate.
    pub scan: u16,
    /// `true` = key release; `false` = key press.
    pub key_up: bool,
}

/// Convert a `&str` to the ordered list of synthesised events.
///
/// Each `char` becomes:
/// - 1 down + 1 up if the char is in the BMP (`< 0x10000`)
/// - 2 down + 2 up if the char is non-BMP (surrogate pair)
///
/// `\n` / `\r` chars produce no events — newlines are app-specific
/// and we don't want to synthesise an Enter keystroke (that could
/// submit a form). Callers wanting newline-preservation should use
/// the Paste strategy instead.
pub fn build_unicode_input_events(text: &str) -> Vec<KeystrokeEvent> {
    let mut events = Vec::with_capacity(text.len() * 2);
    for c in text.chars() {
        if c == '\n' || c == '\r' {
            // Skip; document via &str docs (above).
            continue;
        }
        let mut buf = [0u16; 2];
        let units = c.encode_utf16(&mut buf);
        // Press each code unit, then release each code unit. This
        // pattern is what works empirically across all the targets
        // in our QA matrix; releasing high before pressing low can
        // mis-render in some apps (Chrome address bar specifically).
        for &u in units.iter() {
            events.push(KeystrokeEvent {
                scan: u,
                key_up: false,
            });
        }
        for &u in units.iter() {
            events.push(KeystrokeEvent {
                scan: u,
                key_up: true,
            });
        }
    }
    events
}

// --------------------------------------------------------------------
// OS shims
// --------------------------------------------------------------------

#[cfg(target_os = "windows")]
fn send_ctrl_v() -> AppResult<()> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY,
    };

    const VK_CONTROL: u16 = 0x11;
    const VK_V: u16 = 0x56;

    let mk = |vk: u16, key_up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: 0,
                dwFlags: if key_up {
                    KEYEVENTF_KEYUP
                } else {
                    Default::default()
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };

    let inputs = [
        mk(VK_CONTROL, false),
        mk(VK_V, false),
        mk(VK_V, true),
        mk(VK_CONTROL, true),
    ];
    // SAFETY: `inputs` is a stack array sized at compile time; SendInput reads it.
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        return Err(AppError::Injection(format!(
            "SendInput(Ctrl+V) delivered {sent}/{} events",
            inputs.len()
        )));
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn send_inputs(events: &[KeystrokeEvent]) -> AppResult<InjectionOutcome> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE,
        VIRTUAL_KEY,
    };

    // Map each pure KeystrokeEvent to an INPUT struct.
    let inputs: Vec<INPUT> = events
        .iter()
        .map(|e| {
            let mut flags = KEYEVENTF_UNICODE;
            if e.key_up {
                flags |= KEYEVENTF_KEYUP;
            }
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(0), // ignored for KEYEVENTF_UNICODE
                        wScan: e.scan,
                        dwFlags: flags,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            }
        })
        .collect();

    // SAFETY: `inputs` is `&[INPUT]`; SendInput reads it.
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if (sent as usize) != inputs.len() {
        return Err(AppError::Injection(format!(
            "SendInput(unicode keystrokes) delivered {sent}/{} events",
            inputs.len()
        )));
    }
    Ok(InjectionOutcome::Ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_succeeds() {
        assert!(SendInputInjector::new().is_ok());
    }

    #[test]
    fn abort_strategy_yields_user_opt_out_with_no_os_calls() {
        let injector = SendInputInjector::new().expect("construct");
        let outcome = injector
            .inject("anything", InjectionStrategy::Abort)
            .expect("Abort never returns Err");
        assert_eq!(outcome, InjectionOutcome::AbortedUserOptOut);
    }

    // -----------------------------------------------------------------
    // build_unicode_input_events — pure, deterministic
    // -----------------------------------------------------------------

    #[test]
    fn ascii_produces_paired_down_up_per_char() {
        let events = build_unicode_input_events("ab");
        // 'a' down, 'a' up, 'b' down, 'b' up.
        assert_eq!(events.len(), 4);
        assert_eq!(
            events[0],
            KeystrokeEvent {
                scan: 'a' as u16,
                key_up: false
            }
        );
        assert_eq!(
            events[1],
            KeystrokeEvent {
                scan: 'a' as u16,
                key_up: true
            }
        );
        assert_eq!(
            events[2],
            KeystrokeEvent {
                scan: 'b' as u16,
                key_up: false
            }
        );
        assert_eq!(
            events[3],
            KeystrokeEvent {
                scan: 'b' as u16,
                key_up: true
            }
        );
    }

    #[test]
    fn empty_string_produces_zero_events() {
        assert!(build_unicode_input_events("").is_empty());
    }

    #[test]
    fn bmp_unicode_produces_one_pair() {
        // '世' = U+4E16, in BMP.
        let events = build_unicode_input_events("世");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].scan, 0x4E16);
        assert_eq!(events[1].scan, 0x4E16);
    }

    #[test]
    fn non_bmp_emoji_produces_surrogate_pair() {
        // 🐦 = U+1F426, encodes as surrogate pair D83D / DC26.
        let events = build_unicode_input_events("🐦");
        // High surrogate down + low surrogate down + high surrogate up + low surrogate up.
        assert_eq!(events.len(), 4);
        assert_eq!(
            events[0],
            KeystrokeEvent {
                scan: 0xD83D,
                key_up: false
            }
        );
        assert_eq!(
            events[1],
            KeystrokeEvent {
                scan: 0xDC26,
                key_up: false
            }
        );
        assert_eq!(
            events[2],
            KeystrokeEvent {
                scan: 0xD83D,
                key_up: true
            }
        );
        assert_eq!(
            events[3],
            KeystrokeEvent {
                scan: 0xDC26,
                key_up: true
            }
        );
    }

    #[test]
    fn newlines_are_skipped() {
        // Per docs, newlines don't produce events — orchestrator should
        // route multi-line text through Paste strategy.
        let events = build_unicode_input_events("a\nb\r\nc");
        // Only 'a', 'b', 'c' — 6 events.
        assert_eq!(events.len(), 6);
        assert!(
            events
                .iter()
                .all(|e| e.scan != b'\n' as u16 && e.scan != b'\r' as u16),
            "no newline events: {events:?}"
        );
    }

    #[test]
    fn mixed_bmp_and_non_bmp_preserves_order() {
        let events = build_unicode_input_events("a🐦b");
        // 'a' (2) + '🐦' (4) + 'b' (2) = 8 events.
        assert_eq!(events.len(), 8);
        assert_eq!(events[0].scan, b'a' as u16);
        assert_eq!(events[2].scan, 0xD83D); // high surrogate
        assert_eq!(events[6].scan, b'b' as u16);
    }

    #[test]
    fn long_text_does_not_panic() {
        // 10k chars exercises the Vec growth path.
        let text: String = "a".repeat(10_000);
        let events = build_unicode_input_events(&text);
        assert_eq!(events.len(), 20_000);
    }

    // -----------------------------------------------------------------
    // Live tests (require interactive desktop with focused app)
    // -----------------------------------------------------------------

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "live SendInput; need a focused app to receive — run manually"]
    fn live_keystroke_injects_into_focused_app() {
        let injector = SendInputInjector::new().unwrap();
        let outcome = injector
            .inject("mockingbird-test", InjectionStrategy::Keystroke)
            .unwrap();
        assert_eq!(outcome, InjectionOutcome::Ok);
    }
}
