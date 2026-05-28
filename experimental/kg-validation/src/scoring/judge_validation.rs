//! Judge Validation Protocol — ADR 0048 §G5.
//!
//! Five mechanical gates run before any LLM-judge verdict is allowed
//! to feed scoring:
//!
//! 1. **Calibration set ≥ 90%** (STOP) — judge must hit the
//!    hand-authored gold pairs at `judge-calibration/tag-equivalence.json`.
//! 2. **Per-verdict reasoning audit ≥ 95%** (STOP) — reasoning > 30
//!    chars, references at least one token from BOTH candidate
//!    sides, verdict marker AFTER reasoning.
//! 3. **Cross-judge agreement on 10% sample ≥ 85%** (STOP < 85%,
//!    WARN 85–95%) — second judge from a different model family.
//!    Demotes to WARN-only if the cross-model isn't pulled.
//! 4. **Distribution sanity 40–80% equivalent rate** (WARN) — flag
//!    extreme rates as suspicious, proceed otherwise.
//! 5. **Determinism re-run of first 5 verdicts** (WARN) — same seed
//!    must yield byte-identical output.
//!
//! Output: a [`JvpReport`] persisted to `runs/<run-id>/JUDGE_VALIDATION.json`.

use serde::{Deserialize, Serialize};

use crate::ollama::{GenerateOptions, OllamaDispatcher};
use crate::scoring::judge::{
    judge_tag_equivalence, TagEquivalence, TagJudgeRequest, TagJudgeVerdict,
};
use crate::scoring::metrics::RecordedJudgeCall;

// ────────────────────────────────────────────────────────────────────
// Public types
// ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum GateOutcome {
    Pass,
    Warn,
    Stop,
    Skipped,
}

#[derive(Debug, Clone, Serialize)]
pub struct GateResult {
    pub name: &'static str,
    pub outcome: GateOutcome,
    pub detail: String,
    /// Compact numeric form for the SCORE_SUMMARY.md table; same
    /// shape for every gate (numerator / denominator / percentage)
    /// even when a gate's natural form is a count (Gate 4/5).
    pub numerator: usize,
    pub denominator: usize,
    pub percentage: f64,
    /// Up to ~3 failing-case slices for quick post-mortem in the
    /// JUDGE_VALIDATION.json file. Format depends on the gate.
    pub samples: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum JvpOverall {
    Proceed,
    ProceedWithWarnings,
    Halt,
}

#[derive(Debug, Clone, Serialize)]
pub struct JvpReport {
    pub run_id: String,
    pub primary_judge_model: String,
    pub cross_judge_model: Option<String>,
    pub calibration_set_id: String,
    pub gate1_calibration: GateResult,
    pub gate2_reasoning_audit: GateResult,
    pub gate3_cross_judge: GateResult,
    pub gate4_distribution: GateResult,
    pub gate5_determinism: GateResult,
    pub overall: JvpOverall,
}

// ────────────────────────────────────────────────────────────────────
// Calibration set on-disk shape
// ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct CalibrationSet {
    pub calibration_set_id: String,
    pub pairs: Vec<CalibrationPair>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CalibrationPair {
    pub pair_id: String,
    pub tags_a: Vec<String>,
    pub tags_b: Vec<String>,
    pub expected_verdict: String, // "equivalent" | "not-equivalent"
}

impl CalibrationPair {
    fn expected(&self) -> Option<TagEquivalence> {
        match self.expected_verdict.as_str() {
            "equivalent" => Some(TagEquivalence::Equivalent),
            "not-equivalent" => Some(TagEquivalence::NotEquivalent),
            _ => None,
        }
    }
}

// ────────────────────────────────────────────────────────────────────
// Configuration
// ────────────────────────────────────────────────────────────────────

pub struct JvpConfig<'a, D: OllamaDispatcher> {
    pub run_id: String,
    pub primary_judge: &'a D,
    pub primary_judge_model: String,
    /// `None` ⇒ Gate 3 demotes to WARN-only.
    pub cross_judge: Option<&'a D>,
    pub cross_judge_model: Option<String>,
    pub calibration: CalibrationSet,
    pub judge_options: GenerateOptions,
    /// All verdicts from the scoring pass — fed to Gates 2 + 4.
    pub recorded_verdicts: Vec<RecordedJudgeCall>,
}

