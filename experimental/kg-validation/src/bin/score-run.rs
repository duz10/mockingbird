//! `score-run` — Phase 0 scoring CLI.
//!
//! Reads a `runs/<run-id>/` directory (produced by `run-corpus`),
//! scores it against the corpus answer keys, runs the Judge
//! Validation Protocol, runs the Persona Cross-Reference Pass, and
//! writes:
//!
//! - `runs/<run-id>/SCORE.json` — full [`ScoreReport`]
//! - `runs/<run-id>/JUDGE_VALIDATION.json` — [`JvpReport`]
//! - `runs/<run-id>/PERSONA_REVIEW.md` — PCRP output
//! - `runs/<run-id>/SCORE_SUMMARY.md` — human-readable per-metric
//!   table with pass/fail vs. spec §8.4
//!
//! Hand-rolled flag parsing, same YAGNI logic as `run-corpus`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use kg_validation::ollama::{GenerateOptions, OllamaClient};
use kg_validation::schema::AnswerKey;
use kg_validation::scoring::judge_validation::{
    load_calibration_set, run_jvp, JvpConfig, JvpOverall, JvpReport,
};
use kg_validation::scoring::metrics::{
    compute_stability, score_run, RecordedJudgeCall, ScoreReport, TagJudgeContext,
};
use kg_validation::scoring::persona_review::{
    load_persona_notes, render_markdown, run_pcrp, select_samples, PcrpConfig,
};

const HELP: &str = "\
score-run — Phase 0 scoring CLI for the Mockingbird Knowledge Graph.

USAGE:
  score-run --run-dir <path> [FLAGS]

REQUIRED:
  --run-dir <path>                 runs/<run-id>/ to score

OPTIONAL:
  --corpus-dir <path>              default ./corpus
  --calibration-set <path>         default judge-calibration/tag-equivalence.json
  --judge-model <name>             default gemma2:9b (swapped from llama3.1:8b on 2026-05-29 per Wave 3.2 Gate 3 finding; ADR 0048 G5)
  --cross-judge-model <name>       default llama3.1:8b-instruct-q4_K_M (rotated to cross-check role; Gate 3 demotes to WARN if unpulled)
  --persona-review-model <name>    default llama3.1:8b-instruct-q4_K_M (PCRP reviewer keeps llama3.1; it judges the structured output, not tag equivalence)
  --judge-seed <int>               default 42
  --ollama-url <url>               default http://localhost:11434
  --stability-vs <run-id>          optional sibling run for §8.5 comparison
  --skip-jvp                       dev only — production runs MUST NOT use
  --skip-pcrp                      dev only — production runs MUST NOT use
  --help                           print this and exit
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
    calibration_path: PathBuf,
    judge_model: String,
    cross_judge_model: Option<String>,
    persona_review_model: String,
    judge_seed: i64,
    ollama_url: String,
    stability_vs: Option<String>,
    skip_jvp: bool,
    skip_pcrp: bool,
}

