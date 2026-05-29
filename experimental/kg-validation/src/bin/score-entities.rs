//! `score-entities` — Wave 0.5.4 / `mb-o4ni` entity-extraction probe driver.
//!
//! Walks an existing run's per-segment text (the `raw/<id>/segment.json`
//! files produced by `run-corpus`), calls the new `extract_entities`
//! pass once per segment via Ollama, aggregates entities per
//! dictation, scores against the hand-labeled
//! `corpus/entity-labels.jsonl` ground truth, and writes
//! `runs/<out-run>/entities/<id>.json` + `runs/<out-run>/ENTITY_SCORE.json`
//! + `runs/<out-run>/ENTITY_SUMMARY.md`.
//!
//! Decoupled from the main pipeline orchestrator for the probe phase
//! per ADR 0049 Move 4 / LESSONS PINNED P11. Promotion to in-band
//! depends on the ≥ 50% bar + Wave 0.5.6 REPORT acceptance.
//!
//! ## Why not extend `run-corpus`?
//!
//! `run-corpus` is the production pipeline orchestrator and just got
//! a clean Wave 0.5.3 closure (commit `8fdc7fb`). Touching it for an
//! unproven probe would create a coupling we'd then have to unwind
//! if Wave 0.5.4 rejects. The probe binary reuses the existing
//! per-segment artifact contract — clean failure isolation, zero
//! risk to the freshly-landed Wave 0.5.3 work.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use kg_validation::ollama::{GenerateOptions, OllamaClient};
use kg_validation::passes::extract_entities::{extract_entities, EntityExtraction};
use kg_validation::schema_loader::Schema;
use kg_validation::scoring::entity_quality::{
    load_labels, score_entity_quality, stability_jaccard, EntityQualityScore,
};

const HELP: &str = "\
score-entities — Wave 0.5.4 entity-extraction probe (mb-o4ni)

Reads per-segment text from a source run's raw/<id>/segment.json,
calls extract_entities per segment via Ollama, aggregates per
dictation, and scores against corpus/entity-labels.jsonl.

USAGE:
  score-entities [FLAGS]

FLAGS:
  --source-run <path>        Source run dir (must have raw/<id>/segment.json)
  --out-run <path>           Output run dir (created; gets entities/, ENTITY_SCORE.json, ENTITY_SUMMARY.md)
  --model <name>             Ollama model id (default: qwen2.5:7b-instruct-q4_K_M)
  --seed <i64>               Pin Ollama sampling seed (required for stability eval)
  --labels-path <path>       Ground-truth JSONL (default: corpus/entity-labels.jsonl)
  --ollama-url <url>         Ollama base URL (default: http://localhost:11434)
  --temperature <f32>        Sampling temperature (default: 0.2, per ADR 0048 §G4)
  --compare-against <path>   Optional second ENTITY_SCORE.json or run dir for stability metric
  --help                     Print this and exit
";

#[derive(Debug)]
struct Args {
    source_run: PathBuf,
    out_run: PathBuf,
    model: String,
    seed: i64,
    labels_path: PathBuf,
    ollama_url: String,
    temperature: f32,
    compare_against: Option<PathBuf>,
}

fn main() -> ExitCode {
    match real_main() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::from(1)
        }
    }
}

