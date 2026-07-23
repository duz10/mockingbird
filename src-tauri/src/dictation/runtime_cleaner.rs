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
    // At boot, warm the model up (the first real dictation is still
    // seconds-to-minutes away). If Ollama is down, fall back to
    // passthrough — the self-heal re-check (mb-58i) will upgrade us on
    // a later dictation if Ollama comes up.
    build_llm_cleaner(db, config, true).unwrap_or_else(|| Box::new(PassthroughCleaner::new()))
}

/// mb-58i — lazy Ollama self-heal at a dictation boundary.
///
/// The cleaner is built ONCE at [`DictationRuntime`] spawn. If Ollama
/// wasn't running then, we fell back to [`PassthroughCleaner`] and
/// would stay on raw passthrough until an app restart — even after the
/// user starts Ollama. This re-checks that, cheaply, at the START of a
/// dictation:
///
///   * If the current cleaner is NOT the passthrough fallback (i.e. we
///     already have a live [`LlmCleaner`]), return `None` immediately —
///     **no health check, no cost**. A healthy cleaner never pays for
///     this. (The default `Cleaner::model_name()` is `"passthrough"`;
///     `LlmCleaner` reports its provider/model, never that literal, so
///     this is a reliable "am I the fallback?" signal.)
///   * If we ARE on passthrough, re-run the Ollama health check via
///     [`build_llm_cleaner`]. If Ollama is now reachable, return the
///     freshly-built `LlmCleaner` for the caller to swap in; if it's
///     still down, return `None` (behaviour identical to today).
///
/// Called only at dictation boundaries (not per-keystroke), so it can
/// re-check at most once per dictation — no thrash, no background
/// thread. Cross-platform: helps Windows too if Ollama is restarted
/// after the app (flag for main-merge; when Ollama stays down the
/// behaviour is byte-identical to before).
pub(super) fn maybe_upgrade_from_passthrough(
    current: &dyn Cleaner,
    db: &Arc<Mutex<Connection>>,
    config: &OrchestratorConfig,
) -> Option<Box<dyn Cleaner>> {
    if current.model_name() != "passthrough" {
        // Already have a live LlmCleaner — skip the health check entirely.
        return None;
    }
    // We're on the fallback. Don't warm up: the real dictation about to
    // run IS the warm-up shot, so a separate warm-up thread would just
    // race it for the same VRAM slot.
    let upgraded = build_llm_cleaner(db, config, false)?;
    tracing::info!("cleaner: Ollama now reachable; self-healed passthrough → LlmCleaner (mb-58i)");
    Some(upgraded)
}

