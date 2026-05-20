//! Optional LLM pass over a persisted meeting transcript.
//!
//! **Off the critical recording-to-canonical-transcript path.** The
//! `mc-no-llm-in-critical-path` judge (Wave 6) asserts this both
//! statically (code review) and dynamically (runtime instrumentation:
//! a tracer-counting wrapper around `OllamaProvider` during the
//! integration test that exercises the critical path; counter must
//! be 0).
//!
//! Wave 1 scaffold — types + `todo!()`. Wave 4 ships the impl.
//!
//! ## Binding (ADR 0026)
//!
//! - The `CleanupProvider` trait is NOT extended. This module
//!   constructs a fresh `OllamaProvider` via its existing public
//!   `new()` constructor and drives it through the existing
//!   `CleanupRequest<'_>` per call.
//! - The output is NEVER persisted to the DB. The runtime holds it
//!   in an in-memory `HashMap<Uuid, String>` keyed by a fresh UUID
//!   handed back to the caller as the `id` field; eviction on app
//!   shutdown.
//! - Prompt bodies live as MARKDOWN FILES under `meetings/prompts/`,
//!   loaded at call time via `include_str!`. The `modes` table is not
//!   touched.

use crate::error::AppResult;

/// Which built-in prompt to use, or a user-custom prompt body.
#[derive(Debug, Clone)]
pub enum LlmPassPrompt {
    /// Built-in prompt by name. Resolves to a `meetings/prompts/<name>.md`
    /// at call time via `include_str!`. Wave 4 inventory:
    /// `"summary" | "action_items" | "cleaner_punctuation"`.
    BuiltIn(&'static str),
    /// User-supplied prompt body, passed through verbatim.
    Custom(String),
}

/// One LLM-pass invocation request.
#[derive(Debug, Clone)]
pub struct LlmPassRequest {
    pub meeting_uuid: String,
    pub prompt: LlmPassPrompt,
    /// Optional model override; `None` uses the cleanup-provider default.
    pub model_id: Option<String>,
}

/// One LLM-pass invocation result. The `id` is the in-memory cache
/// handle the runtime returns to the IPC caller; the caller passes it
/// to `meeting_export_markdown` / `meeting_copy_to_clipboard` to embed
/// the LLM output in the export.
#[derive(Debug, Clone)]
pub struct LlmPassResult {
    pub id: String,
    pub text: String,
    pub latency_ms: u64,
}

/// Run one LLM pass. Synchronous (matches the cleanup provider's
/// blocking-`ureq` shape per ADR 0021).
///
/// Wave 1: `todo!()` — Wave 4 ships the impl.
pub fn run_llm_pass(_req: &LlmPassRequest, _transcript_text: &str) -> AppResult<LlmPassResult> {
    todo!("Wave 4: instantiate OllamaProvider; assemble prompt; return LlmPassResult")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_enum_constructs() {
        let _b = LlmPassPrompt::BuiltIn("summary");
        let _c = LlmPassPrompt::Custom("foo".into());
    }

    #[test]
    fn request_constructs() {
        let req = LlmPassRequest {
            meeting_uuid: "u".into(),
            prompt: LlmPassPrompt::BuiltIn("summary"),
            model_id: None,
        };
        assert_eq!(req.meeting_uuid, "u");
    }
}
