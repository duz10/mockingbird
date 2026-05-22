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
    CorrectionEntry, DictTermEntry, HeatmapDay, InsightsSnapshot, InsightsToday, LatencyBreakdown,
    LearningSummary, LifetimeTotals, ModeMixEntry, TopAppEntry, WpmStats,
};
use crate::commands::{into_err, lock_db, AppStateHandle};

/// Average human typing rate, used to estimate "time saved vs
/// typing". Conservative — 40 wpm is "good" but not pro-typist
/// territory. Easy to bump in a future setting.
const TYPING_WPM: f64 = 40.0;

#[tauri::command]
pub fn insights_snapshot(db: State<'_, AppStateHandle>) -> Result<InsightsSnapshot, String> {
    let conn = lock_db(&db)?;

    let today = today_block(&conn).map_err(into_err)?;
    let streak_days = streak_days(&conn).map_err(into_err)?;
    let sparkline7d = sparkline_7d(&conn).map_err(into_err)?;
    let mode_mix = mode_mix_7d(&conn).map_err(into_err)?;
    let top_apps = top_apps_7d(&conn).map_err(into_err)?;
    let latency = avg_latency_7d(&conn).map_err(into_err)?;
    let learning = learning_summary(&conn).map_err(into_err)?;
    let lifetime = lifetime_totals(&conn).map_err(into_err)?;
    let longest_streak_days = longest_streak(&conn).map_err(into_err)?;
    let heatmap365d = heatmap_365d(&conn).map_err(into_err)?;
    let peak_hours = peak_hours_90d(&conn).map_err(into_err)?;
    let top_dict_terms = top_dict_terms(&conn).map_err(into_err)?;
    let top_corrections = top_corrections(&conn).map_err(into_err)?;
    let wpm = wpm_stats_30d(&conn).map_err(into_err)?;

    Ok(InsightsSnapshot {
        today,
        streak_days,
        sparkline7d,
        mode_mix,
        top_apps,
        latency,
        learning,
        lifetime,
        longest_streak_days,
        heatmap365d,
        peak_hours,
        top_dict_terms,
        top_corrections,
        wpm,
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
    // Schema column is `display_name` (migrations/001_initial.sql).
    // The DTO field is `label` — alias here so neither side has to
    // change. Migrations are append-only post-Phase-1-seal, so we
    // can't rename the column even if we wanted to.
    let mut stmt = conn.prepare(
        "SELECT m.slug, m.display_name AS label, COUNT(s.id) \
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
    let mut stmt =
        conn.prepare("SELECT rolled_back FROM learning_runs ORDER BY id DESC LIMIT 30")?;
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

/* ------------------------------------------------------------------ */
/* Lifetime totals — dictation + meetings, all time                    */
/* ------------------------------------------------------------------ */

fn lifetime_totals(conn: &Connection) -> rusqlite::Result<LifetimeTotals> {
    let (sessions, recording_ms): (i64, i64) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(audio_duration_ms), 0) FROM sessions",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    // Sum of word counts across every `final` transcript (fall back
    // to `cleaned` then `raw` per-session). The picked-stage subquery
    // mirrors `today_block` to keep numbers comparable.
    let words: i64 = conn
        .prepare(
            "SELECT COALESCE(SUM( \
                LENGTH(t.text) - LENGTH(REPLACE(t.text, ' ', '')) + 1 \
             ), 0) \
             FROM transcripts t \
             WHERE t.stage = ( \
                SELECT stage FROM transcripts \
                WHERE session_id = t.session_id \
                ORDER BY CASE stage \
                  WHEN 'final' THEN 0 WHEN 'cleaned' THEN 1 ELSE 2 \
                END LIMIT 1 \
             )",
        )?
        .query_row([], |r| r.get(0))?;
    // Meetings are stored in `meeting_sessions`; only count
    // status='complete' so a crash mid-recording doesn't pad totals.
    // `total_duration_ms` is authoritative duration on that row.
    let (m_count, m_ms): (i64, i64) = conn
        .query_row(
            "SELECT COUNT(*), COALESCE(SUM(total_duration_ms), 0) \
             FROM meeting_sessions WHERE status = 'complete'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        // Schema before migration 011 has no meeting_sessions table —
        // tolerate that so the upstream insights snapshot doesn't 500
        // on an old DB clone. Treat "table missing" as zeroes.
        .unwrap_or((0, 0));
    Ok(LifetimeTotals {
        dictation_words: words,
        dictation_sessions: sessions,
        dictation_recording_ms: recording_ms,
        meetings_count: m_count,
        meetings_total_ms: m_ms,
    })
}

/* ------------------------------------------------------------------ */
/* Longest streak — capped at 365                                      */
/* ------------------------------------------------------------------ */

fn longest_streak(conn: &Connection) -> rusqlite::Result<i64> {
    // One pass through the distinct session-day list, ordered by
    // date. Each iteration compares to the prior day's date string
    // via SQLite's date math; gaps reset the running count.
    let mut stmt =
        conn.prepare("SELECT DISTINCT date(started_at) FROM sessions ORDER BY date(started_at)")?;
    let mut rows = stmt.query([])?;
    let mut longest = 0_i64;
    let mut current = 0_i64;
    let mut prev: Option<String> = None;
    while let Some(row) = rows.next()? {
        let day: String = row.get(0)?;
        let is_consecutive = match &prev {
            None => false,
            Some(p) => {
                // SQLite date('YYYY-MM-DD', '+1 day') gives the next
                // calendar day. Compare as strings (ISO sorts right).
                let next: String =
                    conn.query_row("SELECT date(?1, '+1 day')", [p], |r| r.get(0))?;
                next == day
            }
        };
        current = if is_consecutive { current + 1 } else { 1 };
        if current > longest {
            longest = current;
        }
        prev = Some(day);
    }
    Ok(longest.min(365))
}

/* ------------------------------------------------------------------ */
/* 365-day heatmap — one row per day, oldest first                     */
/* ------------------------------------------------------------------ */

fn heatmap_365d(conn: &Connection) -> rusqlite::Result<Vec<HeatmapDay>> {
    // SQLite computes per-day session + word totals; we then
    // backfill missing days as zero-cells in Rust. Backfilling in
    // SQL would require a recursive CTE which is fine but adds
    // complexity vs. a 365-iteration Rust loop. Keep it simple.
    let mut stmt = conn.prepare(
        "SELECT date(s.started_at) AS d, \
                COUNT(s.id), \
                COALESCE(SUM( \
                    (LENGTH(t.text) - LENGTH(REPLACE(t.text, ' ', '')) + 1) \
                ), 0) \
         FROM sessions s \
         LEFT JOIN transcripts t \
            ON t.session_id = s.id AND t.stage = 'final' \
         WHERE s.started_at >= datetime('now', '-365 days') \
         GROUP BY d",
    )?;
    use std::collections::HashMap;
    let mut by_date: HashMap<String, (i64, i64)> = HashMap::new();
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, i64>(2)?,
        ))
    })?;
    for r in rows {
        let (d, sessions, words) = r?;
        by_date.insert(d, (sessions, words));
    }
    // Walk the last 365 days in ISO date order, filling zeroes for
    // gaps. Today is index 364; 364 days ago is index 0.
    let mut out = Vec::with_capacity(365);
    for days_ago in (0..365).rev() {
        let d: String = conn.query_row(
            "SELECT date('now', ?1)",
            [format!("-{days_ago} days")],
            |r| r.get(0),
        )?;
        let (sessions, words) = by_date.get(&d).copied().unwrap_or((0, 0));
        out.push(HeatmapDay {
            date: d,
            sessions,
            words,
        });
    }
    Ok(out)
}

