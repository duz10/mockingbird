//! Stage 4 of the Wave-3 summarization pipeline: **render Blocks to
//! Markdown**.
//!
//! Pure-Rust. No DB, no Ollama. Takes a slice of
//! [`AbstractedBlock`]s (Blocks with optional abstract text already
//! computed) and produces the Markdown body that lands in
//! `activity_sessions.summary_markdown` + the optional export file.
//!
//! ## Output shape
//!
//! ```markdown
//! # Activity — 2026-05-25 14:32 (1 h 12 min)
//!
//! ## Top apps
//! - **chrome.exe** — 42 min
//! - **code.exe** — 26 min
//! - **slack.exe** — 4 min
//!
//! ## Timeline
//!
//! ### 14:32 → 14:48 (16 min) — chrome.exe
//! The user reviewed pull request #482 in mockingbird.
//!
//! ### 14:48 → 15:14 (26 min) — code.exe
//! The user edited the `dictation.rs` source file in VS Code…
//! ```
//!
//! ## Graceful degradation
//!
//! Blocks with `abstract_text = None` render as a fallback line —
//! `_Summary unavailable._` — so the timeline structure survives an
//! Ollama outage. ADR 0040 §Decision item 3.

#![allow(missing_docs)]

use std::collections::BTreeMap;

use super::blocker::Block;

/// A Block decorated with the abstractor's output. `abstract_text`
/// may be a real LLM-generated sentence, the deterministic
/// no-payload template, or `None` if both failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AbstractedBlock {
    pub block: Block,
    pub abstract_text: Option<String>,
    /// Optional user-set label. When present, it replaces the
    /// auto-derived `## Block` heading suffix in the Markdown.
    pub label: Option<String>,
}

/// Top-level entry point. `session_started_iso` is whatever the
/// caller wants the title to say; we don't pull `chrono` in just for
/// a header. `total_duration_ms` is the session's wall-clock span
/// (Stop - Start), separate from the sum of Block durations because
/// idle ≥ 60 s gaps are NOT inside any Block.
pub fn assemble(session_title: &str, total_duration_ms: i64, blocks: &[AbstractedBlock]) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "# Activity — {} ({})\n\n",
        session_title,
        format_duration(total_duration_ms)
    ));

    if blocks.is_empty() {
        out.push_str("_No activity recorded._\n");
        return out;
    }

    // Top-apps section: by total focus time across all Blocks.
    let top = top_apps(blocks);
    if !top.is_empty() {
        out.push_str("## Top apps\n\n");
        for (app, ms) in &top {
            out.push_str(&format!("- **{}** — {}\n", app, format_duration(*ms)));
        }
        out.push('\n');
    }

    out.push_str("## Timeline\n\n");
    for ab in blocks {
        let b = &ab.block;
        let title_part = match (&ab.label, b.primary_title.is_empty()) {
            (Some(l), _) => l.clone(),
            (None, false) => b.primary_title.clone(),
            (None, true) => b.primary_app.clone(),
        };
        out.push_str(&format!(
            "### {} → {} ({}) — {} · {}\n\n",
            format_clock(b.started_at),
            format_clock(b.ended_at),
            format_duration(b.duration_ms()),
            b.primary_app,
            title_part,
        ));
        match &ab.abstract_text {
            Some(text) if !text.trim().is_empty() => {
                out.push_str(text.trim());
                out.push_str("\n\n");
            }
            _ => {
                out.push_str("_Summary unavailable._\n\n");
            }
        }
    }

    out
}

/// "Work-report" variant: drops the per-Block fine detail and emits
/// only the top-line summaries. Used by the export flow's Q9 toggle.
pub fn assemble_work_report(
    session_title: &str,
    total_duration_ms: i64,
    blocks: &[AbstractedBlock],
) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "# Activity — {} ({})\n\n",
        session_title,
        format_duration(total_duration_ms)
    ));
    let top = top_apps(blocks);
    if !top.is_empty() {
        out.push_str("## Top apps\n\n");
        for (app, ms) in &top {
            out.push_str(&format!("- **{}** — {}\n", app, format_duration(*ms)));
        }
        out.push('\n');
    }

    out.push_str("## Highlights\n\n");
    for ab in blocks {
        if let Some(text) = &ab.abstract_text {
            if !text.trim().is_empty() {
                out.push_str("- ");
                out.push_str(text.trim());
                out.push('\n');
            }
        }
    }
    out
}

/// Sort apps by descending focus-time. Returns at most 8 entries to
/// keep the section readable.
fn top_apps(blocks: &[AbstractedBlock]) -> Vec<(String, i64)> {
    let mut totals: BTreeMap<String, i64> = BTreeMap::new();
    for ab in blocks {
        *totals.entry(ab.block.primary_app.clone()).or_insert(0) += ab.block.duration_ms();
    }
    let mut v: Vec<_> = totals.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v.truncate(8);
    v
}

/// Render ms as `1 h 12 min`, `42 min`, `12 s`. Pure-Rust; no `chrono`.
pub fn format_duration(ms: i64) -> String {
    let total_s = ms.max(0) / 1000;
    let h = total_s / 3600;
    let m = (total_s % 3600) / 60;
    let s = total_s % 60;
    if h > 0 {
        if m > 0 {
            format!("{h} h {m} min")
        } else {
            format!("{h} h")
        }
    } else if m > 0 {
        format!("{m} min")
    } else {
        format!("{s} s")
    }
}

