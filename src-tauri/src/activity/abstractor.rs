//! Stage 3 of the Wave-3 summarization pipeline: **LLM-abstract one
//! Block at a time**.
//!
//! Per ADR 0040 §Decision item 2 we drive [`OllamaProvider`] directly
//! — no `CleanupProvider` trait extension, no detour through
//! `meetings::llm_pass`. The call shape per Block is small:
//!
//!   1. Build a structured prompt-context block from the Block's
//!      focus events (app, title, monitor, focused field,
//!      visible-text fragments).
//!   2. If the context is "no payload" (game window, locked screen,
//!      single event with nothing inside), short-circuit to a
//!      deterministic template — ADR 0040 §Decision item 3.
//!   3. Otherwise concatenate the bundled prompt body
//!      (`prompts/abstract_block.md`) + the context block + run
//!      through [`OllamaProvider::cleanup`].
//!   4. On any provider error, fall back to the same template so the
//!      session still gets a renderable summary (Principle 4 of
//!      `mockingbird-activity-capture-plan.md`: degrade gracefully).
//!
//! Provenance: each return value carries the prompt-version SHA the
//! call used. Real LLM responses get the file-hash of the bundled
//! prompts directory; template fallbacks get the sentinel
//! `"template_no_payload_v1"` so the `activity_blocks.prompt_version_sha`
//! column distinguishes them.

// Public IPC-facing structs mirror the v2 UIA payload closely. The
// fields are self-describing (and the schema is documented in
// ADR 0040 + prompts/abstract_block.md). Skip per-field docs here
// to avoid restating obvious wire schema.
#![allow(missing_docs)]

use crate::activity::blocker::Block;
use crate::activity::segmenter::NormalizedEvent;
use crate::cleanup::ollama::OllamaProvider;
use crate::cleanup::provider::{CleanupProvider, CleanupRequest};
use crate::error::AppResult;

/// LLM defaults. Same model as the meeting LLM pass — there's no
/// case for shipping a different model just for activity (Ollama
/// keeps one resident, so re-using cuts cold-load cost).
const DEFAULT_MODEL_ID: &str = "qwen2.5:3b-instruct-q4_K_M";
const DEFAULT_TEMPERATURE: f32 = 0.2;
const DEFAULT_MAX_TOKENS: u32 = 256; // one sentence; the prompt caps it at 25 words

/// Bundled prompt body. Loaded at call-time, not at struct-init —
/// `include_str!` so it's baked into the binary, no IO needed.
const ABSTRACT_PROMPT_BODY: &str = include_str!("prompts/abstract_block.md");

/// Reserved for Wave 4; included in the prompt-set SHA so a future
/// content change automatically updates the hash without us having
/// to remember to bump it. Wave 3 never reads from this file at
/// runtime — only its bytes contribute to provenance.
const ABSTRACT_AUDIO_AWARE_PROMPT_BODY: &str =
    include_str!("prompts/abstract_block.audio_aware.md");

/// Sentinel SHA for the no-payload / fallback-template path.
pub const TEMPLATE_NO_PAYLOAD_SHA: &str = "template_no_payload_v1";

/// The abstractor's output for a single Block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockAbstract {
    /// LLM-generated (or template-generated) one-sentence summary.
    pub text: String,
    /// SHA-256 (lowercase hex) of the prompt set the call used, or
    /// the sentinel above for template fallbacks.
    pub prompt_version_sha: String,
    /// True if the LLM was called; false if the deterministic
    /// template short-circuited.
    pub used_llm: bool,
}

