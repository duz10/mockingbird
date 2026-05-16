//! Windows `SendInput` implementation of [`super::Injector`].
//!
//! **Stub in Wave 1.** Full impl lands in Wave 4 (bd `mb-x7i`):
//! - `Paste` strategy → SendInput Ctrl+V (paste.rs has populated the
//!   clipboard via the save/restore dance).
//! - `Keystroke` strategy → per-character `SendInput` with
//!   `KEYEVENTF_UNICODE`, batching UTF-16 surrogate pairs into a
//!   single `SendInput` call so non-BMP characters (emoji,
//!   mathematical alphanumeric, CJK extension B) survive.
//! - `Abort` strategy → no-op, returns `InjectionOutcome::AbortedUserOptOut`.

use super::{InjectionOutcome, InjectionStrategy, Injector};
use crate::error::{AppError, AppResult};

/// Stub `SendInput`-based injector.
#[derive(Default)]
pub struct SendInputInjector;

impl SendInputInjector {
    /// Construct an injector. No OS resources are acquired.
    pub fn new() -> AppResult<Self> {
        Ok(Self)
    }
}

impl Injector for SendInputInjector {
    fn inject(&self, _text: &str, _strategy: InjectionStrategy) -> AppResult<InjectionOutcome> {
        Err(AppError::Injection(
            "SendInput injector lands in Wave 4 (bd mb-x7i)".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_succeeds() {
        assert!(SendInputInjector::new().is_ok());
    }

    #[test]
    fn inject_is_a_clear_wave4_error() {
        let injector = SendInputInjector::new().expect("construct");
        let err = injector
            .inject("hello", InjectionStrategy::Paste)
            .unwrap_err();
        match err {
            AppError::Injection(msg) => assert!(msg.contains("Wave 4")),
            other => panic!("expected Injection, got {other:?}"),
        }
    }
}