/// Build a live [`LlmCleaner`] IFF the mode looks up cleanly AND Ollama
/// is reachable; otherwise `None` (the caller decides the fallback).
///
/// `warmup` controls whether we fire the background cold-load shot:
/// `true` at boot (first dictation is far off), `false` on the mb-58i
/// self-heal path (the imminent real request is the warm-up).
fn build_llm_cleaner(
    db: &Arc<Mutex<Connection>>,
    config: &OrchestratorConfig,
    warmup: bool,
) -> Option<Box<dyn Cleaner>> {
    let lookup = {
        let conn = match db.lock() {
            Ok(g) => g,
            Err(_) => {
                tracing::warn!("cleaner: db mutex poisoned at boot; using passthrough");
                return None;
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
            return None;
        }
    };

    let provider = OllamaProvider::new();
    match provider.health_check() {
        Ok(_) => {
            // RAM-aware effective-model substitution (ADR 0064) + the
            // ADR 0065 prompt override that rides on it. On non-macOS
            // BOTH the model swap and the override are cfg'd out: `model_id`
            // stays the mode's parity default, `prompt_override` is `None`,
            // and the Windows cleanup path is byte-identical (normal@v5 is
            // never re-evaluated).
            #[cfg(not(target_os = "macos"))]
            let (prompt_override, small_model_fidelity): (Option<String>, bool) = (None, false);
            // ADR 0067 — user-authored per-mode prompt override. Read at
            // the macOS seam only; `None` everywhere else, so the Windows
            // prompt-resolution path is byte-identical.
            #[cfg(not(target_os = "macos"))]
            let user_prompt_override: Option<String> = None;
            #[cfg(target_os = "macos")]
            let user_prompt_override = read_mode_prompt_override(db, &config.mode_slug);
            // On macOS the selector swaps in a model that fits unified
            // memory (e.g. 7B → 3B on an 8 GB box). When it actually
            // downsizes AND the active mode is Normal, also swap Normal's
            // prompt for the hardened `normal_small` variant — the weak 3B
            // leaks normal@v5's few-shot examples otherwise (ADR 0065).
            #[cfg(target_os = "macos")]
            let (model_id, prompt_override, small_model_fidelity) = {
                let parity_model = model_id.clone();
                // ADR 0066: a per-mode user pin (Modes screen) short-
                // circuits the RAM-aware heuristic. `None` = "Auto" =
                // the unchanged pre-0066 behaviour. Read behind the
                // macOS cfg only, so Windows never touches this table.
                let override_model = read_mode_model_override(db, &config.mode_slug);
                let effective = crate::cleanup::model_select::resolve_effective_model(
                    &provider,
                    model_id,
                    override_model,
                );
                let prompt_override = if effective != parity_model && config.mode_slug == "normal" {
                    tracing::info!(
                        parity_model = %parity_model,
                        effective_model = %effective,
                        "ADR 0065: Normal downsized off parity → using hardened `normal_small` prompt"
                    );
                    Some(crate::cleanup::SMALL_MODEL_PROMPT_MODE_SLUG.to_string())
                } else {
                    None
                };
                // Phase 5 / mb-l97 — the content-coverage fidelity
                // fallback applies whenever the effective model is a
                // DOWNSIZE off parity (auto-select on a small Mac OR a
                // user pin to a smaller model), for ANY mode — not just
                // Normal's prompt swap above.
                let small_model_fidelity = effective != parity_model;
                (effective, prompt_override, small_model_fidelity)
            };

            // mb-mac-v1.6.4 — RAM-aware runtime fallback chain (Layer 2).
            // macOS only: the installed models strictly smaller than the
            // effective model, largest-first, so a runtime load/timeout
            // failure can step DOWN instead of dropping to passthrough.
            // Empty on every non-macOS target → the retry loop is inert
            // and the Windows cleanup path is byte-identical.
            #[cfg(not(target_os = "macos"))]
            let runtime_fallback: Vec<String> = Vec::new();
            #[cfg(target_os = "macos")]
            let runtime_fallback =
                crate::cleanup::model_select::runtime_fallback_chain(&provider, &model_id);

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
            if warmup {
                spawn_ollama_warmup(model_id.clone(), temperature as f32);
            }
            Some(Box::new(
                LlmCleaner::new(
                    Box::new(provider),
                    Arc::clone(db),
                    model_id,
                    temperature as f32,
                    max_tokens as u32,
                )
                .with_prompt_mode_override(prompt_override)
                .with_small_model_fidelity(small_model_fidelity)
                .with_user_prompt_override(user_prompt_override)
                .with_runtime_fallback_models(runtime_fallback),
            ))
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                "cleaner: Ollama health check failed; falling back to passthrough \
                 (start Ollama + pull the model to enable LLM cleanup)"
            );
            None
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

/// Read the user's per-mode model pin (ADR 0066), if any.
///
/// `Some(model_id)` = the user explicitly pinned a model for this mode in
/// the Modes screen → use it verbatim. `None` = "Auto" (no row) → the
/// caller falls through to the RAM-aware auto-selection, i.e. the
/// unchanged pre-0066 behaviour.
///
/// macOS-only: the table is written exclusively by the isMac-gated Modes
/// control, and this read sits behind `#[cfg(target_os = "macos")]`, so
/// the Windows cleanup path never queries it (byte-identical guarantee).
/// Any failure (poisoned mutex, missing table on a partially-migrated DB,
/// query error) collapses to `None` → Auto, the safe default.
#[cfg(target_os = "macos")]
fn read_mode_model_override(db: &Arc<Mutex<Connection>>, mode_slug: &str) -> Option<String> {
    let conn = db.lock().ok()?;
    conn.query_row(
        "SELECT model_id FROM mode_model_overrides WHERE mode_slug = ?1",
        [mode_slug],
        |r| r.get::<_, String>(0),
    )
    .ok()
}

/// Read the user's per-mode PROMPT override (ADR 0067), if any.
///
/// `Some(body)` = the user authored a custom cleanup prompt for this mode
/// in the Modes screen -> use it VERBATIM, skipping the small-model tier
/// substitution (precedence: user override > tier substitution > mode
/// default). `None` = no row -> the caller resolves the shipped default
/// (then the tier substitution on a downsized model), i.e. the unchanged
/// behaviour.
///
/// macOS-only: the table is written exclusively by the isMac-gated Modes
/// prompt editor, and this read sits behind `#[cfg(target_os = "macos")]`,
/// so the Windows cleanup path never queries it (byte-identical
/// guarantee). Any failure (poisoned mutex, missing table on a
/// partially-migrated DB, query error) collapses to `None` -> shipped
/// default, the safe default.
#[cfg(target_os = "macos")]
fn read_mode_prompt_override(db: &Arc<Mutex<Connection>>, mode_slug: &str) -> Option<String> {
    let conn = db.lock().ok()?;
    conn.query_row(
        "SELECT prompt_body FROM mode_prompt_overrides WHERE mode_slug = ?1",
        [mode_slug],
        |r| r.get::<_, String>(0),
    )
    .ok()
}
