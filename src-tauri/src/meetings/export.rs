//! Markdown export for a persisted meeting.
//!
//! Serializes a [`MeetingDetail`] into a single Markdown document
//! with YAML frontmatter (title, uuid, started_at, duration, source,
//! formatter_version) and a speaker-tagged body. Optionally appends
//! a trailing "## LLM pass output" section if the caller supplies an
//! in-memory LLM-pass result (see [`super::llm_pass`] — note that LLM
//! output is NEVER persisted to the DB).
//!
//! Wave 4 ships the impl per `docs/phases/phase-mc-wave4-brief.md` §4.4.
//!
//! ### Deviation from Wave 1 scaffold (binding for Wave 6 retro)
//!
//! Wave 1 stubbed `pub fn render_markdown(req: &ExportRequest<'_>)`
//! taking only `meeting_uuid` + `llm_pass_text`. Wave 4 deletes
//! `ExportRequest` (YAGNI — it was a Wave-1 placeholder; nothing else
//! references it) and reshapes the signature to take `&MeetingDetail`
//! directly, per the recommendation in §4.4 of the Wave 4 brief.

use crate::error::AppResult;

use super::capture::MeetingSource;
use super::repo::MeetingDetail;

/// Render the meeting to a Markdown string.
///
/// Layout:
/// ```text
/// ---
/// title: <title or "Untitled meeting">
/// uuid: <uuid>
/// started_at: <ISO-8601>
/// duration: <H:MM:SS>
/// source: <mic|system|both>
/// formatter_version: <e.g. mc-v1>
/// ---
///
/// # <title or "Untitled meeting">
///
/// <body — see body-selection logic below>
///
/// ## LLM pass output   ← only if llm_pass_text is Some
///
/// <llm_pass_text>
/// ```
///
/// Body-selection rules:
///   - source == `Mic`     → `formatted_mic` (or an empty placeholder line)
///   - source == `System`  → `formatted_sys` with each paragraph
///     prefixed by `**Other(s):**`
///   - source == `Both`    → `formatted_merged` if present (its
///     paragraphs already carry `**You:**` / `**Other(s):**` labels
///     courtesy of the formatter), else an interleaved best-effort
///     concat of `formatted_mic` (under `**You:**`) and
///     `formatted_sys` (under `**Other(s):**`).
pub fn render_markdown(detail: &MeetingDetail, llm_pass_text: Option<&str>) -> AppResult<String> {
    let title = detail.title.as_deref().unwrap_or("Untitled meeting");
    let duration = format_duration_hms(detail.total_duration_ms);

    let mut out = String::with_capacity(512 + detail.total_duration_ms as usize / 100);
    out.push_str("---\n");
    out.push_str(&format!("title: {}\n", yaml_scalar(title)));
    out.push_str(&format!("uuid: {}\n", yaml_scalar(&detail.uuid)));
    out.push_str(&format!(
        "started_at: {}\n",
        yaml_scalar(&detail.started_at)
    ));
    out.push_str(&format!("duration: {duration}\n"));
    out.push_str(&format!("source: {}\n", detail.source.as_db_str()));
    out.push_str(&format!(
        "formatter_version: {}\n",
        yaml_scalar(&detail.formatter_version)
    ));
    out.push_str("---\n\n");

    out.push_str(&format!("# {title}\n\n"));

    render_body(detail, &mut out);

    if let Some(llm) = llm_pass_text {
        // Ensure exactly one blank line before the section header
        // regardless of how the body ended.
        if !out.ends_with("\n\n") {
            if out.ends_with('\n') {
                out.push('\n');
            } else {
                out.push_str("\n\n");
            }
        }
        out.push_str("## LLM pass output\n\n");
        out.push_str(llm.trim_end());
        out.push('\n');
    }

    Ok(out)
}

