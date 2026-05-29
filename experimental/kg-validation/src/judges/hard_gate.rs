//! Judge 1: HARD-GATE — `invented_dates_count == 0` across every
//! scored run.
//!
//! Spec §8.4 makes this an absolute floor: a date the answer key
//! says is None but the pipeline emitted is a category-A failure,
//! not a percentage to be averaged. This judge asserts the floor.

use std::path::Path;

use serde::Deserialize;

use super::{JudgeContext, JudgeVerdict};

const NAME: &str = "hard_gate_invented_dates_zero";

#[derive(Deserialize)]
struct ScoreFile {
    per_metric: PerMetric,
}

#[derive(Deserialize)]
struct PerMetric {
    invented_dates_count: usize,
}

pub fn judge(ctx: &JudgeContext) -> JudgeVerdict {
    if ctx.run_dirs.is_empty() {
        return JudgeVerdict::fail(
            NAME,
            "no run directories supplied — cannot assert the hard gate",
        );
    }

    let mut details: Vec<String> = Vec::new();
    let mut violations = 0usize;
    let mut total_runs = 0usize;

    for dir in &ctx.run_dirs {
        let score_path = dir.join("SCORE.json");
        total_runs += 1;
        match read_score(&score_path) {
            Err(e) => {
                violations += 1;
                details.push(format!("{}: failed to read SCORE.json: {e}", dir.display()));
            }
            Ok(s) => {
                let n = s.per_metric.invented_dates_count;
                details.push(format!("{}: invented_dates_count = {n}", dir.display()));
                if n != 0 {
                    violations += 1;
                }
            }
        }
    }

    if violations == 0 {
        JudgeVerdict::pass(
            NAME,
            format!("hard gate holds: invented_dates_count == 0 across all {total_runs} run(s)"),
        )
        .with_details(details)
    } else {
        JudgeVerdict::fail(
            NAME,
            format!(
                "HARD GATE VIOLATED: {violations}/{total_runs} run(s) failed the invented_dates_count==0 check"
            ),
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

    fn write_score(dir: &Path, n: usize) {
        std::fs::create_dir_all(dir).unwrap();
        let body =
            format!("{{\"per_metric\":{{\"invented_dates_count\":{n}, \"junk_correct\":{{}}}}}}");
        std::fs::write(dir.join("SCORE.json"), body).unwrap();
    }

    fn tmp() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "kg-judge-hardgate-{}-{}",
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
    fn passes_when_all_runs_zero() {
        let root = tmp();
        let a = root.join("run-a");
        let b = root.join("run-b");
        write_score(&a, 0);
        write_score(&b, 0);
        let v = judge(&ctx(vec![a, b]));
        assert!(v.passed, "verdict={v:?}");
    }

    #[test]
    fn fails_when_any_run_nonzero() {
        let root = tmp();
        let a = root.join("run-a");
        let b = root.join("run-b");
        write_score(&a, 0);
        write_score(&b, 3);
        let v = judge(&ctx(vec![a, b]));
        assert!(!v.passed, "should fail");
        assert!(v.reasoning.contains("VIOLATED"));
    }

    #[test]
    fn fails_when_no_runs_supplied() {
        let v = judge(&ctx(vec![]));
        assert!(!v.passed);
    }

    #[test]
    fn fails_when_score_json_missing() {
        let root = tmp();
        let a = root.join("run-a"); // no SCORE.json
        std::fs::create_dir_all(&a).unwrap();
        let v = judge(&ctx(vec![a]));
        assert!(!v.passed);
    }
}
