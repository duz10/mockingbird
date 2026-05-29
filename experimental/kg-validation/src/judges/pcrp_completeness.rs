//! Judge 6: PCRP-completeness — `PERSONA_REVIEW.md` exists in the
//! final-iteration run dir AND (trust_eroding_failures_count ≤ 5
//! OR scores exceed thresholds by > 5pts).
//!
//! The OR-clause encodes the §G6 escape valve: if the actual numbers
//! are well above the spec floors, qualitative trust-erosion concerns
//! are downweighted (we have margin). This judge mirrors the
//! `pcrp::trust_eroding_failures_count` exposed in the standard
//! Phase 0 PCRP output.

use std::path::Path;

use serde::Deserialize;

use super::{JudgeContext, JudgeVerdict};

const NAME: &str = "pcrp_completeness_and_trust";
const TRUST_FLOOR: usize = 5;
const SCORE_MARGIN_PCT: f64 = 5.0;

#[derive(Deserialize)]
struct ScoreFile {
    per_metric: PerMetric,
}

#[derive(Deserialize)]
struct PerMetric {
    segmentation_correct: Ratio,
    category_correct: Ratio,
    entry_type_correct: Ratio,
    tag_variant_collapse_correct: Ratio,
}

#[derive(Deserialize, Clone, Copy)]
struct Ratio {
    percentage: f64,
    denominator: usize,
}

pub fn judge(ctx: &JudgeContext) -> JudgeVerdict {
    let final_dir = match ctx.final_run_dir.as_ref() {
        Some(d) => d,
        None => {
            return JudgeVerdict::fail(NAME, "no final_run_dir supplied");
        }
    };
    let pcrp_path = final_dir.join("PERSONA_REVIEW.md");
    let score_path = final_dir.join("SCORE.json");
    let mut details: Vec<String> = Vec::new();

    if !pcrp_path.exists() {
        return JudgeVerdict::fail(
            NAME,
            format!(
                "PERSONA_REVIEW.md missing at {} (final-iteration runs MUST include PCRP)",
                pcrp_path.display()
            ),
        );
    }
    let md = match std::fs::read_to_string(&pcrp_path) {
        Ok(s) => s,
        Err(e) => return JudgeVerdict::fail(NAME, format!("read PERSONA_REVIEW.md: {e}")),
    };

    // Parse trust-eroding count from the canonical line emitted by
    // `persona_review::render_markdown`. Tolerate "0", "1", "12", etc.
    let trust_eroding = parse_trust_eroding(&md);
    details.push(format!(
        "PERSONA_REVIEW.md present ({} bytes); trust_eroding_failures_count = {}",
        md.len(),
        trust_eroding
            .map(|n| n.to_string())
            .unwrap_or_else(|| "(unparseable)".to_string())
    ));

    let score = match read_score(&score_path) {
        Ok(s) => s,
        Err(e) => {
            return JudgeVerdict::fail(
                NAME,
                format!("PCRP present but SCORE.json read failed: {e}"),
            )
            .with_details(details)
        }
    };

    let margin_clauses = [
        (
            "segmentation_correct",
            score.per_metric.segmentation_correct,
            85.0,
        ),
        ("category_correct", score.per_metric.category_correct, 90.0),
        (
            "entry_type_correct",
            score.per_metric.entry_type_correct,
            85.0,
        ),
        (
            "tag_variant_collapse_correct",
            score.per_metric.tag_variant_collapse_correct,
            80.0,
        ),
    ];
    let mut exceeded_by_margin: Vec<String> = Vec::new();
    for (name, r, floor) in &margin_clauses {
        if r.denominator == 0 {
            continue;
        }
        if r.percentage >= floor + SCORE_MARGIN_PCT {
            exceeded_by_margin.push((*name).to_string());
        }
    }
    if !exceeded_by_margin.is_empty() {
        details.push(format!(
            "metrics exceeding spec floor by > {SCORE_MARGIN_PCT:.0}pts: {}",
            exceeded_by_margin.join(", ")
        ));
    } else {
        details.push(format!(
            "no metrics exceed spec floor by > {SCORE_MARGIN_PCT:.0}pts"
        ));
    }

    let trust_ok = matches!(trust_eroding, Some(n) if n <= TRUST_FLOOR);
    let margin_ok = !exceeded_by_margin.is_empty();
    if trust_ok || margin_ok {
        JudgeVerdict::pass(
            NAME,
            format!(
                "PCRP present + (trust_eroding ≤ {TRUST_FLOOR}: {}) OR (any metric > floor+{SCORE_MARGIN_PCT:.0}pts: {})",
                trust_ok, margin_ok
            ),
        )
        .with_details(details)
    } else {
        JudgeVerdict::fail(
            NAME,
            format!(
                "PCRP present but trust_eroding > {TRUST_FLOOR} AND no metric exceeds floor by > {SCORE_MARGIN_PCT:.0}pts (§G6 NO-GO default)"
            ),
        )
        .with_details(details)
    }
}

