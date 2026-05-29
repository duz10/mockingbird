//! Extract pass — pulls title, due date (or null), and raw topic
//! tags from one classified segment.
//!
//! The date hard-gate (PLAN §8.4, LESSONS P9) lives BOTH in the
//! prompt (HARD GATE instructions + an explicit "soon → null"
//! example) AND in validation here: a non-null `due_iso` must be
//! parseable as `YYYY-MM-DD` or the pass reports a `Validation`
//! error so the scorer can flag the dictation. We do NOT silently
//! re-coerce a malformed date to `null`, because that would mask
//! an LLM that's shouting "I want to invent a date" — Wave 3 needs
//! to see the shouting.
//!
//! Graduated verbatim from the sandbox under Wave 2 Task 6
//! (`mb-rtk2`) — import-path rewrites only. The hard-gate logic is
//! load-bearing and intentionally unchanged.

use chrono::{DateTime, Datelike, Duration, NaiveDate, Weekday};
use serde::{Deserialize, Serialize};

use super::super::ollama::{GenerateOptions, OllamaDispatcher};
use super::classify::Classification;
use super::{strip_json_envelope, PassError};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Extraction {
    pub title: String,
    pub due_iso: Option<String>,
    pub raw_topic_tags: Vec<String>,
    /// Wave 0.5.3 (Move 3 / `mb-rzpd`): the closed-vocab prompt asks
    /// the model to surface concepts it wants to tag but cannot find
    /// in the vocabulary, here. Optional + `default` so the
    /// small-conservative (open-vocab) prompt remains JSON-compatible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposed_new_tags: Option<Vec<ProposedNewTag>>,
}

/// One model-suggested out-of-vocabulary tag. Wave 0.5.3 / `mb-rzpd`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ProposedNewTag {
    pub tag: String,
    #[serde(default)]
    pub rationale: String,
}

/// `prompt_body` is the verbatim contents of `prompts/extract.md`
/// (or the override per the model-class profile) as loaded by
/// `crate::kg::schema_loader::Schema`.
#[allow(clippy::too_many_arguments)]
pub fn extract<D: OllamaDispatcher>(
    dispatcher: &D,
    model: &str,
    prompt_body: &str,
    segment: &str,
    classification: &Classification,
    captured_iso: &str,
    options: &GenerateOptions,
) -> Result<Extraction, PassError> {
    let calendar = calendar_context(captured_iso);
    let classification_json = serde_json::to_string(&serde_json::json!({
        "category": format!("{:?}", classification.category).to_lowercase(),
        "entry_type": format!("{:?}", classification.entry_type).to_lowercase(),
    }))
    .expect("serialize classification context");

    let prompt = format!(
        "{prompt_body}\n\nCONTEXT: {calendar}\nSEGMENT:\n{segment}\nCLASSIFICATION: {classification_json}\n"
    );

    let raw = dispatcher.generate(model, &prompt, None, options)?;
    let candidate = strip_json_envelope(&raw);
    let parsed: Extraction = serde_json::from_str(candidate).map_err(|e| PassError::Parse {
        pass: "extract",
        error: e.to_string(),
        raw: raw.clone(),
    })?;

    // Validate: title non-empty.
    if parsed.title.trim().is_empty() {
        return Err(PassError::Validation {
            pass: "extract",
            detail: "title is empty".to_string(),
            raw,
        });
    }
    // Validate: due_iso, when Some, must be YYYY-MM-DD. (PLAN §8.4
    // hard gate, LESSONS P9.)
    if let Some(d) = parsed.due_iso.as_ref() {
        if NaiveDate::parse_from_str(d, "%Y-%m-%d").is_err() {
            return Err(PassError::Validation {
                pass: "extract",
                detail: format!("due_iso not YYYY-MM-DD: {d:?}"),
                raw,
            });
        }
    }

    Ok(parsed)
}

/// Build the one-line calendar reference the extract prompt
/// consumes. Format: `Today is <Wkd> YYYY-MM-DD. Mon=YYYY-MM-DD,
/// Fri=YYYY-MM-DD, next Mon=YYYY-MM-DD.`
///
/// "Mon" / "Fri" are the *upcoming* occurrence (today if today is
/// that day, otherwise the next future one). "next Mon" is the Mon
/// after that, giving the model an explicit anchor when the speaker
/// says "Monday" vs "next Monday."
///
/// Falls back to a bare ISO string if the captured_iso can't be
/// parsed — the pass still works, the model just loses the
/// weekday hint.
fn calendar_context(captured_iso: &str) -> String {
    let date = match DateTime::parse_from_rfc3339(captured_iso) {
        Ok(dt) => dt.date_naive(),
        Err(_) => return format!("captured at {captured_iso}"),
    };
    let weekday = date.weekday();
    let upcoming_mon = next_or_today(date, Weekday::Mon);
    let upcoming_fri = next_or_today(date, Weekday::Fri);
    let next_mon = upcoming_mon + Duration::days(7);
    format!(
        "Today is {wkd} {date}. Mon={mon}, Fri={fri}, next Mon={next_mon}.",
        wkd = weekday_short(weekday),
        date = date.format("%Y-%m-%d"),
        mon = upcoming_mon.format("%Y-%m-%d"),
        fri = upcoming_fri.format("%Y-%m-%d"),
        next_mon = next_mon.format("%Y-%m-%d"),
    )
}

