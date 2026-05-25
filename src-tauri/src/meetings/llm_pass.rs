//! Optional LLM pass over a persisted meeting transcript.
//!
//! **Off the critical recording-to-canonical-transcript path.** The
//! `mc-no-llm-in-critical-path` judge (Wave 6) asserts this both
//! statically (code review) and dynamically (runtime instrumentation:
//! a tracer-counting wrapper around `OllamaProvider` during the
//! integration test that exercises the critical path; counter must
//! be 0).
//!
//! Wave 4 ships the impl per `docs/phases/phase-mc-wave4-brief.md` §4.3.
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

use std::time::Instant;

use uuid::Uuid;

use crate::cleanup::provider::{CleanupProvider, CleanupRequest};
use crate::cleanup::OllamaProvider;
use crate::error::{AppError, AppResult};

/// System header for the summary / action-items built-ins AND for any
/// user-supplied custom prompt — concision is the safer general default.
///
/// Kept short and assertive; the per-pass body in the prompt file does
/// the heavy lifting.
pub const SYSTEM_HEADER_CONCISE: &str = "You are a meeting-transcript assistant. \
Be concise. Do not invent facts not in the transcript.";

/// System header for the `cleaner_punctuation` built-in. The cleaner is
/// a punctuation / whitespace pass, NOT a summarizer; the previous
/// global concise header was instructing the model to drop content,
/// which is exactly wrong for this prompt (ADR 0047 §Wave 1.1).
pub const SYSTEM_HEADER_PUNCTUATION: &str = "You are a transcript punctuation assistant. \
Preserve every word; modify only whitespace and punctuation.";

/// Pick the system header that goes with a given prompt.
///
/// Mapping (ADR 0047 §Wave 1.1):
///   - `BuiltIn("cleaner_punctuation")` → preservation header
///   - `BuiltIn("summary" | "action_items")` → concision header
///   - `BuiltIn(other)` → concision header (also the only sane default
///     if a new built-in is added without updating this mapping —
///     `resolve_builtin` will reject the unknown name first)
///   - `Custom(_)` → concision header. User-custom prompts are
///     general-purpose; concision is the safer fallback and matches
///     the meetings-engine framing this header has always carried.
pub const fn header_for_prompt(prompt: &LlmPassPrompt) -> &'static str {
    match prompt {
        LlmPassPrompt::BuiltIn(name) => match name.as_bytes() {
            b"cleaner_punctuation" => SYSTEM_HEADER_PUNCTUATION,
            _ => SYSTEM_HEADER_CONCISE,
        },
        LlmPassPrompt::Custom(_) => SYSTEM_HEADER_CONCISE,
    }
}

/// Default LLM-pass model when the caller doesn't override. Matches the
/// Phase-4 dictation default; the meeting feature stays in the same
/// local-Ollama lane unless the user picks otherwise.
pub const DEFAULT_MODEL_ID: &str = "qwen2.5:3b-instruct-q4_K_M";

/// Sampling temperature for the LLM pass. Per ADR 0026 we run on the
/// low side (factual summarization) but not 0 — Ollama's 0-temp path
/// has been observed to produce repetitive degenerate output on some
/// quantisations.
pub const DEFAULT_TEMPERATURE: f32 = 0.2;

/// Max response tokens. Long enough for a multi-paragraph summary +
/// action items; short enough that runaway generations don't burn 90s.
pub const DEFAULT_MAX_TOKENS: u32 = 2048;

// Built-in prompt bodies — baked into the binary at compile time. The
// markdown files are under `src-tauri/src/meetings/prompts/`. Adding a
// new built-in is two lines: drop the .md file, add an `include_str!`
// arm to `resolve_builtin`.
const SUMMARY_BODY: &str = include_str!("prompts/summary.md");
const ACTION_ITEMS_BODY: &str = include_str!("prompts/action_items.md");
const CLEANER_PUNCTUATION_BODY: &str = include_str!("prompts/cleaner_punctuation.md");

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

/// Resolve a `LlmPassPrompt` to its body string.
///
/// Pulled out so unit tests can exercise resolution without spinning
/// up a mock Ollama server.
pub fn resolve_prompt_body(prompt: &LlmPassPrompt) -> AppResult<&str> {
    match prompt {
        LlmPassPrompt::BuiltIn(name) => resolve_builtin(name),
        LlmPassPrompt::Custom(body) => Ok(body.as_str()),
    }
}

