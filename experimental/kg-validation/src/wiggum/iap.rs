//! Iteration Acceptance Protocol (IAP) per the Wave 5 kickoff.
//!
//! Accepts a candidate iteration iff ALL of:
//!
//! 1. **Aggregate same-or-better**: weighted sum of metrics is ≥ the
//!    previous-iteration baseline. Weights:
//!    `hard-gate=4, segmentation=2, category=2, entry-type=2,`
//!    `tag-collapse=2, clean-single=1, junk=1` (sum = 14).
//! 2. **No per-metric regression**: strict — any single graded metric
//!    below previous baseline = REJECT.
//! 3. **Hard gate intact**: `invented_dates_count == 0` maintained.
//! 4. **Stability**: re-run at seed 137, structural-metric agreement
//!    ≥ 80% on segmentation / category / entry-type. (Date is graded
//!    against the perfect-bar already, tag-set-exact is observational.)
//! 5. **PCRP trust-eroding count ≤ previous baseline** (cannot increase).
//!
//! On Accept, the caller advances its baseline snapshot to the
//! candidate's numbers. On Reject, the prompt change is reverted but
//! the iteration counter still advances (per kickoff).
//!
//! This module is pure: no I/O, no Ollama, no clocks. The CLI binary
//! `iap-check` is the thin wrapper that reads `SCORE.json` +
//! `PERSONA_REVIEW.md` off disk and feeds them in.

use serde::{Deserialize, Serialize};

/// The six graded metrics + the hard-gate counter + the PCRP signal.
///
/// Percentages are 0.0..=100.0 (matching `Ratio::percentage` shape from
/// `scoring::metrics`). `invented_dates_count` is an absolute count
/// (hard-gate = 0 is the only passing value).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct IapMetrics {
    pub invented_dates_count: usize,
    pub segmentation_correct_pct: f64,
    pub category_correct_pct: f64,
    pub entry_type_correct_pct: f64,
    pub tag_collapse_correct_pct: f64,
    pub clean_single_item_correct_pct: f64,
    pub junk_correct_pct: f64,
    pub pcrp_trust_eroding: usize,
}

/// Stability percentages from the seed-137 sibling-run comparison
/// (spec §8.5 shape, but only the three "structural" agreements are
/// gated by the IAP at the 80% bar — date is already perfect and
/// tag-set-exact is observational).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct IapStability {
    pub segmentation_agreement_pct: f64,
    pub category_agreement_pct: f64,
    pub entry_type_agreement_pct: f64,
    pub date_agreement_pct: f64,
    pub tag_set_exact_agreement_pct: f64,
}

/// What gets handed to [`evaluate`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IapInput {
    pub iteration: u32,
    pub label: String,
    pub baseline: IapMetrics,
    pub candidate: IapMetrics,
    pub candidate_stability: IapStability,
}

/// Result of an IAP check.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "verdict")]
pub enum IapVerdict {
    /// All five admission gates passed.
    Accept {
        weighted_score: f64,
        baseline_weighted: f64,
        delta: f64,
    },
    /// One or more gates failed. `reasons` lists every gate that
    /// failed, in protocol order, so the journal entry can name them
    /// all in one report rather than fail-fast.
    Reject {
        weighted_score: f64,
        baseline_weighted: f64,
        reasons: Vec<String>,
    },
}

/// Stability floor for the three structural agreements (§8.5 + kickoff).
pub const STABILITY_FLOOR_PCT: f64 = 80.0;

/// Compute the weighted aggregate score for one set of metrics.
///
/// Hard-gate contributes its full weight iff `invented_dates_count == 0`,
/// otherwise 0. All other metrics contribute `(pct / 100.0) * weight`.
/// PCRP is NOT in the aggregate — it's a separate admission gate (#5).
///
/// Weights:
///
/// | Metric | Weight |
/// |---|---|
/// | invented_dates (hard-gate, binary) | 4 |
/// | segmentation_correct | 2 |
/// | category_correct | 2 |
/// | entry_type_correct | 2 |
/// | tag_variant_collapse_correct | 2 |
/// | clean_single_item_correct | 1 |
/// | junk_correct | 1 |
///
/// Total = 14.
pub fn weighted_score(m: &IapMetrics) -> f64 {
    let hard = if m.invented_dates_count == 0 {
        4.0
    } else {
        0.0
    };
    let pct = |p: f64, w: f64| (p / 100.0) * w;
    hard + pct(m.segmentation_correct_pct, 2.0)
        + pct(m.category_correct_pct, 2.0)
        + pct(m.entry_type_correct_pct, 2.0)
        + pct(m.tag_collapse_correct_pct, 2.0)
        + pct(m.clean_single_item_correct_pct, 1.0)
        + pct(m.junk_correct_pct, 1.0)
}

