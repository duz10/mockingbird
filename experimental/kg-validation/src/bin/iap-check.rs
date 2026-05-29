//! `iap-check` — Wave 5 Iteration Acceptance Protocol CLI.
//!
//! Reads:
//!   --baseline-snapshot <path.json>      previous-iteration IapMetrics
//!   --candidate-run-dir <runs/iter-N-a/> the candidate (seed 42) run
//!   --stability-run-dir <runs/iter-N-b/> the seed-137 sibling, MUST
//!                                        already have been scored with
//!                                        `--stability-vs iter-N-a`
//!   --iteration <N>
//!   --label "<short description of the prompt change>"
//!   --journal <path.md>                  appended to (created if missing)
//!   [--update-baseline <path.json>]      on Accept, write candidate
//!                                        metrics here (typically same
//!                                        path as --baseline-snapshot)
//!
//! Exit codes:
//!   0  Accept
//!   2  Reject
//!   1  CLI / IO / parse error
//!
//! See `wiggum::iap` for the protocol definition.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde::Deserialize;

use kg_validation::wiggum::iap::{
    evaluate, render_journal_entry, IapInput, IapMetrics, IapStability, IapVerdict,
};

// ── Minimal deserialization slice ────────────────────────────────────
//
// `ScoreReport` (in `scoring::metrics`) only derives `Serialize`. Rather
// than cascade `Deserialize` through the production type graph, we read
// the JSON into a thin local shape with only the fields the IAP needs.
// If `score-run` ever renames these fields, both renames need to happen
// in lockstep — covered by the round-trip integration via the live
// runs/<id>/SCORE.json fixture format.

#[derive(Debug, Deserialize)]
struct ScoreSlice {
    per_metric: PerMetricSlice,
    #[serde(default)]
    stability: Option<StabilitySlice>,
}

#[derive(Debug, Deserialize)]
struct PerMetricSlice {
    clean_single_item_correct: RatioSlice,
    segmentation_correct: RatioSlice,
    category_correct: RatioSlice,
    entry_type_correct: RatioSlice,
    invented_dates_count: usize,
    tag_variant_collapse_correct: RatioSlice,
    junk_correct: RatioSlice,
}

#[derive(Debug, Deserialize)]
struct RatioSlice {
    percentage: f64,
}

#[derive(Debug, Deserialize)]
struct StabilitySlice {
    segmentation_agreement: RatioSlice,
    category_agreement: RatioSlice,
    entry_type_agreement: RatioSlice,
    date_agreement: RatioSlice,
    tag_set_exact_agreement: RatioSlice,
}

const HELP: &str = "\
iap-check - Wave 5 Iteration Acceptance Protocol CLI.

USAGE:
  iap-check --baseline-snapshot <path.json>
            --candidate-run-dir <dir>
            --stability-run-dir <dir>
            --iteration <N>
            --label \"<desc>\"
            --journal <path.md>
            [--update-baseline <path.json>]

EXIT CODES:
  0  Accept
  2  Reject
  1  CLI / IO / parse error
";

fn main() -> ExitCode {
    match real_main() {
        Ok(0) => ExitCode::from(0),
        Ok(2) => ExitCode::from(2),
        Ok(other) => ExitCode::from(other),
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::from(1)
        }
    }
}

struct Args {
    baseline_snapshot: PathBuf,
    candidate_run_dir: PathBuf,
    stability_run_dir: PathBuf,
    iteration: u32,
    label: String,
    journal: PathBuf,
    update_baseline: Option<PathBuf>,
}

fn real_main() -> anyhow::Result<u8> {
    let args = parse_args()?;
    let baseline: IapMetrics = serde_json::from_str(&fs::read_to_string(&args.baseline_snapshot)?)
        .map_err(|e| {
            anyhow::anyhow!(
                "parse baseline snapshot {}: {e}",
                args.baseline_snapshot.display()
            )
        })?;

    let candidate_score = load_score(&args.candidate_run_dir.join("SCORE.json"))?;
    let candidate_pcrp = parse_trust_eroding(&args.candidate_run_dir.join("PERSONA_REVIEW.md"))?;
    let candidate = metrics_from_score(&candidate_score, candidate_pcrp);

    let stability_score = load_score(&args.stability_run_dir.join("SCORE.json"))?;
    let stability = match stability_score.stability.as_ref() {
        Some(s) => IapStability {
            segmentation_agreement_pct: s.segmentation_agreement.percentage,
            category_agreement_pct: s.category_agreement.percentage,
            entry_type_agreement_pct: s.entry_type_agreement.percentage,
            date_agreement_pct: s.date_agreement.percentage,
            tag_set_exact_agreement_pct: s.tag_set_exact_agreement.percentage,
        },
        None => {
            anyhow::bail!(
                "stability run {} has no `stability` block in SCORE.json. \
                 Re-score it with `--stability-vs <candidate-run-id>`.",
                args.stability_run_dir.display()
            );
        }
    };

    let input = IapInput {
        iteration: args.iteration,
        label: args.label.clone(),
        baseline,
        candidate,
        candidate_stability: stability,
    };

    let verdict = evaluate(&input);
    let md = render_journal_entry(&input, &verdict);

    // Always echo to stdout so the operator sees it without reading
    // the file.
    print!("{md}");

    // Append journal.
    append_journal(&args.journal, &md)?;

    // On Accept, optionally advance the baseline snapshot.
    let exit = match &verdict {
        IapVerdict::Accept { .. } => {
            if let Some(p) = args.update_baseline.as_deref() {
                let serialized = serde_json::to_string_pretty(&candidate)?;
                fs::write(p, format!("{serialized}\n"))?;
                println!("[iap-check] baseline advanced -> {}", p.display());
            }
            0u8
        }
        IapVerdict::Reject { .. } => 2u8,
    };

    Ok(exit)
}

