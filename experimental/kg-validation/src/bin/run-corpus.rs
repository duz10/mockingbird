//! `run-corpus` — Phase 0 validation harness CLI.
//!
//! Reads the on-disk corpus, runs each dictation through the four-pass
//! pipeline, persists structured output + raw per-pass artifacts +
//! a `SUMMARY.json` under `runs/<run-id>/`.
//!
//! Hand-rolled flag parsing on purpose: a single binary with seven
//! flags doesn't warrant pulling `clap` into a sandbox crate
//! (YAGNI). If this binary grows, swap in `clap` then.

use std::path::PathBuf;
use std::process::ExitCode;

use kg_validation::harness::runner::{run_corpus, RunConfig};
use kg_validation::ollama::OllamaClient;
use kg_validation::schema_loader::Schema;

const HELP: &str = "\
run-corpus — Phase 0 validation harness for the Mockingbird Knowledge Graph.

USAGE:
  run-corpus [FLAGS]

FLAGS:
  --model <name>          Ollama model id (default: read from SCHEMA.md model_defaults.segment)
  --seed <int>            Per-run sampling seed (default: 42)
  --run-id <name>         Output subdirectory name (default: ISO timestamp)
  --corpus-dir <path>     Corpus root (default: ./corpus)
  --output-dir <path>     Where runs/<run-id>/ lives (default: ./runs)
  --captured-iso <iso>    Anchor for date-resolution (default: 2026-06-14T08:00:00Z)
  --ollama-url <url>      Ollama base URL (default: http://localhost:11434)
  --temperature <f>       Sampling temperature (default: 0.2 per ADR 0048 §G4)
  --num-ctx <int>         Context window size (default: 4096)
  --dry-run               Skip Ollama; verify corpus walk + SUMMARY only
  --help                  Print this and exit
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

fn real_main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // Defaults. `model` starts as `None`; if no `--model` is given,
    // it's filled from `Schema::model_defaults.segment` after the
    // schema loads. This honours the ADR 0049 Move 1 contract that
    // SCHEMA.md is the source of truth for pipeline configuration.
    let mut model: Option<String> = None;
    let mut seed: i64 = 42;
    let mut run_id: Option<String> = None;
    let mut corpus_dir = PathBuf::from("corpus");
    let mut output_dir = PathBuf::from("runs");
    let mut captured_iso = "2026-06-14T08:00:00Z".to_string();
    let mut ollama_url = "http://localhost:11434".to_string();
    let mut temperature: f32 = 0.2;
    let mut num_ctx: usize = 4096;
    let mut dry_run = false;

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "--help" | "-h" => {
                println!("{HELP}");
                return Ok(());
            }
            "--dry-run" => {
                dry_run = true;
            }
            "--model" => {
                model = Some(take_value(&args, &mut i, "--model")?);
            }
            "--seed" => {
                let v = take_value(&args, &mut i, "--seed")?;
                seed = v
                    .parse()
                    .map_err(|e| anyhow::anyhow!("--seed must be int: {e}"))?;
            }
            "--run-id" => {
                run_id = Some(take_value(&args, &mut i, "--run-id")?);
            }
            "--corpus-dir" => {
                corpus_dir = PathBuf::from(take_value(&args, &mut i, "--corpus-dir")?);
            }
            "--output-dir" => {
                output_dir = PathBuf::from(take_value(&args, &mut i, "--output-dir")?);
            }
            "--captured-iso" => {
                captured_iso = take_value(&args, &mut i, "--captured-iso")?;
            }
            "--ollama-url" => {
                ollama_url = take_value(&args, &mut i, "--ollama-url")?;
            }
            "--temperature" => {
                let v = take_value(&args, &mut i, "--temperature")?;
                temperature = v
                    .parse()
                    .map_err(|e| anyhow::anyhow!("--temperature must be f32: {e}"))?;
            }
            "--num-ctx" => {
                let v = take_value(&args, &mut i, "--num-ctx")?;
                num_ctx = v
                    .parse()
                    .map_err(|e| anyhow::anyhow!("--num-ctx must be usize: {e}"))?;
            }
            unknown => {
                anyhow::bail!("unknown flag: {unknown}\n\n{HELP}");
            }
        }
        i += 1;
    }

    let run_id = run_id.unwrap_or_else(|| {
        // Filesystem-safe ISO: replace `:` with `-`.
        chrono::Utc::now().to_rfc3339().replace(':', "-")
    });

    // Load schema early so its defaults can fill in any CLI flags
    // the caller omitted. The schema is itself loaded again below
    // for the runner; the double-load is fine (it's a single tiny
    // file read) and keeps the run-time loader call where the
    // schema is consumed.
    let schema_for_defaults = Schema::load_default()
        .map_err(|e| anyhow::anyhow!("failed to load SCHEMA.md for defaults: {e}"))?;
    let model = model.unwrap_or_else(|| schema_for_defaults.model_defaults.segment.clone());

    let config = RunConfig {
        model,
        seed,
        run_id,
        corpus_dir,
        output_dir,
        captured_iso,
        temperature,
        num_ctx,
        dry_run,
    };

    println!("=== run-corpus ===");
    println!("model       : {}", config.model);
    println!("seed        : {}", config.seed);
    println!("run_id      : {}", config.run_id);
    println!("corpus_dir  : {}", config.corpus_dir.display());
    println!("output_dir  : {}", config.output_dir.display());
    println!("captured_iso: {}", config.captured_iso);
    println!("dry_run     : {}", config.dry_run);
    println!("ollama_url  : {ollama_url}");

    // Re-bind the already-loaded schema to keep the runner call site
    // local. (ADR 0049 Move 1: SCHEMA.md is the contract; the pipeline
    // refuses to start without it.)
    let schema = schema_for_defaults;
    println!(
        "schema      : v{} ({})",
        schema.schema_version, schema.schema_revision
    );
    println!();

    let client = OllamaClient::with_base_url(ollama_url);
    let summary = run_corpus(&client, &schema, &config);

    println!();
    println!("=== SUMMARY ===");
    println!("total      : {}", summary.total_dictations);
    println!("successful : {}", summary.successful);
    println!("failed     : {}", summary.failed);
    println!("errors     : {}", summary.errors.len());
    println!(
        "summary path: {}",
        config
            .output_dir
            .join(&config.run_id)
            .join("SUMMARY.json")
            .display()
    );

    Ok(())
}

fn take_value(args: &[String], i: &mut usize, flag: &str) -> anyhow::Result<String> {
    *i += 1;
    args.get(*i)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("{flag} requires a value"))
}