/// Apply the 5-rule protocol. Pure function.
pub fn evaluate(input: &IapInput) -> IapVerdict {
    let baseline_weighted = weighted_score(&input.baseline);
    let weighted_score_v = weighted_score(&input.candidate);
    let mut reasons: Vec<String> = Vec::new();

    // Rule 1: aggregate same-or-better. Tolerate ~1e-9 fp slop.
    if weighted_score_v + 1e-9 < baseline_weighted {
        reasons.push(format!(
            "rule-1 aggregate regressed: candidate {:.4} < baseline {:.4} (Δ {:.4})",
            weighted_score_v,
            baseline_weighted,
            weighted_score_v - baseline_weighted
        ));
    }

    // Rule 2: strict per-metric no-regression. Hard-gate handled in #3.
    let per_metric_checks: &[(&str, f64, f64)] = &[
        (
            "segmentation_correct",
            input.candidate.segmentation_correct_pct,
            input.baseline.segmentation_correct_pct,
        ),
        (
            "category_correct",
            input.candidate.category_correct_pct,
            input.baseline.category_correct_pct,
        ),
        (
            "entry_type_correct",
            input.candidate.entry_type_correct_pct,
            input.baseline.entry_type_correct_pct,
        ),
        (
            "tag_collapse_correct",
            input.candidate.tag_collapse_correct_pct,
            input.baseline.tag_collapse_correct_pct,
        ),
        (
            "clean_single_item_correct",
            input.candidate.clean_single_item_correct_pct,
            input.baseline.clean_single_item_correct_pct,
        ),
        (
            "junk_correct",
            input.candidate.junk_correct_pct,
            input.baseline.junk_correct_pct,
        ),
    ];
    for (name, cand, base) in per_metric_checks {
        if *cand + 1e-9 < *base {
            reasons.push(format!(
                "rule-2 {name} regressed: candidate {cand:.2}% < baseline {base:.2}%"
            ));
        }
    }

    // Rule 3: hard gate intact.
    if input.candidate.invented_dates_count != 0 {
        reasons.push(format!(
            "rule-3 hard-gate broken: invented_dates_count = {} (must be 0)",
            input.candidate.invented_dates_count
        ));
    }

    // Rule 4: stability floor on the three structural agreements.
    let stability_checks: &[(&str, f64)] = &[
        (
            "segmentation_agreement",
            input.candidate_stability.segmentation_agreement_pct,
        ),
        (
            "category_agreement",
            input.candidate_stability.category_agreement_pct,
        ),
        (
            "entry_type_agreement",
            input.candidate_stability.entry_type_agreement_pct,
        ),
    ];
    for (name, val) in stability_checks {
        if *val + 1e-9 < STABILITY_FLOOR_PCT {
            reasons.push(format!("rule-4 {name} below 80% floor: {val:.2}%"));
        }
    }

    // Rule 5: PCRP trust-eroding count cannot increase.
    if input.candidate.pcrp_trust_eroding > input.baseline.pcrp_trust_eroding {
        reasons.push(format!(
            "rule-5 PCRP trust-eroding rose: candidate {} > baseline {}",
            input.candidate.pcrp_trust_eroding, input.baseline.pcrp_trust_eroding
        ));
    }

    if reasons.is_empty() {
        IapVerdict::Accept {
            weighted_score: weighted_score_v,
            baseline_weighted,
            delta: weighted_score_v - baseline_weighted,
        }
    } else {
        IapVerdict::Reject {
            weighted_score: weighted_score_v,
            baseline_weighted,
            reasons,
        }
    }
}

