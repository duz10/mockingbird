//! Linux implementation of [`super::Injector`].
//!
//! **Stub.** Phase 9 fills this in (likely `xdotool`/`ydotool`
//! semantics — synthetic keyboard events through evdev on Wayland,
//! XTest on X11). PLAN §12 #15.

#![cfg(target_os = "linux")]
#![allow(dead_code)]

use super::{InjectionOutcome, InjectionStrategy, Injector};
use crate::error::{AppError, AppResult};

/// Placeholder for the future evdev / X11 injector.
pub struct LinuxInjector;

impl LinuxInjector {
    /// Phase 9 fills this in.
    pub fn new() -> AppResult<Self> {
        Err(AppError::Injection(
            "Linux injector: Phase 9 (evdev uinput / XTest)".into(),
        ))
    }
}

impl Injector for LinuxInjector {
    fn inject(&self, _text: &str, _strategy: InjectionStrategy) -> AppResult<InjectionOutcome> {
        todo!("Phase 9 — Linux evdev / XTest injection")
    }
}