fn real_main() -> anyhow::Result<()> {
    let args = parse_args()?;

    eprintln!("score-entities mb-o4ni");
    eprintln!("  source-run : {}", args.source_run.display());
    eprintln!("  out-run    : {}", args.out_run.display());
    eprintln!("  model      : {}", args.model);
    eprintln!("  seed       : {}", args.seed);
    eprintln!("  labels     : {}", args.labels_path.display());

    let labels = load_labels(&args.labels_path)?;
    eprintln!("  labeled    : {} dictations\n", labels.len());

    let schema = Schema::load_default()?;
    let prompt_body = schema
        .prompt_body("extract_entities", &args.model)
        .map(str::to_owned)?;
    let profile = schema.profile_for(&args.model).to_string();
    eprintln!(
        "  profile    : {profile} (prompt body {} chars)\n",
        prompt_body.len()
    );

    let dispatcher = OllamaClient::with_base_url(args.ollama_url.clone());
    let opts = GenerateOptions {
        temperature: args.temperature,
        seed: Some(args.seed),
        num_ctx: 4096,
    };

    let entities_out_dir = args.out_run.join("entities");
    std::fs::create_dir_all(&entities_out_dir)?;

    let mut extracted: HashMap<String, EntityExtraction> = HashMap::new();
    let cases = list_persona_cases(&args.source_run.join("raw"))?;
    eprintln!("running entity extraction over {} cases ...\n", cases.len());

    for (i, case) in cases.iter().enumerate() {
        let seg_path = args.source_run.join("raw").join(case).join("segment.json");
        let segments = read_segments(&seg_path).with_context_msg(&seg_path)?;

        let mut all_entities = EntityExtraction::default();
        let mut seg_failures: Vec<String> = Vec::new();
        for (seg_idx, seg_text) in segments.iter().enumerate() {
            match extract_entities(&dispatcher, &args.model, &prompt_body, seg_text, &opts) {
                Ok(e) => {
                    for ent in e.entities {
                        // Dedupe across segments by (name, type).
                        if !all_entities
                            .entities
                            .iter()
                            .any(|x| x.name == ent.name && x.entity_type == ent.entity_type)
                        {
                            all_entities.entities.push(ent);
                        }
                    }
                }
                Err(err) => {
                    seg_failures.push(format!("seg {seg_idx}: {err}"));
                }
            }
        }

        // Persist per-dictation entities so failures and successes are
        // both auditable. Mirror the run-corpus artifact discipline.
        let case_out = entities_out_dir.join(format!("{case}.json"));
        let payload = serde_json::json!({
            "persona_case": case,
            "model": args.model,
            "seed": args.seed,
            "profile": profile,
            "segment_count": segments.len(),
            "segment_failures": seg_failures,
            "entities": all_entities.entities,
        });
        std::fs::write(&case_out, serde_json::to_string_pretty(&payload)?)?;

        extracted.insert(case.clone(), all_entities);
        eprintln!(
            "  [{:02}/{}] {case}: {} entities ({} segs{})",
            i + 1,
            cases.len(),
            extracted[case].entities.len(),
            segments.len(),
            if seg_failures.is_empty() {
                String::new()
            } else {
                format!(", {} fail", seg_failures.len())
            }
        );
    }

    let score = score_entity_quality(&extracted, &labels);

    let stability = match &args.compare_against {
        Some(p) => Some(compute_stability(p, &extracted)?),
        None => None,
    };

    persist_score(&args.out_run, &args, &score, stability)?;

    eprintln!("\nEntity quality (Wave 0.5.4 bar: ≥ 50%):");
    eprintln!(
        "  corpus_average_jaccard       = {:.2}%  ({})",
        score.corpus_average_jaccard * 100.0,
        if score.meets_bar() { "PASS" } else { "FAIL" }
    );
    eprintln!(
        "  corpus_average_fuzzy_jaccard = {:.2}%  (observational sidecar)",
        score.corpus_average_fuzzy_jaccard * 100.0
    );
    if let Some(s) = stability {
        eprintln!(
            "  stability vs comparison      = {:.2}%  ({})",
            s * 100.0,
            if s >= 0.75 { "PASS" } else { "FAIL" }
        );
    }

    Ok(())
}

