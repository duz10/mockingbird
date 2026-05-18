//! `mode_eval` — empirical mode-tuning evaluation harness (ADR 0024).
//!
//! Runs a fixture corpus through the **full cleanup pipeline**
//! (Preprocessor → DB-resolved prompt + dictionary + few-shot →
//! OllamaProvider) for one or more modes and produces a side-by-side
//! markdown report at `docs/cleanup/eval-{label}-{timestamp}.md`.
//!
//! ## Why a separate bin (not a unit test)
//!
//! 1. Live local model — we want REAL Ollama output, not stub.
//! 2. Repeatable manual workflow: edit prompts → re-run → diff
//!    reports. A bin is the right shape; tests want to be silent.
//! 3. Report is the committed artifact backing ADR 0024.
//!
//! ## Pipeline parity
//!
//! Mirrors `LlmCleaner::run_cleanup` step-for-step so the eval
//! measures what users actually get. Only intentional difference:
//! every intermediate stage (preprocessed text, prompt length,
//! provider latency separately from total) is exposed instead of
//! just the final string. Production cleaner stays untouched.
//!
//! ## Usage
//!
//! ```text
//! cargo run --release --bin mode_eval -- [--label baseline] \
//!     [--modes casual,normal,formal] [--only 01_enum_short,02_enum_medium]
//! ```
//!
//! Exits non-zero on harness error (DB/Ollama unreachable). A failed
//! cleanup for any individual fixture is **recorded** in the report,
//! not fatal — the whole point is to see where the LLM struggles.

mod report;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use rusqlite::{params, Connection};
use serde::Deserialize;

use mockingbird_lib::cleanup::{
    few_shot, prompt_builder, CleanupProvider, CleanupRequest, OllamaProvider, Preprocessor,
    PREPROCESSOR_VERSION,
};
use mockingbird_lib::db::{dictionary, prompts, Database};

use report::{render_report, score_preservation, ModeAggregate, PreservationScore};

// --------------------------------------------------------------------
// Fixture schema — mirrors `src-tauri/eval/baseline.json`.
// --------------------------------------------------------------------

/// Embedded at compile time so the bin is self-contained.
const FIXTURES_JSON: &str = include_str!("../../../eval/baseline.json");

#[derive(Debug, Deserialize)]
struct FixtureFile {
    fixtures: Vec<Fixture>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Fixture {
    pub id: String,
    pub category: String,
    pub length: String,
    pub raw: String,
    pub intent: String,
    pub must_preserve: Vec<String>,
    /// Optional equivalence groups. If ANY term in a group appears in the
    /// output, EVERY term in that group counts as preserved. Lets us
    /// declare e.g. `[["bad", "poor", "subpar"]]` so formal register-lift
    /// paraphrases don't register as preservation failures. Empty by
    /// default for fixtures where literal preservation is the bar.
    #[serde(default)]
    pub must_preserve_alts: Vec<Vec<String>>,
    #[serde(default)]
    pub mode_hints: BTreeMap<String, String>,
}

// --------------------------------------------------------------------
// Per-mode config resolved from the live DB (so we test what users get).
// --------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ModeRow {
    pub slug: String,
    pub display_name: String,
    pub model_id: String,
    pub temperature: f32,
    pub max_tokens: u32,
    #[allow(dead_code)] // captured for traceability; not used in the report yet
    pub prompt_id: i64,
    pub prompt_version: i64,
    pub prompt_body: String,
}

fn load_mode(conn: &Connection, slug: &str) -> Result<ModeRow, Box<dyn std::error::Error>> {
    let (display_name, model_id, temperature, max_tokens, prompt_id): (
        String,
        String,
        f32,
        i64,
        i64,
    ) = conn.query_row(
        "SELECT display_name, model_id, temperature, max_tokens, prompt_id
         FROM modes WHERE slug = ?1 AND enabled = 1",
        params![slug],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
    )?;
    let prompt = prompts::get_latest_for_mode(conn, slug)?
        .ok_or_else(|| format!("no prompt rows for mode {slug}"))?;
    Ok(ModeRow {
        slug: slug.to_string(),
        display_name,
        model_id,
        temperature,
        max_tokens: max_tokens as u32,
        prompt_id,
        prompt_version: prompt.version,
        prompt_body: prompt.body,
    })
}

// --------------------------------------------------------------------
// One cleanup attempt — full pipeline, with every stage observable.
// --------------------------------------------------------------------