fn resolve_builtin(name: &str) -> AppResult<&'static str> {
    match name {
        "summary" => Ok(SUMMARY_BODY),
        "action_items" => Ok(ACTION_ITEMS_BODY),
        "cleaner_punctuation" => Ok(CLEANER_PUNCTUATION_BODY),
        other => Err(AppError::MeetingCapture(format!(
            "unknown built-in prompt: {other:?}"
        ))),
    }
}

/// Assemble the full prompt sent to Ollama:
///   `{system_header}\n\n{prompt_body}\n\n---\n\n{transcript_text}`.
///
/// The header is selected via [`header_for_prompt`] so the
/// `cleaner_punctuation` pass gets a preservation header instead of
/// the concision header that fits summary / action_items.
///
/// Pure function so the `mc-no-llm-in-critical-path` judge can prove
/// the formatter doesn't call this transitively.
pub fn assemble_prompt(prompt: &LlmPassPrompt, prompt_body: &str, transcript_text: &str) -> String {
    let header = header_for_prompt(prompt);
    format!("{header}\n\n{prompt_body}\n\n---\n\n{transcript_text}")
}

/// Run one LLM pass. Synchronous (matches the cleanup provider's
/// blocking-`ureq` shape per ADR 0021).
///
/// Test seam: this calls [`run_llm_pass_with_provider`] under the hood;
/// tests inject a custom-base-url `OllamaProvider` directly via that
/// inner function.
pub fn run_llm_pass(req: &LlmPassRequest, transcript_text: &str) -> AppResult<LlmPassResult> {
    let provider = OllamaProvider::new();
    run_llm_pass_with_provider(req, transcript_text, &provider)
}

