//! Judge 4: Sandbox-isolation — git diff against a baseline ref
//! asserts that ONLY paths inside the allowed sandbox surface have
//! been modified across all Phase 0 KG commits.
//!
//! Per ADR 0048 §5 (sandbox location), the allowed surface is:
//! - `experimental/kg-validation/`
//! - `docs/` (ADRs + LESSONS + knowledge-graph spec + judge docs)
//! - `STATUS.md`
//! - `.beads/` (bd database)
//! - `.code_puppy/` (settings / hooks tweaks if any)
//!
//! Anything else flagged is a sealed-surface violation and fails
//! the judge.

use std::path::Path;
use std::process::Command;

use super::{JudgeContext, JudgeVerdict};

const NAME: &str = "sandbox_isolation_phase0_kg";

pub fn judge(ctx: &JudgeContext) -> JudgeVerdict {
    if ctx.baseline_ref.is_empty() {
        return JudgeVerdict::fail(NAME, "no baseline ref supplied (ctx.baseline_ref empty)");
    }
    if ctx.allowed_path_prefixes.is_empty() {
        return JudgeVerdict::fail(
            NAME,
            "no allowed_path_prefixes supplied — refusing to vacuously pass",
        );
    }

    let changed = match list_changed_files(&ctx.repo_root, &ctx.baseline_ref) {
        Ok(c) => c,
        Err(e) => return JudgeVerdict::fail(NAME, format!("git diff failed: {e}")),
    };

    let mut violations: Vec<String> = Vec::new();
    let mut allowed_count = 0usize;
    let mut details: Vec<String> = Vec::new();
    for path in &changed {
        if path.is_empty() {
            continue;
        }
        if ctx
            .allowed_path_prefixes
            .iter()
            .any(|p| path.starts_with(p))
        {
            allowed_count += 1;
        } else {
            violations.push(path.clone());
        }
    }
    details.push(format!(
        "changed-file count: {} ({} allowed, {} violating)",
        changed.len(),
        allowed_count,
        violations.len()
    ));
    if !violations.is_empty() {
        details.push("violations:".into());
        for v in &violations {
            details.push(format!("  {v}"));
        }
    }

    if violations.is_empty() {
        JudgeVerdict::pass(
            NAME,
            format!(
                "all {} changed files lie within the allowed sandbox surface",
                changed.len()
            ),
        )
        .with_details(details)
    } else {
        JudgeVerdict::fail(
            NAME,
            format!(
                "{} file(s) modified outside the allowed sandbox surface",
                violations.len()
            ),
        )
        .with_details(details)
    }
}

fn list_changed_files(repo_root: &Path, baseline_ref: &str) -> anyhow::Result<Vec<String>> {
    let out = Command::new("git")
        .args(["diff", "--name-only", baseline_ref, "HEAD"])
        .current_dir(repo_root)
        .output()
        .map_err(|e| anyhow::anyhow!("spawn git: {e}"))?;
    if !out.status.success() {
        anyhow::bail!(
            "git diff exited non-zero: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let s = String::from_utf8_lossy(&out.stdout).to_string();
    Ok(s.lines().map(|l| l.trim().to_string()).collect())
}

/// Canonical allow-list per ADR 0048 §5 — wired through
/// `bin/run-judges` as the default value.
pub fn default_allowed_prefixes() -> Vec<String> {
    vec![
        "experimental/kg-validation/".to_string(),
        "docs/".to_string(),
        "STATUS.md".to_string(),
        ".beads/".to_string(),
        ".code_puppy/".to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Create a tiny throwaway git repo with two commits: a baseline
    /// commit + a follow-up commit that modifies the given paths.
    /// Returns the repo root.
    fn tiny_repo_with_changes(changes: &[(&str, &str)]) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "kg-judge-sandbox-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&root).unwrap();
        run(&root, &["init", "-q", "-b", "main"]);
        run(&root, &["config", "user.email", "test@test"]);
        run(&root, &["config", "user.name", "test"]);

        // baseline commit
        std::fs::write(root.join("baseline.txt"), "v0").unwrap();
        run(&root, &["add", "."]);
        run(&root, &["commit", "-q", "-m", "baseline"]);
        run(&root, &["tag", "baseline-tag"]);

        // changes
        for (path, content) in changes {
            let full = root.join(path);
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(full, content).unwrap();
        }
        run(&root, &["add", "."]);
        run(&root, &["commit", "-q", "-m", "changes"]);
        root
    }

    fn run(cwd: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn ctx_for(repo: PathBuf) -> JudgeContext {
        JudgeContext {
            run_dirs: Vec::new(),
            final_run_dir: None,
            repo_root: repo,
            baseline_ref: "baseline-tag".to_string(),
            allowed_path_prefixes: default_allowed_prefixes(),
            determinism: None,
        }
    }

    #[test]
    fn passes_when_only_allowed_paths_change() {
        let repo = tiny_repo_with_changes(&[
            ("experimental/kg-validation/src/foo.rs", "fn main() {}"),
            ("docs/adr/0048-something.md", "# adr"),
            ("STATUS.md", "status"),
            (".beads/issues.db", "binary"),
        ]);
        let v = judge(&ctx_for(repo));
        assert!(v.passed, "{v:?}");
    }

    #[test]
    fn fails_when_sealed_surface_modified() {
        let repo = tiny_repo_with_changes(&[
            ("experimental/kg-validation/src/foo.rs", "ok"),
            // VIOLATION: production app code outside sandbox.
            ("src-tauri/src/main.rs", "oops"),
        ]);
        let v = judge(&ctx_for(repo));
        assert!(!v.passed);
        assert!(v
            .details
            .iter()
            .any(|d| d.contains("src-tauri/src/main.rs")));
    }

    #[test]
    fn fails_when_empty_prefix_allow_list() {
        let repo = tiny_repo_with_changes(&[("experimental/kg-validation/x.rs", "x")]);
        let mut c = ctx_for(repo);
        c.allowed_path_prefixes.clear();
        let v = judge(&c);
        assert!(!v.passed);
    }
}
