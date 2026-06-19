//! Activity-capture retention sweep.
//!
//! Phase 10 Wave 5. ADR 0042 (cascade semantics) + AGENTS.md Principle 1.
//!
//! The sweep DELETEs aged-out rows. It NEVER UPDATEs `activity_events`
//! (the immutability trigger would reject; we don't even try). Blocks
//! whose raw events get purged are marked with `raw_events_purged_at`
//! on the derived `activity_blocks` table — a separate UPDATE that
//! doesn't violate Principle 1 because it targets the derived layer.
//!
//! ## Policy
//!
//! Four typed settings drive the sweep:
//!
//! - `activity_retention_events_days`   (`0` = forever; default `0`)
//! - `activity_retention_segments_days` (`0` = forever; default `0`)
//! - `activity_retention_blocks_days`   (`0` = forever; default `0`)
//! - `activity_retention_last_sweep_ms` (timestamp of last successful sweep)
//!
//! All four live in the `settings` table (registered in
//! `settings/model.rs`). Privacy-by-default means we ship `0` for each
//! — the user has to opt in.
//!
//! ## Sweep order (ADR 0042 §Sweep order — binding)
//!
//! For each TTL'd table (in this order):
//!
//! 1. UPDATE `activity_blocks SET raw_events_purged_at = now WHERE
//!    session_id IN (sessions whose events we're about to delete) AND
//!    raw_events_purged_at IS NULL`.
//! 2. DELETE FROM `activity_events WHERE ts < <events_cutoff>`.
//! 3. DELETE FROM `activity_transcript_segments WHERE started_at <
//!    <segments_cutoff>`.
//! 4. (Separate, independent of (1)–(3)) DELETE FROM `activity_blocks
//!    WHERE started_at < <blocks_cutoff>` — only when
//!    `blocks_days > 0`.
//!
//! Wrapped in a single transaction so partial failures don't leave
//! breadcrumbs without the underlying DELETE.

#![allow(missing_docs)]

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime};

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::error::AppResult;
use crate::settings::{model::SettingKey, Settings};

const MS_PER_DAY: i64 = 86_400_000;

/// Sweep cadence — once per day after boot. The boot sweep runs
/// always (its own throttle is `last_sweep_ms`), this is the
/// long-running daemon cadence.
const SWEEP_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// Boot-throttle: only run if at least this much time has elapsed
/// since the last sweep. Prevents the sweep from running on every
/// fast restart during dev.
const BOOT_THROTTLE_MS: i64 = 60 * 60 * 1000; // 1 hour

/// Resolved retention policy. Read from settings via [`load`].
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct RetentionPolicy {
    /// TTL in days for `activity_events`. `0` = forever.
    pub events_days: i64,
    /// TTL in days for `activity_transcript_segments`. `0` = forever.
    pub segments_days: i64,
    /// TTL in days for `activity_blocks`. `0` = forever.
    pub blocks_days: i64,
    /// Unix epoch ms of the last successful sweep. `0` = never.
    pub last_sweep_ms: i64,
}

impl RetentionPolicy {
    /// True iff at least one TTL is non-zero (i.e. the sweep would do
    /// any work).
    pub fn any_ttl_set(&self) -> bool {
        self.events_days > 0 || self.segments_days > 0 || self.blocks_days > 0
    }
}

/// Result of a sweep pass — counts of deleted/updated rows. Returned
/// for diagnostic logging + the Settings UI's "last sweep removed N
/// rows" affordance.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SweepResult {
    pub events_deleted: i64,
    pub segments_deleted: i64,
    pub blocks_deleted: i64,
    pub blocks_marked_purged: i64,
    pub ran_at_ms: i64,
}

/// Pure helper: cutoff timestamp in ms. `None` when `ttl_days == 0`
/// (i.e. retain forever).
pub fn compute_cutoff_ms(now_ms: i64, ttl_days: i64) -> Option<i64> {
    if ttl_days <= 0 {
        None
    } else {
        // Clamp the cutoff at the epoch: a negative cutoff is
        // meaningless (timestamps are >= 0) and a stupid-large TTL
        // would otherwise underflow toward i64::MIN. `.max(0)`
        // mirrors `save_user_policy`'s guard. (mb-mac-v1.9: real
        // bug -- `cutoff_saturates_on_overflow` expected Some(0)
        // but got ~i64::MIN; surfaced on Mac's first real test run.)
        Some(
            now_ms
                .saturating_sub(ttl_days.saturating_mul(MS_PER_DAY))
                .max(0),
        )
    }
}

/// Load the policy from the `settings` table. Missing keys fall back
/// to defaults (which are `0 = forever`).
pub fn load(conn: &Connection) -> AppResult<RetentionPolicy> {
    let s = Settings::new(conn);
    Ok(RetentionPolicy {
        events_days: s.get(SettingKey::ActivityRetentionEventsDays)?,
        segments_days: s.get(SettingKey::ActivityRetentionSegmentsDays)?,
        blocks_days: s.get(SettingKey::ActivityRetentionBlocksDays)?,
        last_sweep_ms: s.get(SettingKey::ActivityRetentionLastSweepMs)?,
    })
}

