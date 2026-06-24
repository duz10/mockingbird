//! `mac_fidelity_fallback_probe` — Phase 5 (mb-l97) live validation of
//! the content-coverage fidelity fallback on the small-model path.
//!
//! Constructs the REAL [`LlmCleaner`] exactly as the macOS RAM-aware
//! downsize seam (`dictation::runtime_cleaner::make_default_cleaner`)
//! does on an 8 GB Mac: the 3B effective model + the `normal_small`
//! prompt override + the new `small_model_fidelity` flag. It then runs
//! the user's EXACT problem transcript through `clean(raw, "normal")`
//! N times at each of two temperatures and reports, per run:
//!   * the verbatim cleaned output,
//!   * whether the coverage fallback fired (provenance carries
//!     `-coverage-fallback`),
//!   * whether the two load-bearing sentences survived.
//!
//! The contract this proves (mb-l97 / the user's #1 rule): the phrases
//! "I want to buy some groceries" and "testing in-app dictation" are
//! NEVER dropped — either the 3B keeps them, OR the fidelity fallback
//! returns the faithful preprocessor text. The probe exits non-zero if
//! ANY run drops either phrase (i.e. the guarantee was violated).
//!
//! Run (via the Mac wrapper, so Ollama on :11434 is reachable):
//!   scripts/dev/cargo-mac.sh run --release \
//!       --example mac_fidelity_fallback_probe
//!
//! Tunables via env: MOCKINGBIRD_PROBE_MODEL (default qwen2.5:3b...),
//! MOCKINGBIRD_PROBE_RUNS (runs per temperature, default 6).

use std::sync::{Arc, Mutex};

use mockingbird_lib::cleanup::{Cleaner, LlmCleaner, OllamaProvider, SMALL_MODEL_PROMPT_MODE_SLUG};
use mockingbird_lib::db::Database;

/// The user's exact problem transcript (kennel + goal brief).
const RAW: &str = "testing in-app dictation i want to um buy some groceries \
                   um i need i need these three things bananas eggs shampoo";

/// Load-bearing content that must survive every run (normalised, lower).
/// We assert on the strongest content words of each target sentence so a
/// legitimate light rephrase still passes, while a wholesale drop fails.
const MUST_SURVIVE: &[(&str, &[&str])] = &[
    ("testing in-app dictation", &["testing", "dictation"]),
    ("I want to buy some groceries", &["buy", "groceries"]),
];

fn normalise(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c == '-' { ' ' } else { c })
        .filter(|c| !"*_`#[]()<>\"'.,;:!?".contains(*c))
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// True iff every salient word of the target phrase is present.
fn phrase_present(output_norm: &str, salient: &[&str]) -> bool {
    salient.iter().all(|w| output_norm.contains(w))
}

fn main() {
    println!("=== mac_fidelity_fallback_probe (Phase 5 — mb-l97) ===");

    let model_id = std::env::var("MOCKINGBIRD_PROBE_MODEL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "qwen2.5:3b-instruct-q4_K_M".to_string());
    let runs: usize = std::env::var("MOCKINGBIRD_PROBE_RUNS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(6);

    println!("MODEL   : {model_id}");
    println!("RUNS/T  : {runs}");
    println!("RAW     : {RAW}\n");

    // Prove Ollama is reachable up front with a clear message.
    OllamaProvider::new()
        .health_check()
        .expect("Ollama health check failed — is `ollama serve` running on :11434?");

    let temps: [f32; 2] = [0.0, 0.3];
    let mut total_runs = 0usize;
    let mut total_fallbacks = 0usize;
    let mut violations = 0usize;

    for &temp in &temps {
        let mut fired = 0usize;
        println!("════════ TEMPERATURE {temp} ════════");
        for i in 1..=runs {
            // Fresh migrated DB per run so the `normal_small` prompt + the
            // `modes` row resolve exactly like production; cheap in-memory.
            let db = Database::open_in_memory().expect("migrated in-memory db");
            let max_tokens: i64 = db
                .conn
                .query_row(
                    "SELECT max_tokens FROM modes WHERE slug = 'normal'",
                    [],
                    |r| r.get(0),
                )
                .expect("normal mode row");
            let db_arc = Arc::new(Mutex::new(db.conn));

            // The EXACT macOS 8 GB downsize wiring: 3B model + normal_small
            // prompt override + the content-coverage fidelity flag.
            let mut cleaner = LlmCleaner::new(
                Box::new(OllamaProvider::new()),
                Arc::clone(&db_arc),
                model_id.clone(),
                temp,
                max_tokens as u32,
            )
            .with_prompt_mode_override(Some(SMALL_MODEL_PROMPT_MODE_SLUG.to_string()))
            .with_small_model_fidelity(true);

            let cleaned = cleaner.clean(RAW, "normal").expect("clean call");
            let provenance = cleaner.model_name().to_string();
            let fell_back = provenance.contains("coverage-fallback");
            if fell_back {
                fired += 1;
            }

            let norm = normalise(&cleaned);
            let missing: Vec<&str> = MUST_SURVIVE
                .iter()
                .filter(|(_, salient)| !phrase_present(&norm, salient))
                .map(|(phrase, _)| *phrase)
                .collect();
            if !missing.is_empty() {
                violations += 1;
            }

            total_runs += 1;
            println!("── run {i}/{runs} (temp {temp}) ──");
            println!("CLEANED   : {cleaned}");
            println!("PROVENANCE: {provenance}  | fidelity-fallback fired: {fell_back}");
            if missing.is_empty() {
                println!("SURVIVED  :  both load-bearing sentences present");
            } else {
                println!("SURVIVED  :  DROPPED → {missing:?}");
            }
            println!();
        }
        total_fallbacks += fired;
        println!("TEMP {temp} SUMMARY: fidelity-fallback fired on {fired}/{runs} runs\n");
    }

    println!("════════ OVERALL ════════");
    println!("total runs              : {total_runs}");
    println!(
        "fidelity-fallback fired : {total_fallbacks}/{total_runs} \
         ({:.0}%)",
        100.0 * total_fallbacks as f32 / total_runs.max(1) as f32
    );
    println!("content-drop violations : {violations}");

    if violations > 0 {
        eprintln!(
            "\nFAIL: {violations} run(s) dropped a load-bearing sentence — \
             the mb-l97 guarantee was violated."
        );
        std::process::exit(1);
    }
    println!("\nPASS: every run preserved both load-bearing sentences (kept by the LLM or recovered by the fidelity fallback).");
}
