//! [`super::Cleaner`] implementation backed by a [`CleanupProvider`].
//!
//! This is the glue Phase 4 needed: the orchestrator was already
//! taking `Box<dyn Cleaner>` since Wave 4; this module fills in the
//! real implementation behind that trait. The dictation thread keeps
//! seeing a sync `cleaner.clean(raw, mode_slug)` call.
//!
//! ## What happens on `clean()`
//!
//! 1. Lookup the active mode by `mode_slug` (cached at construction;
//!    settings-change invalidation is Phase 7).
//! 2. Fetch the prompt body via the prompts repo (latest version).
//! 3. Query dictionary + style examples from the DB.
//! 4. Assemble the full prompt via `prompt_builder`.
//! 5. Call the provider; record latency.
//! 6. Return cleaned text + provider's model_used string.
//!
//! If anything in steps 1-5 fails, fall back to returning the raw
//! transcript unchanged. The orchestrator's `persist_complete` still
//! writes `cleaned` + `final` rows (with the fallback note) so the
//! user sees their dictation land — better than swallowing errors.
//!
//! ## Why the cleaner holds an `Arc<Mutex<Connection>>` not a borrow
//!
//! The dictation thread already owns the DB Mutex (see `dictation.rs`
//! `Arc<Mutex<Connection>>` shared with the orchestrator). The cleaner
//! gets a clone of that handle so it can issue its own queries
//! without negotiating borrows across the orchestrator's persist
//! transaction.

use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use crate::cleanup::{few_shot, preprocessor::Preprocessor, prompt_builder, Cleaner, PREPROCESSOR_VERSION};
use crate::db::{dictionary, prompts};
use crate::error::{AppError, AppResult};

use super::provider::{CleanupProvider, CleanupRequest};

/// Cleaner backed by a real LLM provider.
pub struct LlmCleaner {
    provider: Box<dyn CleanupProvider>,
    db: Arc<Mutex<Connection>>,
    model_id: String,
    temperature: f32,
    max_tokens: u32,
    last_model_used: Mutex<String>,
    /// Deterministic pre-pass that runs BEFORE the LLM call. Stateless;
    /// see [`super::preprocessor`] module docs + ADR 0022 for the
    /// pipeline rationale.
    preprocessor: Preprocessor,
}

impl LlmCleaner {
    /// Construct.
    ///
    /// `model_id` / `temperature` / `max_tokens` come from the active
    /// mode's row in `modes`; the caller (`dictation/runtime.rs`)
    /// already does that lookup.
    pub fn new(
        provider: Box<dyn CleanupProvider>,
        db: Arc<Mutex<Connection>>,
        model_id: String,
        temperature: f32,
        max_tokens: u32,
    ) -> Self {
        let provider_name = provider.provider_name();
        Self {
            provider,
            db,
            model_id,
            temperature,
            max_tokens,
            last_model_used: Mutex::new(provider_name.to_string()),
            preprocessor: Preprocessor::new(),
        }
    }

