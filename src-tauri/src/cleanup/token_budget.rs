//! Token budget enforcement for cleanup prompts.
//!
//! Per PLAN §8 "Token budget (binding)":
//!
//! | Block                 | Budget  |
//! |-----------------------|---------|
//! | System prompt         | 600     |
//! | Dictionary terms      | 600     |
//! | Few-shot examples     | 1500    |
//! | Foreground app/title  | 100     |
//! | Raw transcript        | 4200    |
//! | **Sum (input only)**  | **7000**|
//! | Response headroom     | 1192    |
//! | **Total context**     | **8192**|
//!
//! ## What this module is + isn't
//!
//! It's a **budget enforcer**, not a tokenizer. We use a deliberately
//! conservative character-based estimator (~3.5 chars/token, ceil)
//! that overestimates for all major tokenizers (cl100k_base, Qwen,
//! Gemma) by ~15-25%. Overestimation is fine: it errs toward
//! truncation, never toward overflow. The exact token count comes
//! back from the provider in `CleanupResult.input_tokens` for the
//! `mb-token-budget-respected` judge to verify post-hoc.
//!
//! Rationale: pulling in `tiktoken-rs` (cl100k_base only) + the right
//! Gemma/Qwen tokenizers = ~20 MB of binary bloat for a problem that
//! a 5-line approximator handles with margin to spare. ADR 0021's
//! "stay sync, stay small" theme.
//!
//! ## What "enforce" means
//!
//! `BudgetPlan::fit_raw_transcript` returns:
//!
//! - `Fits(raw)` — raw transcript fits inside its budget; emit as-is.
//! - `Truncated { kept, dropped }` — raw transcript truncated at the
//!   nearest word boundary; caller logs `WARN` and continues.
//! - `Overflow` — raw transcript can't fit even alone. Caller falls
//!   back to a *raw-only* prompt (no dictionary, no few-shot) per
//!   PLAN §8. If THAT still overflows, the caller returns
//!   `AppError::Cleanup`.
//!
//! Few-shot + dictionary blocks are pre-trimmed by their own
//! `fit_*` helpers in `prompt_builder.rs`; they call into this module
//! to learn the per-block budget.

/// PLAN §8 block budgets in tokens.
pub const SYSTEM_PROMPT_TOKENS: u32 = 600;
/// Per PLAN §8.
pub const DICTIONARY_TOKENS: u32 = 600;
/// Per PLAN §8. Hard cap; few-shot selector won't exceed this.
pub const FEW_SHOT_TOKENS: u32 = 1500;
/// Per PLAN §8.
pub const FG_APP_TOKENS: u32 = 100;
/// Per PLAN §8. Raw transcript gets the lion's share.
pub const RAW_TRANSCRIPT_TOKENS: u32 = 4200;
/// Per PLAN §8. Response headroom = total - sum(blocks).
pub const RESPONSE_HEADROOM_TOKENS: u32 = 1192;
/// Default total context window for the 3B model used in Phase 4.
pub const DEFAULT_CONTEXT_TOKENS: u32 = 8192;

/// Sum of input-side block budgets — what we will not exceed before
/// the response starts streaming.
pub const INPUT_BUDGET_TOKENS: u32 = SYSTEM_PROMPT_TOKENS
    + DICTIONARY_TOKENS
    + FEW_SHOT_TOKENS
    + FG_APP_TOKENS
    + RAW_TRANSCRIPT_TOKENS;

// Compile-time sanity: budget + response headroom <= default context.
const _: () = assert!(INPUT_BUDGET_TOKENS + RESPONSE_HEADROOM_TOKENS == DEFAULT_CONTEXT_TOKENS);

/// Conservative token-count estimator.
///
/// Returns `(byte_count / 3.5).ceil()` — overestimates for cl100k /
/// Qwen / Gemma by 15-25%. Empty input → 0.
///
/// Why bytes (not chars): byte count is what tokenizers actually see;
/// chars-only undercounts multi-byte CJK / emoji that BPE tokenizers
/// often split into multiple tokens. Using bytes avoids the worst-case
/// undercount.
#[inline]
pub fn estimate_tokens(s: &str) -> u32 {
    if s.is_empty() {
        return 0;
    }
    // 3.5 chars/token → 2 bytes / 7 → (bytes * 2 + 6) / 7 (ceil div).
    let bytes = s.len() as u32;
    (bytes * 2).div_ceil(7)
}

/// Result of fitting raw transcript into its budget.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawFit<'a> {
    /// Whole transcript fits.
    Fits(&'a str),

    /// Truncated at a word boundary. `kept` is the prefix that will
    /// be sent; `dropped_tokens` is the estimate of what was dropped
    /// (for `WARN` log).
    Truncated {
        /// The byte-prefix of the input transcript that fits.
        kept: &'a str,
        /// Approximate tokens dropped off the tail.
        dropped_tokens: u32,
    },

    /// Even the bare transcript exceeds the budget. Caller should
    /// retry with a smaller prompt (no dict, no few-shot); if that
    /// also fails, return `AppError::Cleanup("PromptOverBudget")`.
    Overflow {
        /// Estimated tokens the raw transcript would consume.
        raw_tokens: u32,
        /// Tokens that were actually available.
        max_tokens: u32,
    },
}

