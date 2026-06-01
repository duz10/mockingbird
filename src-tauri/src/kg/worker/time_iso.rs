//! Pure time / ISO-8601 helpers used across the worker.
//!
//! Split out of `worker.rs` during Wave 1E.7 Part 2 (`mb-5lla`) so
//! the runtime + main-loop file stays under the 600-LoC cap. No
//! behaviour change vs. the pre-split helpers; only the home moves.
//!
//! Everything here is pure (no DB / no FS / no time-zone DB) so it
//! can live-test through the LESSONS P2 throwaway-crate recipe with
//! zero glue.

use std::time::SystemTime;

/// 30-day TTL multiplier (epoch-ms axis). Lives here because the
/// only consumer is [`retention_cutoff_iso`].
const MS_PER_DAY: i64 = 86_400_000;

/// `now()` as an ISO-8601 UTC string with millisecond precision.
pub(super) fn now_iso() -> String {
    iso_from_ms(now_ms())
}

/// ISO-8601 string for "`days` ago", millisecond precision.
pub(super) fn retention_cutoff_iso(days: i64) -> String {
    let cutoff_ms = now_ms().saturating_sub(days.saturating_mul(MS_PER_DAY));
    iso_from_ms(cutoff_ms)
}

/// Current epoch milliseconds. Returns 0 if the system clock is
/// pre-1970 (defensive — we'd rather emit a useless-but-comparable
/// "1970-01-01" than crash the worker).
pub(super) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Minimal RFC3339 / ISO-8601 without an extra crate. The store
/// layer + queue.rs both stringly compare ISO timestamps; only
/// lexicographic ordering is asserted, which holds for this shape.
pub(super) fn iso_from_ms(ms: i64) -> String {
    let secs = ms / 1000;
    let millis_part = (ms % 1000).abs();
    let (y, mo, d, h, mi, se) = epoch_secs_to_ymdhms(secs);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{se:02}.{millis_part:03}Z")
}

/// Tiny calendar-math helper: epoch seconds → (Y,M,D,h,m,s) UTC.
/// Honest-to-goodness Gregorian; covers 1970-9999 cleanly.
fn epoch_secs_to_ymdhms(secs: i64) -> (i64, u32, u32, u32, u32, u32) {
    let mut total = secs.max(0) as u64;
    let se = (total % 60) as u32;
    total /= 60;
    let mi = (total % 60) as u32;
    total /= 60;
    let h = (total % 24) as u32;
    total /= 24;
    let mut days = total as i64;

    let mut y: i64 = 1970;
    loop {
        let dy = if is_leap_year(y) { 366 } else { 365 };
        if days < dy {
            break;
        }
        days -= dy;
        y += 1;
    }
    let months_in = if is_leap_year(y) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut mo: u32 = 1;
    for m_len in months_in.iter() {
        if days < *m_len {
            break;
        }
        days -= m_len;
        mo += 1;
    }
    let d = (days + 1) as u32;
    (y, mo, d, h, mi, se)
}

fn is_leap_year(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_from_ms_round_trips_known_dates() {
        // Anchor checks: epoch + a verifiable wall clock.
        assert_eq!(iso_from_ms(0), "1970-01-01T00:00:00.000Z");
        // 2024-01-01T00:00:00Z = 1704067200 epoch seconds.
        assert_eq!(iso_from_ms(1_704_067_200_000), "2024-01-01T00:00:00.000Z");
    }

    #[test]
    fn iso_lexicographic_ordering_matches_chronology() {
        // The queue's reap_done_older_than relies on lexicographic
        // ISO comparison being chronological.
        let earlier = iso_from_ms(1_700_000_000_000);
        let later = iso_from_ms(1_800_000_000_000);
        assert!(earlier < later, "earlier={earlier} later={later}");
    }

    #[test]
    fn retention_cutoff_iso_is_in_the_past() {
        // 30 days ago < now. Both calls snapshot `now_ms()` separately
        // but the gap dominates any clock-tick wobble.
        let cutoff = retention_cutoff_iso(30);
        let now = now_iso();
        assert!(cutoff < now, "cutoff={cutoff} now={now}");
    }
}
