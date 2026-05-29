//! Judge 3: Stability — `SCORE.json::stability` agreement ≥ 80% on
//! every structural dimension. Date agreement is expected at 100%
//! (deterministic hard-gate output). Tag-set exact agreement is
//! reported but NOT gated (the synonym map deliberately doesn't
//! collapse everything; open vocab is the bug, not the metric).
//!
//! Spec §8.5 reference.

use std::path::Path;

use serde::Deserialize;

use super::{JudgeContext, JudgeVerdict};

const NAME: &str = "stability_meets_spec_8_5";
const STRUCTURAL_FLOOR_PCT: f64 = 80.0;
const DATE_FLOOR_PCT: f64 = 100.0;

#[derive(Deserialize)]
struct ScoreFile {
    run_id: String,
    #[serde(default)]
    stability: Option<Stability>,
}

#[derive(Deserialize)]
struct Stability {
    vs_run_id: String,
    segmentation_agreement: Ratio,
    category_agreement: Ratio,
    entry_type_agreement: Ratio,
    date_agreement: Ratio,
    tag_set_exact_agreement: Ratio,
    total_compared_dictations: usize,
    total_compared_entries: usize,
}

#[derive(Deserialize)]
struct Ratio {
    numerator: usize,
    denominator: usize,
    percentage: f64,
}

pub fn judge(ctx: &JudgeContext) -> JudgeVerdict {
    // Pick the first run dir that contains a `stability` block.
    let mut found_score: Option<(String, Stability)> = None;
    let mut tried: Vec<String> = Vec::new();
    for dir in &ctx.run_dirs {
        let p = dir.join("SCORE.json");
        tried.push(p.display().to_string());
        if let Ok(s) = read_score(&p) {
            if let Some(stab) = s.stability {
                found_score = Some((s.run_id, stab));
                break;
            }
        }
    }

    let (run_id, stab) = match found_score {
        Some(x) => x,
        None => {
            return JudgeVerdict::fail(
                NAME,
                format!(
                    "no SCORE.json with a stability block found among {} candidate(s)",
                    tried.len()
                ),
            )
            .with_details(tried)
        }
    };

    let mut details = vec![
        format!("comparing run {} vs {}", run_id, stab.vs_run_id),
        format!(
            "  dictations compared: {}, entries compared: {}",
            stab.total_compared_dictations, stab.total_compared_entries
        ),
    ];

    let mut any_fail = false;
    for (name, r, floor) in [
        (
            "segmentation_agreement",
            stab.segmentation_agreement,
            STRUCTURAL_FLOOR_PCT,
        ),
        (
            "category_agreement",
            stab.category_agreement,
            STRUCTURAL_FLOOR_PCT,
        ),
        (
            "entry_type_agreement",
            stab.entry_type_agreement,
            STRUCTURAL_FLOOR_PCT,
        ),
        ("date_agreement", stab.date_agreement, DATE_FLOOR_PCT),
    ] {
        let passed = if r.denominator == 0 {
            // No entries to compare: vacuously stable.
            true
        } else {
            r.percentage + 1e-9 >= floor
        };
        if !passed {
            any_fail = true;
        }
        details.push(format!(
            "  {} : {:.1}% ({}/{})  floor {:.0}%  {}",
            name,
            r.percentage,
            r.numerator,
            r.denominator,
            floor,
            if passed { "PASS" } else { "FAIL" }
        ));
    }
    details.push(format!(
        "  tag_set_exact_agreement : {:.1}% ({}/{})  REPORTED (not gated; open vocab per ADR 0048 §G7)",
        stab.tag_set_exact_agreement.percentage,
        stab.tag_set_exact_agreement.numerator,
        stab.tag_set_exact_agreement.denominator,
    ));

    if any_fail {
        JudgeVerdict::fail(NAME, "at least one structural agreement metric below floor")
            .with_details(details)
    } else {
        JudgeVerdict::pass(NAME, "structural stability agreement meets every floor")
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

    fn write_with_stability(dir: &Path, seg: f64, cat: f64, et: f64, date: f64, tag: f64) {
        std::fs::create_dir_all(dir).unwrap();
        let body = serde_json::json!({
            "run_id": "run-b",
            "stability_vs": "run-a",
            "stability": {
                "vs_run_id": "run-a",
                "segmentation_agreement": { "numerator": 1, "denominator": 1, "percentage": seg },
                "category_agreement":     { "numerator": 1, "denominator": 1, "percentage": cat },
                "entry_type_agreement":   { "numerator": 1, "denominator": 1, "percentage": et },
                "date_agreement":         { "numerator": 1, "denominator": 1, "percentage": date },
                "tag_set_exact_agreement":{ "numerator": 1, "denominator": 1, "percentage": tag },
                "total_compared_dictations": 32,
                "total_compared_entries": 65
            }
        });
        std::fs::write(dir.join("SCORE.json"), body.to_string()).unwrap();
    }

    fn write_no_stability(dir: &Path) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(
            dir.join("SCORE.json"),
            r#"{"run_id":"run-a","per_metric":{}}"#,
        )
        .unwrap();
    }

    fn tmp() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "kg-judge-stab-{}-{}",
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
    fn passes_at_real_wave3_numbers() {
        // Wave 3.4 stability: 96.9 / 96.9 / 98.5 / 100 / 83.1.
        let root = tmp();
        let b = root.join("run-b");
        write_with_stability(&b, 96.9, 96.9, 98.5, 100.0, 83.1);
        let v = judge(&ctx(vec![b]));
        assert!(v.passed, "{v:?}");
    }

    #[test]
    fn fails_on_low_category_agreement() {
        let root = tmp();
        let b = root.join("run-b");
        write_with_stability(&b, 90.0, 60.0, 90.0, 100.0, 90.0);
        let v = judge(&ctx(vec![b]));
        assert!(!v.passed);
    }

    #[test]
    fn fails_on_date_below_100() {
        // Date is the hard-gate dimension; anything < 100 means the
        // pipeline produced a different date across runs.
        let root = tmp();
        let b = root.join("run-b");
        write_with_stability(&b, 90.0, 90.0, 90.0, 98.0, 90.0);
        let v = judge(&ctx(vec![b]));
        assert!(!v.passed);
    }

    #[test]
    fn tag_set_exact_below_80_does_not_fail() {
        // Per ADR 0048 §G7: open vocab; tag-set exact is reported but
        // not gated. 50% should still pass if everything else holds.
        let root = tmp();
        let b = root.join("run-b");
        write_with_stability(&b, 90.0, 90.0, 90.0, 100.0, 50.0);
        let v = judge(&ctx(vec![b]));
        assert!(v.passed, "tag-set exact is not gated: {v:?}");
    }

    #[test]
    fn fails_when_no_stability_block_found() {
        let root = tmp();
        let a = root.join("run-a");
        write_no_stability(&a);
        let v = judge(&ctx(vec![a]));
        assert!(!v.passed);
    }
}