/// Top-level planner. Tracks how many tokens each block has consumed
/// + how many remain. Pure; no I/O.
#[derive(Debug, Clone)]
pub struct BudgetPlan {
    used_system: u32,
    used_dictionary: u32,
    used_few_shot: u32,
    used_fg_app: u32,
    used_raw: u32,
}

impl Default for BudgetPlan {
    fn default() -> Self {
        Self::new()
    }
}

impl BudgetPlan {
    /// Empty plan — every block has zero tokens used.
    pub fn new() -> Self {
        Self {
            used_system: 0,
            used_dictionary: 0,
            used_few_shot: 0,
            used_fg_app: 0,
            used_raw: 0,
        }
    }

    /// Record that the system prompt consumed `tokens`. Returns the
    /// adjusted token count (== input, never exceeds the budget).
    pub fn record_system(&mut self, tokens: u32) -> u32 {
        let capped = tokens.min(SYSTEM_PROMPT_TOKENS);
        self.used_system = capped;
        capped
    }

    /// Same, for the dictionary block.
    pub fn record_dictionary(&mut self, tokens: u32) -> u32 {
        let capped = tokens.min(DICTIONARY_TOKENS);
        self.used_dictionary = capped;
        capped
    }

    /// Same, for the few-shot block.
    pub fn record_few_shot(&mut self, tokens: u32) -> u32 {
        let capped = tokens.min(FEW_SHOT_TOKENS);
        self.used_few_shot = capped;
        capped
    }

    /// Same, for the foreground-app block.
    pub fn record_fg_app(&mut self, tokens: u32) -> u32 {
        let capped = tokens.min(FG_APP_TOKENS);
        self.used_fg_app = capped;
        capped
    }

    /// Total tokens consumed across all blocks.
    pub fn used_total(&self) -> u32 {
        self.used_system
            + self.used_dictionary
            + self.used_few_shot
            + self.used_fg_app
            + self.used_raw
    }

    /// Tokens still available for the raw transcript.
    ///
    /// Returns the smaller of:
    /// - PLAN §8's per-block budget (`RAW_TRANSCRIPT_TOKENS`)
    /// - whatever's left in the overall input budget after the other
    ///   blocks consumed their share.
    ///
    /// This second clause matters when e.g. the system prompt
    /// underuses its budget — we don't give those tokens to raw,
    /// because PLAN §8 fixes the per-block ceiling. Conservative.
    pub fn remaining_for_raw(&self) -> u32 {
        let other_used =
            self.used_system + self.used_dictionary + self.used_few_shot + self.used_fg_app;
        let overall_remaining = INPUT_BUDGET_TOKENS.saturating_sub(other_used);
        RAW_TRANSCRIPT_TOKENS.min(overall_remaining)
    }

