//! `cleaner_punctuation_eval` — ADR 0047 §Wave 3.1 regression eval.
//!
//! Runs 20 preamble-bearing synthetic transcripts through the
//! `meetings::llm_pass` engine with `BuiltIn("cleaner_punctuation")` —
//! the SAME path the production "Clean punctuation" Transform on
//! `LlmPassCard` hits — and asserts that every preamble token/phrase
//! survives in the LLM output. This catches a regression of the
//! Wave 1.1 fix (per-pass system header — commit `d6974a2`).
//!
//! ## Why a separate bin (not a unit test)
//!
//! 1. Live local model — REAL Ollama output, not stub. Determinism is
//!    sampled rather than asserted (LLMs aren't deterministic at the
//!    string level; we measure the rate at which preamble content
//!    survives).
//! 2. Fixture corpus may need expansion as new over-consolidation
//!    shapes are reported in field use.
//! 3. The report is the committed artifact backing this ADR section.
//!
//! ## Pass criteria
//!
//! Per fixture: every `expected_preserve` phrase (lowercase, punctuation
//! stripped, whitespace collapsed, surrounded by whitespace) must occur
//! as a substring of the LLM-output text (same normalization).
//!
//! Per-run summary: pass rate (fixtures with ALL phrases preserved).
//! ADR §Wave 3 targets ≥ 18/20 (90 %) on the post-Wave-1 system.
//!
//! ## Usage
//!
//! ```text
//! powershell -File scripts\cargo-with-cuda.ps1 run --release \
//!   --bin cleaner_punctuation_eval [-- --report-path path/to/out.md]
//! ```
//!
//! Exits 0 if pass-rate ≥ floor (default 18/20). Exits 2 below floor
//! so this can be wired into CI later if desired. Exits 1 on harness
//! error (Ollama unreachable, model not pulled, etc.).

use std::path::PathBuf;
use std::time::Instant;

use serde::Deserialize;

use mockingbird_lib::cleanup::OllamaProvider;
use mockingbird_lib::meetings::llm_pass::{
    run_llm_pass_with_provider, LlmPassPrompt, LlmPassRequest,
};

// --------------------------------------------------------------------
// Fixture schema.
// --------------------------------------------------------------------

const FIXTURES_JSON: &str = include_str!("../../../eval/cleaner_punctuation_fixtures.json");

#[derive(Debug, Deserialize)]
struct FixtureFile {
    fixtures: Vec<Fixture>,
}

#[derive(Debug, Deserialize, Clone)]
struct Fixture {
    id: String,
    input: String,
    expected_preserve: Vec<String>,
}

// --------------------------------------------------------------------
// Normalization + matching.
// --------------------------------------------------------------------

/// Lowercase, replace every non-alphanumeric ASCII char with a space,
/// collapse runs of whitespace, surround the whole thing with single
/// spaces. The surrounding spaces let us check phrase boundaries with
/// a plain `contains(" phrase ")` — no regex needed.
fn normalize_for_match(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push(' ');
    let mut last_was_space = true;
    for ch in s.chars() {
        let mapped = if ch.is_ascii_alphanumeric() {
            ch.to_ascii_lowercase()
        } else {
            ' '
        };
        if mapped == ' ' {
            if !last_was_space {
                out.push(' ');
                last_was_space = true;
            }
        } else {
            out.push(mapped);
            last_was_space = false;
        }
    }
    if !out.ends_with(' ') {
        out.push(' ');
    }
    out
}

/// Returns the phrases (in `expected`) that did NOT survive in `output`.
///
/// Both sides are passed through `normalize_for_match`, which surrounds
/// the string with single spaces — so a plain substring check on the
/// normalized phrase enforces whole-word boundaries on both ends
/// ("last week" matches " last week " but not "lastweekend").
fn missing_phrases(output: &str, expected: &[String]) -> Vec<String> {
    let norm_out = normalize_for_match(output);
    expected
        .iter()
        .filter(|phrase| !norm_out.contains(&normalize_for_match(phrase)))
        .cloned()
        .collect()
}

// --------------------------------------------------------------------
// One fixture run.
// --------------------------------------------------------------------

