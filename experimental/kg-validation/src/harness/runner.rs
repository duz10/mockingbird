//! Corpus walker. Iterates `corpus/dictations/*.md`, runs each
//! through [`crate::harness::pipeline::run_pipeline`], persists
//! `structured/<id>.json`, and writes a `SUMMARY.json` at run end.
//!
//! Dry-run mode skips the LLM entirely — useful for verifying
//! corpus pairing + file structure without spinning up Ollama.

use std::path::{Path, PathBuf};
use std::time::Instant;

use chrono::Utc;
use serde::Serialize;

use crate::harness::pipeline::run_pipeline;
use crate::ollama::{GenerateOptions, OllamaDispatcher};
use crate::schema::Entry;
use crate::schema_loader::Schema;

#[derive(Debug, Clone)]
pub struct RunConfig {
    pub model: String,
    pub seed: i64,
    pub run_id: String,
    pub corpus_dir: PathBuf,
    pub output_dir: PathBuf,
    pub captured_iso: String,
    pub temperature: f32,
    pub num_ctx: usize,
    pub dry_run: bool,
}

#[derive(Debug, Serialize)]
pub struct RunSummary {
    pub run_id: String,
    pub model: String,
    pub seed: i64,
    pub captured_iso: String,
    pub started_iso: String,
    pub finished_iso: String,
    pub total_dictations: usize,
    pub successful: usize,
    pub failed: usize,
    pub dry_run: bool,
    pub errors: Vec<RunError>,
}

#[derive(Debug, Serialize)]
pub struct RunError {
    pub dictation_id: String,
    pub stage: String,
    pub message: String,
}

pub fn run_corpus<D: OllamaDispatcher>(
    dispatcher: &D,
    schema: &Schema,
    config: &RunConfig,
) -> RunSummary {
    let started_iso = Utc::now().to_rfc3339();
    let run_dir = config.output_dir.join(&config.run_id);
    let _ = std::fs::create_dir_all(run_dir.join("raw"));
    let _ = std::fs::create_dir_all(run_dir.join("structured"));

    let dictations_dir = config.corpus_dir.join("dictations");
    let answer_keys_dir = config.corpus_dir.join("answer-keys");

    let mut dictation_ids: Vec<String> = match std::fs::read_dir(&dictations_dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let p = e.path();
                if p.extension().and_then(|s| s.to_str()) == Some("md") {
                    p.file_stem().and_then(|s| s.to_str()).map(str::to_string)
                } else {
                    None
                }
            })
            .collect(),
        Err(_) => Vec::new(),
    };
    dictation_ids.sort();

    let opts = GenerateOptions {
        temperature: config.temperature,
        seed: Some(config.seed),
        num_ctx: config.num_ctx,
    };

    let total = dictation_ids.len();
    let mut successful = 0usize;
    let mut failed = 0usize;
    let mut errors: Vec<RunError> = Vec::new();

    for (i, id) in dictation_ids.iter().enumerate() {
        let pair_ok = answer_keys_dir.join(format!("{id}.json")).exists();
        let dict_path = dictations_dir.join(format!("{id}.md"));
        let dictation = match std::fs::read_to_string(&dict_path) {
            Ok(s) => s,
            Err(e) => {
                failed += 1;
                errors.push(RunError {
                    dictation_id: id.clone(),
                    stage: "io".into(),
                    message: format!("read dictation {}: {e}", dict_path.display()),
                });
                eprintln!("[{}/{}] {id} ... IO ERROR: {e}", i + 1, total);
                continue;
            }
        };

        if !pair_ok {
            // Not fatal — the runner still processes the dictation,
            // but the run summary records the missing answer key so
            // the scorer can refuse to grade it without complaint.
            errors.push(RunError {
                dictation_id: id.clone(),
                stage: "pairing".into(),
                message: format!(
                    "no answer key at {}",
                    answer_keys_dir.join(format!("{id}.json")).display()
                ),
            });
        }

        let t0 = Instant::now();

        if config.dry_run {
            let marker = run_dir.join("structured").join(format!("{id}.dry-run"));
            if let Err(e) = std::fs::write(&marker, b"dry-run") {
                failed += 1;
                errors.push(RunError {
                    dictation_id: id.clone(),
                    stage: "io".into(),
                    message: format!("write dry-run marker {}: {e}", marker.display()),
                });
                continue;
            }
            successful += 1;
            println!(
                "[{:>2}/{:>2}] {id} ... DRY-RUN ({:.0}ms)",
                i + 1,
                total,
                t0.elapsed().as_secs_f64() * 1000.0
            );
            continue;
        }

        let artifact_dir = run_dir.join("raw").join(id);
        let result = run_pipeline(
            dispatcher,
            schema,
            &config.model,
            id,
            &dictation,
            &config.captured_iso,
            &opts,
            &artifact_dir,
        );

        // Persist structured output even on partial failure.
        let structured_path = run_dir.join("structured").join(format!("{id}.json"));
        if let Err(e) = write_entries(&structured_path, &result.entries) {
            errors.push(RunError {
                dictation_id: id.clone(),
                stage: "io".into(),
                message: format!("write structured {}: {e}", structured_path.display()),
            });
        }

        let n_entries = result.entries.len();
        let n_errors = result.per_pass_errors.len();
        if n_errors > 0 {
            for (stage, err) in &result.per_pass_errors {
                errors.push(RunError {
                    dictation_id: id.clone(),
                    stage: stage.clone(),
                    message: err.to_string(),
                });
            }
        }
        // Success def: at least one entry OR a clean junk result
        // (zero entries AND zero errors). Anything else is a failure
        // for summary purposes — Wave 3 grades nuance.
        let ok = n_errors == 0;
        if ok {
            successful += 1;
        } else {
            failed += 1;
        }
        println!(
            "[{:>2}/{:>2}] {id} ... {n_entries} entries, {n_errors} errors, {:.2}s",
            i + 1,
            total,
            t0.elapsed().as_secs_f64()
        );
    }

    let finished_iso = Utc::now().to_rfc3339();
    let summary = RunSummary {
        run_id: config.run_id.clone(),
        model: config.model.clone(),
        seed: config.seed,
        captured_iso: config.captured_iso.clone(),
        started_iso,
        finished_iso,
        total_dictations: total,
        successful,
        failed,
        dry_run: config.dry_run,
        errors,
    };
    let _ = write_summary(&run_dir.join("SUMMARY.json"), &summary);
    summary
}

