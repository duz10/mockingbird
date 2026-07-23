//! `mac_cleanup_ollama_probe` — Phase 5 (mb-mac-v1.6.1) deterministic
//! proof that the LIVE `OllamaCleaner` path cleans dictation
//! transcripts, without needing the user to speak.
//!
//! Mirrors `dictation::runtime_cleaner::make_default_cleaner`: it opens
//! a freshly-migrated DB (so the `normal` mode row resolves to the same
//! Windows-parity model the real app would request), constructs the
//! real [`OllamaProvider`] + [`LlmCleaner`], and runs sample transcripts
//! through `clean(raw, "normal")` — the exact call the dictation thread
//! makes.
//!
//! Two cases:
//!   1. The user's actual Phase-3 e2e transcript ("…with with the right
//!      option key"). It is ~10 words — below the Wave-2.2 LLM-skip word
//!      threshold (default 12) — so it exercises the deterministic
//!      preprocessor (stutter-collapse + capitalisation + terminal
//!      punctuation), which is what fixes the "with with" stutter. The
//!      7B LLM is intentionally skipped for short one-liners.
//!   2. A longer multi-sentence utterance that clears the skip threshold
//!      → forces the LIVE `qwen2.5:7b-instruct-q4_K_M` call, proving the
//!      real Ollama model is invoked (non-zero LLM latency, model_used
//!      carries the qwen tag).
//!
//! Run (via the Mac wrapper, so Ollama on :11434 is reachable):
//!   scripts/dev/cargo-mac.sh run --release --example mac_cleanup_ollama_probe
//!
//! Requires `ollama serve` running with the configured model pulled.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use mockingbird_lib::cleanup::{Cleaner, LlmCleaner, OllamaProvider};
use mockingbird_lib::db::Database;

/// Build the real cleaner exactly as `make_default_cleaner` does, then
/// clean `raw` under the `normal` mode and print the before/after.
fn run_one(label: &str, raw: &str) {
    let db = Database::open_in_memory().expect("migrated in-memory db");
    let (mut model_id, temperature, max_tokens): (String, f64, i64) = db
        .conn
        .query_row(
            "SELECT model_id, temperature, max_tokens FROM modes WHERE slug = 'normal'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .expect("normal mode row");

    // 8GB-canary override (mb-mac-v1.6.1): the configured Windows-parity
    // 7B cold-loads in ~55s on an 8GB M3 and blows the request timeout.
    // `MOCKINGBIRD_PROBE_MODEL=<id>` lets us prove the real LlmCleaner
    // path against a Mac-appropriate model without editing migrations.
    if let Ok(override_model) = std::env::var("MOCKINGBIRD_PROBE_MODEL") {
        if !override_model.is_empty() {
            model_id = override_model;
        }
    }

    let db_arc = Arc::new(Mutex::new(db.conn));

    let provider = OllamaProvider::new();
    provider
        .health_check()
        .expect("Ollama health check failed — is `ollama serve` running on :11434?");

    let mut cleaner = LlmCleaner::new(
        Box::new(provider),
        Arc::clone(&db_arc),
        model_id.clone(),
        temperature as f32,
        max_tokens as u32,
    );

    println!("──────────────────────────────────────────────────────────");
    println!("CASE: {label}");
    println!("CONFIGURED MODEL (normal mode row): {model_id}");
    println!("RAW    : {raw}");
    let start = Instant::now();
    let cleaned = cleaner.clean(raw, "normal").expect("clean call");
    let elapsed_ms = start.elapsed().as_millis();
    println!("CLEANED: {cleaned}");
    println!("MODEL_USED (provenance): {}", cleaner.model_name());
    println!("CLEAN_LATENCY_MS: {elapsed_ms}");
    println!("CHANGED (cleaned != raw): {}", cleaned != raw);
}

fn main() {
    println!("=== mac_cleanup_ollama_probe (Phase 5 — mb-mac-v1.6.1) ===");

    run_one(
        "user's Phase-3 e2e transcript (short → preprocessor path)",
        "testing for microphone input with with the right option key",
    );

    run_one(
        "long multi-sentence utterance (forces live 7B LLM)",
        "so um i was thinking that we should we should probably ship the \
         macos port this week and then like start on the meeting capture \
         thing after that because honestly the dictation already works \
         really well and i dont want to lose momentum on it you know",
    );
}
