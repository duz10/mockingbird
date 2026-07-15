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
//! ## No-data vs. silence (mb-x1d)
//!
//! [`compute_dbfs`] returns `Option<f32>` so "no data" is unambiguous
//! and can never collide with a legitimate reading:
//! * `None` — no samples observed yet (empty buffer). The UI maps it to
//!   a flat bar (no glow). Distinct from ANY real dBFS value.
//! * `Some(`[`DBFS_FLOOR`]`)` (`-100.0`) — "silence": an all-zero buffer,
//!   below any realistic noise floor. UI maps to a fully-dim bar.
//! * `Some(0.0)` — full-scale i16 (clipping). A real reading that the
//!   old `0.0` sentinel used to misread as "no data".
//!
//! In the lock-free [`LevelsState`] the "never updated" state is stored
//! as the reserved [`NO_DATA_CDB`] sentinel (`i32::MIN`, a value the
//! ×100 scaling can never produce), mapped back to `None` on read.
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

/// Reserved `cdb` (dBFS × 100) value meaning "no samples observed yet".
/// `i32::MIN` can never be produced by a real reading — `compute_dbfs`
/// is bounded to `[DBFS_FLOOR, 0.0]` → `[-10_000, 0]` after scaling — so
/// it unambiguously encodes "no data" in the atomics without colliding
/// with a legitimate full-scale `0.0` dBFS reading. (mb-x1d)
pub const NO_DATA_CDB: i32 = i32::MIN;

/// Peak-amplitude dBFS for an i16 PCM buffer.
///
/// * `samples.is_empty()` → `None` ("no data yet"; **not** "silence").
///   Explicitly distinct from every real reading, including full-scale
///   `Some(0.0)`. (mb-x1d)
/// * All-zero buffer → `Some(`[`DBFS_FLOOR`]`)`.
/// * `±i16::MAX` peak → `Some(0.0)` (full scale).
/// * Mid-range peak → `Some(20 * log10(peak / 32767))`.
///
/// Pure: no allocation, no I/O, deterministic.
pub fn compute_dbfs(samples: &[i16]) -> Option<f32> {
    if samples.is_empty() {
        return None;
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
        return Some(DBFS_FLOOR);
    }
    let ratio = peak as f32 / i16::MAX as f32;
    let db = 20.0 * ratio.log10();
    Some(db.max(DBFS_FLOOR))
}

/// Map a stored `cdb` back to `Option<f32>` dBFS, honoring the
/// [`NO_DATA_CDB`] sentinel.
fn read_cdb(slot: &AtomicI32) -> Option<f32> {
    let cdb = slot.load(Ordering::Relaxed);
    if cdb == NO_DATA_CDB {
        None
    } else {
        Some(cdb as f32 / 100.0)
    }
}

/// Lock-free holder for the most-recent (mic, sys) dBFS pair.
#[derive(Debug)]
pub struct LevelsState {
    mic_cdb: AtomicI32, // dBFS × 100, rounded
    sys_cdb: AtomicI32,
}