// ────────────────────────────────────────────────────────────────────
// Entry point
// ────────────────────────────────────────────────────────────────────

/// Run all five gates. Gates 4/5 always run; Gate 3 may demote to
/// WARN-only when no cross-judge is configured. Any STOP-class gate
/// short-circuits the overall verdict to `Halt`, but the remaining
/// gates still run so the report is informative.
pub fn run_jvp<D: OllamaDispatcher>(config: JvpConfig<'_, D>) -> JvpReport {
    let g1 = gate1_calibration(
        config.primary_judge,
        &config.primary_judge_model,
        &config.calibration,
        &config.judge_options,
    );
    let g2 = gate2_reasoning_audit(&config.recorded_verdicts);
    let g3 = gate3_cross_judge(
        config.primary_judge,
        &config.primary_judge_model,
        config.cross_judge,
        config.cross_judge_model.as_deref(),
        &config.recorded_verdicts,
        &config.judge_options,
    );
    let g4 = gate4_distribution(&config.recorded_verdicts);
    let g5 = gate5_determinism(
        config.primary_judge,
        &config.primary_judge_model,
        &config.recorded_verdicts,
        &config.judge_options,
    );

    let any_stop = [&g1, &g2, &g3, &g4, &g5]
        .iter()
        .any(|g| g.outcome == GateOutcome::Stop);
    let any_warn = [&g1, &g2, &g3, &g4, &g5]
        .iter()
        .any(|g| g.outcome == GateOutcome::Warn);
    let overall = if any_stop {
        JvpOverall::Halt
    } else if any_warn {
        JvpOverall::ProceedWithWarnings
    } else {
        JvpOverall::Proceed
    };

    JvpReport {
        run_id: config.run_id,
        primary_judge_model: config.primary_judge_model,
        cross_judge_model: config.cross_judge_model,
        calibration_set_id: config.calibration.calibration_set_id,
        gate1_calibration: g1,
        gate2_reasoning_audit: g2,
        gate3_cross_judge: g3,
        gate4_distribution: g4,
        gate5_determinism: g5,
        overall,
    }
}

// ────────────────────────────────────────────────────────────────────
// Gate 1 — calibration set ≥ 90%, STOP
// ────────────────────────────────────────────────────────────────────

fn gate1_calibration<D: OllamaDispatcher>(
    judge: &D,
    model: &str,
    set: &CalibrationSet,
    options: &GenerateOptions,
) -> GateResult {
    let mut correct = 0usize;
    let total = set.pairs.len();
    let mut samples: Vec<String> = Vec::new();
    for pair in &set.pairs {
        let expected = match pair.expected() {
            Some(v) => v,
            None => {
                samples.push(format!("{}: bad expected_verdict in fixture", pair.pair_id));
                continue;
            }
        };
        let req = TagJudgeRequest {
            tags_a: pair.tags_a.clone(),
            tags_b: pair.tags_b.clone(),
        };
        match judge_tag_equivalence(judge, model, &req, options) {
            Ok(v) => {
                if v.verdict == expected {
                    correct += 1;
                } else if samples.len() < 3 {
                    samples.push(format!(
                        "{}: expected {expected:?}, got {:?}",
                        pair.pair_id, v.verdict
                    ));
                }
            }
            Err(e) => {
                if samples.len() < 3 {
                    samples.push(format!("{}: judge error: {e}", pair.pair_id));
                }
            }
        }
    }
    let pct = if total == 0 {
        0.0
    } else {
        100.0 * correct as f64 / total as f64
    };
    let outcome = if total == 0 {
        GateOutcome::Stop
    } else if pct >= 90.0 {
        GateOutcome::Pass
    } else {
        GateOutcome::Stop
    };
    GateResult {
        name: "calibration_set",
        outcome,
        detail: format!(
            "judge got {correct}/{total} ({pct:.1}%) of calibration pairs correct (threshold 90%)"
        ),
        numerator: correct,
        denominator: total,
        percentage: pct,
        samples,
    }
}

// ────────────────────────────────────────────────────────────────────
// Gate 2 — per-verdict reasoning audit ≥ 95%, STOP
// ────────────────────────────────────────────────────────────────────