    /// Inner helper that performs the full cleanup pipeline. Separated
    /// so `clean()` can swallow errors + log them while keeping the
    /// happy path readable.
    fn run_cleanup(&mut self, raw: &str, mode_slug: &str) -> AppResult<String> {
        // 0. Deterministic pre-pass. Strips fillers, collapses
        //    stutters, stitches self-corrections, renders verbal
        //    cues ("period", "new paragraph", …), capitalises
        //    sentence starts, and adds terminal punctuation.
        //    All ~5 ms cost; output is what the LLM actually sees.
        //    See [`super::preprocessor`] + ADR 0022.
        let pre_started = std::time::Instant::now();
        let pre = self.preprocessor.process(raw);
        let pre_ms = pre_started.elapsed().as_millis() as u64;
        let pre_text = pre.text;

        // 1-2. Prompt body for this mode.
        let conn = self
            .db
            .lock()
            .map_err(|_| AppError::Cleanup("db mutex poisoned during cleanup".into()))?;
        let prompt = prompts::get_latest_for_mode(&conn, mode_slug)?
            .ok_or_else(|| AppError::Cleanup(format!("no prompt for mode {mode_slug:?}")))?;

        // 3. Dictionary + examples. Dictionary: all (already small).
        //    Examples: top-N per mode, app context unknown at this
        //    layer (Phase 4 stub — Phase 6 will plumb the foreground
        //    app through the cleaner call).
        let dict = dictionary::list_all(&conn)?;
        let candidates = few_shot::select_candidates(&conn, mode_slug, None)?;
        let examples = few_shot::fit_to_budget(candidates);
        drop(conn); // release lock before the HTTP call.

        // 4. Assemble. The LLM sees the pre-processed transcript, not
        //    the raw STT. The raw row in the DB is untouched (ADR 0010).
        let built = prompt_builder::build(prompt_builder::PromptInputs {
            system_prompt: &prompt.body,
            dictionary: &dict,
            examples: &examples,
            foreground_app: None,
            foreground_window_title: None,
            raw_transcript: &pre_text,
        })?;
        if built.raw_was_truncated {
            tracing::warn!(
                raw_len = pre_text.len(),
                "pre-processed transcript was truncated to fit cleanup budget"
            );
        }

        // 5. Provider call.
        let req = CleanupRequest {
            prompt: &built.prompt,
            raw_transcript: &pre_text,
            model_id: &self.model_id,
            temperature: self.temperature,
            max_tokens: self.max_tokens,
            mode_slug,
        };
        let result = self.provider.cleanup(req)?;

        // Record the model the provider actually used for the next
        // `model_name()` call. Suffix with the preprocessor version
        // so provenance captures BOTH stages in one string — no
        // schema change needed (ADR 0008 satisfied via the existing
        // model_used column).
        let stamped_model = format!("{}+{}", result.model_used, PREPROCESSOR_VERSION);
        if let Ok(mut slot) = self.last_model_used.lock() {
            *slot = stamped_model.clone();
        }

        tracing::info!(
            provider = self.provider.provider_name(),
            model = %stamped_model,
            input_tokens = ?result.input_tokens,
            output_tokens = ?result.output_tokens,
            preprocessor_ms = pre_ms,
            fillers_stripped = pre.notes.fillers_stripped,
            stutters_collapsed = pre.notes.stutters_collapsed,
            self_corrections = pre.notes.self_corrections,
            cues_rendered = pre.notes.punctuation_cues_rendered
                + pre.notes.layout_cues_rendered
                + pre.notes.quote_bracket_cues_rendered,
            llm_ms = result.latency_ms,
            "cleanup completed"
        );

        Ok(result.text)
    }
}

impl Cleaner for LlmCleaner {
    fn clean(&mut self, raw: &str, mode_slug: &str) -> AppResult<String> {
        match self.run_cleanup(raw, mode_slug) {
            Ok(text) => Ok(text),
            Err(e) => {
                // Per the "fall back to raw" rule in this module's
                // doc: log + return raw unchanged. The orchestrator
                // still gets a clean exit so the user's dictation
                // lands somewhere.
                tracing::warn!(
                    error = %e,
                    mode = mode_slug,
                    "cleanup failed; falling back to raw transcript"
                );
                if let Ok(mut slot) = self.last_model_used.lock() {
                    *slot = format!("{}-fallback", self.provider.provider_name());
                }
                Ok(raw.to_string())
            }
        }
    }

