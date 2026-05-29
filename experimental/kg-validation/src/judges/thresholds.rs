//! Judge 2: Thresholds — per-metric pass/fail vs. spec §8.4.
//!
//! Reads each run's `SCORE.json` and asserts:
//!
//! | Metric | Threshold |
//! |---|---|
//! | invented_dates_count | == 0 |
//! | clean_single_item_correct | ~ 100% |
//! | segmentation_correct (multi-item) | ≥ 85% |
//! | category_correct | ≥ 90% |
//! | entry_type_correct | ≥ 85% |
//! | tag_variant_collapse_correct (G7) | ≥ 80% |
//! | junk_correct | ~ 100% |
//!
//! Numeric thresholds are encoded once in [`THRESHOLDS`] so they
//! stay in sync across the verdict surface.

use std::path::Path;

use serde::Deserialize;

use super::{JudgeContext, JudgeVerdict};

const NAME: &str = "thresholds_match_spec_8_4";

#[derive(Deserialize)]
struct ScoreFile {
    run_id: String,
    per_metric: PerMetric,
}

#[derive(Deserialize)]
struct PerMetric {
    clean_single_item_correct: Ratio,
    segmentation_correct: Ratio,
    category_correct: Ratio,
    entry_type_correct: Ratio,
    invented_dates_count: usize,
    tag_variant_collapse_correct: Ratio,
    junk_correct: Ratio,
}

#[derive(Deserialize, Clone, Copy)]
struct Ratio {
    numerator: usize,
    denominator: usize,
    percentage: f64,
}

#[derive(Debug, Clone, Copy)]
struct Threshold {
    name: &'static str,
    pct: f64,
}

const THRESHOLDS: [Threshold; 5] = [
    Threshold {
        name: "segmentation_correct",
        pct: 85.0,
    },
    Threshold {
        name: "category_correct",
        pct: 90.0,
    },
    Threshold {
        name: "entry_type_correct",
        pct: 85.0,
    },
    Threshold {
        name: "tag_variant_collapse_correct",
        pct: 80.0,
    },
    Threshold {
        name: "junk_correct",
        pct: 100.0,
    },
];

pub fn judge(ctx: &JudgeContext) -> JudgeVerdict {
    if ctx.run_dirs.is_empty() {
        return JudgeVerdict::fail(NAME, "no run directories supplied");
    }

    let mut details: Vec<String> = Vec::new();
    let mut any_fail = false;

    for dir in &ctx.run_dirs {
        let score_path = dir.join("SCORE.json");
        let score = match read_score(&score_path) {
            Ok(s) => s,
            Err(e) => {
                any_fail = true;
                details.push(format!("{}: {e}", dir.display()));
                continue;
            }
        };
        details.push(format!("=== {} ===", score.run_id));

        // invented-dates hard gate (mirrored here for self-containment;
        // the hard_gate judge is the canonical assertion).
        if score.per_metric.invented_dates_count == 0 {
            details.push("  invented_dates_count == 0  PASS".into());
        } else {
            any_fail = true;
            details.push(format!(
                "  invented_dates_count == {}  FAIL (threshold 0)",
                score.per_metric.invented_dates_count
            ));
        }

        // Per-metric floors.
        for t in &THRESHOLDS {
            let r = match t.name {
                "segmentation_correct" => score.per_metric.segmentation_correct,
                "category_correct" => score.per_metric.category_correct,
                "entry_type_correct" => score.per_metric.entry_type_correct,
                "tag_variant_collapse_correct" => score.per_metric.tag_variant_collapse_correct,
                "junk_correct" => score.per_metric.junk_correct,
                _ => unreachable!(),
            };
            if r.denominator == 0 {
                details.push(format!("  {} : -/- (denominator 0, skipped)", t.name));
                continue;
            }
            let passed = r.percentage + 1e-9 >= t.pct;
            if !passed {
                any_fail = true;
            }
            details.push(format!(
                "  {} : {:.1}% ({}/{})  threshold {:.0}%  {}",
                t.name,
                r.percentage,
                r.numerator,
                r.denominator,
                t.pct,
                if passed { "PASS" } else { "FAIL" }
            ));
        }

        // Clean-single floor — display only (the v1 charter floor is
        // ~100%, but we surface it as observational because the §8.4
        // spec language is "approximately 100%" rather than hard).
        let cs = score.per_metric.clean_single_item_correct;
        if cs.denominator == 0 {
            details.push("  clean_single_item_correct : -/- (denominator 0)".into());
        } else {
            let passed = (cs.percentage - 100.0).abs() < 1e-9;
            if !passed {
                any_fail = true;
            }
            details.push(format!(
                "  clean_single_item_correct : {:.1}% ({}/{})  threshold ~100%  {}",
                cs.percentage,
                cs.numerator,
                cs.denominator,
                if passed { "PASS" } else { "FAIL" }
            ));
        }
    }

    if !any_fail {
        JudgeVerdict::pass(NAME, "all metrics meet or exceed spec §8.4 thresholds")
            .with_details(details)
    } else {
        JudgeVerdict::fail(
            NAME,
            "at least one metric fails its spec §8.4 threshold (see details)",
        )
        .with_details(details)
    }
}