fn parse_args() -> anyhow::Result<Args> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut baseline_snapshot: Option<PathBuf> = None;
    let mut candidate_run_dir: Option<PathBuf> = None;
    let mut stability_run_dir: Option<PathBuf> = None;
    let mut iteration: Option<u32> = None;
    let mut label: Option<String> = None;
    let mut journal: Option<PathBuf> = None;
    let mut update_baseline: Option<PathBuf> = None;

    let mut i = 0;
    while i < argv.len() {
        let arg = argv[i].as_str();
        match arg {
            "--help" | "-h" => {
                println!("{HELP}");
                std::process::exit(0);
            }
            "--baseline-snapshot" => {
                baseline_snapshot = Some(PathBuf::from(take(&argv, &mut i, arg)?));
            }
            "--candidate-run-dir" => {
                candidate_run_dir = Some(PathBuf::from(take(&argv, &mut i, arg)?));
            }
            "--stability-run-dir" => {
                stability_run_dir = Some(PathBuf::from(take(&argv, &mut i, arg)?));
            }
            "--iteration" => {
                iteration = Some(
                    take(&argv, &mut i, arg)?
                        .parse()
                        .map_err(|e| anyhow::anyhow!("--iteration must be u32: {e}"))?,
                );
            }
            "--label" => label = Some(take(&argv, &mut i, arg)?),
            "--journal" => journal = Some(PathBuf::from(take(&argv, &mut i, arg)?)),
            "--update-baseline" => {
                update_baseline = Some(PathBuf::from(take(&argv, &mut i, arg)?));
            }
            unknown => anyhow::bail!("unknown flag: {unknown}\n\n{HELP}"),
        }
        i += 1;
    }

    Ok(Args {
        baseline_snapshot: baseline_snapshot
            .ok_or_else(|| anyhow::anyhow!("--baseline-snapshot is required"))?,
        candidate_run_dir: candidate_run_dir
            .ok_or_else(|| anyhow::anyhow!("--candidate-run-dir is required"))?,
        stability_run_dir: stability_run_dir
            .ok_or_else(|| anyhow::anyhow!("--stability-run-dir is required"))?,
        iteration: iteration.ok_or_else(|| anyhow::anyhow!("--iteration is required"))?,
        label: label.ok_or_else(|| anyhow::anyhow!("--label is required"))?,
        journal: journal.ok_or_else(|| anyhow::anyhow!("--journal is required"))?,
        update_baseline,
    })
}

fn take(args: &[String], i: &mut usize, flag: &str) -> anyhow::Result<String> {
    *i += 1;
    args.get(*i)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("{flag} requires a value"))
}

fn load_score(path: &Path) -> anyhow::Result<ScoreSlice> {
    let text =
        fs::read_to_string(path).map_err(|e| anyhow::anyhow!("read {}: {e}", path.display()))?;
    let s: ScoreSlice = serde_json::from_str(&text)
        .map_err(|e| anyhow::anyhow!("parse {}: {e}", path.display()))?;
    Ok(s)
}

/// Extract `trust_eroding_failures_count` from PERSONA_REVIEW.md.
/// The renderer emits `- trust_eroding_failures_count: **N**` on its
/// own line; parse that. If the file is missing, treat as 0 with a
/// warning rather than failing — PCRP is opt-out-able at the score
/// step for local dev, and we'd rather degrade gracefully than block.
fn parse_trust_eroding(path: &Path) -> anyhow::Result<usize> {
    if !path.exists() {
        eprintln!(
            "[iap-check] warning: PERSONA_REVIEW.md missing at {}; treating PCRP trust_eroding as 0",
            path.display()
        );
        return Ok(0);
    }
    let text =
        fs::read_to_string(path).map_err(|e| anyhow::anyhow!("read {}: {e}", path.display()))?;
    for line in text.lines() {
        // Tolerate optional leading "- " bullet and any surrounding
        // markdown emphasis.
        let trimmed = line.trim_start_matches('-').trim();
        if let Some(rest) = trimmed.strip_prefix("trust_eroding_failures_count:") {
            let n: usize = rest
                .trim()
                .trim_matches('*')
                .trim()
                .parse()
                .map_err(|e| anyhow::anyhow!("parse trust_eroding from {:?}: {e}", line))?;
            return Ok(n);
        }
    }
    anyhow::bail!(
        "trust_eroding_failures_count not found in {}",
        path.display()
    );
}

fn metrics_from_score(score: &ScoreSlice, pcrp_trust_eroding: usize) -> IapMetrics {
    let m = &score.per_metric;
    IapMetrics {
        invented_dates_count: m.invented_dates_count,
        segmentation_correct_pct: m.segmentation_correct.percentage,
        category_correct_pct: m.category_correct.percentage,
        entry_type_correct_pct: m.entry_type_correct.percentage,
        tag_collapse_correct_pct: m.tag_variant_collapse_correct.percentage,
        clean_single_item_correct_pct: m.clean_single_item_correct.percentage,
        junk_correct_pct: m.junk_correct.percentage,
        pcrp_trust_eroding,
    }
}

fn append_journal(path: &Path, body: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    // Seed the file with a top-line header on first write.
    let needs_header = !path.exists();
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    if needs_header {
        writeln!(
            f,
            "# Phase 0 KG — Wave 5 Iteration Journal\n\nGenerated by `iap-check`. Each iteration appends one `## Iter N` block.\n"
        )?;
    }
    f.write_all(body.as_bytes())?;
    Ok(())
}