fn gate2_reasoning_audit(verdicts: &[RecordedJudgeCall]) -> GateResult {
    if verdicts.is_empty() {
        return GateResult {
            name: "reasoning_audit",
            outcome: GateOutcome::Skipped,
            detail: "no judge verdicts to audit (no tag metric was computed)".into(),
            numerator: 0,
            denominator: 0,
            percentage: 0.0,
            samples: Vec::new(),
        };
    }
    let mut passing = 0usize;
    let total = verdicts.len();
    let mut samples: Vec<String> = Vec::new();
    for call in verdicts {
        match audit_one(call) {
            Ok(()) => passing += 1,
            Err(reason) => {
                if samples.len() < 3 {
                    samples.push(format!(
                        "{} entry[{}]: {reason}",
                        call.dictation_id, call.entry_index
                    ));
                }
            }
        }
    }
    let pct = 100.0 * passing as f64 / total as f64;
    let outcome = if pct >= 95.0 {
        GateOutcome::Pass
    } else {
        GateOutcome::Stop
    };
    GateResult {
        name: "reasoning_audit",
        outcome,
        detail: format!(
            "{passing}/{total} ({pct:.1}%) verdicts passed the reasoning audit (threshold 95%)"
        ),
        numerator: passing,
        denominator: total,
        percentage: pct,
        samples,
    }
}

fn audit_one(call: &RecordedJudgeCall) -> Result<(), String> {
    let v: &TagJudgeVerdict = &call.verdict;
    if v.reasoning.trim().len() <= 30 {
        return Err(format!(
            "reasoning too short ({} chars)",
            v.reasoning.trim().len()
        ));
    }
    // Reasoning must reference at least one TOKEN from BOTH sides.
    let lower = v.reasoning.to_ascii_lowercase();
    let touches_a = side_tokens(&call.tags_a).any(|tok| lower.contains(&tok));
    let touches_b = side_tokens(&call.tags_b).any(|tok| lower.contains(&tok));
    if !touches_a || !touches_b {
        return Err(format!(
            "reasoning does not reference tokens from both sides (a={touches_a}, b={touches_b})"
        ));
    }
    // Verdict marker AFTER reasoning. Re-derive from raw_output —
    // the parser already enforces order, but rubber-stamp protection
    // means we re-check here against the persisted raw text.
    let lower_raw = v.raw_output.to_ascii_lowercase();
    let v_pos = lower_raw
        .find("verdict:")
        .ok_or("no verdict marker in raw")?;
    let r_pos = lower_raw
        .find("reasoning:")
        .ok_or("no reasoning marker in raw")?;
    if r_pos >= v_pos {
        return Err("verdict marker not after reasoning in raw output".into());
    }
    Ok(())
}

/// Yield the lowercase tokens of each tag in `side` plus the split
/// pieces around `-`. So a tag `car-repair` contributes
/// {`car-repair`, `car`, `repair`}.
fn side_tokens(side: &[String]) -> impl Iterator<Item = String> + '_ {
    side.iter().flat_map(|t| {
        let lower = t.to_ascii_lowercase();
        let mut toks: Vec<String> = vec![lower.clone()];
        toks.extend(
            lower
                .split('-')
                .filter(|p| !p.is_empty())
                .map(str::to_string),
        );
        toks.into_iter()
    })
}

// ────────────────────────────────────────────────────────────────────
// Gate 3 — cross-judge agreement on 10% sample, STOP < 85%, WARN 85-95%
// ────────────────────────────────────────────────────────────────────

