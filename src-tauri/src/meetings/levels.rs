//! Per-channel dBFS levels for the live meeting VU display.
//!
//! Owned by [`crate::meetings::capture::TwinStreamCapture`] (one
//! `Arc<LevelsState>` shared across the two owner threads + the
//! tick-emitter thread in `lifecycle.rs`). The owner threads write
//! after every drain; the tick thread reads every 250ms and emits
//! `meeting:tick` to the overlay.
//!
//! ## Storage choice — `AtomicI32` not `Mutex<(f32, f32)>`
//!
//! We store each channel's level as **dBFS × 100, rounded** in an
//! [`AtomicI32`]. Lock-free reads + writes; the 0.01 dB granularity
//! is far below the visual perception threshold for any reasonable
//! VU bar (the eye can't distinguish < ~1 dB on a 100-pixel bar).
//! No risk of mutex poisoning across the owner threads.
//!
//! ## Sentinel convention
//!
//! * `0.0` (the initial atomic value) is the "no data yet" sentinel.
//!   The UI maps it to a flat bar (no glow).
//! * [`DBFS_FLOOR`] (`-100.0`) is "silence" — below any realistic
//!   noise floor. UI maps to a fully-dim bar.
//! * `0.0` returned from [`compute_dbfs`] on a non-empty buffer means
//!   full-scale i16 (clipping). UI may choose to flash the bar.
//!
//! ## Charter
//!
//! ADR 0032 / Phase MC v1.1 / bd: mb-nig. Companion to PLAN §MC.6's
//! `meeting:tick` event that Wave 1 of MC missed.

use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;

use crate::meetings::capture::Channel;

/// Below this we clamp. -100 dBFS is below the noise floor of any
/// realistic capture path.
pub const DBFS_FLOOR: f32 = -100.0;

/// Sentinel meaning "no samples observed yet on this channel".
pub const DBFS_NO_DATA: f32 = 0.0;

/// Peak-amplitude dBFS for an i16 PCM buffer.
///
/// * `samples.is_empty()` → [`DBFS_NO_DATA`] (== `0.0`). Treat as
///   "no data yet" upstream; **not** the same as "silence".
/// * All-zero buffer → [`DBFS_FLOOR`].
/// * `±i16::MAX` peak → `0.0` dBFS (full scale).
/// * Mid-range peak → `20 * log10(peak / 32767)`.
///
/// Pure: no allocation, no I/O, deterministic.
pub fn compute_dbfs(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return DBFS_NO_DATA;
    }
    // Peak amplitude. `i16::MIN.unsigned_abs()` is 32768 which fits
    // in u32; we cap at 32767 (i16::MAX) so the final ratio never
    // exceeds 1.0 (no positive-dBFS readings on hardware-clamped
    // signals).
    let peak = samples
        .iter()
        .map(|&s| s.unsigned_abs() as u32)
        .max()
        .unwrap_or(0)
        .min(i16::MAX as u32);
    if peak == 0 {
        return DBFS_FLOOR;
    }
    let ratio = peak as f32 / i16::MAX as f32;
    let db = 20.0 * ratio.log10();
    db.max(DBFS_FLOOR)
}

/// Lock-free holder for the most-recent (mic, sys) dBFS pair.
#[derive(Debug)]
pub struct LevelsState {
    mic_cdb: AtomicI32, // dBFS × 100, rounded
    sys_cdb: AtomicI32,
}

impl LevelsState {
    /// Fresh holder; both channels initialised to [`DBFS_NO_DATA`].
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            mic_cdb: AtomicI32::new(0),
            sys_cdb: AtomicI32::new(0),
        })
    }

    /// Compute the dBFS for `samples` and store under `channel`.
    /// Empty buffers DO write (the sentinel), so a long quiet drain
    /// still updates "we saw data, it was empty" — important so the
    /// tick thread can distinguish "channel inactive" (still the
    /// initial sentinel) from "channel active, currently silent".
    pub fn update(&self, channel: Channel, samples: &[i16]) {
        let db = compute_dbfs(samples);
        let cdb = (db * 100.0).round() as i32;
        let slot = match channel {
            Channel::Mic => &self.mic_cdb,
            Channel::Sys => &self.sys_cdb,
        };
        slot.store(cdb, Ordering::Relaxed);
    }

    /// Snapshot `(mic_db, sys_db)`. Each channel that has never been
    /// `update`d reports [`DBFS_NO_DATA`].
    pub fn snapshot(&self) -> (f32, f32) {
        let mic = self.mic_cdb.load(Ordering::Relaxed) as f32 / 100.0;
        let sys = self.sys_cdb.load(Ordering::Relaxed) as f32 / 100.0;
        (mic, sys)
    }
}

// =====================================================================
// Tests
// =====================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Tolerance for dBFS equality assertions (0.05 dB == half the
    /// storage quantum + headroom for `log10` rounding).
    const EPS: f32 = 0.05;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() <= EPS
    }

    #[test]
    fn silence_is_floor() {
        let buf = vec![0i16; 1024];
        let db = compute_dbfs(&buf);
        assert_eq!(db, DBFS_FLOOR, "all-zero buffer should clamp to DBFS_FLOOR");
    }

    #[test]
    fn full_scale_is_zero() {
        let buf = vec![i16::MAX; 1024];
        let db = compute_dbfs(&buf);
        assert!(approx(db, 0.0), "i16::MAX peak should be ~0 dBFS, got {db}");
    }

    #[test]
    fn half_scale_is_minus_six() {
        // amplitude ratio 0.5 → 20 * log10(0.5) ≈ -6.02 dBFS
        let buf = vec![i16::MAX / 2; 1024];
        let db = compute_dbfs(&buf);
        assert!(
            db > -6.5 && db < -5.5,
            "half-scale should be ~-6 dBFS, got {db}"
        );
    }

    #[test]
    fn empty_input_is_no_data_sentinel() {
        assert_eq!(compute_dbfs(&[]), DBFS_NO_DATA);
        assert_eq!(compute_dbfs(&[]), 0.0);
    }

    #[test]
    fn i16_min_does_not_overflow() {
        // i16::MIN.unsigned_abs() is 32768; ensure the cap-at-32767
        // path keeps the ratio ≤ 1.0 and the result ≤ 0.0.
        let buf = vec![i16::MIN; 64];
        let db = compute_dbfs(&buf);
        assert!(
            db <= 0.0 + EPS,
            "i16::MIN peak should clamp to ≤ 0 dBFS, got {db}"
        );
    }

    #[test]
    fn levels_state_update_then_snapshot_roundtrip() {
        let state = LevelsState::new();
        // Initial: both channels at sentinel.
        assert_eq!(state.snapshot(), (DBFS_NO_DATA, DBFS_NO_DATA));

        // Mic update only.
        state.update(Channel::Mic, &vec![i16::MAX; 256]);
        let (mic, sys) = state.snapshot();
        assert!(approx(mic, 0.0), "mic should read ~0 dBFS, got {mic}");
        assert_eq!(sys, DBFS_NO_DATA, "sys untouched");

        // Sys update; mic unaffected.
        state.update(Channel::Sys, &vec![0i16; 256]);
        let (mic, sys) = state.snapshot();
        assert!(approx(mic, 0.0), "mic still ~0, got {mic}");
        assert_eq!(sys, DBFS_FLOOR, "sys silenced");
    }
}
