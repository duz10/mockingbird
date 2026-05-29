//! Phase 0 invariant judges (Wave 4, `mb-he98`).
//!
//! Each judge is a small focused module that takes a `JudgeContext`
//! (paths + parsed artifacts) and returns a [`JudgeVerdict`] with
//! a pass/fail and a short reasoning string. Judges are mechanical
//! (no LLM calls) — they assert on JSON / Markdown / git artifacts.
//!
//! ADR 0048 §G7 retired the LLM-judged JVP-completeness judge from
//! the original 7-judge suite; the remaining six are:
//!
//! 1. [`hard_gate`] — `SCORE.json::per_metric::invented_dates_count == 0`
//!    across every scored run.
//! 2. [`thresholds`] — per-metric verdicts vs. spec §8.4 thresholds
//!    (segmentation ≥ 85%, category ≥ 90%, entry-type ≥ 85%,
//!    tag-collapse ≥ 80%, clean-single ≈ 100%, junk = 100%,
//!    invented dates = 0).
//! 3. [`stability`] — `SCORE.json::stability` agreement ≥ 80% on
//!    every structural dimension (date = 100% expected; tag-set
//!    exact reported but not gated, since the synonym map
//!    deliberately doesn't collapse everything).
//! 4. [`sandbox_isolation`] — git diff vs. baseline tag asserts ONLY
//!    paths inside the allowed sandbox surface were modified across
//!    all Phase 0 KG commits.
//! 5. [`determinism`] (live — invokes `run-corpus`) — re-runs a
//!    3-dictation subset at fixed seed and asserts the structured
//!    outputs are byte-identical to the originals.
//! 6. [`pcrp_completeness`] — `PERSONA_REVIEW.md` exists in the
//!    final-iteration run dir AND (trust_eroding ≤ 5 OR scores
//!    exceed thresholds by > 5 points).
//!
//! Each judge ships with a `tests` module that exercises both a
//! known-pass fixture and at least one known-fail fixture.

pub mod determinism;
pub mod hard_gate;
pub mod pcrp_completeness;
pub mod sandbox_isolation;
pub mod stability;
pub mod thresholds;

use std::path::PathBuf;

use serde::Serialize;

/// What every judge returns.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct JudgeVerdict {
    pub name: &'static str,
    pub passed: bool,
    pub reasoning: String,
    pub details: Vec<String>,
}

impl JudgeVerdict {
    pub fn pass(name: &'static str, reasoning: impl Into<String>) -> Self {
        Self {
            name,
            passed: true,
            reasoning: reasoning.into(),
            details: Vec::new(),
        }
    }
    pub fn fail(name: &'static str, reasoning: impl Into<String>) -> Self {
        Self {
            name,
            passed: false,
            reasoning: reasoning.into(),
            details: Vec::new(),
        }
    }
    pub fn with_details(mut self, details: Vec<String>) -> Self {
        self.details = details;
        self
    }
}

/// Shared inputs handed to every judge. Some judges only need a
/// subset; they ignore the rest.
#[derive(Debug, Clone)]
pub struct JudgeContext {
    /// Paths to per-run `runs/<id>/` directories that should be
    /// inspected. The HARD-GATE + Thresholds judges loop over these.
    pub run_dirs: Vec<PathBuf>,
    /// The final-iteration run dir (where PCRP_completeness expects
    /// `PERSONA_REVIEW.md`). Usually the last entry in `run_dirs`.
    pub final_run_dir: Option<PathBuf>,
    /// Repo root, for `sandbox_isolation` git inspection.
    pub repo_root: PathBuf,
    /// Baseline git ref against which `sandbox_isolation` diffs.
    pub baseline_ref: String,
    /// Sandbox-allowed path prefixes for `sandbox_isolation`.
    pub allowed_path_prefixes: Vec<String>,
    /// Models referenced by the spec, for `determinism` to drive
    /// `run-corpus --seed 42`. (Not loaded by every judge.)
    pub determinism: Option<DeterminismConfig>,
}

#[derive(Debug, Clone)]
pub struct DeterminismConfig {
    pub run_corpus_bin: PathBuf,
    pub corpus_dir: PathBuf,
    pub baseline_run_dir: PathBuf,
    pub three_dictation_ids: [String; 3],
    pub model: String,
    pub ollama_url: String,
}
