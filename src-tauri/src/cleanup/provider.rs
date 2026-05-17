//! `CleanupProvider` trait + shared request/response types.
//!
//! Per [ADR 0021](../../../docs/adr/0021-sync-cleanup-provider.md) the
//! trait is **sync** — orchestrator architecture is sync; HTTP is one
//! discrete blocking call per dictation.
//!
//! Implementations live next door: [`super::ollama`] +
//! [`super::claude`]. Tests use [`StubCleanupProvider`] for
//! deterministic transformations without hitting a real model.
//!
//! ## Why a CleanupRequest struct (not 7 positional args)
//!
//! The PLAN §8 signature took `(&prompt, &raw, &model_id, temp,
//! max_tokens)`. As Phase 4 also wires few-shot examples + dictionary
//! terms + foreground-app context into the assembled prompt, the
//! parameter list explodes. Wrap them in a struct now — fewer
//! breaking-change moments later when Phase 8's learning loop wants
//! to add a `correction_history` field.
//!
//! `CleanupRequest` borrows from caller storage (`&str` everywhere)
//! so providers don't force allocations on the hot path.
//!
//! ## What providers MUST NOT do
//!
//! - **Modify `raw_transcript`.** Period. Provenance is total
//!   (ADR 0010). The cleaner gets a borrow and is expected to return
//!   *new* text in `CleanupResult.text`. Hook-engine guards check this
//!   on the orchestrator side.
//! - **Retry forever.** Each provider owns a bounded retry policy;
//!   `cleanup()` MUST return within ~30 s wall-clock or the
//!   orchestrator will assume the LLM died and fall back per ADR 0021.
//! - **Mutate provider state.** `&self` is intentional. State that
//!   needs to evolve (e.g. token-bucket rate limiter) lives behind
//!   an internal `Mutex<...>`.

use crate::error::{AppError, AppResult};

/// What the cleanup layer hands to a provider.
///
/// All fields borrow; the caller (`LlmCleaner`) owns the underlying
/// strings. This keeps the per-call allocation count at zero in the
/// common path.
#[derive(Debug, Clone, Copy)]
pub struct CleanupRequest<'a> {
    /// Fully-assembled prompt: system + dictionary + few-shot +
    /// foreground-app context + raw transcript. Built by
    /// [`super::prompt_builder`]; budget-checked by
    /// [`super::token_budget`] before reaching the provider.
    pub prompt: &'a str,

    /// Raw STT output. Forwarded for provider-side logging only —
    /// providers MUST NOT inject this into the prompt themselves
    /// (it's already inside `prompt`). Useful for telemetry-off
    /// debug correlation in `tracing` spans.
    pub raw_transcript: &'a str,

    /// Provider-specific model identifier. Ollama: `qwen2.5:3b-...`.
    /// Claude: `claude-3-5-haiku-20241022`. The provider's
    /// `supports_model` is checked by the caller before this point.
    pub model_id: &'a str,

    /// Sampling temperature. 0.0–2.0. The cleanup layer pre-clamps.
    pub temperature: f32,

    /// Hard cap on response tokens.
    pub max_tokens: u32,

    /// Mode slug — `"normal"` / `"verbose"` / `"fragment"` / one of
    /// the AI command modes from migration 005. Providers may use it
    /// for per-mode log tags; orchestrator uses it to pick the prompt
    /// file (already baked into `prompt`).
    pub mode_slug: &'a str,
}

/// What the provider returns.
#[derive(Debug, Clone)]
pub struct CleanupResult {
    /// Cleaned text ready to inject. The orchestrator writes this to
    /// the `cleaned` and `final` transcript rows.
    pub text: String,

    /// Exact model identifier the provider used, for the
    /// `transcripts.model_used` provenance column. May differ from
    /// the requested `model_id` if e.g. Ollama redirected to a
    /// quantised variant — the provider records the *actual* model.
    pub model_used: String,

