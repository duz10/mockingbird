//! Assemble the full cleanup prompt: system + dictionary + few-shot +
//! foreground app + raw transcript.
//!
//! Pure function; no I/O. Caller provides every input. Token budget
//! enforced by [`super::token_budget::BudgetPlan`]; the builder
//! shrinks blocks in priority order (dictionary first, then few-shot,
//! then truncate raw) before yielding the final prompt string.
//!
//! ## Output layout
//!
//! ```text
//! {SYSTEM_PROMPT_BODY}
//!
//! # Dictionary
//! - Term: Canonical
//! - ...
//!
//! # Examples
//! Input: ...
//! Output: ...
//! ...
//!
//! # Foreground
//! App: notepad.exe
//! Window: Untitled - Notepad
//!
//! # Transcribed Speech
//! {RAW TRANSCRIPT}
//! ```
//!
//! Blocks with empty content are omitted entirely (no header). Keeps
//! the prompt tight for short dictations.

use crate::db::dictionary::DictionaryEntry;
use crate::db::examples::StyleExample;
use crate::error::{AppError, AppResult};

use super::few_shot;
use super::token_budget::{estimate_tokens, BudgetPlan, RawFit};

/// Inputs to [`build`].
#[derive(Debug, Default)]
pub struct PromptInputs<'a> {
    /// The mode's system prompt (loaded from `cleanup/prompts/*.md`
    /// or fetched from the `prompts` table for non-default modes).
    pub system_prompt: &'a str,

    /// Dictionary entries in priority order (caller decides ordering;
    /// the builder will drop the tail if the block over-runs budget).
    pub dictionary: &'a [DictionaryEntry],

    /// Few-shot examples, pre-selected + pre-budgeted by
    /// [`super::few_shot::select_candidates`] +
    /// [`super::few_shot::fit_to_budget`].
    pub examples: &'a [StyleExample],

    /// Foreground app process name (e.g. `notepad.exe`).
    pub foreground_app: Option<&'a str>,

    /// Foreground window title (e.g. `Untitled - Notepad`).
    pub foreground_window_title: Option<&'a str>,

    /// Raw STT transcript. The thing we're cleaning.
    pub raw_transcript: &'a str,
}

/// Outcome of [`build`].
#[derive(Debug, Clone)]
pub struct BuiltPrompt {
    /// Final assembled prompt string ready to ship to a provider.
    pub prompt: String,

    /// Budget plan post-build — for `mb-token-budget-respected`
    /// judge logging.
    pub plan: BudgetPlan,

    /// `true` if the raw transcript had to be truncated to fit. Caller
    /// should `tracing::warn!` on this so a human can spot it in logs.
    pub raw_was_truncated: bool,
}

/// Render the dictionary block. Empty input → empty string.
fn render_dictionary(entries: &[DictionaryEntry]) -> String {
    if entries.is_empty() {
        return String::new();
    }
    let mut s = String::from("# Dictionary\n");
    for e in entries {
        s.push_str("- ");
        s.push_str(e.term.trim());
        if let Some(c) = e.canonical.as_deref() {
            s.push_str(": ");
            s.push_str(c.trim());
        }
        s.push('\n');
    }
    s.push('\n');
    s
}

/// Render the foreground-app block. Empty if both app + title missing.
fn render_foreground(app: Option<&str>, title: Option<&str>) -> String {
    if app.is_none() && title.is_none() {
        return String::new();
    }
    let mut s = String::from("# Foreground\n");
    if let Some(a) = app {
        s.push_str("App: ");
        s.push_str(a.trim());
        s.push('\n');
    }
    if let Some(t) = title {
        s.push_str("Window: ");
        s.push_str(t.trim());
        s.push('\n');
    }
    s.push('\n');
    s
}