fn parse_trust_eroding(md: &str) -> Option<usize> {
    // Emit shapes we tolerate, including the canonical Markdown bullet
    // form produced by `persona_review::render_markdown`:
    //   - "- trust_eroding_failures_count: **8**"  (canonical)
    //   - "trust_eroding_failures_count: 8"
    //   - "Trust-eroding failures: 8"
    //   - "trust_eroding=8" (debug print fallback)
    //
    // We strip leading Markdown-list noise (spaces / `-` / `*`) before
    // testing prefixes so the bullet form isn't blocked. `**bold**` and
    // similar inline emphasis fall out naturally — `number_after_colon_
    // or_equals` skips non-digits before locking onto the integer.
    for line in md.lines() {
        let l = line.to_lowercase();
        let stripped = l.trim_start_matches(|c: char| c.is_whitespace() || c == '-' || c == '*');
        if let Some(rest) = stripped.strip_prefix("trust_eroding_failures_count") {
            return number_after_colon_or_equals(rest);
        }
        if stripped.starts_with("trust-eroding") || stripped.starts_with("trust eroding") {
            return number_after_colon_or_equals(stripped);
        }
        if let Some(idx) = l.find("trust_eroding=") {
            return number_after_colon_or_equals(&l[idx + "trust_eroding".len()..]);
        }
    }
    None
}

fn number_after_colon_or_equals(s: &str) -> Option<usize> {
    let cleaned: String = s
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    cleaned.parse().ok()
}

fn read_score(path: &Path) -> anyhow::Result<ScoreFile> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("read {}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| anyhow::anyhow!("parse {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmp() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "kg-judge-pcrp-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn write_pcrp(dir: &Path, trust_eroding: usize) {
        std::fs::create_dir_all(dir).unwrap();
        let body = format!(
            "# PERSONA_REVIEW\n\nReviewer: llama3.1:8b\n\ntrust_eroding_failures_count: {trust_eroding}\ntrust_building_wins_count: 9\n\n## Findings\n\n- example\n"
        );
        std::fs::write(dir.join("PERSONA_REVIEW.md"), body).unwrap();
    }

    fn write_score(dir: &Path, cat_pct: f64) {
        // Other metrics set just BELOW their floor+5 margin so the
        // margin-clause check depends solely on `cat_pct`.
        std::fs::create_dir_all(dir).unwrap();
        let body = serde_json::json!({
            "per_metric": {
                "segmentation_correct":      { "percentage": 89.0, "denominator": 15 },
                "category_correct":          { "percentage": cat_pct, "denominator": 55 },
                "entry_type_correct":        { "percentage": 89.0, "denominator": 55 },
                "tag_variant_collapse_correct": { "percentage": 84.0, "denominator": 55 },
            }
        });
        std::fs::write(dir.join("SCORE.json"), body.to_string()).unwrap();
    }

    fn ctx_for(d: PathBuf) -> JudgeContext {
        JudgeContext {
            run_dirs: vec![d.clone()],
            final_run_dir: Some(d),
            repo_root: PathBuf::from("."),
            baseline_ref: String::new(),
            allowed_path_prefixes: Vec::new(),
            determinism: None,
        }
    }

    #[test]
    fn passes_when_trust_eroding_within_floor() {
        let root = tmp();
        let d = root.join("final");
        write_pcrp(&d, 3);
        write_score(&d, 85.0); // below 90+5 margin, but trust_eroding ok
        let v = judge(&ctx_for(d));
        assert!(v.passed, "{v:?}");
    }

    #[test]
    fn passes_when_metric_exceeds_margin_even_with_high_trust_eroding() {
        let root = tmp();
        let d = root.join("final");
        write_pcrp(&d, 12); // above floor
        write_score(&d, 96.0); // above 90 + 5 margin
        let v = judge(&ctx_for(d));
        assert!(v.passed, "should pass via OR clause: {v:?}");
    }

    #[test]
    fn fails_when_high_trust_eroding_and_no_margin() {
        let root = tmp();
        let d = root.join("final");
        write_pcrp(&d, 12);
        write_score(&d, 92.0); // within 90 + 5 margin (not above)
        let v = judge(&ctx_for(d));
        assert!(!v.passed);
        assert!(v.reasoning.contains("NO-GO"));
    }

    #[test]
    fn fails_when_pcrp_missing() {
        let root = tmp();
        let d = root.join("final");
        write_score(&d, 96.0);
        let v = judge(&ctx_for(d));
        assert!(!v.passed);
    }

    #[test]
    fn parses_alternate_label_forms() {
        assert_eq!(parse_trust_eroding("Trust-eroding failures: 7"), Some(7));
        assert_eq!(
            parse_trust_eroding("trust_eroding_failures_count = 0"),
            Some(0)
        );
    }

    #[test]
    fn parses_canonical_render_markdown_bullet_with_bold() {
        // Verbatim shape of what `persona_review::render_markdown` writes.
        let md = "# PERSONA_REVIEW\n\n- trust_eroding_failures_count: **8**\n- trust_building_wins_count: **9**\n";
        assert_eq!(parse_trust_eroding(md), Some(8));
    }
}
