//! `embed-reclassify` — ADR 0049 Wave 0.5.2 / Move 2 head-to-head.
//!
//! Reads an existing structured run (e.g. `runs/iter-1-7b-fix/`),
//! builds an exemplar pool from the corpus answer keys paired with
//! that run's structured bodies, and writes a NEW run dir whose
//! structured outputs are identical to the source except that each
//! entry's `category` + `entry_type` have been replaced by the
//! nearest-exemplar prediction (with leave-one-out exclusion of the
//! entry's own dictation).
//!
//! Score the result with the existing `score-run` binary — same
//! corpus, same scorer, only the classify pass differs. This isolates
//! the classify-pass effect from segmentation / extraction noise.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::ExitCode;

use kg_validation::embeddings::{EmbeddingsDispatcher, OllamaEmbedder};
use kg_validation::exemplars::ExemplarPool;
use kg_validation::schema::Entry;

const HELP: &str = "\
embed-reclassify — replace category + entry_type on an existing run's structured
outputs using a nearest-exemplar embeddings classifier (LOO).

USAGE:
  embed-reclassify [FLAGS]

FLAGS:
  --source-run <path>      Path to source run directory (must contain structured/*.json)
  --out-run <path>         Path to destination run directory (will be created)
  --answer-keys-dir <path> Corpus answer keys (default: ./corpus/answer-keys)
  --embed-model <name>     Embedding model id (default: nomic-embed-text)
  --mode <nearest|centroid> Classifier strategy (default: nearest)
  --ollama-url <url>       Ollama base URL (default: http://localhost:11434)
  --help                   Print this and exit
";

#[derive(Debug, Clone, Copy)]
enum ClassifierMode {
    Nearest,
    Centroid,
}

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

    let mut source_run: Option<PathBuf> = None;
    let mut out_run: Option<PathBuf> = None;
    let mut answer_keys_dir = PathBuf::from("corpus/answer-keys");
    let mut embed_model = "nomic-embed-text".to_string();
    let mut ollama_url = "http://localhost:11434".to_string();
    let mut mode = ClassifierMode::Nearest;

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        let take = |args: &Vec<String>, i: &mut usize, name: &str| -> anyhow::Result<String> {
            *i += 1;
            args.get(*i)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("{name} requires a value"))
        };
        match arg.as_str() {
            "--source-run" => {
                source_run = Some(PathBuf::from(take(&args, &mut i, "--source-run")?))
            }
            "--out-run" => out_run = Some(PathBuf::from(take(&args, &mut i, "--out-run")?)),
            "--answer-keys-dir" => {
                answer_keys_dir = PathBuf::from(take(&args, &mut i, "--answer-keys-dir")?)
            }
            "--embed-model" => embed_model = take(&args, &mut i, "--embed-model")?,
            "--ollama-url" => ollama_url = take(&args, &mut i, "--ollama-url")?,
            "--mode" => {
                let v = take(&args, &mut i, "--mode")?;
                mode = match v.as_str() {
                    "nearest" => ClassifierMode::Nearest,
                    "centroid" => ClassifierMode::Centroid,
                    other => anyhow::bail!("unknown --mode: {other} (try nearest|centroid)"),
                };
            }
            "--help" | "-h" => {
                print!("{HELP}");
                return Ok(());
            }
            other => anyhow::bail!("unknown flag: {other} (use --help)"),
        }
        i += 1;
    }

    let source_run = source_run.ok_or_else(|| anyhow::anyhow!("--source-run is required"))?;
    let out_run = out_run.ok_or_else(|| anyhow::anyhow!("--out-run is required"))?;
    let source_structured = source_run.join("structured");
    let out_structured = out_run.join("structured");

    if !source_structured.is_dir() {
        anyhow::bail!("source structured dir missing: {source_structured:?}");
    }
    std::fs::create_dir_all(&out_structured)?;

    println!("=== embed-reclassify ===");
    println!("source_run       : {source_run:?}");
    println!("out_run          : {out_run:?}");
    println!("answer_keys_dir  : {answer_keys_dir:?}");
    println!("embed_model      : {embed_model}");
    println!("mode             : {mode:?}");
    println!("ollama_url       : {ollama_url}");
    println!();

    let embedder: Box<dyn EmbeddingsDispatcher> =
        Box::new(OllamaEmbedder::with_base_url(ollama_url.clone()));

    println!("[1/3] building exemplar pool ...");
    let pool = ExemplarPool::build_from(
        embedder.as_ref(),
        &embed_model,
        &answer_keys_dir,
        &source_structured,
    )?;
    println!(
        "        pool: {} exemplars from {} dictations",
        pool.exemplars.len(),
        pool.distinct_case_count()
    );

    println!("[2/3] reclassifying source-run structured outputs ...");
    let mut entries_total = 0_usize;
    let mut entries_changed_category = 0_usize;
    let mut entries_changed_entry_type = 0_usize;
    let mut entries_changed_either = 0_usize;
    let mut by_case_change_counts: HashMap<String, (usize, usize)> = HashMap::new();

    let mut source_files: Vec<_> = std::fs::read_dir(&source_structured)?
        .filter_map(|e| e.ok())
        .collect();
    source_files.sort_by_key(|e| e.file_name());

    for (idx, dirent) in source_files.iter().enumerate() {
        let path = dirent.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let case_id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| anyhow::anyhow!("bad file stem: {path:?}"))?
            .to_string();

        let raw = std::fs::read_to_string(&path)?;
        let mut entries: Vec<Entry> = serde_json::from_str(&raw)
            .map_err(|e| anyhow::anyhow!("parse {path:?} as Vec<Entry>: {e}"))?;

        let mut case_changes = (0_usize, 0_usize);
        for entry in entries.iter_mut() {
            entries_total += 1;
            let query = embedder.embed(&embed_model, &entry.body)?;
            let result = match mode {
                ClassifierMode::Nearest => pool.classify_excluding(&case_id, &query),
                ClassifierMode::Centroid => pool.classify_excluding_by_centroid(&case_id, &query),
            }
            .ok_or_else(|| anyhow::anyhow!("pool empty after LOO exclusion of {case_id}"))?;

            let mut changed = false;
            if entry.category != result.category {
                entry.category = result.category;
                entries_changed_category += 1;
                case_changes.0 += 1;
                changed = true;
            }
            if entry.entry_type != result.entry_type {
                entry.entry_type = result.entry_type;
                entries_changed_entry_type += 1;
                case_changes.1 += 1;
                changed = true;
            }
            if changed {
                entries_changed_either += 1;
            }
        }

        let out_path = out_structured.join(format!("{case_id}.json"));
        let serialized = serde_json::to_string_pretty(&entries)?;
        std::fs::write(&out_path, serialized)?;
        if case_changes.0 > 0 || case_changes.1 > 0 {
            by_case_change_counts.insert(case_id.clone(), case_changes);
        }

        println!(
            "        [{:>2}/{}] {} ... {} entries, {}cat / {}type changed",
            idx + 1,
            source_files.len(),
            case_id,
            entries.len(),
            case_changes.0,
            case_changes.1
        );
    }

    // Copy SUMMARY.json across so score-run picks up the run metadata.
    let src_summary = source_run.join("SUMMARY.json");
    if src_summary.is_file() {
        std::fs::copy(&src_summary, out_run.join("SUMMARY.json"))?;
    }

    println!("\n[3/3] writing RECLASSIFY.json metadata ...");
    let meta = serde_json::json!({
        "source_run": source_run.to_string_lossy(),
        "embed_model": embed_model,
        "mode": format!("{mode:?}"),
        "exemplar_count": pool.exemplars.len(),
        "distinct_source_dictations": pool.distinct_case_count(),
        "entries_total": entries_total,
        "entries_changed_category": entries_changed_category,
        "entries_changed_entry_type": entries_changed_entry_type,
        "entries_changed_either": entries_changed_either,
        "cases_with_changes": by_case_change_counts.len(),
    });
    std::fs::write(
        out_run.join("RECLASSIFY.json"),
        serde_json::to_string_pretty(&meta)?,
    )?;

    println!("\n=== SUMMARY ===");
    println!("exemplars        : {}", pool.exemplars.len());
    println!("entries reclassified: {entries_total}");
    println!(
        "  category changed   : {entries_changed_category} ({:.1}%)",
        100.0 * entries_changed_category as f32 / entries_total.max(1) as f32
    );
    println!(
        "  entry_type changed : {entries_changed_entry_type} ({:.1}%)",
        100.0 * entries_changed_entry_type as f32 / entries_total.max(1) as f32
    );
    println!(
        "  either changed     : {entries_changed_either} ({:.1}%)",
        100.0 * entries_changed_either as f32 / entries_total.max(1) as f32
    );

    Ok(())
}