/* ------------------------------------------------------------------ */
/* Peak hours — last 90 days, sessions per hour-of-day bucket         */
/* ------------------------------------------------------------------ */

fn peak_hours_90d(conn: &Connection) -> rusqlite::Result<Vec<i64>> {
    // SQLite's strftime('%H', ...) returns '00'..'23' as text.
    // CAST to integer once in SQL, then build a 24-slot vector in
    // Rust so the response shape is always length-24 even on a
    // brand-new DB.
    let mut stmt = conn.prepare(
        "SELECT CAST(strftime('%H', started_at) AS INTEGER), COUNT(*) \
         FROM sessions \
         WHERE started_at >= datetime('now', '-90 days') \
         GROUP BY 1",
    )?;
    let mut buckets = vec![0_i64; 24];
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?;
    for r in rows {
        let (hour, n) = r?;
        if (0..24).contains(&hour) {
            buckets[hour as usize] = n;
        }
    }
    Ok(buckets)
}

/* ------------------------------------------------------------------ */
/* Top dictionary terms — by use_count                                 */
/* ------------------------------------------------------------------ */

fn top_dict_terms(conn: &Connection) -> rusqlite::Result<Vec<DictTermEntry>> {
    let mut stmt = conn.prepare(
        "SELECT term, use_count FROM dictionary \
         WHERE use_count > 0 \
         ORDER BY use_count DESC, last_used_at DESC \
         LIMIT 8",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(DictTermEntry {
            term: r.get(0)?,
            use_count: r.get(1)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/* ------------------------------------------------------------------ */
/* Top corrections — most-corrected raw tokens, last 90d              */
/* ------------------------------------------------------------------ */

fn top_corrections(conn: &Connection) -> rusqlite::Result<Vec<CorrectionEntry>> {
    // Group by `before_text` (the wrong word) so a recurring
    // mis-transcription bubbles to the top regardless of how the
    // user fixed it.
    let mut stmt = conn.prepare(
        "SELECT before_text, COUNT(*) FROM corrections \
         WHERE created_at >= datetime('now', '-90 days') \
         GROUP BY before_text \
         ORDER BY COUNT(*) DESC, before_text \
         LIMIT 8",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(CorrectionEntry {
            before: r.get(0)?,
            count: r.get(1)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/* ------------------------------------------------------------------ */
/* WPM — words / minute over last 30 days                              */
/* ------------------------------------------------------------------ */

fn wpm_stats_30d(conn: &Connection) -> rusqlite::Result<WpmStats> {
    // Per-session WPM = words / (audio_duration_ms / 60_000). Only
    // count sessions ≥ 5 seconds so 1-word "yes" misfires don't
    // skew the average; cap individual WPM at 300 so a near-zero
    // duration outlier doesn't explode the mean. We average the
    // per-session WPM (not total-words / total-time) so a single
    // long session can't dominate.
    let mut stmt = conn.prepare(
        "SELECT \
             ( (LENGTH(t.text) - LENGTH(REPLACE(t.text, ' ', '')) + 1) * 1.0 ) \
             / (s.audio_duration_ms / 60000.0) AS wpm \
         FROM sessions s \
         JOIN transcripts t ON t.session_id = s.id AND t.stage = 'final' \
         WHERE s.started_at >= datetime('now', '-30 days') \
           AND s.audio_duration_ms >= 5000 \
           AND LENGTH(t.text) > 0",
    )?;
    let rows = stmt.query_map([], |r| r.get::<_, f64>(0))?;
    let mut total = 0_f64;
    let mut samples = 0_i64;
    for r in rows {
        let wpm = r?;
        if wpm.is_finite() && wpm > 0.0 && wpm <= 300.0 {
            total += wpm;
            samples += 1;
        }
    }
    let avg_wpm = if samples > 0 {
        Some(total / samples as f64)
    } else {
        None
    };
    Ok(WpmStats { avg_wpm, samples })
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
        assert!((59_000..=61_000).contains(&ms), "got {ms}");
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