/// Render unix-epoch ms as `HH:MM` local time. We deliberately do NOT
/// pull `chrono`: the formatter is in the assembler's hot path on
/// every summary regeneration, and the cost of a chrono import (a
/// transitive ~10 deps incl. `iana-time-zone`) for two-digit time
/// rendering is not worth it. The trick: compute seconds since the
/// last midnight UTC, then offset by the system's local-time offset.
///
/// For Wave 3 we approximate by treating the timestamp as UTC; in
/// Wave 5 polish, surface the actual local-offset (likely via
/// `time::OffsetDateTime::now_local()` which is in the existing
/// `time` crate from rusqlite's optional features).
fn format_clock(ms: i64) -> String {
    let secs_in_day = (ms.max(0) / 1000) % 86_400;
    let hh = secs_in_day / 3600;
    let mm = (secs_in_day % 3600) / 60;
    format!("{hh:02}:{mm:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity::segmenter::NormalizedEvent;

    fn mk_block(app: &str, title: &str, started: i64, ended: i64) -> Block {
        Block {
            started_at: started,
            ended_at: ended,
            primary_app: app.into(),
            primary_title: title.into(),
            source_event_ids: vec![],
            focus_events: vec![NormalizedEvent::AppFocus {
                event_id: "e1".into(),
                app: app.into(),
                title: title.into(),
                ts: started,
                snapshot_json: None,
            }],
            idle_ms_within: 0,
        }
    }

    fn mk_abstracted(b: Block, text: Option<&str>) -> AbstractedBlock {
        AbstractedBlock {
            block: b,
            abstract_text: text.map(str::to_string),
            label: None,
        }
    }

    #[test]
    fn empty_blocks_yields_no_activity_message() {
        let md = assemble("now", 0, &[]);
        assert!(md.contains("_No activity recorded._"));
    }

    #[test]
    fn assembled_markdown_contains_top_apps_section() {
        let blocks = vec![
            mk_abstracted(
                mk_block("chrome.exe", "Gmail", 0, 60_000),
                Some("Reading email."),
            ),
            mk_abstracted(
                mk_block("code.exe", "main.rs", 60_000, 180_000),
                Some("Coding."),
            ),
        ];
        let md = assemble("2026-05-25 14:00", 180_000, &blocks);
        assert!(md.contains("## Top apps"));
        assert!(md.contains("**code.exe**"));
        assert!(md.contains("**chrome.exe**"));
    }

    #[test]
    fn top_apps_orders_by_focus_time_descending() {
        let blocks = vec![
            mk_abstracted(mk_block("chrome.exe", "T", 0, 30_000), None),
            mk_abstracted(mk_block("code.exe", "T", 30_000, 200_000), None),
            mk_abstracted(mk_block("chrome.exe", "T2", 200_000, 230_000), None),
        ];
        let top = top_apps(&blocks);
        assert_eq!(top[0].0, "code.exe");
        assert!(top[0].1 > top[1].1);
    }

    #[test]
    fn missing_abstract_renders_fallback_line() {
        let blocks = vec![mk_abstracted(mk_block("a.exe", "T", 0, 60_000), None)];
        let md = assemble("now", 60_000, &blocks);
        assert!(md.contains("_Summary unavailable._"));
    }

    #[test]
    fn user_label_overrides_default_title_in_timeline() {
        let mut ab = mk_abstracted(mk_block("a.exe", "T", 0, 60_000), Some("hi"));
        ab.label = Some("My Custom Label".into());
        let md = assemble("now", 60_000, &[ab]);
        assert!(md.contains("My Custom Label"));
    }

    #[test]
    fn format_duration_renders_hours_minutes_seconds() {
        assert_eq!(format_duration(0), "0 s");
        assert_eq!(format_duration(45_000), "45 s");
        assert_eq!(format_duration(60_000), "1 min");
        assert_eq!(format_duration(75_000), "1 min");
        assert_eq!(format_duration(3_600_000), "1 h");
        assert_eq!(format_duration(4_320_000), "1 h 12 min");
        assert_eq!(format_duration(-100), "0 s", "negative input is clamped");
    }

    #[test]
    fn format_clock_renders_hh_mm() {
        // Note: this is UTC-based for v1 (Wave 5 will polish to local).
        // Just verify the format shape.
        let s = format_clock(0);
        assert_eq!(s, "00:00");
        // 1 hour in
        let s = format_clock(3_600_000);
        assert_eq!(s, "01:00");
        // 23:59
        let s = format_clock(((23 * 3600) + 59 * 60) * 1000);
        assert_eq!(s, "23:59");
    }

    #[test]
    fn work_report_drops_per_block_detail_but_keeps_summaries() {
        let blocks = vec![
            mk_abstracted(mk_block("a.exe", "T1", 0, 60_000), Some("Wrote a thing.")),
            mk_abstracted(
                mk_block("b.exe", "T2", 60_000, 120_000),
                Some("Read a doc."),
            ),
            mk_abstracted(mk_block("c.exe", "T3", 120_000, 180_000), None),
        ];
        let md = assemble_work_report("now", 180_000, &blocks);
        assert!(md.contains("## Highlights"));
        assert!(md.contains("- Wrote a thing."));
        assert!(md.contains("- Read a doc."));
        // Block without abstract should not appear as a bullet.
        assert!(!md.contains("- _Summary unavailable._"));
        // Per-Block "###" headers are absent in work-report.
        assert!(!md.contains("### "));
    }

    #[test]
    fn top_apps_truncates_to_eight_entries() {
        let mut blocks = Vec::new();
        for i in 0..12 {
            blocks.push(mk_abstracted(
                mk_block(&format!("app{i}.exe"), "T", 0, 1000),
                None,
            ));
        }
        let top = top_apps(&blocks);
        assert_eq!(top.len(), 8);
    }
}