    /// Wall-clock cleanup latency in milliseconds, for the
    /// `sessions.cleanup_latency_ms` provenance column.
    pub latency_ms: u64,

    /// Provider-reported input token count, when available. Ollama
    /// returns this in the `prompt_eval_count` field of `/api/chat`
    /// responses; Claude returns it in `usage.input_tokens`.
    pub input_tokens: Option<u32>,

    /// Provider-reported output token count, when available.
    pub output_tokens: Option<u32>,
}

/// Sync cleanup-provider trait. See module docs for the rationale.
pub trait CleanupProvider: Send + Sync {
    /// Run cleanup and return the polished text. See
    /// [`CleanupRequest`] for caller-side invariants.
    fn cleanup(&self, request: CleanupRequest<'_>) -> AppResult<CleanupResult>;

    /// Identifier for `transcripts.model_used` provenance and for
    /// the `mb-provider-swappable` judge.
    fn provider_name(&self) -> &'static str;

    /// Whether this provider can serve the given model id. Used by
    /// the per-mode dispatcher to avoid sending an Ollama model id
    /// to the Claude provider.
    fn supports_model(&self, model_id: &str) -> bool;
}

// --------------------------------------------------------------------
// StubCleanupProvider — deterministic transformations for tests.
// --------------------------------------------------------------------

/// Deterministic, no-network cleanup provider for tests + the
/// `mb-modes-differ` judge's fixture suite.
///
/// Behaviour: returns a transformation of the *raw transcript* (not
/// the assembled prompt) keyed off the mode slug. This lets the
/// orchestrator integration tests prove the LLM is "in the loop"
/// without needing Ollama running.
///
/// - `normal` → trim + sentence-case + period
/// - `verbose` → identity (preserves length)
/// - `fragment` → first half of words, lowercased
/// - `rewrite` → reversed words (deterministic, distinguishable)
/// - `expand` → " — " separator + double the words
/// - `summarize` → first 5 words + "..."
/// - anything else → `format!("[{mode}] {raw}")`
///
/// The exact transformations don't matter to the orchestrator; the
/// tests assert that the output **differs** from raw (proving cleanup
/// ran) and **differs across modes** (proving mode-routing works).
#[derive(Debug, Default, Clone, Copy)]
pub struct StubCleanupProvider;

impl StubCleanupProvider {
    /// Construct. No state.
    pub fn new() -> Self {
        Self
    }
}

impl CleanupProvider for StubCleanupProvider {
    fn cleanup(&self, request: CleanupRequest<'_>) -> AppResult<CleanupResult> {
        let raw = request.raw_transcript.trim();
        let text = match request.mode_slug {
            "normal" => {
                let trimmed = raw.trim_end_matches(|c: char| c == '.' || c.is_whitespace());
                if trimmed.is_empty() {
                    String::new()
                } else {
                    let mut chars = trimmed.chars();
                    let first = chars.next().unwrap().to_uppercase().collect::<String>();
                    format!("{}{}.", first, chars.as_str())
                }
            }
            "verbose" => raw.to_string(),
            "fragment" => {
                let words: Vec<&str> = raw.split_whitespace().collect();
                let half = words.len().div_ceil(2);
                words[..half].join(" ").to_lowercase()
            }
            "rewrite" => raw.split_whitespace().rev().collect::<Vec<_>>().join(" "),
            "expand" => format!("{raw} — {raw}"),
            "summarize" => {
                let mut summary: Vec<&str> = raw.split_whitespace().take(5).collect();
                if !summary.is_empty() {
                    let owned = format!("{}...", summary.join(" "));
                    return Ok(CleanupResult {
                        text: owned,
                        model_used: "stub-summarize".into(),
                        latency_ms: 0,
                        input_tokens: Some(raw.split_whitespace().count() as u32),
                        output_tokens: Some(summary.len() as u32),
                    });
                }
                summary.clear();
                String::new()
            }
            other => format!("[{other}] {raw}"),
        };

        Ok(CleanupResult {
            text,
            model_used: format!("stub-{}", request.mode_slug),
            latency_ms: 0,
            input_tokens: Some(raw.split_whitespace().count() as u32),
            output_tokens: None,
        })
    }

