//! `score-run` — Phase 0 scoring CLI.
//!
//! Reads a `runs/<run-id>/` directory (produced by `run-corpus`),
//! scores it against the corpus answer keys per ADR 0048 §G7
//! (deterministic tag-collapse via synonym map; no LLM judge),
//! runs the Persona Cross-Reference Pass, and writes:
//!
//! - `runs/<run-id>/SCORE.json` — full [`ScoreReport`]
//! - `runs/<run-id>/PERSONA_REVIEW.md` — PCRP output
//! - `runs/<run-id>/SCORE_SUMMARY.md` — human-readable per-metric
//!   table with pass/fail vs. spec §8.4 + top-10 tag near-misses
//!
//! The Judge Validation Protocol (`JUDGE_VALIDATION.json`) is NOT
//! produced — tag-collapse no longer goes through an LLM judge, so
//! there is nothing to validate. The JVP machinery in
//! `src/scoring/judge_validation.rs` is preserved for any future
//! LLM-judged metric.
//!
//! Hand-rolled flag parsing, same YAGNI logic as `run-corpus`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use kg_validation::ollama::{GenerateOptions, OllamaClient};
use kg_validation::schema::AnswerKey;
use kg_validation::scoring::metrics::{compute_stability, score_run, ScoreReport};
use kg_validation::scoring::persona_review::{
    load_persona_notes, render_markdown, run_pcrp, select_samples, PcrpConfig,
};
use kg_validation::scoring::tag_collapse::{SynonymMap, TagCollapseScore};

const HELP: &str = "\
score-run - Phase 0 scoring CLI for the Mockingbird Knowledge Graph.

USAGE:
  score-run --run-dir <path> [FLAGS]

REQUIRED:
  --run-dir <path>                 runs/<run-id>/ to score

OPTIONAL:
  --corpus-dir <path>              default ./corpus
  --synonym-map <path>             default judge-calibration/synonym-map.json
                                   (ADR 0048 G7; deterministic tag-collapse)
  --persona-review-model <name>    default llama3.1:8b-instruct-q4_K_M
  --seed <int>                     default 42 (PCRP determinism)
  --ollama-url <url>               default http://localhost:11434
  --stability-vs <run-id>          optional sibling run for spec 8.5 comparison
  --skip-pcrp                      dev only - production runs MUST NOT use
  --skip-tag-collapse              dev only - skip synonym-map scoring
                                   (e.g. when iterating structural metrics only)
  --help                           print this and exit

NOTE: Pre-G7 flags --judge-model, --cross-judge-model, --judge-seed,
--calibration-set, --skip-jvp are no longer accepted. Tag-collapse is
deterministic; JVP architecture is preserved in source for future
LLM-judged metrics but is not invoked.
";

fn main() -> ExitCode {
    match real_main() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}

struct Args {
    run_dir: PathBuf,
    corpus_dir: PathBuf,
    synonym_map_path: PathBuf,
    persona_review_model: String,
    seed: i64,
    ollama_url: String,
    stability_vs: Option<String>,
    skip_pcrp: bool,
    skip_tag_collapse: bool,
}

fn parse_args() -> anyhow::Result<Args> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut run_dir: Option<PathBuf> = None;
    let mut corpus_dir = PathBuf::from("corpus");
    let mut synonym_map_path = PathBuf::from("judge-calibration").join("synonym-map.json");
    let mut persona_review_model = "llama3.1:8b-instruct-q4_K_M".to_string();
    let mut seed: i64 = 42;
    let mut ollama_url = "http://localhost:11434".to_string();
    let mut stability_vs: Option<String> = None;
    let mut skip_pcrp = false;
    let mut skip_tag_collapse = false;

    let mut i = 0;
    while i < argv.len() {
        let arg = argv[i].as_str();
        match arg {
            "--help" | "-h" => {
                println!("{HELP}");
                std::process::exit(0);
            }
            "--skip-pcrp" => skip_pcrp = true,
            "--skip-tag-collapse" => skip_tag_collapse = true,
            "--run-dir" => run_dir = Some(PathBuf::from(take(&argv, &mut i, arg)?)),
            "--corpus-dir" => corpus_dir = PathBuf::from(take(&argv, &mut i, arg)?),
            "--synonym-map" => synonym_map_path = PathBuf::from(take(&argv, &mut i, arg)?),
            "--persona-review-model" => persona_review_model = take(&argv, &mut i, arg)?,
            "--seed" => {
                seed = take(&argv, &mut i, arg)?
                    .parse()
                    .map_err(|e| anyhow::anyhow!("--seed must be int: {e}"))?;
            }
            "--ollama-url" => ollama_url = take(&argv, &mut i, arg)?,
            "--stability-vs" => stability_vs = Some(take(&argv, &mut i, arg)?),
            // Pre-G7 flag deprecation: hard-fail with guidance so old
            // scripts don't silently get the wrong code path.
            "--judge-model"
            | "--cross-judge-model"
            | "--judge-seed"
            | "--calibration-set"
            | "--skip-jvp" => {
                anyhow::bail!(
                    "flag {arg} was removed in ADR 0048 G7. Tag-collapse is now deterministic via --synonym-map. See HELP."
                );
            }
            unknown => anyhow::bail!("unknown flag: {unknown}\n\n{HELP}"),
        }
        i += 1;
    }

    let run_dir = run_dir.ok_or_else(|| anyhow::anyhow!("--run-dir is required\n\n{HELP}"))?;
    Ok(Args {
        run_dir,
        corpus_dir,
        synonym_map_path,
        persona_review_model,
        seed,
        ollama_url,
        stability_vs,
        skip_pcrp,
        skip_tag_collapse,
    })
}