#[derive(Debug)]
pub struct RunResult {
    pub output: String,
    pub preprocessed: String,
    pub preprocess_ms: u64,
    pub fillers_stripped: usize,
    pub stutters_collapsed: usize,
    pub self_corrections: usize,
    pub cues_rendered: usize,
    pub llm_ms: u64,
    pub total_ms: u64,
    #[allow(dead_code)] // surfaced via tracing logs; not in report yet
    pub model_used: String,
    #[allow(dead_code)]
    pub input_tokens: Option<u32>,
    #[allow(dead_code)]
    pub output_tokens: Option<u32>,
    pub error: Option<String>,
}

/// Mirror of `LlmCleaner::run_cleanup` with every stage exposed.
/// Deliberately not reusing `LlmCleaner::clean` because it opaque-boxes
/// the intermediate text; replicating ~30 lines costs less than mutating
/// production for a test rig.
fn run_one(
    provider: &dyn CleanupProvider,
    db: &Arc<Mutex<Connection>>,
    preprocessor: &Preprocessor,
    mode: &ModeRow,
    raw: &str,
) -> RunResult {
    let total_start = Instant::now();

    // Stage 0: preprocessor.
    let pre_start = Instant::now();
    let pre = preprocessor.process(raw);
    let preprocess_ms = pre_start.elapsed().as_millis() as u64;
    let pre_text = pre.text;
    let notes = pre.notes;
    let cues_rendered = notes.punctuation_cues_rendered
        + notes.layout_cues_rendered
        + notes.quote_bracket_cues_rendered;

    let mk_partial = |err: String, model_used: &str| RunResult {
        output: raw.to_string(),
        preprocessed: pre_text.clone(),
        preprocess_ms,
        fillers_stripped: notes.fillers_stripped,
        stutters_collapsed: notes.stutters_collapsed,
        self_corrections: notes.self_corrections,
        cues_rendered,
        llm_ms: 0,
        total_ms: total_start.elapsed().as_millis() as u64,
        model_used: model_used.to_string(),
        input_tokens: None,
        output_tokens: None,
        error: Some(err),
    };

    // Stage 1: DB-side context.
    let conn = match db.lock() {
        Ok(g) => g,
        Err(_) => return mk_partial("db mutex poisoned".into(), "<db-mutex-poisoned>"),
    };
    let dict = dictionary::list_all(&conn).unwrap_or_default();
    let candidates = few_shot::select_candidates(&conn, &mode.slug, None).unwrap_or_default();
    let examples = few_shot::fit_to_budget(candidates);
    drop(conn);

    // Stage 2: assemble prompt.
    let built = match prompt_builder::build(prompt_builder::PromptInputs {
        system_prompt: &mode.prompt_body,
        dictionary: &dict,
        examples: &examples,
        foreground_app: None,
        foreground_window_title: None,
        raw_transcript: &pre_text,
    }) {
        Ok(b) => b,
        Err(e) => return mk_partial(format!("prompt build: {e}"), "<prompt-build-failed>"),
    };

    // Stage 3: provider call.
    let req = CleanupRequest {
        prompt: &built.prompt,
        raw_transcript: &pre_text,
        model_id: &mode.model_id,
        temperature: mode.temperature,
        max_tokens: mode.max_tokens,
        mode_slug: &mode.slug,
    };
    match provider.cleanup(req) {
        Ok(r) => RunResult {
            output: r.text,
            preprocessed: pre_text,
            preprocess_ms,
            fillers_stripped: notes.fillers_stripped,
            stutters_collapsed: notes.stutters_collapsed,
            self_corrections: notes.self_corrections,
            cues_rendered,
            llm_ms: r.latency_ms,
            total_ms: total_start.elapsed().as_millis() as u64,
            model_used: format!("{}+{}", r.model_used, PREPROCESSOR_VERSION),
            input_tokens: r.input_tokens,
            output_tokens: r.output_tokens,
            error: None,
        },
        Err(e) => mk_partial(format!("{e}"), "<provider-error>"),
    }
}

// --------------------------------------------------------------------
// CLI plumbing.
// --------------------------------------------------------------------

#[derive(Debug)]
struct Args {
    label: String,
    modes: Vec<String>,
    only: Option<Vec<String>>,
}

fn parse_args() -> Args {
    let mut label = "baseline".to_string();
    let mut modes = vec![
        "casual".to_string(),
        "normal".to_string(),
        "formal".to_string(),
    ];
    let mut only: Option<Vec<String>> = None;

    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--label" => {
                if let Some(v) = it.next() {
                    label = v;
                }
            }
            "--modes" => {
                if let Some(v) = it.next() {
                    modes = v.split(',').map(|s| s.trim().to_string()).collect();
                }
            }
            "--only" => {
                if let Some(v) = it.next() {
                    only = Some(v.split(',').map(|s| s.trim().to_string()).collect());
                }
            }
            "-h" | "--help" => {
                eprintln!(
                    "Usage: mode_eval [--label NAME] [--modes csv] [--only fixture_id_csv]\n\
                     Defaults: --label baseline --modes casual,normal,formal"
                );
                std::process::exit(0);
            }
            other => eprintln!("warn: ignoring unknown arg `{other}`"),
        }
    }
    Args { label, modes, only }
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn,mode_eval=info")),
        )
        .with_writer(std::io::stderr)
        .init();

    let args = parse_args();
    eprintln!("mode_eval: label={} modes={:?}", args.label, args.modes);

    if let Err(e) = run(args) {
        eprintln!("FATAL: {e}");
        std::process::exit(1);
    }
}