struct FixtureOutcome {
    id: String,
    input: String,
    expected: Vec<String>,
    output: String,
    missing: Vec<String>,
    latency_ms: u64,
    error: Option<String>,
}

impl FixtureOutcome {
    fn passed(&self) -> bool {
        self.error.is_none() && self.missing.is_empty()
    }
}

fn run_one(provider: &OllamaProvider, fixture: &Fixture, model_id: &str) -> FixtureOutcome {
    let req = LlmPassRequest {
        meeting_uuid: format!("eval-{}", fixture.id),
        prompt: LlmPassPrompt::BuiltIn("cleaner_punctuation"),
        model_id: Some(model_id.to_string()),
    };
    let start = Instant::now();
    match run_llm_pass_with_provider(&req, &fixture.input, provider) {
        Ok(result) => {
            let missing = missing_phrases(&result.text, &fixture.expected_preserve);
            FixtureOutcome {
                id: fixture.id.clone(),
                input: fixture.input.clone(),
                expected: fixture.expected_preserve.clone(),
                output: result.text,
                missing,
                latency_ms: result.latency_ms,
                error: None,
            }
        }
        Err(e) => FixtureOutcome {
            id: fixture.id.clone(),
            input: fixture.input.clone(),
            expected: fixture.expected_preserve.clone(),
            output: String::new(),
            missing: fixture.expected_preserve.clone(),
            latency_ms: start.elapsed().as_millis() as u64,
            error: Some(format!("{e}")),
        },
    }
}

// --------------------------------------------------------------------
// Report rendering.
// --------------------------------------------------------------------