fn parse_args() -> anyhow::Result<Args> {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    if raw.iter().any(|a| a == "--help" || a == "-h") {
        println!("{HELP}");
        std::process::exit(0);
    }

    let mut source_run: Option<PathBuf> = None;
    let mut out_run: Option<PathBuf> = None;
    let mut model = "qwen2.5:7b-instruct-q4_K_M".to_string();
    let mut seed: Option<i64> = None;
    let mut labels_path = PathBuf::from("corpus/entity-labels.jsonl");
    let mut ollama_url = "http://localhost:11434".to_string();
    let mut temperature = 0.2_f32;
    let mut compare_against: Option<PathBuf> = None;

    let mut i = 0;
    while i < raw.len() {
        let take = |raw: &[String], i: &mut usize, name: &str| -> anyhow::Result<String> {
            *i += 1;
            raw.get(*i)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("{name} requires a value"))
        };
        match raw[i].as_str() {
            "--source-run" => source_run = Some(PathBuf::from(take(&raw, &mut i, "--source-run")?)),
            "--out-run" => out_run = Some(PathBuf::from(take(&raw, &mut i, "--out-run")?)),
            "--model" => model = take(&raw, &mut i, "--model")?,
            "--seed" => seed = Some(take(&raw, &mut i, "--seed")?.parse()?),
            "--labels-path" => labels_path = PathBuf::from(take(&raw, &mut i, "--labels-path")?),
            "--ollama-url" => ollama_url = take(&raw, &mut i, "--ollama-url")?,
            "--temperature" => temperature = take(&raw, &mut i, "--temperature")?.parse()?,
            "--compare-against" => {
                compare_against = Some(PathBuf::from(take(&raw, &mut i, "--compare-against")?))
            }
            other => anyhow::bail!("unknown flag {other:?}"),
        }
        i += 1;
    }

    Ok(Args {
        source_run: source_run.ok_or_else(|| anyhow::anyhow!("--source-run required"))?,
        out_run: out_run.ok_or_else(|| anyhow::anyhow!("--out-run required"))?,
        model,
        seed: seed.ok_or_else(|| anyhow::anyhow!("--seed required for stability eval"))?,
        labels_path,
        ollama_url,
        temperature,
        compare_against,
    })
}

/// Walk `raw/` and return per-dictation case ids that have a
/// `segment.json` on disk. Sort for stable iteration order.
fn list_persona_cases(raw_dir: &Path) -> anyhow::Result<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(raw_dir)? {
        let e = entry?;
        if !e.file_type()?.is_dir() {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_string();
        if !name.starts_with("persona-") {
            continue;
        }
        if e.path().join("segment.json").exists() {
            out.push(name);
        }
    }
    out.sort();
    Ok(out)
}

/// Per-dictation segment file shape (run-corpus artifact).
#[derive(serde::Deserialize)]
struct SegmentArtifact {
    parsed_segments: Vec<String>,
}

fn read_segments(p: &Path) -> anyhow::Result<Vec<String>> {
    let text = std::fs::read_to_string(p)?;
    let sa: SegmentArtifact = serde_json::from_str(&text)?;
    Ok(sa.parsed_segments)
}

/// Load a comparison set of extracted entities — either directly from
/// the sibling out-run's `entities/<case>.json` files (when the path
/// is a run dir), or from a previously-serialized ENTITY_SCORE.json
/// (when it's a single file). The first form is what `--compare-against`
/// gets in seed-vs-seed stability checks.
fn compute_stability(
    comparison: &Path,
    against: &HashMap<String, EntityExtraction>,
) -> anyhow::Result<f64> {
    let comp_dir = if comparison.is_dir() {
        comparison.join("entities")
    } else {
        anyhow::bail!(
            "--compare-against {} is not a directory; expected a run dir with entities/",
            comparison.display()
        )
    };
    let mut comp: HashMap<String, EntityExtraction> = HashMap::new();
    for entry in std::fs::read_dir(&comp_dir)? {
        let e = entry?;
        if e.path().extension().is_none_or(|x| x != "json") {
            continue;
        }
        let payload: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(e.path())?)?;
        let case = payload
            .get("persona_case")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing persona_case in {}", e.path().display()))?
            .to_string();
        let entities = serde_json::from_value(
            payload
                .get("entities")
                .cloned()
                .unwrap_or_else(|| serde_json::Value::Array(vec![])),
        )?;
        comp.insert(case, EntityExtraction { entities });
    }
    Ok(stability_jaccard(against, &comp))
}