/// Persist the user-tunable fields of the policy. `last_sweep_ms` is
/// owned by the sweep itself and NOT written here — callers should
/// use [`save_user_policy`] which deliberately omits it.
pub fn save_user_policy(
    conn: &Connection,
    events_days: i64,
    segments_days: i64,
    blocks_days: i64,
) -> AppResult<()> {
    let s = Settings::new(conn);
    s.set(SettingKey::ActivityRetentionEventsDays, &events_days.max(0))?;
    s.set(
        SettingKey::ActivityRetentionSegmentsDays,
        &segments_days.max(0),
    )?;
    s.set(SettingKey::ActivityRetentionBlocksDays, &blocks_days.max(0))?;
    Ok(())
}

/// Run the sweep once, transactionally. Returns the row-count summary.
///
/// Safe to call any time — no-op when the policy has no TTLs set.
pub fn sweep_once(conn: &mut Connection, now_ms: i64) -> AppResult<SweepResult> {
    let policy = load(conn)?;
    if !policy.any_ttl_set() {
        // Even with no TTLs, update last_sweep_ms so the daemon
        // throttle behaves sanely (and the UI can show "last sweep:
        // ...").
        Settings::new(conn).set(SettingKey::ActivityRetentionLastSweepMs, &now_ms)?;
        return Ok(SweepResult {
            ran_at_ms: now_ms,
            ..SweepResult::default()
        });
    }

    let events_cutoff = compute_cutoff_ms(now_ms, policy.events_days);
    let segments_cutoff = compute_cutoff_ms(now_ms, policy.segments_days);
    let blocks_cutoff = compute_cutoff_ms(now_ms, policy.blocks_days);

    let tx = conn.transaction()?;
    let mut result = SweepResult {
        ran_at_ms: now_ms,
        ..SweepResult::default()
    };

    // (1) Stamp raw_events_purged_at on Blocks whose underlying events
    // are about to be deleted. ADR 0042 §Sweep order step 1.
    if let Some(cutoff) = events_cutoff {
        let marked = tx.execute(
            "UPDATE activity_blocks \
             SET raw_events_purged_at = ?1 \
             WHERE raw_events_purged_at IS NULL \
               AND session_id IN ( \
                 SELECT DISTINCT session_id FROM activity_events WHERE ts < ?2 \
               )",
            params![now_ms, cutoff],
        )?;
        result.blocks_marked_purged = marked as i64;

        // (2) DELETE aged events.
        let deleted = tx.execute("DELETE FROM activity_events WHERE ts < ?1", params![cutoff])?;
        result.events_deleted = deleted as i64;
    }

    // (3) DELETE aged transcript segments.
    if let Some(cutoff) = segments_cutoff {
        let deleted = tx.execute(
            "DELETE FROM activity_transcript_segments WHERE started_at < ?1",
            params![cutoff],
        )?;
        result.segments_deleted = deleted as i64;
    }

    // (4) DELETE aged Blocks themselves.
    if let Some(cutoff) = blocks_cutoff {
        let deleted = tx.execute(
            "DELETE FROM activity_blocks WHERE started_at < ?1",
            params![cutoff],
        )?;
        result.blocks_deleted = deleted as i64;
    }

    // Update last_sweep_ms inside the same transaction so it's atomic
    // with the deletes.
    tx.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![
            SettingKey::ActivityRetentionLastSweepMs.as_str(),
            serde_json::Value::from(now_ms).to_string(),
        ],
    )?;
    tx.commit()?;

    tracing::info!(
        target: "activity::retention",
        events_deleted = result.events_deleted,
        segments_deleted = result.segments_deleted,
        blocks_deleted = result.blocks_deleted,
        blocks_marked_purged = result.blocks_marked_purged,
        "retention sweep completed"
    );

    Ok(result)
}

/// Spawn a background thread that runs [`sweep_once`] on a daily
/// cadence. The thread also runs an immediate sweep at startup if
/// the last sweep was more than [`BOOT_THROTTLE_MS`] ago.
///
/// The thread is fire-and-forget — we never join it because process
/// exit kills it cleanly. No graceful shutdown needed; the sweep is
/// transactional so an abrupt exit just leaves the next boot to
/// pick up where it left off.
pub fn spawn_daemon(conn: Arc<Mutex<Connection>>) {
    thread::spawn(move || {
        // Initial throttled sweep.
        run_throttled(&conn);
        loop {
            thread::sleep(SWEEP_INTERVAL);
            run_throttled(&conn);
        }
    });
}