/// Compute the prompt-set fingerprint the abstractor would use
/// today. This is the value that gets written to
/// `activity_blocks.prompt_version_sha` /
/// `activity_sessions.prompt_set_sha` for every LLM-using Block.
/// Caller pre-computes it once per session to satisfy "provenance is
/// total" (Principle 2) without re-hashing on every Block.
///
/// Implementation note (ADR 0040 §Decision item 3): we use CRC32 over
/// the concatenated prompt bodies, not SHA-256. The column is named
/// `*_sha` for historical alignment with the meetings schema, but the
/// requirement here is "detect content drift", not cryptographic
/// collision-resistance. CRC32 is in workspace deps already; adding
/// SHA-256 just for a provenance fingerprint isn't worth the
/// dependency weight. The output is `abstract_v1-<crc8hex>` so the
/// `_v1` carries the schema major (bump on prompt-shape changes the
/// CRC alone wouldn't make obvious in a diff) and the CRC carries
/// the content fingerprint.
pub fn current_prompt_set_sha() -> String {
    let mut h = crc32fast::Hasher::new();
    h.update(b"abstract_block.md:");
    h.update(ABSTRACT_PROMPT_BODY.as_bytes());
    h.update(b"\nabstract_block.audio_aware.md:");
    h.update(ABSTRACT_AUDIO_AWARE_PROMPT_BODY.as_bytes());
    format!("abstract_v1-{:08x}", h.finalize())
}

/// The structured Block context the LLM sees. Owns all of its
/// strings — the lifetime tension between borrowed `NormalizedEvent`
/// fields and per-Block parsed JSON is not worth the savings (we
/// run this once per Block, max once per minute in real use). Stable
/// across Wave 3 → Wave 4 (Wave 4 adds an `audio_transcript_excerpt`
/// field without breaking the existing prompt template; the
/// audio-aware prompt picks it up).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BlockContext {
    pub app: String,
    pub title: String,
    pub monitor_name: Option<String>,
    pub duration_human: String,
    pub focused_field: Option<FocusedFieldContext>,
    pub visible_text_fragments: Vec<String>,
    pub password_field_active: bool,
    /// True iff the input had at least one `status.kind="ok"` or
    /// no_payload-distinguishing snapshot. Used by
    /// [`should_short_circuit`] to gate LLM calls.
    pub has_real_payload: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusedFieldContext {
    pub control_type: String,
    pub name: String,
    pub value: String,
}

/// Drive the abstractor against one Block. The provider parameter
/// lets tests inject a custom-base-url [`OllamaProvider`] (mirroring
/// `meetings::llm_pass::run_llm_pass_with_provider`).
pub fn abstract_block_with_provider(
    block: &Block,
    provider: &dyn CleanupProvider,
    prompt_set_sha: &str,
) -> AppResult<BlockAbstract> {
    let context = build_context(block);
    if should_short_circuit(&context, block) {
        return Ok(template_abstract(&context));
    }
    let user_block = render_user_block(&context);
    let full_prompt = format!("{ABSTRACT_PROMPT_BODY}\n\n```\n{user_block}\n```");

    let req = CleanupRequest {
        prompt: full_prompt.as_str(),
        raw_transcript: user_block.as_str(),
        model_id: DEFAULT_MODEL_ID,
        temperature: DEFAULT_TEMPERATURE,
        max_tokens: DEFAULT_MAX_TOKENS,
        mode_slug: "activity:abstract-block",
    };

    match provider.cleanup(req) {
        Ok(result) => Ok(BlockAbstract {
            text: clean_response(&result.text),
            prompt_version_sha: prompt_set_sha.to_string(),
            used_llm: true,
        }),
        Err(e) => {
            // Graceful degradation (Principle 4 of the activity plan,
            // ADR 0040 §Decision item 3). Log + emit a template.
            tracing::warn!(
                target: "activity::abstractor",
                error = %e,
                app = %context.app,
                "LLM abstraction failed; falling back to deterministic template"
            );
            Ok(template_abstract(&context))
        }
    }
}

/// Convenience for the common case: build a default [`OllamaProvider`]
/// and run the abstraction. The caller still has to thread the
/// `prompt_set_sha` because it should be computed once per session
/// for provenance consistency.
pub fn abstract_block(block: &Block, prompt_set_sha: &str) -> AppResult<BlockAbstract> {
    let provider = OllamaProvider::new();
    abstract_block_with_provider(block, &provider, prompt_set_sha)
}