fn read_score(path: &Path) -> anyhow::Result<ScoreFile> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("read {}: {e}", path.display()))?;
    let s: ScoreFile = serde_json::from_str(&text)
        .map_err(|e| anyhow::anyhow!("parse {}: {e}", path.display()))?;
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[allow(clippy::too_many_arguments)]
    fn write_score(
        dir: &Path,
        run_id: &str,
        cs: (usize, usize, f64),
        seg: (usize, usize, f64),
        cat: (usize, usize, f64),
        et: (usize, usize, f64),
        invented: usize,
        tag: (usize, usize, f64),
        junk: (usize, usize, f64),
    ) {
        std::fs::create_dir_all(dir).unwrap();
        let body = serde_json::json!({
            "run_id": run_id,
            "per_metric": {
                "clean_single_item_correct": { "numerator": cs.0, "denominator": cs.1, "percentage": cs.2 },
                "segmentation_correct":      { "numerator": seg.0, "denominator": seg.1, "percentage": seg.2 },
                "category_correct":          { "numerator": cat.0, "denominator": cat.1, "percentage": cat.2 },
                "entry_type_correct":        { "numerator": et.0, "denominator": et.1, "percentage": et.2 },
                "invented_dates_count": invented,
                "tag_variant_collapse_correct": { "numerator": tag.0, "denominator": tag.1, "percentage": tag.2 },
                "junk_correct":              { "numerator": junk.0, "denominator": junk.1, "percentage": junk.2 },
            }
        });
        std::fs::write(dir.join("SCORE.json"), body.to_string()).unwrap();
    }

    fn tmp() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "kg-judge-thresh-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn ctx(dirs: Vec<PathBuf>) -> JudgeContext {
        JudgeContext {
            run_dirs: dirs,
            final_run_dir: None,
            repo_root: PathBuf::from("."),
            baseline_ref: String::new(),
            allowed_path_prefixes: Vec::new(),
            determinism: None,
        }
    }

    #[test]
    fn passes_when_all_thresholds_met() {
        let root = tmp();
        let a = root.join("run-a");
        write_score(
            &a,
            "run-a",
            (15, 15, 100.0),
            (13, 15, 86.7),
            (50, 55, 90.9),
            (47, 55, 85.5),
            0,
            (44, 55, 80.0),
            (2, 2, 100.0),
        );
        let v = judge(&ctx(vec![a]));
        assert!(v.passed, "should pass: {v:?}");
    }

    #[test]
    fn fails_on_category_under_threshold() {
        let root = tmp();
        let a = root.join("run-a");
        write_score(
            &a,
            "run-a",
            (15, 15, 100.0),
            (13, 15, 86.7),
            (37, 55, 67.3), // <-- below 90%
            (47, 55, 85.5),
            0,
            (44, 55, 80.0),
            (2, 2, 100.0),
        );
        let v = judge(&ctx(vec![a]));
        assert!(!v.passed);
        assert!(v
            .details
            .iter()
            .any(|d| d.contains("category_correct") && d.contains("FAIL")));
    }

    #[test]
    fn fails_on_invented_dates_nonzero() {
        let root = tmp();
        let a = root.join("run-a");
        write_score(
            &a,
            "run-a",
            (15, 15, 100.0),
            (13, 15, 86.7),
            (50, 55, 90.9),
            (47, 55, 85.5),
            3, // <-- nonzero
            (44, 55, 80.0),
            (2, 2, 100.0),
        );
        let v = judge(&ctx(vec![a]));
        assert!(!v.passed);
    }

    #[test]
    fn passes_at_exact_threshold() {
        // Boundary: tag-collapse at exactly 80.0% should PASS.
        let root = tmp();
        let a = root.join("run-a");
        write_score(
            &a,
            "run-a",
            (15, 15, 100.0),
            (13, 15, 86.7),
            (50, 55, 90.9),
            (47, 55, 85.5),
            0,
            (44, 55, 80.0),
            (2, 2, 100.0),
        );
        let v = judge(&ctx(vec![a]));
        assert!(v.passed);
    }
}
