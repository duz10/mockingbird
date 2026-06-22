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
    few_shot, preprocessor::Preprocessor, prompt_builder, Cleaner, DictationCleanupLevel,
    ADDITIVE_PROMPT_MODE_SLUG, PREPROCESSOR_VERSION,
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

/// Provenance stamped onto `model_used` for the `DictationCleanupLevel::None`
/// branch — raw STT, no preprocessor, no LLM. See ADR 0047 §Wave 2.1.
const RAW_PASSTHROUGH_PROVENANCE: &str = "raw-passthrough";

/// Infix stamped onto `model_used` for the `DictationCleanupLevel::Medium`
/// branch so provenance distinguishes additive-prompt LLM passes from
/// the High-level mode-specific passes. See ADR 0047 §Wave 2.1.
const ADDITIVE_INFIX: &str = "+additive";

/// Q4_K_M quantisation suffix on Ollama model IDs (e.g.
/// `qwen2.5:7b-instruct-q4_K_M`). Used by [`maybe_promote_to_q5`] to
/// detect substitutable Q4 IDs.
const Q4_K_M_SUFFIX: &str = "-q4_K_M";

/// Q5_K_M quantisation suffix -- the substitute for Q4_K_M when the
/// user has opted in via `SettingKey::PreferQ5Models` (ADR 0047
/// §Wave 2.4).
const Q5_K_M_SUFFIX: &str = "-q5_K_M";

/// Provenance suffix appended to `model_used` when the Q5 opt-in
/// substitution actually fired on this call. Lets downstream queries
/// distinguish an opt-in-promoted call from one that was Q5-by-default
/// in some future migration.
const Q5_OPT_IN_SUFFIX: &str = "+q5-opt-in";

/// If `prefer_q5` is true AND `model_id` ends in `-q4_K_M`, return
/// the Q5 substitution. Otherwise return `model_id` unchanged.
///
/// ADR 0047 §Wave 2.4 — runtime gate for the Q5 opt-in. Done as a
/// pure string substitution (rather than a model-table lookup) so
/// the opt-in is reversible by toggling the setting; no schema
/// rollback needed.
///
/// If the substituted model isn't actually pulled in Ollama, the
/// OllamaProvider's existing error path catches the failure and the
/// `Cleaner::clean` outer match falls back to raw. The cost-of-mistake
/// is one bad cleanup that falls back to raw -- no worse than the
/// existing failure mode for any missing model.
fn maybe_promote_to_q5(model_id: &str, prefer_q5: bool) -> String {
    if prefer_q5 && model_id.ends_with(Q4_K_M_SUFFIX) {
        let stem = &model_id[..model_id.len() - Q4_K_M_SUFFIX.len()];
        format!("{stem}{Q5_K_M_SUFFIX}")
    } else {
        model_id.to_string()
    }
}

/// ASCII-case-insensitive substring search returning the byte offset of
/// the first match. Unlike `haystack.to_lowercase().find(..)` this
/// preserves byte offsets into the ORIGINAL `haystack` (lowercasing can
/// change byte lengths for some Unicode), so the offset is always safe
/// to slice/`replace_range`. The needle is matched ASCII-case-folded;
/// non-ASCII bytes must match exactly. Sufficient for our sentinels,
/// which are pure ASCII.
fn find_ascii_ci(haystack: &str, needle: &str) -> Option<usize> {
    let (hb, nb) = (haystack.as_bytes(), needle.as_bytes());
    if nb.is_empty() || nb.len() > hb.len() {
        return None;
    }
    (0..=hb.len() - nb.len()).find(|&i| hb[i..i + nb.len()].eq_ignore_ascii_case(nb))
}

/// Punctuation-insensitive, case-insensitive containment test: does
/// `haystack` contain `needle` ignoring ASCII punctuation + case +
/// whitespace runs? Used to decide whether a sentinel was actually
/// DICTATED (and is therefore content, not a leak): raw STT is lower-
/// case and lacks the terminal punctuation / colon that the baked-in
/// example carries, so a strict match would wrongly flag legitimately
/// dictated content as a leak.
fn loosely_contains(haystack: &str, needle: &str) -> bool {
    fn norm(s: &str) -> String {
        s.chars()
            .map(|c| {
                if c.is_ascii_punctuation() {
                    ' '
                } else {
                    c.to_ascii_lowercase()
                }
            })
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }
    norm(haystack).contains(&norm(needle))
}

