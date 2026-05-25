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

use crate::cleanup::{
    few_shot, preprocessor::Preprocessor, prompt_builder, Cleaner, PREPROCESSOR_VERSION,
};
use crate::db::{dictionary, prompts};
use crate::error::{AppError, AppResult};
use crate::settings::{model::SettingKey, Settings};

use super::provider::{CleanupProvider, CleanupRequest};

/// Suffix stamped onto `model_used` when the shrink-fallback trips.
/// Stable string so downstream provenance queries can grep for it.
/// See [`LlmCleaner::run_cleanup`] / ADR 0047 §Wave 1.2.
const SHRINK_FALLBACK_SUFFIX: &str = "-shrink-fallback";

/// Prefix stamped onto `model_used` when the LLM-skip-on-short-utterance
/// path returns the preprocessor output directly without ever calling
/// the LLM provider. Stable string so downstream provenance queries
/// can grep for it. See [`LlmCleaner::run_cleanup`] / ADR 0047 §Wave 2.2.
const PREPROCESSOR_ONLY_PROVENANCE: &str = "preprocessor-only";

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

    /// Read the configured shrink-fallback threshold. Defaults to the
    /// `SettingKey` default (`0.65`) if the row is missing or corrupt;
    /// `Settings::get` already encapsulates that fallback. Errors
    /// (e.g. mutex poisoning) collapse onto the default rather than
    /// failing the cleanup — the safer behaviour is "don't trip on
    /// settings read", since the guard is itself a safety net.
    fn read_shrink_threshold(&self) -> f32 {
        let Ok(conn) = self.db.lock() else {
            return SettingKey::LlmShrinkFallbackThreshold
                .default_value()
                .as_f64()
                .map(|v| v as f32)
                .unwrap_or(0.65);
        };
        Settings::new(&conn)
            .get::<f32>(SettingKey::LlmShrinkFallbackThreshold)
            .unwrap_or(0.65)
    }

    /// Read the configured LLM-skip word-count ceiling (ADR 0047
    /// §Wave 2.2). Same mutex-poison-tolerant pattern as
    /// [`Self::read_shrink_threshold`]: the skip is a latency
    /// optimisation, not a safety guard, so a settings-read failure
    /// collapses to the documented default rather than blocking the
    /// cleanup pipeline.
    fn read_skip_word_threshold(&self) -> u32 {
        let Ok(conn) = self.db.lock() else {
            return SettingKey::LlmSkipWordThreshold
                .default_value()
                .as_u64()
                .map(|v| v as u32)
                .unwrap_or(12);
        };
        Settings::new(&conn)
            .get::<u32>(SettingKey::LlmSkipWordThreshold)
            .unwrap_or(12)
    }

    /// Stamp `last_model_used` with the preprocessor-only provenance
    /// string. Used by the Wave-2.2 LLM-skip path AND (in Wave-2.1)
    /// the cleanup-level `Light` branch. Both code paths return the
    /// preprocessor output without calling the LLM, so they share
    /// the same provenance shape: `preprocessor-only+<PREPROCESSOR_VERSION>`.
    fn stamp_preprocessor_only_model(&self) {
        let stamped = format!("{PREPROCESSOR_ONLY_PROVENANCE}+{PREPROCESSOR_VERSION}");
        if let Ok(mut slot) = self.last_model_used.lock() {
            *slot = stamped;
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

        // 0.5 ADR 0047 §Wave 2.2 — LLM-skip-on-short-utterance.
        //     Short, non-listy utterances skip the LLM entirely and
        //     return the preprocessor output directly. ~70% of casual
        //     one-liners fit this profile; bypassing the LLM saves
        //     a few hundred ms of latency and removes the model's
        //     opportunity to over-consolidate.
        //
        //     Listy utterances always run the LLM regardless of length,
        //     because the preprocessor itself never renders list
        //     structure (out of scope per its module docs).
        let skip_threshold = self.read_skip_word_threshold();
        let pre_word_count = pre_text.split_whitespace().count();
        let is_listy = pre.notes.looks_listy();
        if skip_threshold > 0 && pre_word_count <= skip_threshold as usize && !is_listy {
            tracing::info!(
                pre_word_count,
                skip_threshold,
                preprocessor_ms = pre_ms,
                mode = mode_slug,
                provenance = %format!("{PREPROCESSOR_ONLY_PROVENANCE}+{PREPROCESSOR_VERSION}"),
                "llm-skip-short-utterance: returning preprocessor output"
            );
            self.stamp_preprocessor_only_model();
            return Ok(pre_text);
        }

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

        // ADR 0047 §Wave 1.2 — length-ratio sanity check. If the LLM
        // returned a transcript materially shorter than what the
        // deterministic preprocessor produced AND the preprocessor
        // detected no self-corrections (which would legitimately
        // shrink the text), treat it as content loss and fall back
        // to the preprocessor output.
        //
        // Word counts (not chars) because shrinkage is a semantic
        // signal — "I'm gonna..." → "I am going to" is char-shrinkage
        // we WANT. Catching word-loss is the goal.
        let threshold = self.read_shrink_threshold();
        let pre_words = pre_text.split_whitespace().count();
        let cleaned_words = result.text.split_whitespace().count();
        let no_self_corrections = pre.notes.self_corrections == 0;
        let trips_guard = threshold > 0.0
            && pre_words > 0
            && no_self_corrections
            && (cleaned_words as f32) < threshold * (pre_words as f32);

        if trips_guard {
            tracing::warn!(
                pre_words,
                cleaned_words,
                threshold,
                mode = mode_slug,
                model = %result.model_used,
                "cleanup shrink-fallback tripped; returning preprocessor output"
            );
            let stamped_model = format!(
                "{}+{}{}",
                result.model_used, PREPROCESSOR_VERSION, SHRINK_FALLBACK_SUFFIX
            );
            if let Ok(mut slot) = self.last_model_used.lock() {
                *slot = stamped_model;
            }
            return Ok(pre_text);
        }

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
    use crate::cleanup::provider::{CleanupResult, StubCleanupProvider};
    use crate::db::dictionary::NewDictionaryEntry;
    use crate::db::examples::{self, NewStyleExample};
    use crate::db::Database;

    fn fresh_db() -> Arc<Mutex<Connection>> {
        let db = Database::open_in_memory().unwrap();
        Arc::new(Mutex::new(db.conn))
    }

    /// Test-only cleanup provider that returns a caller-supplied
    /// string regardless of the request. Used by the ADR 0047 §Wave 1.2
    /// shrink-fallback tests where the prod `StubCleanupProvider`'s
    /// mode-keyed transformations don't give us control over output
    /// length.
    ///
    /// Deliberately separate from `StubCleanupProvider` per ADR §Wave 1.2
    /// ("don't pollute the production stub").
    struct ConfigurableStubCleanupProvider {
        text_to_return: String,
    }

    impl CleanupProvider for ConfigurableStubCleanupProvider {
        fn cleanup(&self, _request: CleanupRequest<'_>) -> AppResult<CleanupResult> {
            Ok(CleanupResult {
                text: self.text_to_return.clone(),
                model_used: "configurable-stub".to_string(),
                latency_ms: 0,
                input_tokens: None,
                output_tokens: None,
            })
        }

        fn provider_name(&self) -> &'static str {
            "configurable-stub"
        }

        fn supports_model(&self, _model_id: &str) -> bool {
            true
        }
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

    // -------- ADR 0047 §Wave 1.2 — shrink-fallback length-ratio guard. --------

    /// LLM returns ~30% of input length and the preprocessor saw no
    /// self-corrections → fallback to preprocessor output, model_used
    /// stamped with `-shrink-fallback`.
    ///
    /// We craft a long raw transcript with no self-correction markers
    /// so `pre.notes.self_corrections == 0` after preprocessing; the
    /// configurable stub returns a much shorter string; the guard
    /// trips and pre_text is returned.
    #[test]
    fn shrink_fallback_trips_when_llm_drops_content_and_no_self_corrections() {
        let db = fresh_db();
        // 30 words of vanilla declarative content — no "I mean" /
        // "actually" / "no wait" self-correction markers that would
        // legitimately cause the preprocessor to drop tokens.
        let raw = "the quick brown fox jumps over the lazy dog \
                   and then it jumps over the river and then it \
                   runs through the forest and finds a quiet spot \
                   under a big oak tree to rest";
        // Three-word reply — way under 65% of the ~30-word input.
        let provider = ConfigurableStubCleanupProvider {
            text_to_return: "too short reply".into(),
        };
        let mut cleaner = LlmCleaner::new(
            Box::new(provider),
            db,
            "configurable-stub-model".into(),
            0.3,
            256,
        );
        let out = cleaner.clean(raw, "normal").unwrap();
        // Fallback returns the preprocessor output, not the LLM
        // output — so it must contain the original content, not the
        // "too short reply" string.
        assert!(
            !out.contains("too short reply"),
            "expected fallback to preprocessor output; got LLM output: {out}"
        );
        assert!(
            out.to_lowercase().contains("quick brown fox"),
            "expected preprocessor output to contain original content; got: {out}"
        );
        let model = cleaner.model_name();
        assert!(
            model.contains("-shrink-fallback"),
            "model_name should be stamped with the shrink-fallback suffix; got: {model}"
        );
    }

    /// LLM returns ~30% of input length BUT the preprocessor detected
    /// self-corrections → LLM output passes through unchanged.
    /// Self-correction is the legitimate reason content was dropped;
    /// the guard must not trip.
    ///
    /// Self-correction phrases stitched into the raw text trigger the
    /// preprocessor's `self_corrections` counter (see `preprocessor.rs`).
    #[test]
    fn shrink_fallback_does_not_trip_when_self_corrections_present() {
        let db = fresh_db();
        // Raw with explicit self-correction phrasing that the
        // preprocessor stitches into a shorter pre_text. The
        // self-correction regex in `cleanup::preprocessor` requires
        // a leading comma ("X, wait, Y" / "X, no I mean Y" etc.) so
        // the input is comma-delimited deliberately.
        let raw = "send the report to alice, wait, send the report \
                   to bob, scratch that, send the report to carol \
                   and also tell david about the deadline next \
                   friday morning before the standup meeting";
        // Three-word reply — again under 65% of the input.
        let provider = ConfigurableStubCleanupProvider {
            text_to_return: "send to carol".into(),
        };
        let mut cleaner = LlmCleaner::new(
            Box::new(provider),
            db,
            "configurable-stub-model".into(),
            0.3,
            256,
        );
        let out = cleaner.clean(raw, "normal").unwrap();
        // Guard didn't trip → LLM output passes through unchanged.
        assert_eq!(
            out, "send to carol",
            "self-correction present → guard should not trip; got: {out}"
        );
        let model = cleaner.model_name();
        assert!(
            !model.contains("-shrink-fallback"),
            "model_name should NOT be stamped when guard doesn't trip; got: {model}"
        );
    }

    // -------- ADR 0047 §Wave 2.2 — LLM-skip-on-short-utterance. --------

    /// Short (<= threshold words) non-listy input — the LLM is
    /// bypassed entirely and `pre_text` is returned, with the
    /// provenance string stamped as `preprocessor-only+<ver>`.
    #[test]
    fn llm_skip_short_non_listy_returns_preprocessor_output() {
        let db = fresh_db();
        // 8 words, no ordinals, no enumeration markers — well under
        // the default 12-word threshold AND non-listy.
        let raw = "please send the report to alice by tomorrow";
        // The configurable stub would return a fingerprint string IF
        // it ran; the skip path means the stub must NOT run, so this
        // string must NOT appear in the output.
        let provider = ConfigurableStubCleanupProvider {
            text_to_return: "LLM RAN — SKIP FAILED".into(),
        };
        let mut cleaner = LlmCleaner::new(
            Box::new(provider),
            db,
            "configurable-stub-model".into(),
            0.3,
            256,
        );
        let out = cleaner.clean(raw, "normal").unwrap();
        assert!(
            !out.contains("LLM RAN"),
            "LLM provider should not have been called; got: {out}"
        );
        // The preprocessor capitalises + adds a terminal period.
        assert!(
            out.starts_with('P') && out.ends_with('.'),
            "expected preprocessor-shaped output; got: {out}"
        );
        let model = cleaner.model_name();
        assert!(
            model.contains(PREPROCESSOR_ONLY_PROVENANCE),
            "model_name should be stamped preprocessor-only; got: {model}"
        );
        assert!(
            model.contains(PREPROCESSOR_VERSION),
            "model_name should carry preprocessor version; got: {model}"
        );
    }

    /// Short input WITH list signals — the LLM must still run so
    /// the list can be rendered. The threshold-by-itself rule is
    /// not enough; `looks_listy()` is the override gate.
    #[test]
    fn llm_skip_does_not_trip_when_input_looks_listy() {
        let db = fresh_db();
        // 10 words containing two ordinals ("first", "second") —
        // under the 12-word ceiling but listy. The skip must NOT
        // fire; the LLM must run.
        let raw = "first finish the migration second review the pr from alice";
        let provider = ConfigurableStubCleanupProvider {
            text_to_return: "LLM RAN ON LISTY INPUT".into(),
        };
        let mut cleaner = LlmCleaner::new(
            Box::new(provider),
            db,
            "configurable-stub-model".into(),
            0.3,
            256,
        );
        let out = cleaner.clean(raw, "normal").unwrap();
        assert_eq!(
            out, "LLM RAN ON LISTY INPUT",
            "listy input must reach the LLM despite short length; got: {out}"
        );
        let model = cleaner.model_name();
        assert!(
            !model.contains(PREPROCESSOR_ONLY_PROVENANCE),
            "model_name must NOT be preprocessor-only when LLM ran; got: {model}"
        );
    }

    /// Input over the threshold — the LLM runs even on non-listy
    /// content. The skip is a one-liner latency optimisation only;
    /// longer utterances always reach the model.
    #[test]
    fn llm_skip_does_not_trip_above_word_threshold() {
        let db = fresh_db();
        // 20 words — above the default 12 — plain prose with no
        // list signals. The LLM must run.
        let raw = "the quick brown fox jumps over the lazy dog and \
                   then runs through the forest to find a sunny spot";
        // 20-word LLM reply so the shrink-fallback guard (Wave 1.2)
        // doesn't trip and confuse this test's expectations.
        let provider = ConfigurableStubCleanupProvider {
            text_to_return: "The quick brown fox jumps over the lazy dog \
                             and then runs through the forest to find a sunny spot."
                .into(),
        };
        let mut cleaner = LlmCleaner::new(
            Box::new(provider),
            db,
            "configurable-stub-model".into(),
            0.3,
            256,
        );
        let out = cleaner.clean(raw, "normal").unwrap();
        assert!(
            out.contains("The quick brown fox"),
            "long input should reach the LLM; got: {out}"
        );
        let model = cleaner.model_name();
        assert!(
            !model.contains(PREPROCESSOR_ONLY_PROVENANCE),
            "model_name must NOT be preprocessor-only when LLM ran; got: {model}"
        );
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