// ---------------------------------------------------------------------------
// Context assembly
// ---------------------------------------------------------------------------

/// Build the structured context from a Block's focus events. Uses
/// the FIRST focus event with a v2 snapshot as the canonical context
/// source — its UIA payload is usually the richest. Falls back to
/// the Block's primary_app/title when no snapshots exist.
fn build_context(block: &Block) -> BlockContext {
    let duration_human = crate::activity::assembler::format_duration(block.duration_ms());
    let mut ctx = BlockContext {
        app: block.primary_app.clone(),
        title: block.primary_title.clone(),
        duration_human,
        ..Default::default()
    };

    let first_snap = block.focus_events.iter().find_map(|e| match e {
        NormalizedEvent::AppFocus { snapshot_json, .. } => snapshot_json.as_deref(),
        _ => None,
    });

    let mut v2_snapshot_parsed = false;
    if let Some(json) = first_snap {
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json) {
            if parsed.get("schema").and_then(|s| s.as_str()) == Some("v2") {
                v2_snapshot_parsed = true;
                populate_from_v2(&mut ctx, &parsed);
            }
        }
    }
    // Wave-1B-only sessions (no v2 snapshot anywhere) can still have a
    // real payload to talk about if the Block carries a non-empty
    // title — the LLM call is informative even with thin context.
    // Crucially: we only apply this promotion when no v2 snapshot was
    // parsed. If v2 ran and said `no_payload`, that verdict is
    // authoritative (ADR 0040 §Decision item 3) and we must NOT
    // re-promote.
    if !v2_snapshot_parsed
        && !ctx.has_real_payload
        && !ctx.title.trim().is_empty()
        && !block.focus_events.is_empty()
    {
        ctx.has_real_payload = true;
    }
    ctx
}