/// Collapse runs of blank lines left behind after excising a leaked
/// example, and trim leading/trailing whitespace. Intra-line spacing is
/// left untouched (we never want to mangle a code block on the rare
/// guard path).
fn collapse_blank_lines(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_blank = false;
    for line in s.lines() {
        let trimmed_end = line.trim_end();
        if trimmed_end.is_empty() {
            if !out.is_empty() && !prev_blank {
                out.push('\n');
            }
            prev_blank = true;
        } else {
            out.push_str(trimmed_end);
            out.push('\n');
            prev_blank = false;
        }
    }
    out.trim().to_string()
}

/// Small-model example-leak guard (ADR 0065). Strip any baked-in
/// example `sentinel` that leaked into `cleaned` but was NOT present in
/// `raw_input` (case-insensitive). Returns the (possibly shortened)
/// text and a bool that is `true` iff at least one sentinel was
/// removed.
///
/// Only ever removes text the model could not have legitimately
/// produced from its input — a sentinel that also appears in the input
/// is real user content and is left alone. This is the symmetric twin
/// of the shrink-fallback guard (which catches DROPPED content). The
/// caller scopes it to the small-model override path so the 7B /
/// Windows path runs none of this logic.
fn strip_leaked_examples(cleaned: &str, raw_input: &str, sentinels: &[&str]) -> (String, bool) {
    let mut out = cleaned.to_string();
    let mut stripped = false;
    for s in sentinels {
        // If the speaker actually dictated the sentinel, it is content,
        // not a leak — leave it. Punctuation-insensitive because raw
        // STT lacks the example's colon / terminal period.
        if loosely_contains(raw_input, s) {
            continue;
        }
        while let Some(pos) = find_ascii_ci(&out, s) {
            out.replace_range(pos..pos + s.len(), "");
            stripped = true;
        }
    }
    if stripped {
        out = collapse_blank_lines(&out);
    }
    (out, stripped)
}

/// Distinctive example sentences baked into the `normal_small` prompt
/// (ADR 0065). On weak (≈3B) local models the few-shot examples can
/// leak verbatim into the output (in-context example bleed). These
/// strings are the canaries: if one appears in the cleaned output but
/// was NOT in the raw input, it is a leaked example, not user content.
///
/// They are compile-time constants kept in lockstep with the example
/// block of `src-tauri/src/cleanup/prompts/normal_small.md`. Edit the
/// prompt's examples → update these (the unit tests assert the guard
/// behaviour, not the exact text, so they won't catch drift on their
/// own — keep them paired).
const NORMAL_SMALL_EXAMPLE_SENTINELS: &[&str] = &[
    "Example input number three is a run-on sentence.",
    "It shows two independent clauses that should be split apart.",
    "Here's my list of keyboard supplies:",
];