fn render_body(detail: &MeetingDetail, out: &mut String) {
    match detail.source {
        MeetingSource::Mic => {
            let body = detail
                .formatted_mic
                .as_deref()
                .unwrap_or("_(no transcript)_");
            out.push_str(body.trim_end());
            out.push('\n');
        }
        MeetingSource::System => {
            let body = detail
                .formatted_sys
                .as_deref()
                .unwrap_or("_(no transcript)_");
            // Tag every paragraph with the speaker. Splits on blank
            // lines (the formatter's paragraph delimiter).
            for (idx, para) in body.split("\n\n").enumerate() {
                if idx > 0 {
                    out.push_str("\n\n");
                }
                out.push_str("**Other(s):** ");
                out.push_str(para.trim());
            }
            out.push('\n');
        }
        MeetingSource::Both => {
            if let Some(merged) = detail.formatted_merged.as_deref() {
                out.push_str(merged.trim_end());
                out.push('\n');
                return;
            }
            // Merged failed — best-effort interleave. NOT a strict
            // chronological merge (we don't have segment timestamps
            // here); just `**You:**` block followed by `**Other(s):**`
            // block. The UI labels this output as "merge unavailable
            // — showing channels sequentially".
            if let Some(mic) = detail.formatted_mic.as_deref() {
                for (idx, para) in mic.split("\n\n").enumerate() {
                    if idx > 0 {
                        out.push_str("\n\n");
                    }
                    out.push_str("**You:** ");
                    out.push_str(para.trim());
                }
                out.push('\n');
            }
            if let Some(sys) = detail.formatted_sys.as_deref() {
                if detail.formatted_mic.is_some() {
                    out.push('\n');
                }
                for (idx, para) in sys.split("\n\n").enumerate() {
                    if idx > 0 {
                        out.push_str("\n\n");
                    }
                    out.push_str("**Other(s):** ");
                    out.push_str(para.trim());
                }
                out.push('\n');
            }
        }
    }
}

/// Wrap a YAML scalar in double quotes if it contains characters that
/// would otherwise confuse a permissive YAML parser. Keeps the simple
/// case (ASCII alphanumeric + `-_.` + spaces) un-quoted.
fn yaml_scalar(s: &str) -> String {
    let needs_quote = s.is_empty()
        || s.chars()
            .any(|c| matches!(c, ':' | '#' | '\n' | '"' | '\'' | '[' | ']' | '{' | '}'));
    if needs_quote {
        let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
        format!("\"{escaped}\"")
    } else {
        s.to_string()
    }
}