fn gate3_cross_judge<D: OllamaDispatcher>(
    _primary: &D,
    _primary_model: &str,
    cross: Option<&D>,
    cross_model: Option<&str>,
    verdicts: &[RecordedJudgeCall],
    options: &GenerateOptions,
) -> GateResult {
    let (cross, cross_model) = match (cross, cross_model) {
        (Some(c), Some(m)) => (c, m),
        _ => {
            return GateResult {
                name: "cross_judge",
                outcome: GateOutcome::Warn,
                detail:
                    "no cross-judge model configured — Gate 3 demoted to WARN-only per ADR 0048 G5"
                        .into(),
                numerator: 0,
                denominator: 0,
                percentage: 0.0,
                samples: Vec::new(),
            };
        }
    };
    if verdicts.is_empty() {
        return GateResult {
            name: "cross_judge",
            outcome: GateOutcome::Skipped,
            detail: "no judge verdicts to cross-check".into(),
            numerator: 0,
            denominator: 0,
            percentage: 0.0,
            samples: Vec::new(),
        };
    }

    // Deterministic 10% sample: every 10th verdict, minimum 1.
    let sample_size = (verdicts.len() / 10).max(1);
    let step = verdicts.len().max(1) / sample_size.max(1);
    let mut sampled: Vec<&RecordedJudgeCall> = Vec::with_capacity(sample_size);
    let mut i = 0usize;
    while sampled.len() < sample_size && i < verdicts.len() {
        sampled.push(&verdicts[i]);
        i = i.saturating_add(step.max(1));
    }

    let mut agree = 0usize;
    let total = sampled.len();
    let mut samples: Vec<String> = Vec::new();
    for call in &sampled {
        let req = TagJudgeRequest {
            tags_a: call.tags_a.clone(),
            tags_b: call.tags_b.clone(),
        };
        match judge_tag_equivalence(cross, cross_model, &req, options) {
            Ok(v2) => {
                if v2.verdict == call.verdict.verdict {
                    agree += 1;
                } else if samples.len() < 3 {
                    samples.push(format!(
                        "{} entry[{}]: primary={:?} cross={:?}",
                        call.dictation_id, call.entry_index, call.verdict.verdict, v2.verdict
                    ));
                }
            }
            Err(e) => {
                if samples.len() < 3 {
                    samples.push(format!(
                        "{} entry[{}]: cross-judge errored: {e}",
                        call.dictation_id, call.entry_index
                    ));
                }
            }
        }
    }
    let pct = if total == 0 {
        0.0
    } else {
        100.0 * agree as f64 / total as f64
    };
    let outcome = if total == 0 {
        GateOutcome::Skipped
    } else if pct >= 95.0 {
        GateOutcome::Pass
    } else if pct >= 85.0 {
        GateOutcome::Warn
    } else {
        GateOutcome::Stop
    };
    GateResult {
        name: "cross_judge",
        outcome,
        detail: format!(
            "cross-judge agreed on {agree}/{total} ({pct:.1}%) of 10% sample (STOP < 85%, WARN 85-95%, PASS >= 95%)"
        ),
        numerator: agree,
        denominator: total,
        percentage: pct,
        samples,
    }
}

// ────────────────────────────────────────────────────────────────────
// Gate 4 — distribution sanity 40-80% equivalent rate, WARN
// ────────────────────────────────────────────────────────────────────

fn gate4_distribution(verdicts: &[RecordedJudgeCall]) -> GateResult {
    if verdicts.is_empty() {
        return GateResult {
            name: "distribution_sanity",
            outcome: GateOutcome::Skipped,
            detail: "no verdicts to inspect".into(),
            numerator: 0,
            denominator: 0,
            percentage: 0.0,
            samples: Vec::new(),
        };
    }
    let total = verdicts.len();
    let eq = verdicts
        .iter()
        .filter(|c| c.verdict.verdict == TagEquivalence::Equivalent)
        .count();
    let pct = 100.0 * eq as f64 / total as f64;
    let outcome = if (40.0..=80.0).contains(&pct) {
        GateOutcome::Pass
    } else {
        GateOutcome::Warn
    };
    GateResult {
        name: "distribution_sanity",
        outcome,
        detail: format!(
            "judge marked {eq}/{total} ({pct:.1}%) of verdicts equivalent (in-band 40-80%)"
        ),
        numerator: eq,
        denominator: total,
        percentage: pct,
        samples: Vec::new(),
    }
}

// ────────────────────────────────────────────────────────────────────
// Gate 5 — determinism re-run of first 5 verdicts, WARN
// ────────────────────────────────────────────────────────────────────