/// Render a Markdown journal entry suitable for appending to
/// `runs/ITERATION_JOURNAL.md`. The first line is an `## Iter N`
/// header so the journal has natural anchors.
pub fn render_journal_entry(input: &IapInput, verdict: &IapVerdict) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "## Iter {} — {}\n\n",
        input.iteration, input.label
    ));
    let (verdict_str, weighted, baseline_w, delta_opt, reasons_opt): (
        &str,
        f64,
        f64,
        Option<f64>,
        Option<&[String]>,
    ) = match verdict {
        IapVerdict::Accept {
            weighted_score,
            baseline_weighted,
            delta,
        } => (
            "✅ ACCEPT",
            *weighted_score,
            *baseline_weighted,
            Some(*delta),
            None,
        ),
        IapVerdict::Reject {
            weighted_score,
            baseline_weighted,
            reasons,
        } => (
            "❌ REJECT",
            *weighted_score,
            *baseline_weighted,
            None,
            Some(reasons.as_slice()),
        ),
    };

    s.push_str(&format!("**Verdict:** {verdict_str}\n\n"));
    s.push_str(&format!(
        "**Weighted score:** {weighted:.4} / 14.0 (baseline {baseline_w:.4}"
    ));
    if let Some(d) = delta_opt {
        s.push_str(&format!(", Δ {d:+.4}"));
    }
    s.push_str(")\n\n");

    s.push_str("### Metrics\n\n");
    s.push_str("| Metric | Baseline | Candidate | Δ |\n|---|---|---|---|\n");
    let rows: &[(&str, f64, f64)] = &[
        (
            "segmentation_correct (%)",
            input.baseline.segmentation_correct_pct,
            input.candidate.segmentation_correct_pct,
        ),
        (
            "category_correct (%)",
            input.baseline.category_correct_pct,
            input.candidate.category_correct_pct,
        ),
        (
            "entry_type_correct (%)",
            input.baseline.entry_type_correct_pct,
            input.candidate.entry_type_correct_pct,
        ),
        (
            "tag_collapse_correct (%)",
            input.baseline.tag_collapse_correct_pct,
            input.candidate.tag_collapse_correct_pct,
        ),
        (
            "clean_single_item_correct (%)",
            input.baseline.clean_single_item_correct_pct,
            input.candidate.clean_single_item_correct_pct,
        ),
        (
            "junk_correct (%)",
            input.baseline.junk_correct_pct,
            input.candidate.junk_correct_pct,
        ),
    ];
    for (name, base, cand) in rows {
        s.push_str(&format!(
            "| {name} | {base:.2} | {cand:.2} | {:+.2} |\n",
            cand - base
        ));
    }
    s.push_str(&format!(
        "| invented_dates_count (hard-gate) | {} | {} | {} |\n",
        input.baseline.invented_dates_count,
        input.candidate.invented_dates_count,
        signed_isize(
            input.candidate.invented_dates_count,
            input.baseline.invented_dates_count
        )
    ));
    s.push_str(&format!(
        "| pcrp_trust_eroding | {} | {} | {} |\n",
        input.baseline.pcrp_trust_eroding,
        input.candidate.pcrp_trust_eroding,
        signed_isize(
            input.candidate.pcrp_trust_eroding,
            input.baseline.pcrp_trust_eroding
        )
    ));

    s.push_str("\n### Stability (seed 137 vs seed 42)\n\n");
    s.push_str(&format!(
        "- segmentation: {:.2}% (floor 80%)\n",
        input.candidate_stability.segmentation_agreement_pct
    ));
    s.push_str(&format!(
        "- category: {:.2}% (floor 80%)\n",
        input.candidate_stability.category_agreement_pct
    ));
    s.push_str(&format!(
        "- entry_type: {:.2}% (floor 80%)\n",
        input.candidate_stability.entry_type_agreement_pct
    ));
    s.push_str(&format!(
        "- date: {:.2}% (observational)\n",
        input.candidate_stability.date_agreement_pct
    ));
    s.push_str(&format!(
        "- tag_set_exact: {:.2}% (observational)\n",
        input.candidate_stability.tag_set_exact_agreement_pct
    ));

    if let Some(reasons) = reasons_opt {
        s.push_str("\n### Rejection reasons\n\n");
        for r in reasons {
            s.push_str(&format!("- {r}\n"));
        }
    }

    s.push('\n');
    s
}

fn signed_isize(cand: usize, base: usize) -> String {
    // Compare as i64 to allow negative deltas without underflow.
    let d = cand as i64 - base as i64;
    if d == 0 {
        "0".to_string()
    } else {
        format!("{d:+}")
    }
}