fn take(args: &[String], i: &mut usize, flag: &str) -> anyhow::Result<String> {
    *i += 1;
    args.get(*i)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("{flag} requires a value"))
}

fn real_main() -> anyhow::Result<()> {
    let args = parse_args()?;
    let structured_dir = args.run_dir.join("structured");
    let answer_keys_dir = args.corpus_dir.join("answer-keys");
    let dictations_dir = args.corpus_dir.join("dictations");

    println!("=== score-run ===");
    println!("run_dir              : {}", args.run_dir.display());
    println!("structured_dir       : {}", structured_dir.display());
    println!("answer_keys_dir      : {}", answer_keys_dir.display());
    println!("synonym_map          : {}", args.synonym_map_path.display());
    println!("persona_review_model : {}", args.persona_review_model);
    println!("seed                 : {}", args.seed);
    println!("ollama_url           : {}", args.ollama_url);
    println!("skip_pcrp            : {}", args.skip_pcrp);
    println!("skip_tag_collapse    : {}", args.skip_tag_collapse);
    println!();

    // ── 1. Load synonym map (unless --skip-tag-collapse) ─────────
    let synonym_map = if args.skip_tag_collapse {
        println!("[1/3] tag-collapse skipped (--skip-tag-collapse).");
        None
    } else {
        println!(
            "[1/3] loading synonym map from {} ...",
            args.synonym_map_path.display()
        );
        let m = SynonymMap::load(&args.synonym_map_path)?;
        println!(
            "        loaded synonym-map {} ({} variant->canonical entries)",
            m.version,
            m.variant_to_canonical.len()
        );
        Some(m)
    };

    // ── 2. Score the run ─────────────────────────────────────────
    let run_id = args
        .run_dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("run")
        .to_string();

    println!("[2/3] scoring run ...");
    let mut score = score_run(
        &run_id,
        &structured_dir,
        &answer_keys_dir,
        synonym_map.as_ref(),
    )?;

    // ── Stability (optional) ──────────────────────────────────────
    if let Some(other) = args.stability_vs.as_deref() {
        let other_structured = args
            .run_dir
            .parent()
            .ok_or_else(|| anyhow::anyhow!("--run-dir has no parent for --stability-vs"))?
            .join(other)
            .join("structured");
        println!(
            "        stability vs {other} ({}) ...",
            other_structured.display()
        );
        let s = compute_stability(&structured_dir, &other_structured, other)?;
        score.stability_vs = Some(other.to_string());
        score.stability = Some(s);
    }

    // Persist SCORE.json now so it's on disk even if a later step fails.
    let score_path = args.run_dir.join("SCORE.json");
    std::fs::write(&score_path, serde_json::to_string_pretty(&score)?)?;
    println!("        wrote {}", score_path.display());

    // ── 3. PCRP ──────────────────────────────────────────────────
    if !args.skip_pcrp {
        println!("[3/3] running PCRP ...");
        let primary = OllamaClient::with_base_url(args.ollama_url.clone());
        let pcrp_options = GenerateOptions {
            temperature: 0.2,
            seed: Some(args.seed),
            num_ctx: 4096,
        };
        let answer_keys = load_answer_keys(&answer_keys_dir)?;
        let raw_dictations = load_raw_dictations(&dictations_dir)?;
        let pipeline_outputs = load_pipeline_output_strings(&structured_dir)?;
        let samples = select_samples(&score, &answer_keys, &raw_dictations, &pipeline_outputs);
        println!(
            "        selected {} samples across {} personas",
            samples.len(),
            count_personas(&samples)
        );
        let corpus_notes_path = args.corpus_dir.join("CORPUS_NOTES.md");
        let persona_notes = if corpus_notes_path.exists() {
            load_persona_notes(&corpus_notes_path)?
        } else {
            HashMap::new()
        };
        let cfg = PcrpConfig {
            run_id: run_id.clone(),
            reviewer_model: args.persona_review_model.clone(),
            options: pcrp_options,
            persona_notes,
            samples,
        };
        let pcrp_report = run_pcrp(&primary, cfg)?;
        let md = render_markdown(&pcrp_report);
        let pcrp_path = args.run_dir.join("PERSONA_REVIEW.md");
        std::fs::write(&pcrp_path, md)?;
        println!(
            "        wrote {} (trust_eroding={}, trust_building={})",
            pcrp_path.display(),
            pcrp_report.trust_eroding_failures_count,
            pcrp_report.trust_building_wins_count
        );
    } else {
        println!("[3/3] PCRP skipped (--skip-pcrp).");
    }

    // ── 4. SCORE_SUMMARY.md ──────────────────────────────────────
    println!("writing SCORE_SUMMARY.md ...");
    let summary_md = render_score_summary(&score);
    let summary_path = args.run_dir.join("SCORE_SUMMARY.md");
    std::fs::write(&summary_path, &summary_md)?;
    println!("        wrote {}", summary_path.display());
    println!();
    println!("{summary_md}");

    Ok(())
}

