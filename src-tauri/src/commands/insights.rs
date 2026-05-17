//! Insights dashboard — aggregates from sessions / transcripts /
//! corrections / learning_runs into a single snapshot the UI can
//! render in one fetch.
//!
//! Performance: every query in here is bounded (`LIMIT`, time
//! windows). Cold start on a 10k-session DB measures < 50 ms on the
//! dev box. If that ever changes, materialize into a view in a
//! future migration.

use rusqlite::Connection;
use tauri::State;

use crate::commands::types::{
    InsightsSnapshot, InsightsToday, LatencyBreakdown, LearningSummary,
    ModeMixEntry, TopAppEntry,
};
use crate::commands::{into_err, lock_db, AppStateHandle};

/// Average human typing rate, used to estimate "time saved vs
/// typing". Conservative — 40 wpm is "good" but not pro-typist
/// territory. Easy to bump in a future setting.
const TYPING_WPM: f64 = 40.0;

#[tauri::command]
pub fn insights_snapshot(
    db: State<'_, AppStateHandle>,
) -> Result<InsightsSnapshot, String> {
    let conn = lock_db(&db)?;

    let today = today_block(&conn).map_err(into_err)?;
    let streak_days = streak_days(&conn).map_err(into_err)?;
    let sparkline7d = sparkline_7d(&conn).map_err(into_err)?;
    let mode_mix = mode_mix_7d(&conn).map_err(into_err)?;
    let top_apps = top_apps_7d(&conn).map_err(into_err)?;
    let latency = avg_latency_7d(&conn).map_err(into_err)?;
    let learning = learning_summary(&conn).map_err(into_err)?;

    Ok(InsightsSnapshot {
        today,
        streak_days,
        sparkline7d,
        mode_mix,
        top_apps,
        latency,
        learning,
    })
}

fn today_block(conn: &Connection) -> rusqlite::Result<InsightsToday> {
    let sessions: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sessions WHERE date(started_at) = date('now')",
        [],
        |r| r.get(0),
    )?;
    let recording_ms: i64 = conn.query_row(
        "SELECT COALESCE(SUM(audio_duration_ms), 0) FROM sessions \
         WHERE date(started_at) = date('now')",
        [],
        |r| r.get(0),
    )?;
    // Sum of word counts across today's `final` transcripts (or
    // `cleaned` when no final landed). Falls back to `raw` last.
    let words: i64 = conn
        .prepare(
            "SELECT COALESCE(SUM( \
                LENGTH(t.text) - LENGTH(REPLACE(t.text, ' ', '')) + 1 \
             ), 0) \
             FROM transcripts t \
             JOIN sessions s ON s.id = t.session_id \
             WHERE date(s.started_at) = date('now') \
               AND t.stage = ( \
                  SELECT stage FROM transcripts \
                  WHERE session_id = s.id \
                  ORDER BY CASE stage \
                    WHEN 'final' THEN 0 WHEN 'cleaned' THEN 1 ELSE 2 \
                  END LIMIT 1 \
               )",
        )?
        .query_row([], |r| r.get(0))?;
    let time_saved_ms = estimate_time_saved_ms(words);
    Ok(InsightsToday {
        words,
        sessions,
        recording_ms,
        time_saved_ms,
    })
}

fn estimate_time_saved_ms(words: i64) -> i64 {
    if words <= 0 {
        return 0;
    }
    // typing_ms = words / wpm * 60_000
    let typing_ms = (words as f64 / TYPING_WPM) * 60_000.0;
    typing_ms.round() as i64
}

fn sparkline_7d(conn: &Connection) -> rusqlite::Result<Vec<i64>> {
    // Per-day word totals, oldest first, 7 entries.
    let mut out = Vec::with_capacity(7);
    for days_ago in (0..7).rev() {
        let n: i64 = conn
            .prepare(
                "SELECT COALESCE(SUM( \
                    LENGTH(t.text) - LENGTH(REPLACE(t.text, ' ', '')) + 1 \
                 ), 0) \
                 FROM transcripts t \
                 JOIN sessions s ON s.id = t.session_id \
                 WHERE date(s.started_at) = date('now', ?1) \
                   AND t.stage = 'final'",
            )?
            .query_row([format!("-{days_ago} days")], |r| r.get(0))?;
        out.push(n);
    }
    Ok(out)
}