fn gate5_determinism<D: OllamaDispatcher>(
    judge: &D,
    model: &str,
    verdicts: &[RecordedJudgeCall],
    options: &GenerateOptions,
) -> GateResult {
    if verdicts.is_empty() {
        return GateResult {
            name: "determinism_recheck",
            outcome: GateOutcome::Skipped,
            detail: "no verdicts to re-check".into(),
            numerator: 0,
            denominator: 0,
            percentage: 0.0,
            samples: Vec::new(),
        };
    }
    let sample_size = verdicts.len().min(5);
    let mut identical = 0usize;
    let mut samples: Vec<String> = Vec::new();
    for call in verdicts.iter().take(sample_size) {
        let req = TagJudgeRequest {
            tags_a: call.tags_a.clone(),
            tags_b: call.tags_b.clone(),
        };
        match judge_tag_equivalence(judge, model, &req, options) {
            Ok(v2) => {
                if v2.raw_output == call.verdict.raw_output {
                    identical += 1;
                } else if samples.len() < 3 {
                    samples.push(format!(
                        "{} entry[{}]: re-run output diverged",
                        call.dictation_id, call.entry_index
                    ));
                }
            }
            Err(e) => {
                if samples.len() < 3 {
                    samples.push(format!(
                        "{} entry[{}]: re-run errored: {e}",
                        call.dictation_id, call.entry_index
                    ));
                }
            }
        }
    }
    let pct = 100.0 * identical as f64 / sample_size as f64;
    let outcome = if identical == sample_size {
        GateOutcome::Pass
    } else {
        GateOutcome::Warn
    };
    GateResult {
        name: "determinism_recheck",
        outcome,
        detail: format!(
            "{identical}/{sample_size} ({pct:.1}%) re-runs produced byte-identical output"
        ),
        numerator: identical,
        denominator: sample_size,
        percentage: pct,
        samples,
    }
}

// ────────────────────────────────────────────────────────────────────
// Calibration-set loader
// ────────────────────────────────────────────────────────────────────