fn load_answer_keys(dir: &Path) -> anyhow::Result<HashMap<String, AnswerKey>> {
    let mut out = HashMap::new();
    for e in std::fs::read_dir(dir)? {
        let p = e?.path();
        if p.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let text = std::fs::read_to_string(&p)?;
        let key: AnswerKey = serde_json::from_str(&text)
            .map_err(|e| anyhow::anyhow!("parse {}: {e}", p.display()))?;
        out.insert(key.dictation_id.clone(), key);
    }
    Ok(out)
}

fn load_raw_dictations(dir: &Path) -> anyhow::Result<HashMap<String, String>> {
    let mut out = HashMap::new();
    for e in std::fs::read_dir(dir)? {
        let p = e?.path();
        if p.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        let id = p
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| anyhow::anyhow!("bad filename: {}", p.display()))?
            .to_string();
        out.insert(id, std::fs::read_to_string(&p)?);
    }
    Ok(out)
}

fn load_pipeline_output_strings(dir: &Path) -> anyhow::Result<HashMap<String, String>> {
    let mut out = HashMap::new();
    if !dir.exists() {
        return Ok(out);
    }
    for e in std::fs::read_dir(dir)? {
        let p = e?.path();
        if p.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let id = p
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| anyhow::anyhow!("bad filename: {}", p.display()))?
            .to_string();
        out.insert(id, std::fs::read_to_string(&p)?);
    }
    Ok(out)
}

fn count_personas(samples: &[kg_validation::scoring::persona_review::PcrpSample]) -> usize {
    let mut s: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for x in samples {
        s.insert(x.persona_id.as_str());
    }
    s.len()
}