fn render_report(
    label: &str,
    timestamp_utc: &str,
    model_id: &str,
    outcomes: &[FixtureOutcome],
    grid_elapsed_secs: f32,
) -> String {
    let total = outcomes.len();
    let passed = outcomes.iter().filter(|o| o.passed()).count();
    let errored = outcomes.iter().filter(|o| o.error.is_some()).count();
    let avg_latency = if total > 0 {
        outcomes.iter().map(|o| o.latency_ms).sum::<u64>() / total as u64
    } else {
        0
    };

    let mut md = String::new();
    md.push_str(&format!(
        "# `cleaner_punctuation` regression eval — {label}\n\n"
    ));
    md.push_str("ADR 0047 §Wave 3.1. Each fixture sends a preamble-bearing synthetic transcript\n");
    md.push_str(
        "through `meetings::llm_pass::run_llm_pass_with_provider(_, _, &OllamaProvider)`\n",
    );
    md.push_str("with `BuiltIn(\"cleaner_punctuation\")`. The same path the production\n");
    md.push_str(
        "\"Clean punctuation\" Transform on `LlmPassCard` hits. Each `expected_preserve`\n",
    );
    md.push_str(
        "phrase must appear in the LLM output (normalized: lowercase, punctuation stripped,\n",
    );
    md.push_str("whitespace collapsed, whole-word match).\n\n");

    md.push_str("## Summary\n\n");
    md.push_str("| Field | Value |\n|---|---|\n");
    md.push_str(&format!("| Timestamp (UTC) | `{timestamp_utc}` |\n"));
    md.push_str(&format!("| Model | `{model_id}` |\n"));
    md.push_str(&format!("| Fixtures | {total} |\n"));
    md.push_str(&format!("| Passed | **{passed} / {total}** |\n"));
    md.push_str(&format!("| Errored | {errored} |\n"));
    md.push_str(&format!("| Avg latency | {avg_latency} ms |\n"));
    md.push_str(&format!(
        "| Grid wall-clock | {grid_elapsed_secs:.1} s |\n\n"
    ));

    md.push_str("## Per-fixture results\n\n");
    md.push_str("| ID | Pass | Missing phrases | Latency |\n");
    md.push_str("|---|---|---|---|\n");
    for o in outcomes {
        let pass_cell = if o.passed() { "✓" } else { "✗" };
        let missing_cell = if let Some(e) = &o.error {
            format!("ERROR: {e}")
        } else if o.missing.is_empty() {
            "—".to_string()
        } else {
            o.missing
                .iter()
                .map(|p| format!("`{p}`"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        md.push_str(&format!(
            "| `{}` | {} | {} | {} ms |\n",
            o.id, pass_cell, missing_cell, o.latency_ms
        ));
    }
    md.push('\n');

    md.push_str("## Failures — full text\n\n");
    let failures: Vec<&FixtureOutcome> = outcomes.iter().filter(|o| !o.passed()).collect();
    if failures.is_empty() {
        md.push_str("None — all 20 fixtures preserved every expected phrase. 🎉\n\n");
    } else {
        for o in &failures {
            md.push_str(&format!("### `{}`\n\n", o.id));
            md.push_str(&format!("**Input:**\n\n```\n{}\n```\n\n", o.input));
            md.push_str(&format!(
                "**Expected to preserve:** {}\n\n",
                o.expected
                    .iter()
                    .map(|p| format!("`{p}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            md.push_str(&format!("**LLM output:**\n\n```\n{}\n```\n\n", o.output));
            if let Some(e) = &o.error {
                md.push_str(&format!("**Error:** `{e}`\n\n"));
            } else {
                md.push_str(&format!(
                    "**Missing:** {}\n\n",
                    o.missing
                        .iter()
                        .map(|p| format!("`{p}`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            md.push_str("---\n\n");
        }
    }

    md
}

// --------------------------------------------------------------------
// CLI.
// --------------------------------------------------------------------

struct Args {
    label: String,
    model_id: String,
    floor: usize,
    report_path: Option<PathBuf>,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            label: "adr0047-wave3".to_string(),
            // Default meetings-model (matches `meetings::llm_pass::DEFAULT_MODEL_ID`).
            // Kept as a string here rather than importing the constant so a future
            // model-id rename doesn't silently change the eval's anchor.
            model_id: "qwen2.5:3b-instruct-q4_K_M".to_string(),
            floor: 18,
            report_path: None,
        }
    }
}

fn parse_args() -> Args {
    let mut a = Args::default();
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--label" => {
                if let Some(v) = it.next() {
                    a.label = v;
                }
            }
            "--model" => {
                if let Some(v) = it.next() {
                    a.model_id = v;
                }
            }
            "--floor" => {
                if let Some(v) = it.next() {
                    a.floor = v.parse().unwrap_or(18);
                }
            }
            "--report-path" => {
                if let Some(v) = it.next() {
                    a.report_path = Some(PathBuf::from(v));
                }
            }
            "-h" | "--help" => {
                eprintln!(
                    "Usage: cleaner_punctuation_eval [--label NAME] [--model ID] \
                     [--floor N] [--report-path PATH]\n\
                     Defaults: --label adr0047-wave3 --model qwen2.5:3b-instruct-q4_K_M --floor 18"
                );
                std::process::exit(0);
            }
            other => eprintln!("warn: ignoring unknown arg `{other}`"),
        }
    }
    a
}

// --------------------------------------------------------------------
// Entrypoint.
// --------------------------------------------------------------------

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                tracing_subscriber::EnvFilter::new("warn,cleaner_punctuation_eval=info")
            }),
        )
        .with_writer(std::io::stderr)
        .init();

    let args = parse_args();
    eprintln!(
        "cleaner_punctuation_eval: label={} model={} floor={}",
        args.label, args.model_id, args.floor
    );

    match run(args) {
        Ok(exit_code) => std::process::exit(exit_code),
        Err(e) => {
            eprintln!("FATAL: {e}");
            std::process::exit(1);
        }
    }
}

fn run(args: Args) -> Result<i32, Box<dyn std::error::Error>> {
    let file: FixtureFile = serde_json::from_str(FIXTURES_JSON)?;
    let total = file.fixtures.len();
    eprintln!("cleaner_punctuation_eval: {total} fixtures loaded");

    let provider = OllamaProvider::new();
    provider
        .health_check()
        .map_err(|e| format!("Ollama health-check failed (is `ollama serve` running?): {e}"))?;
    eprintln!("cleaner_punctuation_eval: Ollama healthy");

    let grid_start = Instant::now();
    let mut outcomes = Vec::with_capacity(total);
    for (i, fx) in file.fixtures.iter().enumerate() {
        eprintln!("[{}/{}] {}", i + 1, total, fx.id);
        let outcome = run_one(&provider, fx, &args.model_id);
        let status = if outcome.passed() {
            "PASS"
        } else if outcome.error.is_some() {
            "ERROR"
        } else {
            "FAIL"
        };
        eprintln!(
            "    {} ({} ms){}",
            status,
            outcome.latency_ms,
            if outcome.missing.is_empty() {
                String::new()
            } else {
                format!(" missing={:?}", outcome.missing)
            }
        );
        outcomes.push(outcome);
    }
    let grid_elapsed = grid_start.elapsed();

    let passed = outcomes.iter().filter(|o| o.passed()).count();
    let ts = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let report = render_report(
        &args.label,
        &ts,
        &args.model_id,
        &outcomes,
        grid_elapsed.as_secs_f32(),
    );

    let out_path = args.report_path.unwrap_or_else(|| {
        PathBuf::from("docs")
            .join("cleanup")
            .join(format!("eval-{}-{}.md", args.label, ts))
    });
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&out_path, &report)?;
    eprintln!(
        "\ncleaner_punctuation_eval: wrote report → {}",
        out_path.display()
    );

    println!(
        "\n=== cleaner_punctuation_eval summary ===\n  passed: {passed}/{total}\n  floor:  {}\n  wall:   {:.1}s\n  report: {}",
        args.floor,
        grid_elapsed.as_secs_f32(),
        out_path.display()
    );

    if passed >= args.floor {
        Ok(0)
    } else {
        eprintln!("FAIL: {passed}/{total} below floor {}", args.floor);
        Ok(2)
    }
}

// --------------------------------------------------------------------
// Tests for the harness internals (matching logic, normalization).
// LLM-driven fixture pass-rate is captured in the report, not asserted
// here — would require live Ollama in test runs.
// --------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_lowercases_and_strips_punctuation() {
        let got = normalize_for_match("Hello, World! It's me.");
        assert_eq!(got, " hello world it s me ");
    }

    #[test]
    fn normalize_collapses_runs_of_whitespace() {
        let got = normalize_for_match("a\t\tb\n\nc   d");
        assert_eq!(got, " a b c d ");
    }

    #[test]
    fn missing_phrases_finds_dropped_word() {
        let output = "I was wondering about the project status";
        let expected = vec!["yesterday".to_string(), "wondering".to_string()];
        let missing = missing_phrases(output, &expected);
        assert_eq!(missing, vec!["yesterday".to_string()]);
    }

    #[test]
    fn missing_phrases_matches_multiword_phrase_with_boundary() {
        let output = "the meeting last week was productive";
        let expected = vec!["last week".to_string()];
        assert!(missing_phrases(output, &expected).is_empty());
    }

    #[test]
    fn missing_phrases_rejects_substring_inside_a_word() {
        // "lastweekend" must NOT count as preserving "last week".
        let output = "the lastweekend retrospective";
        let expected = vec!["last week".to_string()];
        let missing = missing_phrases(output, &expected);
        assert_eq!(missing, vec!["last week".to_string()]);
    }

    #[test]
    fn missing_phrases_punctuation_in_output_does_not_block_match() {
        let output = "yesterday, I was thinking — kind of — about it.";
        let expected = vec!["kind of".to_string(), "yesterday".to_string()];
        assert!(missing_phrases(output, &expected).is_empty());
    }

    #[test]
    fn missing_phrases_empty_expected_means_none_missing() {
        assert!(missing_phrases("anything goes", &[]).is_empty());
    }

    #[test]
    fn fixtures_file_parses_and_has_twenty() {
        let file: FixtureFile = serde_json::from_str(FIXTURES_JSON).expect("parse fixtures");
        assert_eq!(file.fixtures.len(), 20, "Wave 3.1 specifies 20 fixtures");
        for fx in &file.fixtures {
            assert!(!fx.id.is_empty(), "fixture id is required");
            assert!(!fx.input.is_empty(), "{} has empty input", fx.id);
            assert!(
                !fx.expected_preserve.is_empty(),
                "{} has no expected_preserve",
                fx.id
            );
        }
    }
}