fn parse_args() -> anyhow::Result<Args> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut run_dir: Option<PathBuf> = None;
    let mut corpus_dir = PathBuf::from("corpus");
    let mut calibration_path = PathBuf::from("judge-calibration").join("tag-equivalence.json");
    // Wave 3.3 (2026-05-29): primary swapped to gemma2:9b on the back
    // of Wave 3.2's Gate 3 STOP (llama3.1:8b proved more permissive on
    // equivalence than gemma2:9b on the real corpus; gemma2:9b is the
    // more discriminating judge). llama3.1 rotated to the cross-check
    // slot. PCRP reviewer stays on llama3.1 — that role grades
    // structured-output quality vs. persona notes, not tag equivalence.
    let mut judge_model = "gemma2:9b".to_string();
    let mut cross_judge_model: Option<String> = Some("llama3.1:8b-instruct-q4_K_M".to_string());
    let mut persona_review_model = "llama3.1:8b-instruct-q4_K_M".to_string();
    let mut judge_seed: i64 = 42;
    let mut ollama_url = "http://localhost:11434".to_string();
    let mut stability_vs: Option<String> = None;
    let mut skip_jvp = false;
    let mut skip_pcrp = false;

    let mut i = 0;
    while i < argv.len() {
        let arg = argv[i].as_str();
        match arg {
            "--help" | "-h" => {
                println!("{HELP}");
                std::process::exit(0);
            }
            "--skip-jvp" => skip_jvp = true,
            "--skip-pcrp" => skip_pcrp = true,
            "--run-dir" => run_dir = Some(PathBuf::from(take(&argv, &mut i, arg)?)),
            "--corpus-dir" => corpus_dir = PathBuf::from(take(&argv, &mut i, arg)?),
            "--calibration-set" => calibration_path = PathBuf::from(take(&argv, &mut i, arg)?),
            "--judge-model" => judge_model = take(&argv, &mut i, arg)?,
            "--cross-judge-model" => {
                let v = take(&argv, &mut i, arg)?;
                cross_judge_model = if v == "none" { None } else { Some(v) };
            }
            "--persona-review-model" => persona_review_model = take(&argv, &mut i, arg)?,
            "--judge-seed" => {
                judge_seed = take(&argv, &mut i, arg)?
                    .parse()
                    .map_err(|e| anyhow::anyhow!("--judge-seed must be int: {e}"))?;
            }
            "--ollama-url" => ollama_url = take(&argv, &mut i, arg)?,
            "--stability-vs" => stability_vs = Some(take(&argv, &mut i, arg)?),
            unknown => anyhow::bail!("unknown flag: {unknown}\n\n{HELP}"),
        }
        i += 1;
    }

    let run_dir = run_dir.ok_or_else(|| anyhow::anyhow!("--run-dir is required\n\n{HELP}"))?;
    Ok(Args {
        run_dir,
        corpus_dir,
        calibration_path,
        judge_model,
        cross_judge_model,
        persona_review_model,
        judge_seed,
        ollama_url,
        stability_vs,
        skip_jvp,
        skip_pcrp,
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
    println!("calibration_set      : {}", args.calibration_path.display());
    println!("judge_model          : {}", args.judge_model);
    println!(
        "cross_judge_model    : {}",
        args.cross_judge_model.as_deref().unwrap_or("(none)")
    );
    println!("persona_review_model : {}", args.persona_review_model);
    println!("judge_seed           : {}", args.judge_seed);
    println!("ollama_url           : {}", args.ollama_url);
    println!("skip_jvp             : {}", args.skip_jvp);
    println!("skip_pcrp            : {}", args.skip_pcrp);
    println!();

    let judge_options = GenerateOptions {
        temperature: 0.2,
        seed: Some(args.judge_seed),
        num_ctx: 4096,
    };

    let primary_judge = OllamaClient::with_base_url(args.ollama_url.clone());
    let cross_judge = OllamaClient::with_base_url(args.ollama_url.clone());

    // ── 1. Score the run ──────────────────────────────────────────
    let verdict_sink: std::cell::RefCell<Vec<RecordedJudgeCall>> =
        std::cell::RefCell::new(Vec::new());
    let tag_ctx = if args.skip_jvp {
        None
    } else {
        Some(TagJudgeContext {
            dispatcher: &primary_judge,
            model: &args.judge_model,
            options: &judge_options,
            verdict_sink: Some(&verdict_sink),
        })
    };

    let run_id = args
        .run_dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("run")
        .to_string();

    println!("[1/4] scoring run ...");
    let mut score = score_run(&run_id, &structured_dir, &answer_keys_dir, tag_ctx)?;

    // ── 2. Stability (optional) ───────────────────────────────────
    if let Some(other) = args.stability_vs.as_deref() {
        let other_structured = args
            .run_dir
            .parent()
            .ok_or_else(|| anyhow::anyhow!("--run-dir has no parent for --stability-vs"))?
            .join(other)
            .join("structured");
        println!(
            "[1.5/4] stability vs {other} ({}) ...",
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

    // ── 3. JVP ────────────────────────────────────────────────────
    let jvp_report = if args.skip_jvp {
        println!("[2/4] JVP skipped (--skip-jvp). Tag metric and JUDGE_VALIDATION omitted.");
        None
    } else {
        println!("[2/4] running JVP (5 gates) ...");
        let calibration = load_calibration_set(&args.calibration_path)?;
        let cross_judge_opt = args.cross_judge_model.as_ref().map(|_| &cross_judge);
        let recorded = verdict_sink.borrow().clone();
        let cfg = JvpConfig {
            run_id: run_id.clone(),
            primary_judge: &primary_judge,
            primary_judge_model: args.judge_model.clone(),
            cross_judge: cross_judge_opt,
            cross_judge_model: args.cross_judge_model.clone(),
            calibration,
            judge_options: judge_options.clone(),
            recorded_verdicts: recorded,
        };
        let report = run_jvp(cfg);
        let jvp_path = args.run_dir.join("JUDGE_VALIDATION.json");
        std::fs::write(&jvp_path, serde_json::to_string_pretty(&report)?)?;
        println!("        wrote {}", jvp_path.display());
        Some(report)
    };

    // ── 4. PCRP ───────────────────────────────────────────────────
    if !args.skip_pcrp {
        println!("[3/4] running PCRP ...");
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
            options: judge_options.clone(),
            persona_notes,
            samples,
        };
        let pcrp_report = run_pcrp(&primary_judge, cfg)?;
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
        println!("[3/4] PCRP skipped (--skip-pcrp).");
    }

    // ── 5. SCORE_SUMMARY.md ───────────────────────────────────────
    println!("[4/4] writing SCORE_SUMMARY.md ...");
    let summary_md = render_score_summary(&score, jvp_report.as_ref());
    let summary_path = args.run_dir.join("SCORE_SUMMARY.md");
    std::fs::write(&summary_path, &summary_md)?;
    println!("        wrote {}", summary_path.display());
    println!();
    println!("{summary_md}");

    if let Some(report) = jvp_report.as_ref() {
        if report.overall == JvpOverall::Halt {
            eprintln!(
                "JVP HALT: judge validation failed. Tag metric is INVALID. See JUDGE_VALIDATION.json."
            );
            return Err(anyhow::anyhow!("JVP halt"));
        }
    }
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

fn render_score_summary(score: &ScoreReport, jvp: Option<&JvpReport>) -> String {
    let mut s = String::new();
    s.push_str(&format!("# SCORE_SUMMARY — `{}`\n\n", score.run_id));
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

    s.push_str("## Per-metric vs. spec §8.4 thresholds\n\n");
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
            "✅ PASS"
        } else {
            "❌ FAIL"
        }
    ));
    s.push_str(&row(
        "Tag-variant collapse correct",
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

    if let Some(jvp) = jvp {
        s.push_str("\n## JVP gates (ADR 0048 §G5)\n\n");
        s.push_str(&format!("- overall: **{:?}**\n", jvp.overall));
        s.push_str(&format!(
            "- Gate 1 (calibration): {:?} — {}\n",
            jvp.gate1_calibration.outcome, jvp.gate1_calibration.detail
        ));
        // Observational companion — never gates, always reports.
        s.push_str(&format!(
            "- Gate 1 borderline (observational): {} ({}/{}) — {}\n",
            format_args!("{:.1}%", jvp.gate1_borderline.percentage),
            jvp.gate1_borderline.numerator,
            jvp.gate1_borderline.denominator,
            jvp.gate1_borderline.detail
        ));
        if !jvp.gate1_borderline.per_dimension.is_empty() {
            s.push_str("  - per dimension:\n");
            for d in &jvp.gate1_borderline.per_dimension {
                s.push_str(&format!(
                    "    - `{}`: {}/{} ({:.1}%)\n",
                    d.dimension, d.numerator, d.denominator, d.percentage
                ));
            }
        }
        s.push_str(&format!(
            "- Gate 2 (reasoning audit): {:?} — {}\n",
            jvp.gate2_reasoning_audit.outcome, jvp.gate2_reasoning_audit.detail
        ));
        s.push_str(&format!(
            "- Gate 3 (cross-judge): {:?} — {}\n",
            jvp.gate3_cross_judge.outcome, jvp.gate3_cross_judge.detail
        ));
        s.push_str(&format!(
            "- Gate 4 (distribution): {:?} — {}\n",
            jvp.gate4_distribution.outcome, jvp.gate4_distribution.detail
        ));
        s.push_str(&format!(
            "- Gate 5 (determinism): {:?} — {}\n",
            jvp.gate5_determinism.outcome, jvp.gate5_determinism.detail
        ));
    }

    if let Some(stab) = score.stability.as_ref() {
        s.push_str("\n## Stability vs. ");
        s.push_str(&format!("`{}` (spec §8.5)\n\n", stab.vs_run_id));
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

fn row(name: &str, pct: &f64, n: usize, d: usize, threshold: f64, floor: bool) -> String {
    let verdict = if d == 0 {
        "—"
    } else if *pct + 1e-9 >= threshold {
        "✅ PASS"
    } else {
        "❌ FAIL"
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
