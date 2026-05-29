//! `run-judges` — Phase 0 KG invariant-judge runner.
//!
//! Executes all 6 judges in the configured order, prints a verdict
//! table to stdout, and exits non-zero if any judge fails.
//!
//! Usage:
//!
//! ```text
//! run-judges --runs runs/run-a-baseline,runs/run-b-stability \
//!            --final-run runs/run-a-baseline \
//!            --baseline-ref phase-mc-complete \
//!            [--enable-determinism --model qwen2.5:3b-instruct-q4_K_M \
//!             --run-corpus target/release/run-corpus \
//!             --corpus-dir corpus]
//! ```

use std::path::PathBuf;
use std::process::ExitCode;

use kg_validation::judges::{
    determinism, hard_gate, pcrp_completeness, sandbox_isolation, stability, thresholds,
    JudgeContext, JudgeVerdict,
};

const HELP: &str = "\
run-judges - Phase 0 KG invariant-judge runner (Wave 4 / mb-he98)

USAGE:
  run-judges --runs <dir>[,<dir>...] [FLAGS]

REQUIRED:
  --runs <dir>[,<dir>...]    one or more runs/<run-id>/ to inspect

OPTIONAL:
  --final-run <dir>          dir containing PERSONA_REVIEW.md (default: last --runs)
  --baseline-ref <ref>       git ref for sandbox-isolation baseline (default: phase-0-kg-start)
  --enable-determinism       opt in to live determinism judge (calls run-corpus)
  --run-corpus <path>        path to run-corpus binary (required if --enable-determinism)
  --corpus-dir <path>        corpus root (required if --enable-determinism)
  --model <name>             ollama model for determinism (default: qwen2.5:3b-instruct-q4_K_M)
  --ollama-url <url>         default http://localhost:11434
  --help

EXIT:
  0  all judges PASS
  1  at least one judge FAILED
  2  argument error
";

fn main() -> ExitCode {
    match real_main() {
        Ok(0) => ExitCode::SUCCESS,
        Ok(n) => ExitCode::from(n),
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(2)
        }
    }
}

struct Args {
    runs: Vec<PathBuf>,
    final_run: Option<PathBuf>,
    baseline_ref: String,
    enable_determinism: bool,
    run_corpus_bin: Option<PathBuf>,
    corpus_dir: Option<PathBuf>,
    model: String,
    ollama_url: String,
}

fn parse_args() -> anyhow::Result<Args> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut runs: Vec<PathBuf> = Vec::new();
    let mut final_run: Option<PathBuf> = None;
    let mut baseline_ref = "phase-0-kg-start".to_string();
    let mut enable_determinism = false;
    let mut run_corpus_bin: Option<PathBuf> = None;
    let mut corpus_dir: Option<PathBuf> = None;
    let mut model = "qwen2.5:3b-instruct-q4_K_M".to_string();
    let mut ollama_url = "http://localhost:11434".to_string();

    let mut i = 0;
    while i < argv.len() {
        let a = argv[i].as_str();
        match a {
            "--help" | "-h" => {
                println!("{HELP}");
                std::process::exit(0);
            }
            "--runs" => {
                runs = take(&argv, &mut i, a)?
                    .split(',')
                    .map(PathBuf::from)
                    .collect()
            }
            "--final-run" => final_run = Some(PathBuf::from(take(&argv, &mut i, a)?)),
            "--baseline-ref" => baseline_ref = take(&argv, &mut i, a)?,
            "--enable-determinism" => enable_determinism = true,
            "--run-corpus" => run_corpus_bin = Some(PathBuf::from(take(&argv, &mut i, a)?)),
            "--corpus-dir" => corpus_dir = Some(PathBuf::from(take(&argv, &mut i, a)?)),
            "--model" => model = take(&argv, &mut i, a)?,
            "--ollama-url" => ollama_url = take(&argv, &mut i, a)?,
            unknown => anyhow::bail!("unknown flag: {unknown}\n\n{HELP}"),
        }
        i += 1;
    }

    if runs.is_empty() {
        anyhow::bail!("--runs is required (one or more run-dirs, comma-separated)\n\n{HELP}");
    }
    if enable_determinism && (run_corpus_bin.is_none() || corpus_dir.is_none()) {
        anyhow::bail!("--enable-determinism requires --run-corpus AND --corpus-dir");
    }

    Ok(Args {
        runs,
        final_run,
        baseline_ref,
        enable_determinism,
        run_corpus_bin,
        corpus_dir,
        model,
        ollama_url,
    })
}

fn take(args: &[String], i: &mut usize, flag: &str) -> anyhow::Result<String> {
    *i += 1;
    args.get(*i)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("{flag} requires a value"))
}

fn real_main() -> anyhow::Result<u8> {
    let args = parse_args()?;
    let final_run = args.final_run.clone().or_else(|| args.runs.last().cloned());

    // Repo root assumed to be CWD (matches the wrapper invocation
    // pattern: `cd experimental/kg-validation && run-judges ...`).
    let repo_root = std::env::current_dir()
        .ok()
        .and_then(|p| p.parent().and_then(|q| q.parent()).map(|q| q.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));

    let determinism_cfg = if args.enable_determinism {
        Some(determinism::default_config(
            args.run_corpus_bin.clone().unwrap(),
            args.corpus_dir.clone().unwrap(),
            args.runs[0].clone(), // baseline = first run
            args.model.clone(),
            args.ollama_url.clone(),
        ))
    } else {
        None
    };

    let ctx = JudgeContext {
        run_dirs: args.runs.clone(),
        final_run_dir: final_run,
        repo_root,
        baseline_ref: args.baseline_ref.clone(),
        allowed_path_prefixes: sandbox_isolation::default_allowed_prefixes(),
        determinism: determinism_cfg,
    };

    println!("=== run-judges ===");
    println!("runs                 : {:?}", args.runs);
    println!(
        "final-run            : {:?}",
        ctx.final_run_dir.as_ref().map(|p| p.display().to_string())
    );
    println!("baseline-ref         : {}", ctx.baseline_ref);
    println!(
        "determinism          : {}",
        if args.enable_determinism {
            "LIVE"
        } else {
            "skipped"
        }
    );
    println!();

    let verdicts: Vec<JudgeVerdict> = vec![
        hard_gate::judge(&ctx),
        thresholds::judge(&ctx),
        stability::judge(&ctx),
        sandbox_isolation::judge(&ctx),
        determinism::judge(&ctx),
        pcrp_completeness::judge(&ctx),
    ];

    println!("# Verdicts\n");
    println!("| Judge | Verdict | Reasoning |");
    println!("|---|---|---|");
    let mut failed = 0usize;
    for v in &verdicts {
        if !v.passed {
            failed += 1;
        }
        let mark = if v.passed { "PASS" } else { "FAIL" };
        println!("| `{}` | **{mark}** | {} |", v.name, v.reasoning);
    }
    println!();
    for v in &verdicts {
        if !v.details.is_empty() {
            println!("## `{}` details\n", v.name);
            for d in &v.details {
                println!("- {d}");
            }
            println!();
        }
    }

    println!(
        "summary: {}/{} judges passed",
        verdicts.len() - failed,
        verdicts.len()
    );

    Ok(if failed == 0 { 0 } else { 1 })
}