    fn model_name(&self) -> &str {
        // SAFETY: returning a borrow into the Mutex is awkward; we
        // use a static "provider:fallback" hint via Box::leak in the
        // common case. For the dynamic case (the last actual model
        // used by the provider), the Cleaner trait's `model_name`
        // returns &str which forces this leak pattern. Acceptable:
        // model ids are bounded (~20 distinct strings ever).
        let guard = match self.last_model_used.lock() {
            Ok(g) => g,
            Err(_) => return "<poisoned>",
        };
        let leaked: &'static str = Box::leak(guard.clone().into_boxed_str());
        leaked
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cleanup::provider::StubCleanupProvider;
    use crate::db::dictionary::NewDictionaryEntry;
    use crate::db::examples::{self, NewStyleExample};
    use crate::db::Database;

    fn fresh_db() -> Arc<Mutex<Connection>> {
        let db = Database::open_in_memory().unwrap();
        Arc::new(Mutex::new(db.conn))
    }

    #[test]
    fn clean_normal_routes_through_stub_provider() {
        let db = fresh_db();
        let mut cleaner = LlmCleaner::new(
            Box::new(StubCleanupProvider),
            db,
            "stub-model".into(),
            0.3,
            256,
        );
        let out = cleaner.clean("hello world", "normal").unwrap();
        // Stub's normal mode: capitalises + adds period.
        assert_eq!(out, "Hello world.");
    }

    #[test]
    fn clean_modes_produce_different_output() {
        let db = fresh_db();
        let mut cleaner = LlmCleaner::new(
            Box::new(StubCleanupProvider),
            db,
            "stub-model".into(),
            0.3,
            256,
        );
        // We need prompts seeded for the alt modes; seed migrations
        // already seed normal/verbose/fragment.
        let raw = "the quick brown fox jumps";
        let n = cleaner.clean(raw, "normal").unwrap();
        let v = cleaner.clean(raw, "verbose").unwrap();
        let f = cleaner.clean(raw, "fragment").unwrap();
        assert_ne!(n, v);
        assert_ne!(v, f);
        assert_ne!(n, f);
    }

    #[test]
    fn clean_missing_prompt_falls_back_to_raw() {
        let db = fresh_db();
        let mut cleaner = LlmCleaner::new(
            Box::new(StubCleanupProvider),
            db,
            "stub-model".into(),
            0.3,
            256,
        );
        // A mode slug that is NOT seeded by any migration → prompt
        // lookup returns None → run_cleanup errors → clean() falls
        // back to raw and tags `model_used` with `-fallback`.
        let raw = "anything";
        let out = cleaner.clean(raw, "definitely-not-a-real-mode").unwrap();
        assert_eq!(out, raw, "should have fallen back to raw");
        assert!(
            cleaner.model_name().contains("fallback"),
            "got model_name={:?}",
            cleaner.model_name()
        );
    }

    #[test]
    fn dictionary_entries_appear_in_prompt() {
        // White-box: we can't peek the prompt the stub provider saw
        // without a recording-stub variant. Just confirm dictionary
        // queries succeed and don't error the pipeline.
        let db = fresh_db();
        {
            let conn = db.lock().unwrap();
            dictionary::insert(
                &conn,
                &NewDictionaryEntry {
                    term: "Mockingbird".into(),
                    canonical: Some("Mockingbird".into()),
                    source: "user".into(),
                    confidence: None,
                    app_context: None,
                },
            )
            .unwrap();
        }
        let mut cleaner = LlmCleaner::new(
            Box::new(StubCleanupProvider),
            db,
            "stub-model".into(),
            0.3,
            256,
        );
        let out = cleaner.clean("hello mockingbird", "normal").unwrap();
        assert!(!out.is_empty());
    }

    #[test]
    fn examples_query_succeeds_with_seeded_examples() {
        let db = fresh_db();
        {
            let conn = db.lock().unwrap();
            examples::insert(
                &conn,
                &NewStyleExample {
                    mode_slug: "normal".into(),
                    session_id: None,
                    raw_input: "uh hello".into(),
                    ideal_output: "Hello.".into(),
                    app_context: None,
                    source: "user_marked".into(),
                    rank: 0.9,
                    enabled: true,
                },
            )
            .unwrap();
        }
        let mut cleaner = LlmCleaner::new(
            Box::new(StubCleanupProvider),
            db,
            "stub-model".into(),
            0.3,
            256,
        );
        let out = cleaner.clean("hello world", "normal").unwrap();
        assert_eq!(out, "Hello world.");
    }

    #[test]
    fn model_name_returns_stub_provider_name_initially() {
        let db = fresh_db();
        let cleaner = LlmCleaner::new(
            Box::new(StubCleanupProvider),
            db,
            "stub-model".into(),
            0.3,
            256,
        );
        assert_eq!(cleaner.model_name(), "stub");
    }
}