/// Convert milliseconds → `H:MM:SS` (e.g. `1:23:45` or `0:00:42`).
fn format_duration_hms(total_ms: u64) -> String {
    let total_secs = total_ms / 1_000;
    let h = total_secs / 3_600;
    let m = (total_secs % 3_600) / 60;
    let s = total_secs % 60;
    format!("{h}:{m:02}:{s:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meetings::persist::MeetingStatus;

    fn fixture_detail() -> MeetingDetail {
        MeetingDetail {
            uuid: "uuid-123".to_string(),
            title: Some("Weekly sync".to_string()),
            started_at: "2026-05-20T10:00:00Z".to_string(),
            ended_at: "2026-05-20T11:00:00Z".to_string(),
            status: MeetingStatus::Complete,
            error_message: None,
            source: MeetingSource::Mic,
            total_duration_ms: 3_600_000,
            mic_duration_ms: Some(3_600_000),
            sys_duration_ms: None,
            formatter_version: "mc-v1".to_string(),
            whisper_model_id: "whisper-large-v3-turbo-q5_0".to_string(),
            formatted_mic: Some("Hello world.\n\nSecond paragraph.".to_string()),
            formatted_sys: None,
            formatted_merged: None,
        }
    }

    #[test]
    fn frontmatter_round_trips() {
        let detail = fixture_detail();
        let md = render_markdown(&detail, None).unwrap();
        // Frontmatter block: split on the second `---` line.
        let mut iter = md.split("---\n");
        let _first = iter.next();
        let yaml = iter.next().expect("frontmatter block present");
        // All six keys must appear.
        for key in [
            "title:",
            "uuid:",
            "started_at:",
            "duration:",
            "source:",
            "formatter_version:",
        ] {
            assert!(
                yaml.contains(key),
                "missing key {key} in frontmatter:\n{yaml}"
            );
        }
        // Specific value checks.
        assert!(yaml.contains("uuid: uuid-123"));
        assert!(yaml.contains("source: mic"));
        assert!(yaml.contains("duration: 1:00:00"));
    }

    #[test]
    fn mic_only_renders_body_without_other_label() {
        let detail = fixture_detail();
        let md = render_markdown(&detail, None).unwrap();
        assert!(md.contains("Hello world."));
        assert!(md.contains("Second paragraph."));
        assert!(
            !md.contains("**Other(s):**"),
            "mic-only should not label other speaker"
        );
        assert!(
            !md.contains("**You:**"),
            "mic-only should not label own speaker"
        );
    }

    #[test]
    fn system_only_labels_other_speaker() {
        let mut detail = fixture_detail();
        detail.source = MeetingSource::System;
        detail.formatted_mic = None;
        detail.formatted_sys = Some("First.\n\nSecond.".to_string());
        detail.mic_duration_ms = None;
        detail.sys_duration_ms = Some(60_000);

        let md = render_markdown(&detail, None).unwrap();
        assert!(md.contains("**Other(s):** First."));
        assert!(md.contains("**Other(s):** Second."));
    }

    #[test]
    fn both_renders_merged_when_present() {
        let mut detail = fixture_detail();
        detail.source = MeetingSource::Both;
        detail.formatted_sys = Some("Other paragraph.".to_string());
        detail.formatted_merged = Some("**You:** hi\n\n**Other(s):** hello".to_string());
        detail.sys_duration_ms = Some(60_000);

        let md = render_markdown(&detail, None).unwrap();
        assert!(md.contains("**You:** hi"));
        assert!(md.contains("**Other(s):** hello"));
        // Body should be the merged stream — not a sequential interleave
        // of mic-then-sys (which would put `Hello world.` ahead of
        // `**You:** hi`).
        let merged_idx = md.find("**You:** hi").unwrap();
        let mic_body_idx = md.find("Hello world.");
        assert!(
            mic_body_idx.map_or(true, |i| i > merged_idx),
            "merged stream should win over raw mic body"
        );
    }

    #[test]
    fn both_falls_back_to_interleave_when_merge_missing() {
        let mut detail = fixture_detail();
        detail.source = MeetingSource::Both;
        detail.formatted_sys = Some("Sys body.".to_string());
        detail.formatted_merged = None;
        detail.sys_duration_ms = Some(60_000);

        let md = render_markdown(&detail, None).unwrap();
        assert!(md.contains("**You:** Hello world."));
        assert!(md.contains("**You:** Second paragraph."));
        assert!(md.contains("**Other(s):** Sys body."));
    }

    #[test]
    fn llm_pass_section_appended_when_present() {
        let detail = fixture_detail();
        let md = render_markdown(&detail, Some("- bullet 1\n- bullet 2")).unwrap();
        assert!(md.contains("## LLM pass output"));
        assert!(md.ends_with("- bullet 1\n- bullet 2\n"));
    }

    #[test]
    fn llm_pass_section_omitted_when_none() {
        let detail = fixture_detail();
        let md = render_markdown(&detail, None).unwrap();
        assert!(!md.contains("## LLM pass output"));
    }

    #[test]
    fn untitled_meeting_renders_fallback_title() {
        let mut detail = fixture_detail();
        detail.title = None;
        let md = render_markdown(&detail, None).unwrap();
        assert!(md.contains("title: Untitled meeting"));
        assert!(md.contains("# Untitled meeting"));
    }

    #[test]
    fn duration_formats_h_mm_ss() {
        assert_eq!(format_duration_hms(0), "0:00:00");
        assert_eq!(format_duration_hms(42_000), "0:00:42");
        assert_eq!(format_duration_hms(60_000), "0:01:00");
        assert_eq!(format_duration_hms(3_600_000), "1:00:00");
        assert_eq!(format_duration_hms(3_661_000), "1:01:01");
        // 4-hour cap from PLAN — make sure long-form formats sanely.
        assert_eq!(format_duration_hms(4 * 3_600_000), "4:00:00");
    }

    #[test]
    fn yaml_scalar_quotes_when_needed() {
        assert_eq!(yaml_scalar("simple"), "simple");
        assert_eq!(yaml_scalar("with spaces"), "with spaces");
        assert_eq!(yaml_scalar("with: colon"), "\"with: colon\"");
        assert_eq!(yaml_scalar("with\"quote"), "\"with\\\"quote\"");
        assert_eq!(yaml_scalar(""), "\"\"");
    }

    #[test]
    fn export_with_no_mic_body_uses_placeholder() {
        let mut detail = fixture_detail();
        detail.formatted_mic = None;
        let md = render_markdown(&detail, None).unwrap();
        assert!(md.contains("_(no transcript)_"));
    }
}