pub fn load_calibration_set(path: &std::path::Path) -> anyhow::Result<CalibrationSet> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("read calibration set {}: {e}", path.display()))?;
    let set: CalibrationSet = serde_json::from_str(&text)
        .map_err(|e| anyhow::anyhow!("parse calibration set {}: {e}", path.display()))?;
    if set.pairs.is_empty() {
        anyhow::bail!("calibration set is empty: {}", path.display());
    }
    Ok(set)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ollama::testing::MockOllama;
    use crate::scoring::judge::TagJudgeVerdict;

    fn opts() -> GenerateOptions {
        GenerateOptions::default()
    }

    fn cal(pairs: Vec<(&str, &[&str], &[&str], &str)>) -> CalibrationSet {
        CalibrationSet {
            calibration_set_id: "test".into(),
            pairs: pairs
                .into_iter()
                .map(|(id, a, b, exp)| CalibrationPair {
                    pair_id: id.into(),
                    tags_a: a.iter().map(|s| s.to_string()).collect(),
                    tags_b: b.iter().map(|s| s.to_string()).collect(),
                    expected_verdict: exp.into(),
                })
                .collect(),
        }
    }

    fn rec(
        id: &str,
        idx: usize,
        a: &[&str],
        b: &[&str],
        verdict: TagEquivalence,
        reasoning: &str,
    ) -> RecordedJudgeCall {
        let raw = format!(
            "REASONING: {reasoning}\nVERDICT: {}",
            match verdict {
                TagEquivalence::Equivalent => "equivalent",
                TagEquivalence::NotEquivalent => "not-equivalent",
            }
        );
        RecordedJudgeCall {
            dictation_id: id.into(),
            entry_index: idx,
            tags_a: a.iter().map(|s| s.to_string()).collect(),
            tags_b: b.iter().map(|s| s.to_string()).collect(),
            verdict: TagJudgeVerdict {
                reasoning: reasoning.into(),
                verdict,
                raw_output: raw,
            },
        }
    }

    #[test]
    fn gate1_passes_on_perfect_judge() {
        // Anchors are chosen so they only appear in the actual
        // A/B query line at the bottom of the prompt, NOT in the
        // judge prompt's in-context examples (which mention
        // car-repair / taxes / etc. and would collide with naive
        // substring matching).
        let mock = MockOllama::new()
            .respond_when(
                "\"kid-stuff\"",
                "REASONING: same kid topic, just different wording\nVERDICT: equivalent",
            )
            .respond_when(
                "\"alpha-tag\"",
                "REASONING: alpha and beta are unrelated domains here\nVERDICT: not-equivalent",
            );
        let set = cal(vec![
            (
                "cal-eq-001",
                &["kid-stuff"],
                &["kid-stuff", "children"],
                "equivalent",
            ),
            (
                "cal-diff-001",
                &["alpha-tag"],
                &["beta-tag"],
                "not-equivalent",
            ),
        ]);
        let g = gate1_calibration(&mock, "judge", &set, &opts());
        assert_eq!(g.outcome, GateOutcome::Pass);
        assert_eq!(g.numerator, 2);
        assert_eq!(g.denominator, 2);
    }

    #[test]
    fn gate1_stops_on_bad_judge() {
        // Judge says equivalent for everything — fails the diff cases.
        let mock = MockOllama::new()
            .default_response("REASONING: same in some loose sense, sure\nVERDICT: equivalent");
        let set = cal(vec![
            ("cal-eq-001", &["a"], &["a"], "equivalent"),
            ("cal-diff-001", &["taxes"], &["vacation"], "not-equivalent"),
            (
                "cal-diff-002",
                &["marcus"],
                &["marketing"],
                "not-equivalent",
            ),
        ]);
        let g = gate1_calibration(&mock, "judge", &set, &opts());
        assert_eq!(g.outcome, GateOutcome::Stop);
        assert!(g.percentage < 90.0);
    }

    #[test]
    fn gate2_passes_on_well_formed_audit() {
        let v = vec![
            rec("d1", 0, &["car-repair", "auto"], &["car-repair", "auto-maintenance"],
                TagEquivalence::Equivalent,
                "both sets name a car repair note; auto and auto-maintenance are equivalent in context"),
            rec("d2", 0, &["taxes"], &["vacation"], TagEquivalence::NotEquivalent,
                "taxes is finance and vacation is travel; unrelated for filing purposes"),
        ];
        let g = gate2_reasoning_audit(&v);
        assert_eq!(g.outcome, GateOutcome::Pass);
        assert_eq!(g.numerator, 2);
    }

    #[test]
    fn gate2_stops_on_short_reasoning() {
        let v = vec![
            rec(
                "d1",
                0,
                &["car-repair"],
                &["auto-maintenance"],
                TagEquivalence::Equivalent,
                "yes",
            ),
            rec(
                "d2",
                0,
                &["taxes"],
                &["vacation"],
                TagEquivalence::NotEquivalent,
                "taxes is finance and vacation is travel; unrelated for filing purposes",
            ),
        ];
        let g = gate2_reasoning_audit(&v);
        assert_eq!(g.outcome, GateOutcome::Stop);
        assert!(g.samples.iter().any(|s| s.contains("too short")));
    }

    #[test]
    fn gate2_stops_when_reasoning_doesnt_reference_both_sides() {
        let v = vec![rec(
            "d1",
            0,
            &["car-repair"],
            &["auto-maintenance"],
            TagEquivalence::Equivalent,
            // Says car-repair but never mentions auto/auto-maintenance.
            "we have a car-repair note that needs filing; this seems straightforward to me",
        )];
        let g = gate2_reasoning_audit(&v);
        assert_eq!(g.outcome, GateOutcome::Stop);
        assert!(g.samples.iter().any(|s| s.contains("both sides")));
    }

    #[test]
    fn gate3_warn_when_no_cross_judge_configured() {
        let v = vec![rec(
            "d1",
            0,
            &["a"],
            &["b"],
            TagEquivalence::Equivalent,
            "long enough reasoning that names both sides a and b for the audit",
        )];
        // Cross-judge type must match primary — use MockOllama for both `None`s.
        let g = gate3_cross_judge::<MockOllama>(&MockOllama::new(), "p", None, None, &v, &opts());
        assert_eq!(g.outcome, GateOutcome::Warn);
        assert!(g.detail.contains("WARN-only"));
    }

    #[test]
    fn gate3_pass_when_cross_judge_agrees() {
        let primary = MockOllama::new();
        let cross = MockOllama::new()
            .default_response("REASONING: same call, valid for both a and b\nVERDICT: equivalent");
        let v: Vec<RecordedJudgeCall> = (0..10)
            .map(|i| {
                rec(
                    "d",
                    i,
                    &["a"],
                    &["b"],
                    TagEquivalence::Equivalent,
                    "long enough reasoning that names both sides a and b for the audit",
                )
            })
            .collect();
        let g = gate3_cross_judge(&primary, "p", Some(&cross), Some("x"), &v, &opts());
        assert_eq!(g.outcome, GateOutcome::Pass);
    }

    #[test]
    fn gate3_stop_when_cross_judge_disagrees() {
        let primary = MockOllama::new();
        let cross = MockOllama::new().default_response(
            "REASONING: clearly different concepts to me\nVERDICT: not-equivalent",
        );
        let v: Vec<RecordedJudgeCall> = (0..10)
            .map(|i| {
                rec(
                    "d",
                    i,
                    &["a"],
                    &["b"],
                    TagEquivalence::Equivalent,
                    "long enough reasoning that names both sides a and b for the audit",
                )
            })
            .collect();
        let g = gate3_cross_judge(&primary, "p", Some(&cross), Some("x"), &v, &opts());
        assert_eq!(g.outcome, GateOutcome::Stop);
    }

    #[test]
    fn gate4_warn_on_collapsed_distribution() {
        let v: Vec<RecordedJudgeCall> = (0..10)
            .map(|i| {
                rec(
                    "d",
                    i,
                    &["a"],
                    &["b"],
                    TagEquivalence::Equivalent,
                    "ok with a and b",
                )
            })
            .collect();
        let g = gate4_distribution(&v);
        assert_eq!(g.outcome, GateOutcome::Warn);
        assert!(g.percentage > 80.0);
    }

    #[test]
    fn gate4_pass_in_band() {
        let mut v: Vec<RecordedJudgeCall> = (0..5)
            .map(|i| rec("d", i, &["a"], &["b"], TagEquivalence::Equivalent, "ok"))
            .collect();
        for i in 0..5 {
            v.push(rec(
                "d",
                i + 5,
                &["a"],
                &["b"],
                TagEquivalence::NotEquivalent,
                "ok",
            ));
        }
        let g = gate4_distribution(&v);
        assert_eq!(g.outcome, GateOutcome::Pass);
    }

    #[test]
    fn gate5_warn_on_diverging_rerun() {
        let primary = MockOllama::new().default_response(
            "REASONING: this is a NEW response distinct from the recorded raw\nVERDICT: equivalent",
        );
        let v = vec![rec(
            "d1",
            0,
            &["a"],
            &["b"],
            TagEquivalence::Equivalent,
            "long enough reasoning that names both sides a and b for the audit",
        )];
        let g = gate5_determinism(&primary, "p", &v, &opts());
        assert_eq!(g.outcome, GateOutcome::Warn);
    }

    #[test]
    fn overall_halt_when_any_gate_stops() {
        let primary = MockOllama::new().default_response(
            "REASONING: bad and lazy reasoning, says equivalent always\nVERDICT: equivalent",
        );
        let v: Vec<RecordedJudgeCall> = (0..10)
            .map(|i| {
                rec(
                    "d",
                    i,
                    &["a"],
                    &["b"],
                    TagEquivalence::Equivalent,
                    "long enough reasoning that names both sides a and b for the audit",
                )
            })
            .collect();
        let set = cal(vec![
            ("cal-diff-001", &["taxes"], &["vacation"], "not-equivalent"),
            (
                "cal-diff-002",
                &["marcus"],
                &["marketing"],
                "not-equivalent",
            ),
        ]);
        let report = run_jvp(JvpConfig {
            run_id: "test".into(),
            primary_judge: &primary,
            primary_judge_model: "p".into(),
            cross_judge: None::<&MockOllama>,
            cross_judge_model: None,
            calibration: set,
            judge_options: opts(),
            recorded_verdicts: v,
        });
        assert_eq!(report.overall, JvpOverall::Halt);
        assert_eq!(report.gate1_calibration.outcome, GateOutcome::Stop);
    }

    #[test]
    fn loader_round_trips_real_calibration_file() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("judge-calibration")
            .join("tag-equivalence.json");
        if !path.exists() {
            return; // sandbox built without calibration set
        }
        let set = load_calibration_set(&path).unwrap();
        assert_eq!(set.calibration_set_id, "tag-equivalence-v1");
        assert!(set.pairs.len() >= 12);
        // Every pair has a parseable expected verdict.
        for p in &set.pairs {
            assert!(
                p.expected().is_some(),
                "pair {} has bad expected_verdict={:?}",
                p.pair_id,
                p.expected_verdict
            );
        }
    }
}