fn run_throttled(conn: &Arc<Mutex<Connection>>) {
    let now = now_ms();
    let policy = match conn.lock() {
        Ok(c) => match load(&c) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(target: "activity::retention", error = %e, "failed to load policy");
                return;
            }
        },
        Err(_) => {
            tracing::warn!(target: "activity::retention", "db mutex poisoned");
            return;
        }
    };
    if policy.last_sweep_ms != 0 && (now - policy.last_sweep_ms) < BOOT_THROTTLE_MS {
        tracing::debug!(
            target: "activity::retention",
            last = policy.last_sweep_ms,
            "skipping sweep — throttle window"
        );
        return;
    }
    if let Ok(mut c) = conn.lock() {
        if let Err(e) = sweep_once(&mut c, now) {
            tracing::warn!(target: "activity::retention", error = %e, "sweep failed");
        }
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cutoff_zero_means_forever() {
        assert_eq!(compute_cutoff_ms(1_000_000, 0), None);
        assert_eq!(compute_cutoff_ms(1_000_000, -5), None);
    }

    #[test]
    fn cutoff_subtracts_days_in_ms() {
        // 7 days * 86_400_000 ms = 604_800_000
        assert_eq!(
            compute_cutoff_ms(1_000_000_000, 7),
            Some(1_000_000_000 - 604_800_000)
        );
    }

    #[test]
    fn cutoff_saturates_on_overflow() {
        // Stupid-large TTL with a small `now` should not panic.
        let r = compute_cutoff_ms(1_000, i64::MAX);
        assert_eq!(r, Some(0));
    }

    #[test]
    fn policy_any_ttl_set_logic() {
        let p = RetentionPolicy {
            events_days: 0,
            segments_days: 0,
            blocks_days: 0,
            last_sweep_ms: 0,
        };
        assert!(!p.any_ttl_set());

        let p = RetentionPolicy {
            events_days: 30,
            segments_days: 0,
            blocks_days: 0,
            last_sweep_ms: 0,
        };
        assert!(p.any_ttl_set());
    }

    // ----------------------------------------------------------
    // Phase 10 Wave 6.B — retention-preserves-abstracts judge fixture.
    // ADR 0042 (cascade-option-(a)).
    // ----------------------------------------------------------

    #[test]
    fn sweep_marks_blocks_and_deletes_events() {
        use crate::activity::blocks_persist::insert_block;
        use crate::activity::persist::{insert_event, insert_session};
        use crate::db::Database;

        let mut db = Database::open_in_memory().expect("open in-memory db");
        let now: i64 = 10 * MS_PER_DAY; // pick a clean "now".
        let day = MS_PER_DAY;

        // Seed session + 3 events (5d, 3d, 0d old) + 1 block referencing all 3.
        let sid = insert_session(&db.conn, now - 6 * day).unwrap();
        let e5 = insert_event(
            &db.conn,
            &sid,
            now - 5 * day,
            "app_switch",
            Some("a.exe"),
            Some("t"),
            None,
        )
        .unwrap();
        let e3 = insert_event(
            &db.conn,
            &sid,
            now - 3 * day,
            "app_switch",
            Some("a.exe"),
            Some("t"),
            None,
        )
        .unwrap();
        let _e0 = insert_event(
            &db.conn,
            &sid,
            now,
            "app_switch",
            Some("a.exe"),
            Some("t"),
            None,
        )
        .unwrap();

        let src_json = serde_json::to_string(&vec![e5.clone(), e3.clone()]).unwrap();
        let bid = insert_block(
            &db.conn,
            &sid,
            now - 5 * day,
            now - 3 * day,
            "a.exe",
            "t",
            Some("wrote the Wave 6.A judge slate"),
            &src_json,
            "abstract_v1-deadbeef",
            now - 5 * day,
        )
        .unwrap();

        // Set events_days = 1 — anything older than 1 day must die.
        save_user_policy(&db.conn, 1, 0, 0).unwrap();

        let result = sweep_once(&mut db.conn, now).expect("sweep_once");
        assert_eq!(
            result.events_deleted, 2,
            "events older than now-1d should be deleted"
        );
        assert_eq!(
            result.blocks_marked_purged, 1,
            "block whose events got deleted should be marked purged"
        );
        assert_eq!(
            result.blocks_deleted, 0,
            "blocks_days=0 — no block deletions"
        );

        // Block survives + its abstract is byte-for-byte intact.
        let (abs_text, purged_at): (Option<String>, Option<i64>) = db
            .conn
            .query_row(
                "SELECT generated_abstract, raw_events_purged_at \
                 FROM activity_blocks WHERE id = ?1",
                params![&bid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            abs_text.as_deref(),
            Some("wrote the Wave 6.A judge slate"),
            "abstract text must survive the sweep byte-for-byte"
        );
        assert_eq!(
            purged_at,
            Some(now),
            "raw_events_purged_at must equal the sweep timestamp"
        );

        // Exactly one event survived — the now-0d one.
        let n: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM activity_events WHERE session_id = ?1",
                params![&sid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1, "only the now-0d event should survive");
    }
}