/// Provenance suffix stamped onto `model_used` when the small-model
/// example-leak guard stripped (or fell back over) a leaked example
/// sentence. Symmetric twin of [`SHRINK_FALLBACK_SUFFIX`]: that guard
/// catches DROPPED content, this one catches ADDED content. ADR 0065.
const EXAMPLE_LEAK_GUARD_SUFFIX: &str = "-example-leak-guard";

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
    /// Optional prompt-slug override (ADR 0065). When `Some(slug)`, the
    /// High-level cleanup path resolves its prompt from `slug` instead
    /// of the tone `mode_slug` (e.g. `normal_small` instead of
    /// `normal`). Set ONLY at the macOS RAM-aware downsize seam in
    /// `dictation/runtime_cleaner.rs`; `None` everywhere else, so the
    /// 7B / Windows path is byte-identical and `normal@v5` is never
    /// re-evaluated. Also gates the example-leak guard below — that
    /// new logic runs ONLY when this is `Some`.
    prompt_mode_override: Option<String>,
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
            prompt_mode_override: None,
        }
    }

    /// Builder-style setter for the prompt-slug override (ADR 0065).
    ///
    /// Consumes + returns `self` so the call site reads as one
    /// expression: `LlmCleaner::new(..).with_prompt_mode_override(o)`.
    /// Passing `None` is an explicit no-op that leaves the default
    /// tone-mode behaviour intact — that is exactly what the non-macOS
    /// build does, keeping the Windows path byte-identical.
    pub fn with_prompt_mode_override(mut self, override_slug: Option<String>) -> Self {
        self.prompt_mode_override = override_slug;
        self
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

    /// Read the configured dictation cleanup level (ADR 0047 §Wave 2.1).
    /// Same mutex-poison + missing-row tolerance as the other readers:
    /// failures collapse to `High` (the default), which preserves
    /// existing behaviour rather than silently downgrading the user's
    /// pipeline.
    fn read_cleanup_level(&self) -> DictationCleanupLevel {
        let Ok(conn) = self.db.lock() else {
            return DictationCleanupLevel::default();
        };
        Settings::new(&conn)
            .get::<DictationCleanupLevel>(SettingKey::DictationCleanupLevel)
            .unwrap_or_default()
    }

    /// Read the Q5 opt-in flag (ADR 0047 §Wave 2.4). Mutex-poison +
    /// missing-row tolerant: failures collapse to `false` (the default),
    /// which keeps the pipeline on Q4 -- the conservative answer.
    fn read_prefer_q5(&self) -> bool {
        let Ok(conn) = self.db.lock() else {
            return false;
        };
        Settings::new(&conn)
            .get::<bool>(SettingKey::PreferQ5Models)
            .unwrap_or(false)
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
        // -1. Cleanup-level dial (ADR 0047 §Wave 2.1). Branches:
        //     - None   -> raw STT passthrough (skip preprocessor + LLM)
        //     - Light  -> preprocessor only (skip LLM regardless)
        //     - Medium -> preprocessor + LLM with additive prompt
        //     - High   -> preprocessor + LLM with mode prompt (default)
        //     The skip-short-utterance check at step 0.5 still applies
        //     to Medium + High; at None / Light there's no LLM to skip.
        let level = self.read_cleanup_level();
        if matches!(level, DictationCleanupLevel::None) {
            tracing::info!(
                mode = mode_slug,
                level = "none",
                provenance = %RAW_PASSTHROUGH_PROVENANCE,
                "cleanup level=None: returning raw STT unchanged"
            );
            if let Ok(mut slot) = self.last_model_used.lock() {
                *slot = RAW_PASSTHROUGH_PROVENANCE.to_string();
            }
            return Ok(raw.to_string());
        }

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

        if matches!(level, DictationCleanupLevel::Light) {
            tracing::info!(
                mode = mode_slug,
                level = "light",
                preprocessor_ms = pre_ms,
                fillers_stripped = pre.notes.fillers_stripped,
                stutters_collapsed = pre.notes.stutters_collapsed,
                provenance = %format!("{PREPROCESSOR_ONLY_PROVENANCE}+{PREPROCESSOR_VERSION}"),
                "cleanup level=Light: returning preprocessor output"
            );
            self.stamp_preprocessor_only_model();
            return Ok(pre_text);
        }

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

        // 1-2. Prompt body. At level=Medium we override the tone-mode
        //      lookup and use the additive prompt regardless of
        //      mode_slug (the level dial is orthogonal to tone). At
        //      level=High we use the tone-mode prompt as before.
        let conn = self
            .db
            .lock()
            .map_err(|_| AppError::Cleanup("db mutex poisoned during cleanup".into()))?;
        // ADR 0065 — small-model prompt override. Precedence:
        //   Medium level   -> additive prompt (level dial wins; it is
        //                      orthogonal to tone and already ignores
        //                      mode_slug).
        //   override set    -> the override slug (e.g. `normal_small`),
        //                      set ONLY at the macOS downsize seam.
        //   otherwise       -> the tone mode_slug, exactly as before.
        // On the 7B / Windows path the override is always `None`, so
        // this collapses to the pre-ADR-0065 `mode_slug` behaviour.
        let prompt_slug = if matches!(level, DictationCleanupLevel::Medium) {
            ADDITIVE_PROMPT_MODE_SLUG
        } else {
            self.prompt_mode_override.as_deref().unwrap_or(mode_slug)
        };
        let prompt = prompts::get_latest_for_mode(&conn, prompt_slug)?
            .ok_or_else(|| AppError::Cleanup(format!("no prompt for mode {prompt_slug:?}")))?;

        // 3. Dictionary + examples. Dictionary: all (already small).
        //    Examples: top-N per mode, app context unknown at this
        //    layer (Phase 4 stub — Phase 6 will plumb the foreground
        //    app through the cleaner call). At level=Medium we key
        //    few-shot on the additive slug too — tone-mode style
        //    examples encourage register-shifting / consolidating
        //    output, which would directly contradict the additive
        //    prompt's "preserve every word" rule. With no seeded
        //    examples for `normal_additive`, the prompt's three
        //    baked-in examples carry the few-shot load.
        let dict = dictionary::list_all(&conn)?;
        let candidates = few_shot::select_candidates(&conn, prompt_slug, None)?;
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
        //
        // ADR 0047 §Wave 2.4 — substitute Q5_K_M for Q4_K_M when the
        // user has opted in via SettingKey::PreferQ5Models. Done at
        // request time (not at construction) so toggling the setting
        // takes effect on the next dictation without restart. The
        // substitution is purely string-suffix-based so it's generic
        // across casual / normal / formal (all three share the
        // qwen2.5:Nb-instruct-q4_K_M shape since migrations 008/021).
        let prefer_q5 = self.read_prefer_q5();
        let resolved_model_id = maybe_promote_to_q5(&self.model_id, prefer_q5);
        let q5_opt_in_fired = resolved_model_id != self.model_id;
        let req = CleanupRequest {
            prompt: &built.prompt,
            raw_transcript: &pre_text,
            model_id: &resolved_model_id,
            temperature: self.temperature,
            max_tokens: self.max_tokens,
            mode_slug,
        };
        let mut result = self.provider.cleanup(req)?;

        // ADR 0065 — small-model example-leak guard. Symmetric twin of
        // the shrink-fallback below: that guard catches DROPPED content,
        // this one catches ADDED content (a baked-in few-shot example
        // sentence the weak 3B copied verbatim into its output). Scoped
        // to the small-model override path via `prompt_mode_override`,
        // so the 7B / Windows path runs ZERO new logic and stays
        // byte-identical.
        let mut example_leak_stripped = false;
        if self.prompt_mode_override.is_some() {
            let (guarded, stripped) =
                strip_leaked_examples(&result.text, raw, NORMAL_SMALL_EXAMPLE_SENTINELS);
            if stripped {
                example_leak_stripped = true;
                if guarded.trim().is_empty() {
                    // The output was ENTIRELY a leaked example — there is
                    // nothing real to inject. Fall back to the
                    // deterministic preprocessor output (never inject an
                    // empty cleanup).
                    tracing::warn!(
                        mode = mode_slug,
                        prompt_slug,
                        model = %result.model_used,
                        "example-leak guard: cleaned output was entirely a leaked \
                         example; falling back to preprocessor output"
                    );
                    let stamped = format!(
                        "{}+{}{EXAMPLE_LEAK_GUARD_SUFFIX}",
                        result.model_used, PREPROCESSOR_VERSION
                    );
                    if let Ok(mut slot) = self.last_model_used.lock() {
                        *slot = stamped;
                    }
                    return Ok(pre_text);
                }
                tracing::warn!(
                    mode = mode_slug,
                    prompt_slug,
                    model = %result.model_used,
                    "example-leak guard: stripped a leaked few-shot example from output"
                );
                result.text = guarded;
            }
        }

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

        // Q5-opt-in provenance suffix (ADR 0047 §Wave 2.4). Append
        // whenever the substitution fired on this call so the
        // `transcripts.model_used` column tells the truth about WHICH
        // quantisation actually ran -- not just what the modes table
        // configured. Useful for grep-style provenance forensics and
        // for the `edit_free_send` metric (Wave 2.5) to slice opt-in
        // vs base-Q4 cleanup quality once both populations exist.
        let q5_suffix = if q5_opt_in_fired {
            Q5_OPT_IN_SUFFIX
        } else {
            ""
        };

        if trips_guard {
            tracing::warn!(
                pre_words,
                cleaned_words,
                threshold,
                mode = mode_slug,
                model = %result.model_used,
                q5_opt_in = q5_opt_in_fired,
                "cleanup shrink-fallback tripped; returning preprocessor output"
            );
            let stamped_model = format!(
                "{}+{}{SHRINK_FALLBACK_SUFFIX}{q5_suffix}",
                result.model_used, PREPROCESSOR_VERSION
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
        // model_used column). At level=Medium also stamp the additive
        // infix so the dial choice is recoverable from `model_used`
        // alone. At Q5 opt-in, append the q5-opt-in suffix likewise.
        let mut stamped_model = if matches!(level, DictationCleanupLevel::Medium) {
            format!(
                "{}{ADDITIVE_INFIX}+{}{q5_suffix}",
                result.model_used, PREPROCESSOR_VERSION
            )
        } else {
            format!("{}+{}{q5_suffix}", result.model_used, PREPROCESSOR_VERSION)
        };
        // ADR 0065 — record when the example-leak guard stripped (but
        // did not fully suppress) a leaked example, so provenance is
        // recoverable from `model_used` alone.
        if example_leak_stripped {
            stamped_model.push_str(EXAMPLE_LEAK_GUARD_SUFFIX);
        }
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
            q5_opt_in = q5_opt_in_fired,
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

    // ── ADR 0065 small-model example-leak guard ──────────────────────

    #[test]
    fn find_ascii_ci_matches_case_insensitively_with_real_offsets() {
        assert_eq!(find_ascii_ci("hello WORLD", "world"), Some(6));
        assert_eq!(find_ascii_ci("hello world", "WORLD"), Some(6));
        assert_eq!(find_ascii_ci("nope", "world"), None);
        assert_eq!(find_ascii_ci("", "x"), None);
        assert_eq!(find_ascii_ci("abc", ""), None);
    }

    #[test]
    fn guard_strips_leaked_example_not_in_input() {
        // The classic leak: 3B prepends a baked-in example sentence to
        // the real (grocery) cleanup.
        let leaked = "Example input number three is a run-on sentence.\n\n\
                      Bananas, eggs, and shampoo.";
        let raw = "i need bananas eggs shampoo";
        let (out, stripped) = strip_leaked_examples(leaked, raw, NORMAL_SMALL_EXAMPLE_SENTINELS);
        assert!(stripped, "the leaked sentinel should have been detected");
        assert!(
            !out.contains("Example input number three"),
            "sentinel must be removed; got: {out}"
        );
        assert!(
            out.contains("Bananas, eggs, and shampoo."),
            "real content must survive; got: {out}"
        );
    }

    #[test]
    fn guard_keeps_sentinel_that_was_actually_dictated() {
        // If the speaker really dictated the keyboard-supplies line, it
        // is content, not a leak — do not strip it.
        let cleaned = "Here's my list of keyboard supplies:\n\n- air duster";
        let raw = "here's my list of keyboard supplies first is air duster";
        let (out, stripped) = strip_leaked_examples(cleaned, raw, NORMAL_SMALL_EXAMPLE_SENTINELS);
        assert!(!stripped, "dictated content must not be treated as a leak");
        assert_eq!(out, cleaned);
    }

    #[test]
    fn guard_noop_when_no_sentinel_present() {
        let cleaned = "Bananas, eggs, and shampoo.";
        let (out, stripped) = strip_leaked_examples(
            cleaned,
            "bananas eggs shampoo",
            NORMAL_SMALL_EXAMPLE_SENTINELS,
        );
        assert!(!stripped);
        assert_eq!(out, cleaned);
    }

    #[test]
    fn guard_reports_fully_leaked_output_as_empty_after_strip() {
        // Output that is ONLY a leaked example collapses to empty, which
        // the caller uses to trigger the preprocessor fallback.
        let leaked = "Example input number three is a run-on sentence. \
                      It shows two independent clauses that should be split apart.";
        let (out, stripped) = strip_leaked_examples(
            leaked,
            "totally different input",
            NORMAL_SMALL_EXAMPLE_SENTINELS,
        );
        assert!(stripped);
        assert!(
            out.trim().is_empty(),
            "expected empty after strip; got: {out:?}"
        );
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
        // Isolate from the Wave 2.2 short-utterance skip (which would
        // bypass the LLM for this 5-word input, making every mode
        // return identical preprocessor output) and the Wave 1.2
        // shrink-fallback guard (which would reject `fragment`'s
        // deliberately shorter output). Both guards have dedicated
        // tests. (mb-mac-v1.9)
        disable_skip(&db);
        disable_shrink_guard(&db);
        let mut cleaner = LlmCleaner::new(
            Box::new(StubCleanupProvider),
            db,
            "stub-model".into(),
            0.3,
            256,
        );
        // Seed migrations seed normal/verbose/fragment. We do NOT
        // assert normal != verbose: once the preprocessor has
        // capitalised + punctuated this short input, the `normal`
        // (capitalise + period) and `verbose` (identity) stub
        // transforms coincide -- a stub artifact, not a routing
        // failure. `fragment` (first-half, lowercased) stays
        // distinct from both, which is what proves mode routing.
        // (mb-mac-v1.9: previously asserted normal != verbose.)
        let raw = "the quick brown fox jumps";
        let n = cleaner.clean(raw, "normal").unwrap();
        let v = cleaner.clean(raw, "verbose").unwrap();
        let f = cleaner.clean(raw, "fragment").unwrap();
        assert_ne!(n, f);
        assert_ne!(v, f);
    }

    #[test]
    fn clean_missing_prompt_falls_back_to_raw() {
        let db = fresh_db();
        // Disable the Wave 2.2 short-utterance skip: the 1-word input
        // below would otherwise short-circuit to preprocessor output
        // BEFORE the prompt lookup runs, so the missing-prompt ->
        // fallback path under test would never be exercised.
        // (mb-mac-v1.9)
        disable_skip(&db);
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
        // The 5-word fingerprint is intentionally shorter than the
        // 10-word input; disable the Wave 1.2 shrink-fallback guard
        // so it survives -- this test is about the listy override of
        // the skip, not the shrink guard. (mb-mac-v1.9)
        disable_shrink_guard(&db);
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

    // -------- ADR 0047 §Wave 2.1 — DictationCleanupLevel dial. --------

    /// Helper: write a DictationCleanupLevel into the settings table
    /// before constructing the cleaner. Mirrors what the UI / IPC
    /// would do at runtime.
    fn set_level(db: &Arc<Mutex<Connection>>, level: DictationCleanupLevel) {
        let conn = db.lock().unwrap();
        Settings::new(&conn)
            .set(SettingKey::DictationCleanupLevel, &level)
            .unwrap();
    }

    /// Disable the Wave 1.2 shrink-fallback guard so a test can assert
    /// the provider's (possibly short) output passes through verbatim.
    /// The guard itself is covered by the `shrink_fallback_*` tests.
    /// (mb-mac-v1.9)
    fn disable_shrink_guard(db: &Arc<Mutex<Connection>>) {
        let conn = db.lock().unwrap();
        Settings::new(&conn)
            .set(SettingKey::LlmShrinkFallbackThreshold, &0.0f32)
            .unwrap();
    }

    /// Disable the Wave 2.2 short-utterance LLM-skip so a test that
    /// uses a short input still routes through the provider. The skip
    /// itself is covered by the `llm_skip_*` tests. (mb-mac-v1.9)
    fn disable_skip(db: &Arc<Mutex<Connection>>) {
        let conn = db.lock().unwrap();
        Settings::new(&conn)
            .set(SettingKey::LlmSkipWordThreshold, &0u32)
            .unwrap();
    }

    /// Level = None: raw STT passthrough. Preprocessor + LLM are both
    /// bypassed; provenance is `raw-passthrough`.
    #[test]
    fn level_none_returns_raw_unchanged_and_skips_everything() {
        let db = fresh_db();
        set_level(&db, DictationCleanupLevel::None);
        // Provider returns a fingerprint so we can detect erroneous calls.
        let provider = ConfigurableStubCleanupProvider {
            text_to_return: "LLM RAN AT LEVEL NONE".into(),
        };
        let mut cleaner = LlmCleaner::new(
            Box::new(provider),
            db,
            "configurable-stub-model".into(),
            0.3,
            256,
        );
        let raw = "um  uh hello world";
        let out = cleaner.clean(raw, "normal").unwrap();
        assert_eq!(out, raw, "level=None must return raw verbatim");
        assert_eq!(cleaner.model_name(), RAW_PASSTHROUGH_PROVENANCE);
    }

    /// Level = Light: preprocessor runs; LLM bypassed regardless of
    /// word count or list shape. Provenance is `preprocessor-only+<ver>`.
    #[test]
    fn level_light_returns_preprocessor_output_and_skips_llm() {
        let db = fresh_db();
        set_level(&db, DictationCleanupLevel::Light);
        let provider = ConfigurableStubCleanupProvider {
            text_to_return: "LLM RAN AT LEVEL LIGHT".into(),
        };
        let mut cleaner = LlmCleaner::new(
            Box::new(provider),
            db,
            "configurable-stub-model".into(),
            0.3,
            256,
        );
        // 25-word input — ABOVE the 12-word skip threshold — so we
        // know Light's LLM bypass is driven by the level, not the
        // Wave-2.2 short-utterance skip.
        let raw = "this is a longer dictation that goes well past twelve words \
                   so the short utterance skip does not fire for this test case at all";
        let out = cleaner.clean(raw, "normal").unwrap();
        assert!(
            !out.contains("LLM RAN"),
            "level=Light must not call LLM; got: {out}"
        );
        assert!(
            out.starts_with('T') && out.ends_with('.'),
            "level=Light should return preprocessor-shaped output; got: {out}"
        );
        let model = cleaner.model_name();
        assert!(
            model.contains(PREPROCESSOR_ONLY_PROVENANCE),
            "level=Light provenance must be preprocessor-only; got: {model}"
        );
    }

    /// Level = Medium: preprocessor + LLM with the additive prompt
    /// (regardless of mode_slug). Provenance carries the `+additive`
    /// infix so the dial choice is recoverable.
    #[test]
    fn level_medium_uses_additive_prompt_regardless_of_tone() {
        let db = fresh_db();
        set_level(&db, DictationCleanupLevel::Medium);
        let provider = ConfigurableStubCleanupProvider {
            text_to_return: "Medium output.".into(),
        };
        // The short "Medium output." fingerprint would trip the
        // Wave 1.2 shrink-fallback guard against the long input; this
        // test is about the additive-prompt branch, not the guard.
        // (mb-mac-v1.9)
        disable_shrink_guard(&db);
        let mut cleaner = LlmCleaner::new(
            Box::new(provider),
            db,
            "configurable-stub-model".into(),
            0.3,
            256,
        );
        // Long enough to bypass the short-utterance skip; uses the
        // "casual" tone slug to prove the additive prompt overrides
        // the tone-mode lookup.
        let raw = "this is a fifteen word dictation about the additive only \
                   prompt branch for level medium today";
        let out = cleaner.clean(raw, "casual").unwrap();
        assert_eq!(
            out, "Medium output.",
            "level=Medium should pass through the configurable stub's LLM output"
        );
        let model = cleaner.model_name();
        assert!(
            model.contains(ADDITIVE_INFIX.trim_start_matches('+')),
            "level=Medium provenance must carry the additive infix; got: {model}"
        );
        assert!(
            model.contains(PREPROCESSOR_VERSION),
            "level=Medium provenance must carry preprocessor version; got: {model}"
        );
    }

    /// Level = High: existing behaviour. Provenance is
    /// `<model>+<preprocessor_version>`, no additive infix.
    #[test]
    fn level_high_preserves_existing_behaviour() {
        let db = fresh_db();
        // High is the default — don't even need to set it.
        let provider = ConfigurableStubCleanupProvider {
            text_to_return: "High output.".into(),
        };
        // The short "High output." fingerprint would trip the Wave 1.2
        // shrink-fallback guard against the long input; this test is
        // about the level=High provenance, not the guard. (mb-mac-v1.9)
        disable_shrink_guard(&db);
        let mut cleaner = LlmCleaner::new(
            Box::new(provider),
            db,
            "configurable-stub-model".into(),
            0.3,
            256,
        );
        let raw = "this is a fifteen word dictation that should reach the LLM \
                   at level high without the additive infix today";
        let out = cleaner.clean(raw, "normal").unwrap();
        assert_eq!(
            out, "High output.",
            "level=High should pass through the configurable stub's LLM output"
        );
        let model = cleaner.model_name();
        assert!(
            !model.contains(ADDITIVE_INFIX.trim_start_matches('+')),
            "level=High must NOT carry additive infix; got: {model}"
        );
        assert!(
            model.contains(PREPROCESSOR_VERSION),
            "level=High provenance must carry preprocessor version; got: {model}"
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

    // -------- ADR 0047 §Wave 2.4 — Q5_K_M opt-in substitution. --------

    /// Pure-function unit tests for `maybe_promote_to_q5`. No DB, no
    /// stub provider; just verify the suffix-substitution logic.
    #[test]
    fn maybe_promote_to_q5_substitutes_q4_suffix_when_opted_in() {
        assert_eq!(
            maybe_promote_to_q5("qwen2.5:7b-instruct-q4_K_M", true),
            "qwen2.5:7b-instruct-q5_K_M"
        );
        assert_eq!(
            maybe_promote_to_q5("qwen2.5:3b-instruct-q4_K_M", true),
            "qwen2.5:3b-instruct-q5_K_M"
        );
    }

    #[test]
    fn maybe_promote_to_q5_is_a_no_op_when_opt_out() {
        // Even when the model_id matches the substitution pattern,
        // prefer_q5=false must leave the id untouched.
        assert_eq!(
            maybe_promote_to_q5("qwen2.5:7b-instruct-q4_K_M", false),
            "qwen2.5:7b-instruct-q4_K_M"
        );
    }

    #[test]
    fn maybe_promote_to_q5_is_a_no_op_for_non_q4_model_ids() {
        // Already-Q5 stays Q5 (no double-substitution).
        assert_eq!(
            maybe_promote_to_q5("qwen2.5:7b-instruct-q5_K_M", true),
            "qwen2.5:7b-instruct-q5_K_M"
        );
        // Different quantisation (e.g. q8_0) is left alone -- the
        // substitution is suffix-specific.
        assert_eq!(
            maybe_promote_to_q5("qwen2.5:7b-instruct-q8_0", true),
            "qwen2.5:7b-instruct-q8_0"
        );
        // Cloud / non-Ollama model ids never match the suffix.
        assert_eq!(
            maybe_promote_to_q5("claude-3-5-sonnet-20241022", true),
            "claude-3-5-sonnet-20241022"
        );
    }

    /// Test-only stub provider that echoes the request's `model_id`
    /// back as `result.model_used`. Lets the Q5 opt-in tests assert
    /// what the LlmCleaner actually asked the provider for, not just
    /// what got persisted to `last_model_used`.
    struct ModelEchoingStubCleanupProvider {
        text_to_return: String,
    }

    impl CleanupProvider for ModelEchoingStubCleanupProvider {
        fn cleanup(&self, request: CleanupRequest<'_>) -> AppResult<CleanupResult> {
            Ok(CleanupResult {
                text: self.text_to_return.clone(),
                // KEY: echo the model_id the cleaner sent us, so the
                // test can verify whether the Q5 substitution fired.
                model_used: request.model_id.to_string(),
                latency_ms: 0,
                input_tokens: None,
                output_tokens: None,
            })
        }

        fn provider_name(&self) -> &'static str {
            "model-echoing-stub"
        }

        fn supports_model(&self, _model_id: &str) -> bool {
            true
        }
    }

    /// Helper: flip the PreferQ5Models setting before constructing
    /// the cleaner. Mirrors what the Settings UI / IPC would do.
    fn set_prefer_q5(db: &Arc<Mutex<Connection>>, prefer: bool) {
        let conn = db.lock().unwrap();
        Settings::new(&conn)
            .set(SettingKey::PreferQ5Models, &prefer)
            .unwrap();
    }

    /// PreferQ5Models=false (default) → model_id passes through to the
    /// provider unchanged AND the `+q5-opt-in` provenance suffix is
    /// absent. Covers both casual and normal mode slugs to confirm the
    /// behaviour isn't tone-specific.
    #[test]
    fn q5_opt_in_off_preserves_q4_model_id() {
        for mode_slug in ["casual", "normal"] {
            let db = fresh_db();
            // PreferQ5Models default is false; do not set explicitly
            // (and assert the default-off behaviour holds).
            let provider = ModelEchoingStubCleanupProvider {
                text_to_return: "Echoed Q4 reply.".into(),
            };
            let mut cleaner = LlmCleaner::new(
                Box::new(provider),
                db,
                "qwen2.5:7b-instruct-q4_K_M".into(),
                0.3,
                256,
            );
            let raw = "this is a fifteen word dictation that should reach the LLM \
                       at level high without any q five substitution today";
            let _out = cleaner.clean(raw, mode_slug).unwrap();
            let model = cleaner.model_name();
            assert!(
                model.starts_with("qwen2.5:7b-instruct-q4_K_M"),
                "mode={mode_slug}: provider should have received Q4 id; provenance: {model}"
            );
            assert!(
                !model.contains(Q5_OPT_IN_SUFFIX.trim_start_matches('+')),
                "mode={mode_slug}: q5-opt-in suffix must be absent when default-off; got: {model}"
            );
        }
    }

    /// PreferQ5Models=true AND model_id ends in `-q4_K_M` → the
    /// provider receives the `-q5_K_M` substitution AND the
    /// `+q5-opt-in` provenance suffix is stamped. Covers both casual
    /// and normal mode slugs to confirm the substitution is generic,
    /// not casual-specific (per dispatch spec).
    #[test]
    fn q5_opt_in_on_substitutes_q5_and_stamps_provenance() {
        for mode_slug in ["casual", "normal"] {
            let db = fresh_db();
            set_prefer_q5(&db, true);
            let provider = ModelEchoingStubCleanupProvider {
                text_to_return: "Echoed Q5 reply.".into(),
            };
            let mut cleaner = LlmCleaner::new(
                Box::new(provider),
                db,
                "qwen2.5:7b-instruct-q4_K_M".into(),
                0.3,
                256,
            );
            let raw = "this is a fifteen word dictation that should reach the LLM \
                       so the q five substitution path fires for the model id";
            let _out = cleaner.clean(raw, mode_slug).unwrap();
            let model = cleaner.model_name();
            assert!(
                model.starts_with("qwen2.5:7b-instruct-q5_K_M"),
                "mode={mode_slug}: provider should have received Q5 id; provenance: {model}"
            );
            assert!(
                model.contains("q5-opt-in"),
                "mode={mode_slug}: provenance must carry q5-opt-in suffix; got: {model}"
            );
            assert!(
                model.contains(PREPROCESSOR_VERSION),
                "mode={mode_slug}: provenance must still carry preprocessor version; got: {model}"
            );
        }
    }

    /// PreferQ5Models=true but the active model_id is already Q5 (or
    /// a different quantisation) → no double-substitution, no
    /// provenance suffix. The opt-in is a no-op when there's nothing
    /// to promote.
    #[test]
    fn q5_opt_in_on_does_not_double_substitute_existing_q5() {
        let db = fresh_db();
        set_prefer_q5(&db, true);
        let provider = ModelEchoingStubCleanupProvider {
            text_to_return: "Already Q5.".into(),
        };
        let mut cleaner = LlmCleaner::new(
            Box::new(provider),
            db,
            "qwen2.5:7b-instruct-q5_K_M".into(),
            0.3,
            256,
        );
        let raw = "this is a fifteen word dictation that should reach the LLM \
                   today without the substitution firing because already q five";
        let _out = cleaner.clean(raw, "normal").unwrap();
        let model = cleaner.model_name();
        assert!(
            model.starts_with("qwen2.5:7b-instruct-q5_K_M"),
            "already-Q5 model_id should pass through; got: {model}"
        );
        assert!(
            !model.contains("q5-opt-in"),
            "q5-opt-in suffix must be absent when substitution didn't fire; got: {model}"
        );
    }
}
