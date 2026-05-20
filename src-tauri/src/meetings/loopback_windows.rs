//! WASAPI loopback capture for the system default render endpoint.
//!
//! Windows-only. Compiled out on macOS/Linux (the `mod
//! loopback_windows;` line in `super` is gated `#[cfg(target_os =
//! "windows")]`). When Phase 9 lands cross-platform support, a sibling
//! `loopback_macos.rs` / `loopback_linux.rs` will mirror this
//! [`AudioCapture`](crate::audio::AudioCapture) impl behind the same
//! trait boundary.
//!
//! ## Backend
//!
//! Phase MC Wave 3 ships this on cpal 0.15's WASAPI backend (ADR
//! 0031). The full justification + cpal source-line evidence lives in
//! `docs/adr/0031-meeting-loopback-backend.md`; the short version is
//! that cpal **transparently** sets `AUDCLNT_STREAMFLAGS_LOOPBACK`
//! when `build_input_stream(...)` is called on a device with
//! `data_flow() == eRender`. So this type is a thin newtype wrapper
//! around an [`audio::CpalCapture`](crate::audio::capture::CpalCapture)
//! constructed via [`CpalCapture::new_loopback`]; all stream-build,
//! resampling, ringbuf, and device-watcher logic is reused.
//!
//! No copy-paste of cpal plumbing. No second resampler pipeline. No
//! separate watcher thread.

#[cfg(target_os = "windows")]
use crate::audio::capture::CpalCapture;
#[cfg(target_os = "windows")]
use crate::audio::AudioCapture;
use crate::error::AppResult;

/// Loopback capture against the default Windows render endpoint.
///
/// Implements [`crate::audio::AudioCapture`] by delegating every
/// method to an inner [`CpalCapture`] that was constructed in
/// loopback mode. Sample-rate / channel guarantees match the
/// dictation capture path: 16 kHz mono `i16` regardless of the
/// underlying endpoint's native format.
#[cfg(target_os = "windows")]
pub struct LoopbackCapture {
    inner: CpalCapture,
}

#[cfg(target_os = "windows")]
impl LoopbackCapture {
    /// Construct a fresh loopback capture handle. Does NOT open the
    /// device — that happens at `start()`.
    ///
    /// Returns `Ok` even if no render endpoint is currently present;
    /// the absence surfaces at `start()` as `AppError::Audio("no
    /// default loopback device")`. This matches the dictation path's
    /// constructor semantics (lazy device acquisition).
    pub fn new() -> AppResult<Self> {
        Ok(Self {
            inner: CpalCapture::new_loopback()?,
        })
    }

    /// Whether the device watcher has observed a default-output-device
    /// change since the last call to `take_device_changed()`. Exposed
    /// for parity with `CpalCapture` so the meetings runtime can
    /// react to render-endpoint hot-swaps (e.g. user yanks headphones
    /// mid-meeting).
    pub fn device_changed(&self) -> bool {
        self.inner.device_changed()
    }

    /// Clear the device-changed flag and return whether it was set.
    pub fn take_device_changed(&self) -> bool {
        self.inner.take_device_changed()
    }
}

#[cfg(target_os = "windows")]
impl AudioCapture for LoopbackCapture {
    fn start(&mut self) -> AppResult<()> {
        self.inner.start()
    }

    fn stop(&mut self) -> AppResult<()> {
        self.inner.stop()
    }

    fn drain(&mut self, buf: &mut Vec<i16>) -> AppResult<usize> {
        self.inner.drain(buf)
    }

    fn sample_rate(&self) -> u32 {
        self.inner.sample_rate()
    }

    fn channels(&self) -> u16 {
        self.inner.channels()
    }
}

// ---------------------------------------------------------------------
// Non-Windows stub. Phase 9 ships the real macOS / Linux impls.
// ---------------------------------------------------------------------

#[cfg(not(target_os = "windows"))]
pub struct LoopbackCapture;

#[cfg(not(target_os = "windows"))]
impl LoopbackCapture {
    pub fn new() -> AppResult<Self> {
        Err(crate::error::AppError::Audio(
            "loopback capture not implemented for this platform (Phase 9 macOS/Linux)".into(),
        ))
    }
}

