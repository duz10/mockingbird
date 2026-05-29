//! Judge 5: Determinism — re-run `run-corpus --seed 42` on a
//! 3-dictation subset and assert the produced
//! `structured/<id>.json` files are byte-identical to the baseline
//! run's same three.
//!
//! Determinism is the *floor* of reproducibility: the same code +
//! same seed + same model + same Ollama options must produce
//! byte-identical structured outputs. If this fails, the §8.5
//! stability run can't be trusted either.
//!
//! This judge is OPTIONAL — pass `JudgeContext::determinism = Some(_)`
//! to invoke it. The runner skips it (verdict=pass with "skipped"
//! reasoning) when not configured, so the rest of the suite stays
//! usable offline.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::{DeterminismConfig, JudgeContext, JudgeVerdict};

const NAME: &str = "determinism_seed42_byte_identical";

pub fn judge(ctx: &JudgeContext) -> JudgeVerdict {
    let cfg = match ctx.determinism.as_ref() {
        Some(c) => c,
        None => {
            return JudgeVerdict::pass(
                NAME,
                "SKIPPED: no DeterminismConfig supplied (set ctx.determinism to invoke)",
            );
        }
    };

    // Sanity: baseline run dir must exist and contain the three
    // baseline files we'll compare against. Otherwise we can't
    // judge.
    for id in &cfg.three_dictation_ids {
        let baseline = cfg
            .baseline_run_dir
            .join("structured")
            .join(format!("{id}.json"));
        if !baseline.exists() {
            return JudgeVerdict::fail(
                NAME,
                format!(
                    "baseline file missing: {} (cannot compare)",
                    baseline.display()
                ),
            );
        }
    }

    // Write a tmp subset corpus by symlinking (or copying when
    // symlink unavailable on Windows without admin) the three
    // dictation+answer-key pairs into a sandbox.
    let workdir = std::env::temp_dir().join(format!(
        "kg-det-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let _ = std::fs::create_dir_all(&workdir);
    let subset_dict = workdir.join("dictations");
    let subset_keys = workdir.join("answer-keys");
    if let Err(e) = std::fs::create_dir_all(&subset_dict) {
        return JudgeVerdict::fail(NAME, format!("create subset dictations dir: {e}"));
    }
    if let Err(e) = std::fs::create_dir_all(&subset_keys) {
        return JudgeVerdict::fail(NAME, format!("create subset answer-keys dir: {e}"));
    }
    for id in &cfg.three_dictation_ids {
        let from_d = cfg.corpus_dir.join("dictations").join(format!("{id}.md"));
        let to_d = subset_dict.join(format!("{id}.md"));
        let from_k = cfg
            .corpus_dir
            .join("answer-keys")
            .join(format!("{id}.json"));
        let to_k = subset_keys.join(format!("{id}.json"));
        if let Err(e) = std::fs::copy(&from_d, &to_d) {
            return JudgeVerdict::fail(NAME, format!("copy dictation {}: {e}", from_d.display()));
        }
        if let Err(e) = std::fs::copy(&from_k, &to_k) {
            return JudgeVerdict::fail(NAME, format!("copy answer-key {}: {e}", from_k.display()));
        }
    }

    let rerun_dir = workdir.join("rerun");
    let out = Command::new(&cfg.run_corpus_bin)
        .args([
            "--corpus-dir",
            workdir.to_str().unwrap_or(""),
            "--output-dir",
            rerun_dir.to_str().unwrap_or(""),
            "--run-id",
            "determinism-judge-rerun",
            "--seed",
            "42",
            "--model",
            cfg.model.as_str(),
            "--ollama-url",
            cfg.ollama_url.as_str(),
        ])
        .output();
    let out = match out {
        Ok(o) => o,
        Err(e) => return JudgeVerdict::fail(NAME, format!("spawn run-corpus: {e}")),
    };
    if !out.status.success() {
        return JudgeVerdict::fail(
            NAME,
            format!(
                "run-corpus exited non-zero: {}",
                String::from_utf8_lossy(&out.stderr)
            ),
        );
    }

    let mut details: Vec<String> = Vec::new();
    let mut mismatches = 0usize;
    let rerun_structured = rerun_dir.join("determinism-judge-rerun").join("structured");
    for id in &cfg.three_dictation_ids {
        let baseline = cfg
            .baseline_run_dir
            .join("structured")
            .join(format!("{id}.json"));
        let actual = rerun_structured.join(format!("{id}.json"));
        match compare_files(&baseline, &actual) {
            Ok(true) => details.push(format!("  {id}: byte-identical PASS")),
            Ok(false) => {
                mismatches += 1;
                details.push(format!("  {id}: DIFFER (byte mismatch)"));
            }
            Err(e) => {
                mismatches += 1;
                details.push(format!("  {id}: compare error: {e}"));
            }
        }
    }

    if mismatches == 0 {
        JudgeVerdict::pass(
            NAME,
            format!(
                "all {} structured outputs byte-identical at seed 42",
                cfg.three_dictation_ids.len()
            ),
        )
        .with_details(details)
    } else {
        JudgeVerdict::fail(
            NAME,
            format!(
                "{mismatches}/{} byte mismatches at seed 42 — determinism floor broken",
                cfg.three_dictation_ids.len()
            ),
        )
        .with_details(details)
    }
}

fn compare_files(a: &Path, b: &Path) -> anyhow::Result<bool> {
    let ba = std::fs::read(a)?;
    let bb = std::fs::read(b)?;
    Ok(ba == bb)
}

/// Construct a `DeterminismConfig` using the standard Phase 0
/// defaults: 3 representative dictations spanning clean-single,
/// multi-item rambler, and junk; model from the spec; baseline
/// run-a.
pub fn default_config(
    run_corpus_bin: PathBuf,
    corpus_dir: PathBuf,
    baseline_run_dir: PathBuf,
    model: String,
    ollama_url: String,
) -> DeterminismConfig {
    DeterminismConfig {
        run_corpus_bin,
        corpus_dir,
        baseline_run_dir,
        three_dictation_ids: [
            "persona-01-case-01".to_string(), // clean-single
            "persona-02-case-01".to_string(), // clean-2-item
            "persona-01-case-05".to_string(), // junk
        ],
        model,
        ollama_url,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skipped_when_no_config() {
        let ctx = JudgeContext {
            run_dirs: Vec::new(),
            final_run_dir: None,
            repo_root: PathBuf::from("."),
            baseline_ref: String::new(),
            allowed_path_prefixes: Vec::new(),
            determinism: None,
        };
        let v = judge(&ctx);
        assert!(v.passed);
        assert!(v.reasoning.contains("SKIPPED"));
    }

    #[test]
    fn fails_when_baseline_files_missing() {
        let tmp = std::env::temp_dir().join(format!(
            "kg-det-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let cfg = DeterminismConfig {
            run_corpus_bin: PathBuf::from("does-not-exist"),
            corpus_dir: tmp.clone(),
            baseline_run_dir: tmp.clone(),
            three_dictation_ids: ["missing-1".into(), "missing-2".into(), "missing-3".into()],
            model: "x".into(),
            ollama_url: "x".into(),
        };
        let ctx = JudgeContext {
            run_dirs: Vec::new(),
            final_run_dir: None,
            repo_root: PathBuf::from("."),
            baseline_ref: String::new(),
            allowed_path_prefixes: Vec::new(),
            determinism: Some(cfg),
        };
        let v = judge(&ctx);
        assert!(!v.passed);
        assert!(v.reasoning.contains("baseline file missing"));
    }

    #[test]
    fn compare_files_detects_byte_mismatch() {
        let tmp = std::env::temp_dir();
        let a = tmp.join(format!("kg-det-cmp-a-{}.bin", std::process::id()));
        let b = tmp.join(format!("kg-det-cmp-b-{}.bin", std::process::id()));
        std::fs::write(&a, b"hello world").unwrap();
        std::fs::write(&b, b"hello world").unwrap();
        assert!(compare_files(&a, &b).unwrap());
        std::fs::write(&b, b"hello world!").unwrap();
        assert!(!compare_files(&a, &b).unwrap());
    }
}