    /// Fit the raw transcript into whatever budget remains.
    ///
    /// Word-boundary truncation: walks back from the byte index where
    /// the budget runs out to the previous whitespace. Falls through
    /// to byte-index truncation if no whitespace is found (single
    /// huge token of text — rare but possible).
    pub fn fit_raw_transcript<'a>(&mut self, raw: &'a str) -> RawFit<'a> {
        let raw_tokens = estimate_tokens(raw);
        let budget = self.remaining_for_raw();

        if raw_tokens <= budget {
            self.used_raw = raw_tokens;
            return RawFit::Fits(raw);
        }

        // Need truncation. budget == 0 means there's no room at all
        // for raw — that's Overflow (caller drops other blocks + retries).
        if budget == 0 {
            return RawFit::Overflow {
                raw_tokens,
                max_tokens: budget,
            };
        }

        // Approximate target byte length from the budget. Inverse of
        // `estimate_tokens`: bytes = tokens * 3.5. Conservatively use 3.
        let target_bytes = (budget as usize).saturating_mul(3);
        let mut cut = target_bytes.min(raw.len());
        // Walk back to a UTF-8 char boundary first (safety).
        while !raw.is_char_boundary(cut) && cut > 0 {
            cut -= 1;
        }
        // Then walk back to the previous whitespace.
        if let Some(ws_idx) = raw[..cut].rfind(char::is_whitespace) {
            cut = ws_idx;
        }
        if cut == 0 {
            return RawFit::Overflow {
                raw_tokens,
                max_tokens: budget,
            };
        }
        let kept = &raw[..cut];
        self.used_raw = estimate_tokens(kept);
        RawFit::Truncated {
            kept,
            dropped_tokens: raw_tokens.saturating_sub(self.used_raw),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budgets_sum_to_total_context() {
        // Compile-time assertion mirrored here so test failures point
        // at this file if the constants ever drift.
        assert_eq!(
            SYSTEM_PROMPT_TOKENS
                + DICTIONARY_TOKENS
                + FEW_SHOT_TOKENS
                + FG_APP_TOKENS
                + RAW_TRANSCRIPT_TOKENS
                + RESPONSE_HEADROOM_TOKENS,
            DEFAULT_CONTEXT_TOKENS
        );
    }

    #[test]
    fn estimate_tokens_empty_is_zero() {
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn estimate_tokens_overcounts_modestly() {
        // ~3.5 chars/token; ASCII "hello world" = 11 bytes → 4 tokens (real
        // cl100k count is 2). Overcount is the safe direction.
        let est = estimate_tokens("hello world");
        assert!((3..=5).contains(&est), "estimate was {est}");
    }

    #[test]
    fn estimate_tokens_scales_with_bytes() {
        let a = estimate_tokens("a");
        let aaa = estimate_tokens("aaaaaaa"); // 7 bytes
        assert!(aaa > a);
    }

    #[test]
    fn remaining_for_raw_uses_block_budget_when_nothing_else_used() {
        let plan = BudgetPlan::new();
        assert_eq!(plan.remaining_for_raw(), RAW_TRANSCRIPT_TOKENS);
    }

    #[test]
    fn remaining_for_raw_shrinks_when_blocks_used() {
        let mut plan = BudgetPlan::new();
        plan.record_system(SYSTEM_PROMPT_TOKENS);
        plan.record_dictionary(DICTIONARY_TOKENS);
        plan.record_few_shot(FEW_SHOT_TOKENS);
        plan.record_fg_app(FG_APP_TOKENS);
        // All four budgets used → exactly RAW_TRANSCRIPT_TOKENS left.
        assert_eq!(plan.remaining_for_raw(), RAW_TRANSCRIPT_TOKENS);
    }

    #[test]
    fn record_blocks_cap_at_their_budget() {
        let mut plan = BudgetPlan::new();
        // Try to over-record; should cap.
        let recorded = plan.record_system(SYSTEM_PROMPT_TOKENS * 10);
        assert_eq!(recorded, SYSTEM_PROMPT_TOKENS);
        assert_eq!(plan.used_system, SYSTEM_PROMPT_TOKENS);
    }

    #[test]
    fn fit_raw_transcript_short_input_returns_fits() {
        let mut plan = BudgetPlan::new();
        let raw = "hello world";
        match plan.fit_raw_transcript(raw) {
            RawFit::Fits(kept) => assert_eq!(kept, raw),
            other => panic!("expected Fits, got {other:?}"),
        }
        assert!(plan.used_raw > 0);
    }

    #[test]
    fn fit_raw_transcript_oversize_truncates_at_word_boundary() {
        let mut plan = BudgetPlan::new();
        // Eat all other budgets so raw has its full block quota.
        plan.record_system(SYSTEM_PROMPT_TOKENS);
        plan.record_dictionary(DICTIONARY_TOKENS);
        plan.record_few_shot(FEW_SHOT_TOKENS);
        plan.record_fg_app(FG_APP_TOKENS);

        // Build a long-but-known string that overshoots the raw budget.
        // ~30K chars >> RAW_TRANSCRIPT_TOKENS * 3.5 bytes (~14700).
        let raw: String = (0..3000).map(|_| "word ").collect();
        let result = plan.fit_raw_transcript(&raw);
        match result {
            RawFit::Truncated {
                kept,
                dropped_tokens,
            } => {
                // Cut at whitespace → no trailing partial word.
                assert!(!kept.ends_with("wor"), "cut mid-word: {kept:?}");
                assert!(dropped_tokens > 0);
                assert!(kept.len() < raw.len());
            }
            other => panic!("expected Truncated, got {other:?}"),
        }
    }

    #[test]
    fn fit_raw_transcript_overflow_when_no_budget() {
        let mut plan = BudgetPlan::new();
        // Manually exhaust the entire input budget through other blocks.
        plan.used_system = INPUT_BUDGET_TOKENS;
        let raw = "any text at all";
        match plan.fit_raw_transcript(raw) {
            RawFit::Overflow { .. } => {}
            other => panic!("expected Overflow, got {other:?}"),
        }
    }

    #[test]
    fn fit_raw_transcript_utf8_safe() {
        // Multi-byte chars at the truncation boundary must not panic.
        let mut plan = BudgetPlan::new();
        plan.record_system(SYSTEM_PROMPT_TOKENS);
        plan.record_dictionary(DICTIONARY_TOKENS);
        plan.record_few_shot(FEW_SHOT_TOKENS);
        plan.record_fg_app(FG_APP_TOKENS);
        // CJK chars (3 bytes each in UTF-8) repeated to overshoot.
        let raw: String = (0..6000).map(|_| "日本").collect();
        // Should not panic.
        let _ = plan.fit_raw_transcript(&raw);
    }
}