/// Shrink dictionary entries until the rendered block fits the budget.
/// Drops from the tail (preserves caller-supplied priority order).
fn fit_dictionary(entries: &[DictionaryEntry], budget_tokens: u32) -> Vec<DictionaryEntry> {
    let mut kept = Vec::new();
    let mut running = estimate_tokens("# Dictionary\n\n");
    for e in entries {
        let per_entry = estimate_tokens("- ")
            + estimate_tokens(&e.term)
            + e.canonical
                .as_deref()
                .map(|c| estimate_tokens(": ") + estimate_tokens(c))
                .unwrap_or(0)
            + estimate_tokens("\n");
        if running + per_entry > budget_tokens {
            break;
        }
        running += per_entry;
        kept.push(e.clone());
    }
    kept
}

/// Build the full prompt. Returns [`AppError::Cleanup`] only when the
/// raw transcript cannot fit even after dropping every optional block.
pub fn build(inputs: PromptInputs<'_>) -> AppResult<BuiltPrompt> {
    let mut plan = BudgetPlan::new();

    // --- 1. System prompt — always included, capped at its budget.
    plan.record_system(estimate_tokens(inputs.system_prompt));

    // --- 2. Dictionary — fit to budget, render.
    let dict_fit = fit_dictionary(inputs.dictionary, super::token_budget::DICTIONARY_TOKENS);
    let dict_block = render_dictionary(&dict_fit);
    plan.record_dictionary(estimate_tokens(&dict_block));

    // --- 3. Few-shot — caller is expected to have already fit-to-budget,
    //        but we record the actual rendered cost.
    let fs_block = few_shot::render(inputs.examples);
    plan.record_few_shot(estimate_tokens(&fs_block));

    // --- 4. Foreground — fits in 100 tokens trivially.
    let fg_block = render_foreground(inputs.foreground_app, inputs.foreground_window_title);
    plan.record_fg_app(estimate_tokens(&fg_block));

    // --- 5. Raw transcript — fit, or fall back, or fail.
    let (raw_text, raw_was_truncated) = match plan.fit_raw_transcript(inputs.raw_transcript) {
        RawFit::Fits(s) => (s.to_string(), false),
        RawFit::Truncated {
            kept,
            dropped_tokens,
        } => {
            tracing::warn!(
                dropped_tokens,
                kept_len = kept.len(),
                "raw transcript truncated to fit cleanup budget"
            );
            (kept.to_string(), true)
        }
        RawFit::Overflow { raw_tokens, .. } => {
            // Fallback path: drop dictionary + few-shot, try again.
            tracing::warn!(
                raw_tokens,
                "raw transcript exceeds budget; retrying without dict + few-shot"
            );
            let mut fallback = BudgetPlan::new();
            fallback.record_system(estimate_tokens(inputs.system_prompt));
            fallback.record_fg_app(estimate_tokens(&fg_block));
            match fallback.fit_raw_transcript(inputs.raw_transcript) {
                RawFit::Fits(s) => {
                    plan = fallback;
                    (s.to_string(), false)
                }
                RawFit::Truncated {
                    kept,
                    dropped_tokens,
                } => {
                    plan = fallback;
                    tracing::warn!(
                        dropped_tokens,
                        kept_len = kept.len(),
                        "raw transcript truncated in fallback path"
                    );
                    (kept.to_string(), true)
                }
                RawFit::Overflow {
                    raw_tokens,
                    max_tokens,
                } => {
                    return Err(AppError::Cleanup(format!(
                        "PromptOverBudget: raw transcript needs {raw_tokens} tokens; \
                         max available {max_tokens}"
                    )));
                }
            }
        }
    };

    // --- Assemble. The dict / few-shot / fg blocks may be empty (no
    //     header in that case). Raw transcript is always last so the
    //     LLM's "continue the prompt" instinct lines up with cleanup.
    let mut prompt = String::with_capacity(
        inputs.system_prompt.len()
            + dict_block.len()
            + fs_block.len()
            + fg_block.len()
            + raw_text.len()
            + 64,
    );
    prompt.push_str(inputs.system_prompt.trim_end());
    prompt.push_str("\n\n");
    prompt.push_str(&dict_block);
    prompt.push_str(&fs_block);
    prompt.push_str(&fg_block);
    prompt.push_str("# Transcribed Speech\n");
    prompt.push_str(raw_text.trim());
    prompt.push('\n');

    Ok(BuiltPrompt {
        prompt,
        plan,
        raw_was_truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::dictionary::DictionaryEntry;

    fn dict_entry(term: &str, canonical: Option<&str>) -> DictionaryEntry {
        DictionaryEntry {
            id: 1,
            term: term.into(),
            canonical: canonical.map(String::from),
            source: "user".into(),
            confidence: 1.0,
            app_context: None,
            use_count: 0,
            last_used_at: None,
            created_at: "now".into(),
        }
    }

    #[test]
    fn build_minimal_inputs_renders_system_and_raw_only() {
        let result = build(PromptInputs {
            system_prompt: "SYSTEM",
            raw_transcript: "hello there",
            ..Default::default()
        })
        .unwrap();
        let p = result.prompt;
        assert!(p.starts_with("SYSTEM"));
        assert!(p.contains("# Transcribed Speech"));
        assert!(p.contains("hello there"));
        assert!(!p.contains("# Dictionary"), "no dict → no header");
        assert!(!p.contains("# Examples"), "no examples → no header");
        assert!(!p.contains("# Foreground"), "no fg → no header");
        assert!(!result.raw_was_truncated);
    }

    #[test]
    fn build_includes_dictionary_block_when_provided() {
        let entries = vec![
            dict_entry("Mockingbird", Some("Mockingbird")),
            dict_entry("kubectl", None),
        ];
        let result = build(PromptInputs {
            system_prompt: "SYSTEM",
            dictionary: &entries,
            raw_transcript: "hello",
            ..Default::default()
        })
        .unwrap();
        assert!(result.prompt.contains("# Dictionary"));
        assert!(result.prompt.contains("- Mockingbird: Mockingbird"));
        assert!(result.prompt.contains("- kubectl"));
        assert!(
            !result.prompt.contains("kubectl:"),
            "no canonical → no colon"
        );
    }

    #[test]
    fn build_includes_foreground_when_provided() {
        let result = build(PromptInputs {
            system_prompt: "SYSTEM",
            foreground_app: Some("notepad.exe"),
            foreground_window_title: Some("Untitled - Notepad"),
            raw_transcript: "hi",
            ..Default::default()
        })
        .unwrap();
        assert!(result.prompt.contains("# Foreground"));
        assert!(result.prompt.contains("App: notepad.exe"));
        assert!(result.prompt.contains("Window: Untitled - Notepad"));
    }

    #[test]
    fn build_truncates_raw_when_oversize() {
        let huge: String = "word ".repeat(20_000);
        let result = build(PromptInputs {
            system_prompt: "SYSTEM",
            raw_transcript: &huge,
            ..Default::default()
        })
        .unwrap();
        assert!(result.raw_was_truncated);
        assert!(result.prompt.len() < huge.len() + 100);
    }

    #[test]
    fn fit_dictionary_drops_tail_under_budget() {
        let entries: Vec<DictionaryEntry> = (0..10_000)
            .map(|i| dict_entry(&format!("term{i}"), Some(&format!("Canonical{i}"))))
            .collect();
        // Tiny budget — should keep very few.
        let kept = fit_dictionary(&entries, 50);
        assert!(kept.len() < 10, "kept {} entries", kept.len());
    }

    #[test]
    fn build_ordering_is_stable() {
        // System → Dict → Examples → FG → Raw.
        let entries = vec![dict_entry("X", Some("x"))];
        let result = build(PromptInputs {
            system_prompt: "SYSTEM",
            dictionary: &entries,
            foreground_app: Some("a.exe"),
            raw_transcript: "raw",
            ..Default::default()
        })
        .unwrap();
        let p = &result.prompt;
        let sys_idx = p.find("SYSTEM").unwrap();
        let dict_idx = p.find("# Dictionary").unwrap();
        let fg_idx = p.find("# Foreground").unwrap();
        let raw_idx = p.find("# Transcribed Speech").unwrap();
        assert!(sys_idx < dict_idx);
        assert!(dict_idx < fg_idx);
        assert!(fg_idx < raw_idx);
    }
}