/// Same as [`run_llm_pass`] but the caller supplies the provider. The
/// public surface stays slim (`run_llm_pass`); this is the seam that
/// tests use to point at a `with_base_url(...)` instance.
pub fn run_llm_pass_with_provider(
    req: &LlmPassRequest,
    transcript_text: &str,
    provider: &OllamaProvider,
) -> AppResult<LlmPassResult> {
    let prompt_body = resolve_prompt_body(&req.prompt)?;
    let full_prompt = assemble_prompt(&req.prompt, prompt_body, transcript_text);
    let model_id = req
        .model_id
        .as_deref()
        .unwrap_or(DEFAULT_MODEL_ID)
        .to_string();
    let mode_slug = "meeting:llm-pass";

    let cleanup_req = CleanupRequest {
        prompt: full_prompt.as_str(),
        raw_transcript: transcript_text,
        model_id: model_id.as_str(),
        temperature: DEFAULT_TEMPERATURE,
        max_tokens: DEFAULT_MAX_TOKENS,
        mode_slug,
    };

    let start = Instant::now();
    let result = provider.cleanup(cleanup_req)?;
    // The provider already measures its own latency, but we re-measure
    // here because the LlmPassResult's caller-visible `latency_ms` is
    // the wall-clock cost of the *pass*, not just the HTTP leg. They
    // happen to coincide today (no client-side post-processing yet),
    // but recording it independently keeps the invariant honest.
    let latency_ms = start.elapsed().as_millis() as u64;

    Ok(LlmPassResult {
        id: Uuid::new_v4().to_string(),
        text: result.text,
        latency_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

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

    #[test]
    fn builtin_prompt_resolves_to_markdown() {
        let body = resolve_prompt_body(&LlmPassPrompt::BuiltIn("summary")).unwrap();
        // `prompts/summary.md` opens with this exact phrase. If the
        // file is rewritten this assertion pinpoints the breakage.
        assert!(
            body.starts_with("You are summarizing a meeting transcript"),
            "summary.md content drifted; first 80 chars: {:?}",
            &body[..body.len().min(80)]
        );
    }

    #[test]
    fn builtin_prompt_action_items_resolves() {
        let body = resolve_prompt_body(&LlmPassPrompt::BuiltIn("action_items")).unwrap();
        assert!(!body.is_empty());
    }

    #[test]
    fn builtin_prompt_cleaner_punctuation_resolves() {
        let body = resolve_prompt_body(&LlmPassPrompt::BuiltIn("cleaner_punctuation")).unwrap();
        assert!(!body.is_empty());
    }

    #[test]
    fn builtin_prompt_unknown_name_errors() {
        let err = resolve_prompt_body(&LlmPassPrompt::BuiltIn("nonexistent")).unwrap_err();
        match err {
            AppError::MeetingCapture(msg) => {
                assert!(
                    msg.contains("unknown built-in prompt"),
                    "expected message to identify the unknown-prompt error: {msg}"
                );
                assert!(msg.contains("nonexistent"));
            }
            other => panic!("expected AppError::MeetingCapture, got {other:?}"),
        }
    }

    #[test]
    fn custom_prompt_passes_through_verbatim() {
        // Bind the variant locally so the &str the resolver returns
        // outlives the prompt (Custom owns its body; BuiltIn returns
        // &'static so doesn't need this dance).
        let prompt = LlmPassPrompt::Custom("hello world".into());
        let body = resolve_prompt_body(&prompt).unwrap();
        assert_eq!(body, "hello world");
    }

    #[test]
    fn assemble_prompt_layout_is_stable() {
        let prompt = LlmPassPrompt::BuiltIn("summary");
        let assembled = assemble_prompt(&prompt, "BODY", "TRANSCRIPT");
        assert!(assembled.starts_with(SYSTEM_HEADER_CONCISE));
        assert!(assembled.contains("\n\nBODY\n\n---\n\nTRANSCRIPT"));
        // System header MUST come first so the model sees it before
        // any user content. This is the only ordering invariant we pin.
        let header_idx = assembled.find(SYSTEM_HEADER_CONCISE).unwrap();
        let body_idx = assembled.find("BODY").unwrap();
        let transcript_idx = assembled.find("TRANSCRIPT").unwrap();
        assert!(header_idx < body_idx);
        assert!(body_idx < transcript_idx);
    }

    #[test]
    fn assemble_prompt_handles_empty_transcript() {
        let prompt = LlmPassPrompt::BuiltIn("summary");
        let assembled = assemble_prompt(&prompt, "BODY", "");
        assert!(assembled.ends_with("\n\n---\n\n"));
    }

    // -------- ADR 0047 §Wave 1.1 — per-pass system header mapping. --------

    /// `cleaner_punctuation`'s assembled prompt must NOT carry the
    /// concision instruction — that was the production bug. The
    /// punctuation pass exists to preserve every word; "be concise"
    /// is exactly the wrong steer.
    #[test]
    fn cleaner_punctuation_prompt_does_not_include_concise() {
        let prompt = LlmPassPrompt::BuiltIn("cleaner_punctuation");
        let assembled = assemble_prompt(&prompt, "BODY", "TRANSCRIPT");
        assert!(
            !assembled.to_lowercase().contains("concise"),
            "cleaner_punctuation header must not contain 'concise'; got: {assembled}"
        );
        // Sanity: the preservation header IS present.
        assert!(
            assembled.contains("Preserve every word"),
            "expected preservation header for cleaner_punctuation; got: {assembled}"
        );
    }

    /// `summary`'s assembled prompt MUST keep the concision
    /// instruction — this is the legacy behaviour we want to preserve.
    #[test]
    fn summary_prompt_keeps_concise_header() {
        let prompt = LlmPassPrompt::BuiltIn("summary");
        let assembled = assemble_prompt(&prompt, "BODY", "TRANSCRIPT");
        assert!(
            assembled.to_lowercase().contains("concise"),
            "summary header should request concision; got: {assembled}"
        );
    }

    /// `action_items` also gets the concision header (legacy parity).
    #[test]
    fn action_items_prompt_keeps_concise_header() {
        let prompt = LlmPassPrompt::BuiltIn("action_items");
        let assembled = assemble_prompt(&prompt, "BODY", "TRANSCRIPT");
        assert!(
            assembled.to_lowercase().contains("concise"),
            "action_items header should request concision; got: {assembled}"
        );
    }

    /// `Custom(_)` falls back to the concision header per ADR 0047
    /// §Wave 1.1 — user-custom prompts are general-purpose and concision
    /// is the safer default. (The dictation-driven LLM-pass path wraps
    /// its bodies in `Custom`; it accepts this trade-off.)
    #[test]
    fn custom_prompt_gets_concise_header() {
        let prompt = LlmPassPrompt::Custom("do whatever".into());
        let assembled = assemble_prompt(&prompt, "BODY", "TRANSCRIPT");
        assert!(
            assembled.contains(SYSTEM_HEADER_CONCISE),
            "Custom prompts should get the concision header; got: {assembled}"
        );
    }

    /// `header_for_prompt` directly: the mapping is the contract.
    #[test]
    fn header_for_prompt_maps_each_variant() {
        assert_eq!(
            header_for_prompt(&LlmPassPrompt::BuiltIn("cleaner_punctuation")),
            SYSTEM_HEADER_PUNCTUATION
        );
        assert_eq!(
            header_for_prompt(&LlmPassPrompt::BuiltIn("summary")),
            SYSTEM_HEADER_CONCISE
        );
        assert_eq!(
            header_for_prompt(&LlmPassPrompt::BuiltIn("action_items")),
            SYSTEM_HEADER_CONCISE
        );
        assert_eq!(
            header_for_prompt(&LlmPassPrompt::Custom("x".into())),
            SYSTEM_HEADER_CONCISE
        );
    }

    /// One-shot HTTP/1.1 server that responds with a canned Ollama
    /// `/api/chat` reply. Returns the bound port + a join handle.
    ///
    /// Avoids pulling in `httpmock`/`wiremock` as a dev-dep — the
    /// surface we exercise is a single POST + single response, which
    /// is well within hand-rolled-TCP territory. See the wave brief's
    /// deviations section for the justification.
    fn spawn_mock_ollama(canned_text: &'static str) -> (u16, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind 127.0.0.1:0");
        let port = listener.local_addr().unwrap().port();
        let handle = thread::spawn(move || {
            let (mut stream, _peer) = listener.accept().expect("accept");
            // Drain the request headers + body so the client's write
            // completes cleanly. We don't actually inspect them.
            let mut buf = [0_u8; 4096];
            // Read at least one chunk; in practice ureq sends the
            // full request in one TCP segment for a request this size.
            let _ = stream.read(&mut buf);
            let body = format!(
                r#"{{"model":"qwen2.5:3b-instruct-q4_K_M","message":{{"content":"{canned_text}"}},"prompt_eval_count":12,"eval_count":3}}"#
            );
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).expect("write");
            let _ = stream.flush();
        });
        (port, handle)
    }

    #[test]
    fn run_llm_pass_against_mock_ollama() {
        let (port, handle) = spawn_mock_ollama("ok");
        let provider = OllamaProvider::with_base_url(format!("http://127.0.0.1:{port}"));
        let req = LlmPassRequest {
            meeting_uuid: "uuid-mock".into(),
            prompt: LlmPassPrompt::Custom("Summarize.".into()),
            model_id: Some("qwen2.5:3b-instruct-q4_K_M".into()),
        };
        let result = run_llm_pass_with_provider(&req, "Hello world.", &provider).unwrap();
        assert_eq!(result.text, "ok");
        assert!(!result.id.is_empty());
        // `latency_ms` is u64 — a u64 >= 0 assertion is always true and
        // clippy will gripe. The meaningful invariant is just that the
        // call returned and populated the field; the type itself
        // enforces non-negativity.
        let _ = result.latency_ms;
        handle.join().expect("mock server thread");
    }

    #[test]
    fn run_llm_pass_with_unknown_builtin_short_circuits_before_http() {
        // No mock server stood up — proving we never reach the HTTP
        // call when the prompt resolution fails. If we did, the test
        // would hang on `ureq` connect timeout (~30s).
        let req = LlmPassRequest {
            meeting_uuid: "uuid-bad".into(),
            prompt: LlmPassPrompt::BuiltIn("does-not-exist"),
            model_id: None,
        };
        let provider = OllamaProvider::with_base_url("http://127.0.0.1:1".to_string());
        let err = run_llm_pass_with_provider(&req, "anything", &provider).unwrap_err();
        match err {
            AppError::MeetingCapture(msg) => assert!(msg.contains("unknown built-in prompt")),
            other => panic!("expected MeetingCapture, got {other:?}"),
        }
    }
}
