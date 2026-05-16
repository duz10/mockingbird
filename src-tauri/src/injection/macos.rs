//! macOS implementation of [`super::Injector`].
//!
//! **Stub.** Phase 9 fills this in (likely `CGEventCreateKeyboardEvent`
//! + NSPasteboard for the paste path). PLAN §12 #15.

#![cfg(target_os = "macos")]
#![allow(dead_code)]

use super::{InjectionOutcome, InjectionStrategy, Injector};
use crate::error::{AppError, AppResult};

/// Placeholder for the future CoreGraphics-based injector.
pub struct MacInjector;

impl MacInjector {
    /// Phase 9 fills this in.
    pub fn new() -> AppResult<Self> {
        Err(AppError::Injection(
            "macOS injector: Phase 9 (CGEvent + NSPasteboard)".into(),
        ))
    }
}

impl Injector for MacInjector {
    fn inject(&self, _text: &str, _strategy: InjectionStrategy) -> AppResult<InjectionOutcome> {
        todo!("Phase 9 — macOS CGEvent + NSPasteboard injection")
    }
}