fn populate_from_v2(ctx: &mut BlockContext, v: &serde_json::Value) {
    // App / title come from the v2 payload when present — the
    // segmenter already aligns these with the Block's primary, but
    // the v2 fields are the canonical source.
    if let Some(s) = v.get("app").and_then(|a| a.as_str()) {
        if !s.is_empty() {
            ctx.app = s.to_string();
        }
    }
    if let Some(s) = v.get("title").and_then(|a| a.as_str()) {
        if !s.is_empty() {
            ctx.title = s.to_string();
        }
    }
    if let Some(m) = v.get("monitor") {
        if let Some(name) = m.get("name").and_then(|n| n.as_str()) {
            ctx.monitor_name = Some(name.to_string());
        }
    }
    if let Some(pwd) = v.get("passwordFieldActive").and_then(|p| p.as_bool()) {
        ctx.password_field_active = pwd;
    }
    if let Some(ff) = v.get("focusedField") {
        if !ff.is_null() {
            let name = ff
                .get("name")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string();
            let ct = ff
                .get("controlType")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string();
            let val = ff
                .get("value")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string();
            if !name.is_empty() || !ct.is_empty() || !val.is_empty() {
                ctx.focused_field = Some(FocusedFieldContext {
                    control_type: ct,
                    name,
                    value: val,
                });
            }
        }
    }
    if let Some(arr) = v.get("visibleTextFragments").and_then(|f| f.as_array()) {
        ctx.visible_text_fragments = arr
            .iter()
            .filter_map(|x| x.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
    }
    // has_real_payload follows the snapshot status. Any status that
    // ISN'T explicitly `no_payload` counts as real (ok + failed both
    // do — failed because the OS told us *something* tried to read).
    let no_payload = v
        .get("status")
        .and_then(|s| s.get("kind"))
        .and_then(|k| k.as_str())
        == Some("no_payload");
    ctx.has_real_payload = !no_payload;
}

/// Render the user-visible block of the LLM prompt — the part that
/// goes inside the triple-backtick fence after the bundled prompt
/// body. Format matches the examples in `prompts/abstract_block.md`
/// (key: value lines plus the structured `focusedField` line).
fn render_user_block(ctx: &BlockContext) -> String {
    let mut s = String::with_capacity(512);
    s.push_str(&format!("app: {}\n", ctx.app));
    s.push_str(&format!("title: {}\n", ctx.title));
    s.push_str(&format!("duration: {}\n", ctx.duration_human));
    if let Some(m) = &ctx.monitor_name {
        s.push_str(&format!("monitor: {m}\n"));
    }
    if let Some(ff) = &ctx.focused_field {
        // Mirror the prompt's example shape: `{ controlType: "Edit", value: "..." }`
        // (kept compact; the LLM's example fixtures use this same shape).
        s.push_str(&format!(
            "focusedField: {{ controlType: \"{}\", name: \"{}\", value: \"{}\" }}\n",
            ff.control_type, ff.name, ff.value
        ));
    } else {
        s.push_str("focusedField: null\n");
    }
    if !ctx.visible_text_fragments.is_empty() {
        // Cap the count we forward to the LLM. Wave 2 already pre-
        // truncates at the capture site, but a paranoid cap here
        // protects future captures that might be looser.
        const MAX_FRAGMENTS: usize = 12;
        let preview: Vec<String> = ctx
            .visible_text_fragments
            .iter()
            .take(MAX_FRAGMENTS)
            .map(|f| format!("{f:?}")) // debug-quoted to escape \n etc.
            .collect();
        s.push_str(&format!("visibleTextFragments: [{}]\n", preview.join(", ")));
    } else {
        s.push_str("visibleTextFragments: []\n");
    }
    if ctx.password_field_active {
        s.push_str("note: a password field was focused — content redacted.\n");
    }
    s
}

/// Decide whether to skip the LLM and render the deterministic
/// template instead. Triggers:
///
///   - The Block has zero focus events (defensive).
///   - The Block's snapshot status is `no_payload` (game / locked /
///     opaque) — ADR 0040 §Decision item 3.
///   - The Block has a `passwordFieldActive` flag (redacted content
///     must NOT be sent to the LLM).
fn should_short_circuit(ctx: &BlockContext, block: &Block) -> bool {
    if block.focus_events.is_empty() {
        return true;
    }
    if ctx.password_field_active {
        return true;
    }
    !ctx.has_real_payload
}

/// Deterministic template — used when we skip the LLM. The output
/// matches the prompt's tone ("The user X in Y") so a user reading a
/// mixed-LLM-and-template summary doesn't see a tonal seam.
fn template_abstract(ctx: &BlockContext) -> BlockAbstract {
    let text = if ctx.title.trim().is_empty() {
        format!("The user spent {} in {}.", ctx.duration_human, ctx.app)
    } else {
        format!(
            "The user spent {} in {}: {}.",
            ctx.duration_human, ctx.app, ctx.title
        )
    };
    BlockAbstract {
        text,
        prompt_version_sha: TEMPLATE_NO_PAYLOAD_SHA.to_string(),
        used_llm: false,
    }
}

/// Tidy the LLM's response: strip leading/trailing whitespace, drop
/// surrounding quote marks if the model insists on them, collapse
/// internal whitespace, and cap at a sensible length (the prompt asks
/// for one sentence but models occasionally barf paragraphs).
fn clean_response(raw: &str) -> String {
    let trimmed = raw.trim();
    let stripped = trimmed
        .trim_start_matches(['"', '\'', '`'])
        .trim_end_matches(['"', '\'', '`'])
        .trim();
    // Drop "Summary:" / "Output:" prefixes if the model adds them.
    let stripped = stripped
        .strip_prefix("Summary:")
        .or_else(|| stripped.strip_prefix("summary:"))
        .or_else(|| stripped.strip_prefix("Output:"))
        .unwrap_or(stripped)
        .trim();
    // Cap to first paragraph (the prompt requires a single sentence,
    // but if the model emits multiple, take the first).
    let first_para = stripped.split("\n\n").next().unwrap_or(stripped);
    first_para.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppError;

    /// Test double for `CleanupProvider` — returns a fixed response or
    /// error. Lets us prove the abstractor's pipeline without needing
    /// Ollama running.
    struct StubProvider {
        response: Result<String, String>,
    }

    impl CleanupProvider for StubProvider {
        fn cleanup(
            &self,
            _req: CleanupRequest<'_>,
        ) -> AppResult<crate::cleanup::provider::CleanupResult> {
            match &self.response {
                Ok(text) => Ok(crate::cleanup::provider::CleanupResult {
                    text: text.clone(),
                    model_used: "stub".into(),
                    latency_ms: 1,
                    input_tokens: None,
                    output_tokens: None,
                }),
                Err(msg) => Err(AppError::Cleanup(msg.clone())),
            }
        }
        fn provider_name(&self) -> &'static str {
            "stub"
        }
        fn supports_model(&self, _: &str) -> bool {
            true
        }
    }

    fn focus_event(app: &str, title: &str, ts: i64, snap: Option<&str>) -> NormalizedEvent {
        NormalizedEvent::AppFocus {
            event_id: format!("e@{ts}"),
            app: app.into(),
            title: title.into(),
            ts,
            snapshot_json: snap.map(str::to_string),
        }
    }

    fn block_with(events: Vec<NormalizedEvent>, app: &str, title: &str, dur_ms: i64) -> Block {
        Block {
            started_at: 0,
            ended_at: dur_ms,
            primary_app: app.into(),
            primary_title: title.into(),
            source_event_ids: vec![],
            focus_events: events,
            idle_ms_within: 0,
        }
    }

    #[test]
    fn prompt_set_sha_is_deterministic_versioned_and_short() {
        let a = current_prompt_set_sha();
        let b = current_prompt_set_sha();
        assert_eq!(a, b, "prompt_set_sha must be deterministic");
        assert!(
            a.starts_with("abstract_v1-"),
            "prompt_set_sha should carry the schema major: {a}"
        );
        assert_eq!(a.len(), "abstract_v1-".len() + 8);
    }

    #[test]
    fn empty_block_short_circuits_to_template() {
        let block = block_with(vec![], "a.exe", "T", 60_000);
        let provider = StubProvider {
            response: Ok("SHOULD NOT BE CALLED".into()),
        };
        let out = abstract_block_with_provider(&block, &provider, "fake_sha").unwrap();
        assert!(!out.used_llm);
        assert_eq!(out.prompt_version_sha, TEMPLATE_NO_PAYLOAD_SHA);
        assert!(out.text.starts_with("The user spent"));
    }

    #[test]
    fn no_payload_only_block_short_circuits_to_template() {
        let snap =
            r#"{"schema":"v2","app":"game.exe","title":"Doom","status":{"kind":"no_payload"}}"#;
        let block = block_with(
            vec![focus_event("game.exe", "Doom", 0, Some(snap))],
            "game.exe",
            "Doom",
            120_000,
        );
        let provider = StubProvider {
            response: Ok("NOPE".into()),
        };
        let out = abstract_block_with_provider(&block, &provider, "fake_sha").unwrap();
        assert!(!out.used_llm);
        assert_eq!(out.prompt_version_sha, TEMPLATE_NO_PAYLOAD_SHA);
        assert!(out.text.contains("game.exe"));
        assert!(out.text.contains("Doom"));
    }

    #[test]
    fn block_with_real_v2_payload_calls_llm() {
        let snap = r#"{"schema":"v2","app":"code.exe","title":"main.rs","status":{"kind":"ok"}}"#;
        let block = block_with(
            vec![focus_event("code.exe", "main.rs", 0, Some(snap))],
            "code.exe",
            "main.rs",
            60_000,
        );
        let provider = StubProvider {
            response: Ok("The user edited main.rs.".into()),
        };
        let out = abstract_block_with_provider(&block, &provider, "sha-abc").unwrap();
        assert!(out.used_llm);
        assert_eq!(out.prompt_version_sha, "sha-abc");
        assert_eq!(out.text, "The user edited main.rs.");
    }

    #[test]
    fn provider_error_falls_back_to_template_with_sentinel_sha() {
        let snap = r#"{"schema":"v2","app":"code.exe","title":"main.rs","status":{"kind":"ok"}}"#;
        let block = block_with(
            vec![focus_event("code.exe", "main.rs", 0, Some(snap))],
            "code.exe",
            "main.rs",
            60_000,
        );
        let provider = StubProvider {
            response: Err("ollama refused connection".into()),
        };
        let out = abstract_block_with_provider(&block, &provider, "sha-abc").unwrap();
        assert!(!out.used_llm);
        assert_eq!(out.prompt_version_sha, TEMPLATE_NO_PAYLOAD_SHA);
        assert!(out.text.contains("code.exe"));
    }

    #[test]
    fn password_field_block_short_circuits_no_matter_the_payload() {
        let snap = r#"{"schema":"v2","app":"chrome.exe","title":"Login","status":{"kind":"ok"},"passwordFieldActive":true}"#;
        let block = block_with(
            vec![focus_event("chrome.exe", "Login", 0, Some(snap))],
            "chrome.exe",
            "Login",
            30_000,
        );
        let provider = StubProvider {
            response: Ok("should not get here".into()),
        };
        let out = abstract_block_with_provider(&block, &provider, "sha-abc").unwrap();
        assert!(!out.used_llm, "password-field block must not call LLM");
    }

    #[test]
    fn clean_response_strips_quotes_and_prefixes() {
        assert_eq!(clean_response("\"hello.\""), "hello.");
        assert_eq!(clean_response("Summary: did a thing."), "did a thing.");
        assert_eq!(
            clean_response("  some   multi   spaced   text  "),
            "some multi spaced text"
        );
        assert_eq!(
            clean_response("first paragraph.\n\nsecond paragraph."),
            "first paragraph."
        );
    }

    #[test]
    fn populate_from_v2_extracts_focused_field_and_fragments() {
        let snap = r#"{"schema":"v2","app":"code.exe","title":"main.rs","monitor":{"name":"\\\\.\\DISPLAY1"},"focusedField":{"controlType":"Edit","name":"editor","value":"fn main()"},"visibleTextFragments":["hello","world"],"status":{"kind":"ok"},"passwordFieldActive":false}"#;
        let block = block_with(
            vec![focus_event("code.exe", "main.rs", 0, Some(snap))],
            "code.exe",
            "main.rs",
            60_000,
        );
        let ctx = build_context(&block);
        assert_eq!(ctx.app, "code.exe");
        assert_eq!(ctx.title, "main.rs");
        assert!(ctx.monitor_name.is_some());
        assert!(ctx.focused_field.is_some());
        assert_eq!(ctx.visible_text_fragments, vec!["hello", "world"]);
        assert!(ctx.has_real_payload);
    }

    #[test]
    fn render_user_block_includes_app_title_duration_and_fragments() {
        let ctx = BlockContext {
            app: "code.exe".into(),
            title: "main.rs".into(),
            duration_human: "5 min".into(),
            visible_text_fragments: vec!["alpha".into(), "beta".into()],
            ..Default::default()
        };
        let s = render_user_block(&ctx);
        assert!(s.contains("app: code.exe"));
        assert!(s.contains("title: main.rs"));
        assert!(s.contains("duration: 5 min"));
        assert!(s.contains("alpha"));
        assert!(s.contains("beta"));
    }
}