fn persist_score(
    out_run: &Path,
    args: &Args,
    score: &EntityQualityScore,
    stability: Option<f64>,
) -> anyhow::Result<()> {
    let summary_path = out_run.join("ENTITY_SCORE.json");
    let payload = serde_json::json!({
        "wave": "0.5.4 / mb-o4ni",
        "model": args.model,
        "seed": args.seed,
        "source_run": args.source_run.display().to_string(),
        "labels_path": args.labels_path.display().to_string(),
        "score": score,
        "stability_jaccard": stability,
        "meets_bar": score.meets_bar(),
        "bar": 0.50,
    });
    std::fs::write(&summary_path, serde_json::to_string_pretty(&payload)?)?;

    let md_path = out_run.join("ENTITY_SUMMARY.md");
    let md = render_summary(args, score, stability);
    std::fs::write(&md_path, md)?;

    Ok(())
}

fn render_summary(args: &Args, score: &EntityQualityScore, stability: Option<f64>) -> String {
    let mut s = String::new();
    s.push_str("# ENTITY_SUMMARY — Wave 0.5.4 / mb-o4ni\n\n");
    s.push_str(&format!("- **model**: `{}`\n", args.model));
    s.push_str(&format!("- **seed**: `{}`\n", args.seed));
    s.push_str(&format!(
        "- **source-run**: `{}`\n",
        args.source_run.display()
    ));
    s.push_str(&format!("- **labels**: `{}`\n", args.labels_path.display()));
    s.push_str(&format!(
        "- **scored**: {} dictations\n\n",
        score.scored_count
    ));

    s.push_str("## Headline\n\n");
    s.push_str(&format!(
        "- **corpus_average_jaccard** = **{:.2}%**  (bar: ≥ 50% → {})\n",
        score.corpus_average_jaccard * 100.0,
        if score.meets_bar() {
            "**PASS**"
        } else {
            "**FAIL**"
        }
    ));
    s.push_str(&format!(
        "- corpus_average_fuzzy_jaccard = {:.2}%  (observational sidecar; Levenshtein ≤ 2 + alias match)\n",
        score.corpus_average_fuzzy_jaccard * 100.0
    ));
    if let Some(s_val) = stability {
        s.push_str(&format!(
            "- **stability_jaccard** = **{:.2}%**  (bar: ≥ 75% → {})\n",
            s_val * 100.0,
            if s_val >= 0.75 {
                "**PASS**"
            } else {
                "**FAIL**"
            }
        ));
    }
    s.push('\n');

    s.push_str("## Per-dictation scorecard\n\n");
    s.push_str("| case | strict | fuzzy | missed | extra |\n");
    s.push_str("|---|---:|---:|---|---|\n");
    let mut rows: Vec<_> = score.per_entry.iter().collect();
    rows.sort_by(|a, b| a.persona_case.cmp(&b.persona_case));
    for e in &rows {
        let missed = if e.missed.is_empty() {
            "—".to_string()
        } else {
            e.missed
                .iter()
                .map(|(n, t)| format!("{n}:{t}"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let extra = if e.extra.is_empty() {
            "—".to_string()
        } else {
            e.extra
                .iter()
                .map(|(n, t)| format!("{n}:{t}"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        s.push_str(&format!(
            "| {} | {:.0}% | {:.0}% | {} | {} |\n",
            e.persona_case,
            e.jaccard * 100.0,
            e.fuzzy_jaccard * 100.0,
            missed,
            extra,
        ));
    }
    s
}

trait ContextExt<T> {
    fn with_context_msg(self, p: &Path) -> anyhow::Result<T>;
}

impl<T, E: std::fmt::Display> ContextExt<T> for Result<T, E> {
    fn with_context_msg(self, p: &Path) -> anyhow::Result<T> {
        self.map_err(|e| anyhow::anyhow!("{}: {e}", p.display()))
    }
}