impl LevelsState {
    /// Fresh holder; both channels initialised to the [`NO_DATA_CDB`]
    /// sentinel (reported as `None` until the first `update`).
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            mic_cdb: AtomicI32::new(NO_DATA_CDB),
            sys_cdb: AtomicI32::new(NO_DATA_CDB),
        })
    }

    /// Compute the dBFS for `samples` and store under `channel`.
    /// Empty buffers DO write (the sentinel), so a long quiet drain
    /// still updates "we saw data, it was empty" — important so the
    /// tick thread can distinguish "channel inactive" (still the
    /// initial sentinel) from "channel active, currently silent".
    pub fn update(&self, channel: Channel, samples: &[i16]) {
        let cdb = match compute_dbfs(samples) {
            Some(db) => (db * 100.0).round() as i32,
            None => NO_DATA_CDB,
        };
        let slot = match channel {
            Channel::Mic => &self.mic_cdb,
            Channel::Sys => &self.sys_cdb,
        };
        slot.store(cdb, Ordering::Relaxed);
    }

    /// Snapshot `(mic_db, sys_db)`. Each channel that has never been
    /// `update`d (or whose last drain was empty) reports `None`. (mb-x1d)
    pub fn snapshot(&self) -> (Option<f32>, Option<f32>) {
        (read_cdb(&self.mic_cdb), read_cdb(&self.sys_cdb))
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
        assert_eq!(
            compute_dbfs(&buf),
            Some(DBFS_FLOOR),
            "all-zero buffer should clamp to DBFS_FLOOR"
        );
    }

    #[test]
    fn full_scale_is_zero() {
        let buf = vec![i16::MAX; 1024];
        let db = compute_dbfs(&buf).expect("non-empty buffer -> Some");
        assert!(approx(db, 0.0), "i16::MAX peak should be ~0 dBFS, got {db}");
    }

    #[test]
    fn half_scale_is_minus_six() {
        // amplitude ratio 0.5 → 20 * log10(0.5) ≈ -6.02 dBFS
        let buf = vec![i16::MAX / 2; 1024];
        let db = compute_dbfs(&buf).expect("non-empty buffer -> Some");
        assert!(
            db > -6.5 && db < -5.5,
            "half-scale should be ~-6 dBFS, got {db}"
        );
    }

    #[test]
    fn empty_input_is_none() {
        assert_eq!(compute_dbfs(&[]), None);
    }

    #[test]
    fn full_scale_zero_is_not_no_data() {
        // mb-x1d regression: a legitimate full-scale 0 dBFS reading must
        // be distinguishable from "no data yet". Under the old `0.0`
        // sentinel these were indistinguishable.
        let full = compute_dbfs(&vec![i16::MAX; 256]);
        assert_eq!(full, Some(0.0), "full-scale peak reads Some(0.0)");
        assert_ne!(full, compute_dbfs(&[]), "full-scale 0 dBFS != no-data");
    }

    #[test]
    fn i16_min_does_not_overflow() {
        // i16::MIN.unsigned_abs() is 32768; ensure the cap-at-32767
        // path keeps the ratio ≤ 1.0 and the result ≤ 0.0.
        let buf = vec![i16::MIN; 64];
        let db = compute_dbfs(&buf).expect("non-empty buffer -> Some");
        assert!(
            db <= 0.0 + EPS,
            "i16::MIN peak should clamp to ≤ 0 dBFS, got {db}"
        );
    }

    #[test]
    fn levels_state_update_then_snapshot_roundtrip() {
        let state = LevelsState::new();
        // Initial: both channels report no data.
        assert_eq!(state.snapshot(), (None, None));

        // Mic update only.
        state.update(Channel::Mic, &vec![i16::MAX; 256]);
        let (mic, sys) = state.snapshot();
        assert!(
            approx(mic.expect("mic updated"), 0.0),
            "mic should read ~0 dBFS, got {mic:?}"
        );
        assert_eq!(sys, None, "sys untouched");

        // Sys update; mic unaffected.
        state.update(Channel::Sys, &vec![0i16; 256]);
        let (mic, sys) = state.snapshot();
        assert!(
            approx(mic.expect("mic still set"), 0.0),
            "mic still ~0, got {mic:?}"
        );
        assert_eq!(sys, Some(DBFS_FLOOR), "sys silenced");
    }

    #[test]
    fn full_scale_update_is_distinct_from_fresh_channel() {
        // mb-x1d: a channel that drained a full-scale (0 dBFS) buffer
        // must NOT read the same as a never-updated channel.
        let state = LevelsState::new();
        state.update(Channel::Mic, &vec![i16::MAX; 256]);
        let (mic, sys) = state.snapshot();
        assert_eq!(mic, Some(0.0), "drained full-scale -> Some(0.0)");
        assert_eq!(sys, None, "never-updated -> None");
        assert_ne!(mic, sys);
    }
}
