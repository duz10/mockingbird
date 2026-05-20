//! WASAPI loopback capture for the system default output device.
//!
//! Windows-only. Compiled out on macOS/Linux (the `mod loopback_windows`
//! line in `super` is gated `#[cfg(target_os = "windows")]`). When
//! Phase 9 lands cross-platform support, a sibling `loopback_macos.rs`
//! / `loopback_linux.rs` will mirror this trait impl.
//!
//! Wave 1 scaffold — struct + `todo!()` impl of [`super::capture::TwinStreamCapture`]
//! is intentionally deferred. Wave 3 picks between:
//!   (a) `cpal` 0.15's loopback support (preferred; pure-Rust);
//!   (b) the `wasapi` crate (direct binding; more control).
//! Decision recorded in the Wave 2 brief.

use crate::error::AppResult;

/// WASAPI loopback capture against the default render endpoint.
///
/// Pure stub in Wave 1; Wave 3 ships the real impl + the picked
/// backend (`cpal` loopback vs. `wasapi`).
#[derive(Debug, Default)]
pub struct LoopbackCapture {
    _private: (),
}

impl LoopbackCapture {
    pub fn new() -> AppResult<Self> {
        Ok(Self { _private: () })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Smoke: construct doesn't panic. Wave 3 replaces with cpal /
    /// wasapi-driven integration tests (gated `#[cfg(target_os = "windows")]`
    /// + `#[cfg(feature = "audio-hardware-tests")]` so CI without a
    /// loopback endpoint stays green).
    #[test]
    fn new_smoke() {
        let _ = LoopbackCapture::new().expect("construct");
    }
}