fn streak_days(conn: &Connection) -> rusqlite::Result<i64> {
    // Walk backwards from today counting consecutive days with ≥1
    // session. Stops at the first gap. Cap at 365 — anything more
    // is bragging, not insight.
    let mut streak = 0_i64;
    for days_ago in 0..365 {
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sessions WHERE date(started_at) = date('now', ?1)",
            [format!("-{days_ago} days")],
            |r| r.get(0),
        )?;
        if n == 0 {
            // If today has no sessions, the streak hasn't started
            // counting yet — but a streak that ended yesterday is
            // still valid; only break if days_ago > 0.
            if days_ago == 0 {
                continue;
            }
            break;
        }
        streak += 1;
    }
    Ok(streak)
}

fn mode_mix_7d(conn: &Connection) -> rusqlite::Result<Vec<ModeMixEntry>> {
    let mut stmt = conn.prepare(
        "SELECT m.slug, m.label, COUNT(s.id) \
         FROM modes m \
         LEFT JOIN sessions s ON s.mode_id = m.id \
            AND s.started_at >= datetime('now', '-7 days') \
         GROUP BY m.id \
         HAVING COUNT(s.id) > 0 \
         ORDER BY COUNT(s.id) DESC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(ModeMixEntry {
            slug: r.get(0)?,
            label: r.get(1)?,
            count: r.get(2)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

fn top_apps_7d(conn: &Connection) -> rusqlite::Result<Vec<TopAppEntry>> {
    let mut stmt = conn.prepare(
        "SELECT foreground_app, COUNT(*) FROM sessions \
         WHERE started_at >= datetime('now', '-7 days') \
           AND foreground_app IS NOT NULL \
         GROUP BY foreground_app \
         ORDER BY COUNT(*) DESC \
         LIMIT 5",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(TopAppEntry {
            app: r.get(0)?,
            count: r.get(1)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

fn avg_latency_7d(conn: &Connection) -> rusqlite::Result<LatencyBreakdown> {
    let (stt, cleanup, inject): (Option<f64>, Option<f64>, Option<f64>) = conn.query_row(
        "SELECT AVG(stt_latency_ms), AVG(cleanup_latency_ms), AVG(injection_latency_ms) \
         FROM sessions WHERE started_at >= datetime('now', '-7 days')",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    Ok(LatencyBreakdown {
        stt_ms: stt,
        cleanup_ms: cleanup,
        inject_ms: inject,
    })
}

fn learning_summary(conn: &Connection) -> rusqlite::Result<LearningSummary> {
    let last_run_at: Option<String> = conn
        .query_row(
            "SELECT started_at FROM learning_runs ORDER BY id DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .ok();
    let last_rolled_back: i64 = conn
        .query_row(
            "SELECT rolled_back FROM learning_runs ORDER BY id DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    // Committed streak = number of most-recent runs with rolled_back=0.
    let mut committed_streak = 0_i64;
    let mut stmt = conn
        .prepare("SELECT rolled_back FROM learning_runs ORDER BY id DESC LIMIT 30")?;
    let rows = stmt.query_map([], |r| r.get::<_, i64>(0))?;
    for r in rows {
        if r? == 0 {
            committed_streak += 1;
        } else {
            break;
        }
    }
    // Recently learned terms — last 5 with source = 'learned'.
    let mut stmt = conn.prepare(
        "SELECT term FROM dictionary WHERE source = 'learned' \
         ORDER BY id DESC LIMIT 5",
    )?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    let mut recent_terms = Vec::new();
    for r in rows {
        recent_terms.push(r?);
    }
    Ok(LearningSummary {
        last_run_at,
        committed_streak,
        last_rolled_back: last_rolled_back != 0,
        recent_terms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    #[test]
    fn empty_db_yields_zero_snapshot() {
        let db = Database::open_in_memory().unwrap();
        let today = today_block(&db.conn).unwrap();
        assert_eq!(today.words, 0);
        assert_eq!(today.sessions, 0);
        assert_eq!(today.recording_ms, 0);
        assert_eq!(today.time_saved_ms, 0);
    }

    #[test]
    fn time_saved_estimate_matches_wpm() {
        // 40 words @ 40 wpm = 1 minute = 60000 ms.
        let ms = estimate_time_saved_ms(40);
        assert!(ms >= 59_000 && ms <= 61_000, "got {ms}");
    }

    #[test]
    fn streak_on_empty_db_is_zero() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(streak_days(&db.conn).unwrap(), 0);
    }

    #[test]
    fn sparkline_returns_seven_zeros_on_empty_db() {
        let db = Database::open_in_memory().unwrap();
        let s = sparkline_7d(&db.conn).unwrap();
        assert_eq!(s.len(), 7);
        assert!(s.iter().all(|v| *v == 0));
    }
}