    fn provider_name(&self) -> &'static str {
        "stub"
    }

    fn supports_model(&self, _model_id: &str) -> bool {
        true
    }
}

/// Convenience: convert a `ureq::Error` into [`AppError::Cleanup`]
/// with a useful message. Provider impls share this via
/// `map_err(provider_http_error)`.
pub(crate) fn provider_http_error(e: ureq::Error) -> AppError {
    match e {
        ureq::Error::Status(code, resp) => {
            let body = resp
                .into_string()
                .unwrap_or_else(|ioe| format!("<body read failed: {ioe}>"));
            AppError::Cleanup(format!("http {code}: {body}"))
        }
        ureq::Error::Transport(t) => AppError::Cleanup(format!("transport: {t}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req<'a>(mode: &'a str, raw: &'a str) -> CleanupRequest<'a> {
        CleanupRequest {
            prompt: "test prompt",
            raw_transcript: raw,
            model_id: "stub-model",
            temperature: 0.3,
            max_tokens: 256,
            mode_slug: mode,
        }
    }

    #[test]
    fn stub_normal_capitalises_and_periods() {
        let p = StubCleanupProvider;
        let r = p.cleanup(req("normal", "hello world")).unwrap();
        assert_eq!(r.text, "Hello world.");
        assert_eq!(r.model_used, "stub-normal");
        // Trait method reachable through the value (verifies trait-in-scope).
        assert_eq!(
            <StubCleanupProvider as CleanupProvider>::provider_name(&p),
            "stub"
        );
    }

    #[test]
    fn stub_verbose_preserves_input() {
        let p = StubCleanupProvider;
        let r = p
            .cleanup(req("verbose", "uh so basically the thing"))
            .unwrap();
        assert_eq!(r.text, "uh so basically the thing");
    }

    #[test]
    fn stub_fragment_keeps_first_half_lowercase() {
        let p = StubCleanupProvider;
        let r = p
            .cleanup(req("fragment", "ONE TWO THREE FOUR FIVE SIX"))
            .unwrap();
        // 6 words / div_ceil(2) = 3 words.
        assert_eq!(r.text, "one two three");
    }

    #[test]
    fn stub_rewrite_reverses_words() {
        let p = StubCleanupProvider;
        let r = p.cleanup(req("rewrite", "the quick brown fox")).unwrap();
        assert_eq!(r.text, "fox brown quick the");
    }

    #[test]
    fn stub_expand_duplicates() {
        let p = StubCleanupProvider;
        let r = p.cleanup(req("expand", "hi")).unwrap();
        assert_eq!(r.text, "hi — hi");
    }

    #[test]
    fn stub_summarize_first_five_words_plus_ellipsis() {
        let p = StubCleanupProvider;
        let r = p
            .cleanup(req("summarize", "one two three four five six seven"))
            .unwrap();
        assert_eq!(r.text, "one two three four five...");
    }

    #[test]
    fn stub_modes_produce_distinguishable_output() {
        let p = StubCleanupProvider;
        let raw = "hello there my friend";
        let outputs: Vec<String> = [
            "normal",
            "verbose",
            "fragment",
            "rewrite",
            "expand",
            "summarize",
        ]
        .iter()
        .map(|m| p.cleanup(req(m, raw)).unwrap().text)
        .collect();

        // mb-modes-differ: each mode produces a different string.
        for (i, a) in outputs.iter().enumerate() {
            for (j, b) in outputs.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b, "modes {i} and {j} returned identical output: {a:?}");
                }
            }
        }
    }

    #[test]
    fn stub_supports_any_model_and_reports_name() {
        let p = StubCleanupProvider;
        assert!(p.supports_model("anything"));
        assert_eq!(p.provider_name(), "stub");
    }
}