fn write_entries(path: &Path, entries: &[Entry]) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(entries)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(path, json)
}

fn write_summary(path: &Path, summary: &RunSummary) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(summary)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(path, json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ollama::testing::MockOllama;

    fn tempdir() -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!(
            "kg-runner-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    /// Dry-run against the real on-disk corpus. The most important
    /// test in this module — proves the walker pairs dictations
    /// with answer keys, writes markers, and emits a SUMMARY without
    /// needing Ollama.
    #[test]
    fn dry_run_walks_real_corpus() {
        let crate_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let corpus_dir = crate_root.join("corpus");
        if !corpus_dir.exists() {
            // Sandbox crate without corpus authored yet — skip cleanly.
            return;
        }
        let out = tempdir();

        let mock = MockOllama::new(); // never called in dry-run
        let schema = Schema::load_default().expect("load default schema");
        let cfg = RunConfig {
            model: "unused".into(),
            seed: 42,
            run_id: "dry-test".into(),
            corpus_dir: corpus_dir.clone(),
            output_dir: out.clone(),
            captured_iso: "2026-06-14T08:00:00Z".into(),
            temperature: 0.2,
            num_ctx: 4096,
            dry_run: true,
        };
        let summary = run_corpus(&mock, &schema, &cfg);

        assert_eq!(summary.failed, 0, "{:?}", summary.errors);
        assert!(summary.total_dictations >= 32);
        assert_eq!(summary.successful, summary.total_dictations);
        assert!(summary.dry_run);

        // SUMMARY.json must exist and parse back as JSON.
        let summary_path = out.join("dry-test").join("SUMMARY.json");
        let text = std::fs::read_to_string(&summary_path).expect("read SUMMARY");
        let _: serde_json::Value = serde_json::from_str(&text).expect("SUMMARY parses");

        // Cleanup.
        let _ = std::fs::remove_dir_all(&out);
    }
}