fn next_or_today(d: NaiveDate, target: Weekday) -> NaiveDate {
    let diff = (target.num_days_from_monday() as i64 - d.weekday().num_days_from_monday() as i64)
        .rem_euclid(7);
    d + Duration::days(diff)
}

fn weekday_short(w: Weekday) -> &'static str {
    match w {
        Weekday::Mon => "Mon",
        Weekday::Tue => "Tue",
        Weekday::Wed => "Wed",
        Weekday::Thu => "Thu",
        Weekday::Fri => "Fri",
        Weekday::Sat => "Sat",
        Weekday::Sun => "Sun",
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::ollama::testing::MockOllama;
    use super::super::super::schema::{Category, EntryType};
    use super::*;

    fn opts() -> GenerateOptions {
        GenerateOptions::default()
    }

    const TEST_PROMPT: &str = "dummy extract prompt body";

    fn classification() -> Classification {
        Classification {
            category: Category::Personal,
            entry_type: EntryType::Task,
        }
    }

    #[test]
    fn parses_date() {
        let mock = MockOllama::new().default_response(
            r#"{"title":"Call daycare about Tyler","due_iso":"2026-06-15","raw_topic_tags":["daycare","kids"]}"#,
        );
        let r = extract(
            &mock,
            "m",
            TEST_PROMPT,
            "Call the daycare on Monday about Tyler's spot.",
            &classification(),
            "2026-06-14T08:00:00Z",
            &opts(),
        )
        .unwrap();
        assert_eq!(r.due_iso.as_deref(), Some("2026-06-15"));
        assert_eq!(r.title, "Call daycare about Tyler");
        assert_eq!(r.raw_topic_tags, vec!["daycare", "kids"]);
    }

    #[test]
    fn emits_null_due_for_vague_phrase() {
        let mock = MockOllama::new().default_response(
            r#"{"title":"Start a woodworking podcast","due_iso":null,"raw_topic_tags":["podcast","woodworking"]}"#,
        );
        let r = extract(
            &mock,
            "m",
            TEST_PROMPT,
            "I should start a woodworking podcast soon.",
            &classification(),
            "2026-06-14T08:00:00Z",
            &opts(),
        )
        .unwrap();
        assert!(r.due_iso.is_none(), "vague 'soon' must yield null");
    }

    #[test]
    fn rejects_malformed_due_iso() {
        let mock = MockOllama::new()
            .default_response(r#"{"title":"Test","due_iso":"next Friday","raw_topic_tags":["x"]}"#);
        let err = extract(
            &mock,
            "m",
            TEST_PROMPT,
            "anything",
            &classification(),
            "2026-06-14T08:00:00Z",
            &opts(),
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("extract"));
        assert!(msg.contains("YYYY-MM-DD"));
        // Critical for Wave 3: raw output preserved.
        assert!(msg.contains("next Friday"));
    }

    #[test]
    fn rejects_empty_title() {
        let mock = MockOllama::new()
            .default_response(r#"{"title":"   ","due_iso":null,"raw_topic_tags":["x"]}"#);
        let err = extract(
            &mock,
            "m",
            TEST_PROMPT,
            "anything",
            &classification(),
            "2026-06-14T08:00:00Z",
            &opts(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("title is empty"));
    }

    #[test]
    fn calendar_context_uses_sunday_anchor() {
        // 2026-06-14 is a Sunday (per CORPUS_NOTES capture anchor).
        let cx = calendar_context("2026-06-14T08:00:00Z");
        assert!(cx.contains("Sun 2026-06-14"), "got: {cx}");
        assert!(cx.contains("Mon=2026-06-15"), "got: {cx}");
        assert!(cx.contains("Fri=2026-06-19"), "got: {cx}");
        assert!(cx.contains("next Mon=2026-06-22"), "got: {cx}");
    }

    #[test]
    fn calendar_context_when_today_is_monday_returns_today() {
        let cx = calendar_context("2026-06-15T08:00:00Z");
        // Today IS Mon, so upcoming Mon == today.
        assert!(cx.contains("Mon 2026-06-15"), "got: {cx}");
        assert!(cx.contains("Mon=2026-06-15"), "got: {cx}");
        assert!(cx.contains("next Mon=2026-06-22"), "got: {cx}");
    }
}
