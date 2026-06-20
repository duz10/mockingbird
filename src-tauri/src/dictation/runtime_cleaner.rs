//! Cleaner-factory glue for the dictation runtime.
//!
//! Split out of `runtime.rs` (ADR 0063) to keep that module under the
//! 600-line cap and to give the "how do we build the cleanup stage"
//! concern its own cohesive home. Pure cross-platform code — the
//! Ollama health-check + passthrough fallback are identical on every
//! target.

// Cross-platform module, but only *reached* from the
// `any(windows, macos)` runtime spawn path; dead elsewhere.
#![cfg_attr(not(any(target_os = "windows", target_os = "macos")), allow(dead_code))]

use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use super::OrchestratorConfig;
use crate::cleanup::{Cleaner, LlmCleaner, OllamaProvider, PassthroughCleaner};

/// Build the cleaner the dictation thread will use.
///
/// Strategy: try to construct an [`LlmCleaner`] wired to a local
/// Ollama via the mode's configured model. If Ollama is unreachable
/// (no service running, wrong port, network blocked), log a `WARN`
/// and fall back to [`PassthroughCleaner`] — the user still gets
/// their raw transcript injected, with the cleanup phase a no-op.
///
/// Phase 7 will replace this with a settings-driven dispatcher that
/// picks Ollama vs Claude per-mode. For Phase 4 we hard-default to
/// the Normal mode's `ollama` provider — that's the PLAN §8 default.
pub(super) fn make_default_cleaner(
    db: &Arc<Mutex<Connection>>,
    config: &OrchestratorConfig,
) -> Box<dyn Cleaner> {
    let lookup = {
        let conn = match db.lock() {
            Ok(g) => g,
            Err(_) => {
                tracing::warn!("cleaner: db mutex poisoned at boot; using passthrough");
                return Box::new(PassthroughCleaner::new());
            }
        };
        conn.query_row(
            "SELECT model_id, temperature, max_tokens FROM modes WHERE slug = ?1",
            [&config.mode_slug],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, f64>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            },
        )
    };
    let (model_id, temperature, max_tokens) = match lookup {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(
                error = ?e,
                mode = %config.mode_slug,
                "cleaner: mode lookup failed; using passthrough"
            );
            return Box::new(PassthroughCleaner::new());
        }
    };

    let provider = OllamaProvider::new();
    match provider.health_check() {
        Ok(_) => {
            tracing::info!(
                model = %model_id,
                temperature,
                max_tokens,
                "cleaner: Ollama reachable; using LlmCleaner"
            );
            // **Warm-up shot.** Ollama loads the model into VRAM on
            // the first /api/chat request — a cold qwen2.5:3b can
            // take 30-60s, which is right at our 30s REQUEST_TIMEOUT.
            // First real dictation reliably times out. Pay the cost
            // here in the background: fire one minimal /api/chat
            // while the user is still reading the splash / opening
            // their target app. Errors are ignored — worst case the
            // first real dictation still times out, which is no
            // worse than today's behavior. (LESSONS 2026-05-17
            // phase5-smoketest, third pass.)
            spawn_ollama_warmup(model_id.clone(), temperature as f32);
            Box::new(LlmCleaner::new(
                Box::new(provider),
                Arc::clone(db),
                model_id,
                temperature as f32,
                max_tokens as u32,
            ))
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                "cleaner: Ollama health check failed; falling back to passthrough \
                 (start Ollama + pull the model to enable LLM cleanup)"
            );
            Box::new(PassthroughCleaner::new())
        }
    }
}

/// Fire-and-forget thread that sends one minimal `/api/chat` request
/// to Ollama to pay the cold model-load cost before the user's first
/// dictation. Errors are logged at debug-level and never surface to
/// the caller — a failed warm-up is no worse than not warming up.
///
/// Uses a fresh [`OllamaProvider`] (cheap; just a ureq::Agent + a
/// base-URL string) so we don't have to thread a clone of the
/// cleaner's provider through. The model_id + temperature match the
/// configured mode so the warm-up populates the SAME VRAM slot the
/// first real request will hit.
fn spawn_ollama_warmup(model_id: String, temperature: f32) {
    std::thread::Builder::new()
        .name("ollama-warmup".into())
        .spawn(move || {
            use crate::cleanup::provider::{CleanupProvider, CleanupRequest};
            let provider = OllamaProvider::new();
            // Minimal prompt: enough tokens to force a real forward
            // pass (= VRAM load) but tiny enough to return quickly
            // on a warm model. `num_predict: 1` caps generation at
            // a single token; we throw the result away.
            let req = CleanupRequest {
                prompt: "Respond with the single word: ok",
                raw_transcript: "hi",
                model_id: &model_id,
                temperature,
                max_tokens: 1,
                mode_slug: "warmup",
            };
            let start = std::time::Instant::now();
            match provider.cleanup(req) {
                Ok(_) => tracing::info!(
                    model = %model_id,
                    warmup_ms = start.elapsed().as_millis() as u64,
                    "ollama warmup complete; first real dictation will hit a hot model"
                ),
                Err(e) => tracing::debug!(
                    error = %e,
                    model = %model_id,
                    "ollama warmup failed (non-fatal); first real cleanup may cold-load"
                ),
            }
        })
        .ok(); // If we can't even spawn a thread, the app has bigger problems.
}