fn render_score_summary(score: &ScoreReport) -> String {
    let mut s = String::new();
    s.push_str(&format!("# SCORE_SUMMARY - `{}`\n\n", score.run_id));
    s.push_str(&format!(
        "- total dictations: **{}**\n",
        score.total_dictations
    ));
    s.push_str(&format!("- graded: **{}**\n", score.graded_dictations));
    if !score.ungradable_dictations.is_empty() {
        s.push_str(&format!(
            "- ungradable: {} ({})\n",
            score.ungradable_dictations.len(),
            score.ungradable_dictations.join(", ")
        ));
    }
    s.push_str(&format!(
        "- match algorithm: `{}`\n\n",
        score.match_algorithm
    ));

    s.push_str("## Per-metric vs. spec 8.4 thresholds\n\n");
    s.push_str("| Metric | Result | Threshold | Verdict |\n|---|---|---|---|\n");
    let m = &score.per_metric;
    s.push_str(&row(
        "Clean single-item correct",
        &m.clean_single_item_correct.percentage,
        m.clean_single_item_correct.numerator,
        m.clean_single_item_correct.denominator,
        100.0,
        false,
    ));
    s.push_str(&row(
        "Segmentation correct (multi-item)",
        &m.segmentation_correct.percentage,
        m.segmentation_correct.numerator,
        m.segmentation_correct.denominator,
        85.0,
        false,
    ));
    s.push_str(&row(
        "Category correct",
        &m.category_correct.percentage,
        m.category_correct.numerator,
        m.category_correct.denominator,
        90.0,
        false,
    ));
    s.push_str(&row(
        "Entry-type correct",
        &m.entry_type_correct.percentage,
        m.entry_type_correct.numerator,
        m.entry_type_correct.denominator,
        85.0,
        false,
    ));
    s.push_str(&format!(
        "| **Invented dates count (HARD GATE)** | {} | **0** | {} |\n",
        m.invented_dates_count,
        if m.invented_dates_count == 0 {
            "PASS"
        } else {
            "FAIL"
        }
    ));
    s.push_str(&row(
        "Tag-variant collapse correct (G7)",
        &m.tag_variant_collapse_correct.percentage,
        m.tag_variant_collapse_correct.numerator,
        m.tag_variant_collapse_correct.denominator,
        80.0,
        true,
    ));
    s.push_str(&row(
        "Junk-bucket handled correctly",
        &m.junk_correct.percentage,
        m.junk_correct.numerator,
        m.junk_correct.denominator,
        100.0,
        false,
    ));

    if let Some(tc) = score.tag_collapse.as_ref() {
        s.push_str(&render_tag_collapse_section(tc));
    }

    if let Some(stab) = score.stability.as_ref() {
        s.push_str("\n## Stability vs. ");
        s.push_str(&format!("`{}` (spec 8.5)\n\n", stab.vs_run_id));
        s.push_str(&format!(
            "- compared dictations: {} ({} entries)\n",
            stab.total_compared_dictations, stab.total_compared_entries
        ));
        s.push_str(&format!(
            "- segmentation agreement: {:.1}% ({}/{})\n",
            stab.segmentation_agreement.percentage,
            stab.segmentation_agreement.numerator,
            stab.segmentation_agreement.denominator
        ));
        s.push_str(&format!(
            "- category agreement: {:.1}% ({}/{})\n",
            stab.category_agreement.percentage,
            stab.category_agreement.numerator,
            stab.category_agreement.denominator
        ));
        s.push_str(&format!(
            "- entry-type agreement: {:.1}% ({}/{})\n",
            stab.entry_type_agreement.percentage,
            stab.entry_type_agreement.numerator,
            stab.entry_type_agreement.denominator
        ));
        s.push_str(&format!(
            "- date agreement: {:.1}% ({}/{})\n",
            stab.date_agreement.percentage,
            stab.date_agreement.numerator,
            stab.date_agreement.denominator
        ));
        s.push_str(&format!(
            "- tag-set exact agreement: {:.1}% ({}/{})\n",
            stab.tag_set_exact_agreement.percentage,
            stab.tag_set_exact_agreement.numerator,
            stab.tag_set_exact_agreement.denominator
        ));
    }
    s
}

fn render_tag_collapse_section(tc: &TagCollapseScore) -> String {
    let mut s = String::new();
    s.push_str("\n## Tag-collapse detail (ADR 0048 G7)\n\n");
    s.push_str(&format!(
        "- synonym-map version: `{}`\n",
        tc.synonym_map_version
    ));
    s.push_str(&format!("- total entries scored: {}\n", tc.total_entries));
    s.push_str("- per-threshold pass counts (observational; only 1.0 gates):\n");
    s.push_str(&format!(
        "  - Jaccard >= 1.00 (PRIMARY): {} ({:.1}%)\n",
        tc.correct_at_exact,
        tc.exact_percentage()
    ));
    s.push_str(&format!(
        "  - Jaccard >= 0.80         : {} ({:.1}%)\n",
        tc.correct_at_0_8,
        pct(tc.correct_at_0_8, tc.total_entries)
    ));
    s.push_str(&format!(
        "  - Jaccard >= 0.67         : {} ({:.1}%)\n",
        tc.correct_at_0_67,
        pct(tc.correct_at_0_67, tc.total_entries)
    ));
    s.push_str(&format!(
        "  - Jaccard >= 0.50         : {} ({:.1}%)\n",
        tc.correct_at_0_5,
        pct(tc.correct_at_0_5, tc.total_entries)
    ));

    if !tc.near_miss_top.is_empty() {
        s.push_str("\n### Top near-misses (Wave 5 iteration candidates)\n\n");
        s.push_str("| # | Actual canonical | Expected canonical | Freq | Examples |\n|---|---|---|---|---|\n");
        for (i, nm) in tc.near_miss_top.iter().enumerate() {
            s.push_str(&format!(
                "| {} | `{}` | `{}` | {} | {} |\n",
                i + 1,
                nm.actual_tag,
                nm.expected_tag,
                nm.frequency,
                nm.example_dictation_ids.join(", ")
            ));
        }
    }
    s
}

fn pct(n: usize, d: usize) -> f64 {
    if d == 0 {
        0.0
    } else {
        100.0 * n as f64 / d as f64
    }
}

fn row(name: &str, pct: &f64, n: usize, d: usize, threshold: f64, floor: bool) -> String {
    let verdict = if d == 0 {
        "-"
    } else if *pct + 1e-9 >= threshold {
        "PASS"
    } else {
        "FAIL"
    };
    let label = if floor {
        format!(">= {threshold:.0}%")
    } else if (threshold - 100.0).abs() < f64::EPSILON {
        "~100%".to_string()
    } else {
        format!(">= {threshold:.0}%")
    };
    format!("| {name} | {pct:.1}% ({n}/{d}) | {label} | {verdict} |\n")
}