// ─── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn baseline_iter0() -> IapMetrics {
        // run-a-baseline numbers from STATUS.md Wave 3 sealed matrix.
        IapMetrics {
            invented_dates_count: 0,
            segmentation_correct_pct: 86.666_666_666_666_67,
            category_correct_pct: 67.272_727_272_727_27,
            entry_type_correct_pct: 78.181_818_181_818_19,
            tag_collapse_correct_pct: 9.090_909_090_909_092,
            clean_single_item_correct_pct: 6.666_666_666_666_67,
            junk_correct_pct: 100.0,
            pcrp_trust_eroding: 8,
        }
    }

    fn perfect_stability() -> IapStability {
        IapStability {
            segmentation_agreement_pct: 96.9,
            category_agreement_pct: 96.9,
            entry_type_agreement_pct: 98.5,
            date_agreement_pct: 100.0,
            tag_set_exact_agreement_pct: 83.1,
        }
    }

    #[test]
    fn weighted_score_baseline_is_around_9_9() {
        let m = baseline_iter0();
        let w = weighted_score(&m);
        // 4.0 (hard) + 1.734 (seg) + 1.345 (cat) + 1.564 (type) +
        // 0.182 (tag) + 0.067 (clean) + 1.0 (junk) ≈ 9.892
        assert!(
            (w - 9.892).abs() < 0.01,
            "weighted_score baseline drifted: {w}"
        );
    }

    #[test]
    fn hard_gate_breach_zeros_its_4_weight() {
        let mut m = baseline_iter0();
        m.invented_dates_count = 1;
        let w = weighted_score(&m);
        // Should be ~4.0 lower than the clean baseline.
        let w_clean = weighted_score(&baseline_iter0());
        assert!(
            (w_clean - w - 4.0).abs() < 0.01,
            "hard gate did not zero its 4-weight: clean={w_clean} breached={w}"
        );
    }

    #[test]
    fn identical_candidate_accepts() {
        let baseline = baseline_iter0();
        let input = IapInput {
            iteration: 1,
            label: "noop".into(),
            baseline,
            candidate: baseline,
            candidate_stability: perfect_stability(),
        };
        let v = evaluate(&input);
        assert!(matches!(v, IapVerdict::Accept { .. }), "got {v:?}");
    }

    #[test]
    fn category_regression_rejects_via_rule_2() {
        let baseline = baseline_iter0();
        let mut candidate = baseline;
        candidate.category_correct_pct = 66.0; // 1.3pt drop
                                               // Also bump segmentation enough to keep the aggregate even, so
                                               // rule 1 doesn't also fire — we want to prove rule 2 stands alone.
        candidate.segmentation_correct_pct = 95.0;
        let input = IapInput {
            iteration: 1,
            label: "regress-category".into(),
            baseline,
            candidate,
            candidate_stability: perfect_stability(),
        };
        match evaluate(&input) {
            IapVerdict::Reject { reasons, .. } => {
                assert!(
                    reasons
                        .iter()
                        .any(|r| r.contains("rule-2 category_correct")),
                    "reasons did not mention category regression: {reasons:?}"
                );
            }
            v => panic!("expected Reject, got {v:?}"),
        }
    }

    #[test]
    fn hard_gate_breach_rejects_via_rule_3() {
        let baseline = baseline_iter0();
        let mut candidate = baseline;
        candidate.invented_dates_count = 1;
        candidate.tag_collapse_correct_pct = 100.0; // try to mask via aggregate
        let input = IapInput {
            iteration: 1,
            label: "invent-dates".into(),
            baseline,
            candidate,
            candidate_stability: perfect_stability(),
        };
        match evaluate(&input) {
            IapVerdict::Reject { reasons, .. } => {
                assert!(
                    reasons.iter().any(|r| r.contains("rule-3 hard-gate")),
                    "reasons did not mention hard gate: {reasons:?}"
                );
            }
            v => panic!("expected Reject, got {v:?}"),
        }
    }

    #[test]
    fn stability_below_80_rejects_via_rule_4() {
        let baseline = baseline_iter0();
        let candidate = baseline; // strictly equal
        let mut stab = perfect_stability();
        stab.category_agreement_pct = 75.0;
        let input = IapInput {
            iteration: 1,
            label: "unstable-category".into(),
            baseline,
            candidate,
            candidate_stability: stab,
        };
        match evaluate(&input) {
            IapVerdict::Reject { reasons, .. } => {
                assert!(
                    reasons
                        .iter()
                        .any(|r| r.contains("rule-4 category_agreement")),
                    "reasons did not mention stability: {reasons:?}"
                );
            }
            v => panic!("expected Reject, got {v:?}"),
        }
    }

    #[test]
    fn pcrp_increase_rejects_via_rule_5() {
        let baseline = baseline_iter0();
        let mut candidate = baseline;
        candidate.pcrp_trust_eroding = 9; // baseline is 8
        let input = IapInput {
            iteration: 1,
            label: "pcrp-regress".into(),
            baseline,
            candidate,
            candidate_stability: perfect_stability(),
        };
        match evaluate(&input) {
            IapVerdict::Reject { reasons, .. } => {
                assert!(
                    reasons.iter().any(|r| r.contains("rule-5 PCRP")),
                    "reasons did not mention PCRP: {reasons:?}"
                );
            }
            v => panic!("expected Reject, got {v:?}"),
        }
    }

    #[test]
    fn improvement_across_the_board_accepts_with_positive_delta() {
        let baseline = baseline_iter0();
        let mut candidate = baseline;
        candidate.clean_single_item_correct_pct = 80.0;
        candidate.category_correct_pct = 90.0;
        candidate.entry_type_correct_pct = 88.0;
        candidate.tag_collapse_correct_pct = 50.0;
        candidate.segmentation_correct_pct = 90.0;
        candidate.pcrp_trust_eroding = 4;
        let input = IapInput {
            iteration: 2,
            label: "everything-up".into(),
            baseline,
            candidate,
            candidate_stability: perfect_stability(),
        };
        match evaluate(&input) {
            IapVerdict::Accept {
                delta,
                weighted_score,
                ..
            } => {
                assert!(delta > 0.0, "delta not positive: {delta}");
                assert!(
                    weighted_score > 10.0,
                    "weighted not above 10: {weighted_score}"
                );
            }
            v => panic!("expected Accept, got {v:?}"),
        }
    }

    #[test]
    fn rejection_collects_all_failing_rules() {
        // Construct a candidate that fails rules 1, 2, 3, 4, 5 at once.
        let baseline = baseline_iter0();
        let mut candidate = baseline;
        candidate.invented_dates_count = 1; // rule 3
        candidate.category_correct_pct = 30.0; // rule 2 + drags aggregate (rule 1)
        candidate.pcrp_trust_eroding = 20; // rule 5
        let mut stab = perfect_stability();
        stab.segmentation_agreement_pct = 50.0; // rule 4
        let input = IapInput {
            iteration: 3,
            label: "everything-down".into(),
            baseline,
            candidate,
            candidate_stability: stab,
        };
        match evaluate(&input) {
            IapVerdict::Reject { reasons, .. } => {
                let joined = reasons.join(" | ");
                assert!(joined.contains("rule-1"), "missing rule-1: {joined}");
                assert!(joined.contains("rule-2"), "missing rule-2: {joined}");
                assert!(joined.contains("rule-3"), "missing rule-3: {joined}");
                assert!(joined.contains("rule-4"), "missing rule-4: {joined}");
                assert!(joined.contains("rule-5"), "missing rule-5: {joined}");
            }
            v => panic!("expected Reject, got {v:?}"),
        }
    }

    #[test]
    fn journal_entry_renders_for_accept() {
        let baseline = baseline_iter0();
        let mut candidate = baseline;
        candidate.clean_single_item_correct_pct = 80.0;
        let input = IapInput {
            iteration: 1,
            label: "segmenter when-in-doubt".into(),
            baseline,
            candidate,
            candidate_stability: perfect_stability(),
        };
        let v = evaluate(&input);
        let md = render_journal_entry(&input, &v);
        assert!(md.contains("## Iter 1"));
        assert!(md.contains("segmenter when-in-doubt"));
        assert!(md.contains("ACCEPT"));
        // Δ row format.
        assert!(md.contains("+73.33") || md.contains("+73.34"));
    }

    #[test]
    fn journal_entry_renders_for_reject_with_reasons() {
        let baseline = baseline_iter0();
        let mut candidate = baseline;
        candidate.invented_dates_count = 2;
        let input = IapInput {
            iteration: 4,
            label: "broke-the-gate".into(),
            baseline,
            candidate,
            candidate_stability: perfect_stability(),
        };
        let v = evaluate(&input);
        let md = render_journal_entry(&input, &v);
        assert!(md.contains("## Iter 4"));
        assert!(md.contains("REJECT"));
        assert!(md.contains("rule-3 hard-gate"));
    }

    #[test]
    fn signed_isize_handles_negative_delta() {
        assert_eq!(signed_isize(3, 8), "-5");
        assert_eq!(signed_isize(8, 3), "+5");
        assert_eq!(signed_isize(5, 5), "0");
    }
}