fn run(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    let file: FixtureFile = serde_json::from_str(FIXTURES_JSON)?;
    let fixtures: Vec<Fixture> = match args.only {
        Some(ref ids) => file
            .fixtures
            .into_iter()
            .filter(|f| ids.iter().any(|id| id == &f.id))
            .collect(),
        None => file.fixtures,
    };
    if fixtures.is_empty() {
        return Err("no fixtures selected".into());
    }
    eprintln!("mode_eval: {} fixtures selected", fixtures.len());

    // In-memory DB — migrations seed modes + prompts identically to
    // production first-boot, so what we test is what users get on
    // a fresh install.
    let db = Database::open_in_memory()?;
    let db_arc: Arc<Mutex<Connection>> = Arc::new(Mutex::new(db.conn));

    let modes: Vec<ModeRow> = {
        let conn = db_arc.lock().map_err(|_| "db mutex poisoned")?;
        args.modes
            .iter()
            .map(|s| load_mode(&conn, s))
            .collect::<Result<Vec<_>, _>>()?
    };

    let provider = OllamaProvider::new();
    provider
        .health_check()
        .map_err(|e| format!("Ollama health-check failed (is `ollama serve` running?): {e}"))?;
    eprintln!("mode_eval: Ollama healthy");

    let preprocessor = Preprocessor::new();

    let mut runs: BTreeMap<(String, String), (PreservationScore, RunResult)> = BTreeMap::new();
    let mut aggregates: BTreeMap<String, ModeAggregate> = BTreeMap::new();
    let total_calls = fixtures.len() * modes.len();
    let mut done = 0usize;
    let grid_start = Instant::now();

    // Mode-major iteration: run all fixtures for casual first, then all
    // for normal, then all for formal. Ollama unloads a model when a
    // request for a different one comes in (5-10s reload each time on
    // a 6 GB card). With 39 fixtures × 3 modes that's 78 model swaps in
    // fixture-major order vs. 2 in mode-major — saved >10 min wall.
    for m in &modes {
        eprintln!("--- mode: {} (model={}) ---", m.slug, m.model_id);
        for fx in &fixtures {
            done += 1;
            eprintln!(
                "[{}/{}] {} x {} (raw={}ch)",
                done,
                total_calls,
                fx.id,
                m.slug,
                fx.raw.len()
            );
            let result = run_one(&provider, &db_arc, &preprocessor, m, &fx.raw);
            let score =
                score_preservation(&result.output, &fx.must_preserve, &fx.must_preserve_alts);
            aggregates
                .entry(m.slug.clone())
                .or_default()
                .record(score, &result);
            runs.insert((fx.id.clone(), m.slug.clone()), (score, result));
        }
    }
    let grid_elapsed = grid_start.elapsed();
    eprintln!(
        "mode_eval: grid complete in {:.1}s ({} calls)",
        grid_elapsed.as_secs_f32(),
        total_calls
    );

    let ts = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let report_md = render_report(&args.label, &ts, &modes, &fixtures, &runs, &aggregates);

    let out_dir = PathBuf::from("docs").join("cleanup");
    std::fs::create_dir_all(&out_dir)?;
    let out_path = out_dir.join(format!("eval-{}-{}.md", args.label, ts));
    std::fs::write(&out_path, &report_md)?;
    eprintln!("mode_eval: wrote report → {}", out_path.display());

    println!("\n=== Mode-eval summary (label={}) ===", args.label);
    for m in &modes {
        let agg = aggregates.get(&m.slug).cloned().unwrap_or_default();
        println!(
            "  {}: N={} errors={} preserve={:.1}% (full {} / partial {} / zero {}), avg LLM={}ms, max LLM={}ms",
            m.slug,
            agg.fixtures_run,
            agg.fixtures_errored,
            agg.avg_preservation_pct(),
            agg.preservation_full,
            agg.preservation_partial,
            agg.preservation_zero,
            agg.avg_llm_ms(),
            agg.max_llm_ms,
        );
    }
    println!("=== report: {} ===", out_path.display());
    Ok(())
}