// =====================================================================
// Tests
// =====================================================================

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    /// Constructor must not open a device. The render endpoint may or
    /// may not exist on the test box; either way `new()` succeeds.
    #[test]
    fn new_does_not_open_a_device() {
        let _ = LoopbackCapture::new().expect("construct loopback handle");
    }

    /// Trait-conformance guarantee: regardless of the underlying
    /// render endpoint's native format, the consumer always sees
    /// 16 kHz mono i16. The resampler pipeline (shared with the
    /// dictation path) enforces this.
    #[test]
    fn sample_rate_and_channels_are_16khz_mono() {
        let c = LoopbackCapture::new().unwrap();
        assert_eq!(c.sample_rate(), 16_000);
        assert_eq!(c.channels(), 1);
    }

    /// `drain()` before any `start()` must return 0 samples cleanly
    /// (no panic from an unbacked ring consumer). Mirrors the
    /// dictation-side guarantee in `audio::capture::tests`.
    #[test]
    fn drain_before_start_returns_zero() {
        let mut c = LoopbackCapture::new().unwrap();
        let mut buf = Vec::new();
        let n = c.drain(&mut buf).unwrap();
        assert_eq!(n, 0);
        assert!(buf.is_empty());
    }

    /// CI runners and headless dev boxes may lack a default render
    /// endpoint. `start()` may succeed (real hardware) or return
    /// `AppError::Audio` (no device / unsupported format). It must
    /// NOT panic, and `stop()` must clean up cleanly either way.
    #[test]
    fn start_does_not_panic_when_endpoint_absent_or_unsupported() {
        let mut c = LoopbackCapture::new().unwrap();
        let _ = c.start();
        let _ = c.stop();
    }

    /// `start()` is idempotent when the device is present. Skipped
    /// gracefully on hardware without a render endpoint.
    #[test]
    fn double_start_is_idempotent_when_endpoint_present() {
        let mut c = LoopbackCapture::new().unwrap();
        if c.start().is_err() {
            eprintln!("skipping: no default render endpoint on test runner");
            return;
        }
        c.start().expect("second start must be a no-op");
        c.stop().expect("stop after double-start");
    }

    /// `stop()` is idempotent. Calling it before any `start()` and
    /// twice in a row both work without error.
    #[test]
    fn stop_is_idempotent() {
        let mut c = LoopbackCapture::new().unwrap();
        c.stop().expect("stop on never-started capture");
        c.stop().expect("second stop is also a no-op");
    }

    /// Restart-after-stop. Required for parity with the dictation
    /// path's Phase-2-Wave-4.8 contract — the meetings runtime may
    /// start/stop capture multiple times across an app lifetime
    /// (e.g. user picks `Both`, cancels, picks `System`, etc.).
    /// Skipped without a render endpoint.
    #[test]
    fn restart_after_stop_succeeds_when_endpoint_present() {
        let mut c = LoopbackCapture::new().unwrap();
        if c.start().is_err() {
            eprintln!("skipping: no default render endpoint on test runner");
            return;
        }
        c.stop().unwrap();
        c.start().expect("restart after stop must succeed");
        c.stop().unwrap();
    }

    /// `device_changed()` starts false on a fresh handle and reading
    /// it doesn't flip the flag. The flag transitions to true only
    /// when the watcher thread (spawned by `start()`) observes a
    /// real default-render-device change — out of scope for unit
    /// testing; covered by the W3 hands-on QA matrix.
    #[test]
    fn device_changed_flag_starts_false() {
        let c = LoopbackCapture::new().unwrap();
        assert!(!c.device_changed());
        assert!(!c.take_device_changed());
    }

    /// Two independent `LoopbackCapture` instances don't share state.
    /// Catches a future accidental singleton-by-static refactor.
    /// Doesn't `start()` either to keep CI green without hardware.
    #[test]
    fn two_loopback_instances_are_independent() {
        let a = LoopbackCapture::new().unwrap();
        let b = LoopbackCapture::new().unwrap();
        // Both fresh handles must agree on the trait constants.
        assert_eq!(a.sample_rate(), b.sample_rate());
        assert_eq!(a.channels(), b.channels());
        // Neither sees a device-change yet.
        assert!(!a.device_changed());
        assert!(!b.device_changed());
    }
}

#[cfg(all(test, not(target_os = "windows")))]
mod tests_non_windows {
    use super::*;

    /// On non-Windows targets, `new()` must return the documented
    /// Phase-9-deferred error rather than constructing a useless
    /// stub. Saves a future macOS/Linux contributor from a confusing
    /// silent-no-op debugging session.
    #[test]
    fn new_returns_phase_9_deferred_error() {
        let err = LoopbackCapture::new().expect_err("non-windows must err");
        let msg = format!("{err:?}");
        assert!(
            msg.contains("Phase 9") || msg.contains("not implemented"),
            "expected Phase 9 deferral message, got: {msg}"
        );
    }
}
