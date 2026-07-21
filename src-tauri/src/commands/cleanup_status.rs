//! Cleanup-engine status IPC — "am I getting AI-cleaned text or raw?".
//!
//! On macOS the `.app` bundles the Whisper STT model, but LLM cleanup
//! needs Ollama (a *separate* install) plus a locally-pulled model. When
//! Ollama isn't running — or is running but has no usable cleanup model —
//! the dictation pipeline silently falls back to [`PassthroughCleaner`]
//! and the user gets their raw transcript with no idea why. This command
//! surfaces that state so the UI can explain it (and how to fix it) on
//! the Settings, Dictations, and Modes screens.
//!
//! ## Layering / reuse
//!
//! It composes the SAME pieces the dictation thread uses so the screen
//! can never disagree with what actually runs:
//!   * [`OllamaProvider::list_models`] — reachability + installed tags
//!     (a single cheap `GET /api/tags`, mirroring the health check in
//!     `dictation/runtime_cleaner.rs::make_default_cleaner`).
//!   * [`detect_memory_budget`] + [`select_effective_model`] (ADR 0064) —
//!     the RAM-aware effective-model resolution for the representative
//!     `normal` mode.
//!
//! ## Cross-platform
//!
//! The command is additive and cross-platform-safe. On non-macOS
//! [`detect_memory_budget`] returns `None`, so `ram_tier` is `null` and
//! `effective_model` is the parity default (identity). The UI that
//! CONSUMES this is `isMac`-gated, so Windows never renders the surfaces
//! and its behaviour is byte-identical.

use serde::Serialize;
use tauri::State;

use crate::cleanup::model_select::{detect_memory_budget, select_effective_model};
use crate::cleanup::OllamaProvider;
use crate::commands::{lock_db, AppStateHandle};

/// Memory at/above this is the "high" tier (parity 7B-class allowed).
/// Mirrors `model_select::HIGH_TIER_MIN_BYTES` (private there); kept in
/// sync deliberately — this is only for the human-facing tier LABEL, not
/// the selection logic (which lives in `select_effective_model`).
const HIGH_TIER_MIN_BYTES: u64 = 16 * 1024 * 1024 * 1024; // 16 GiB

/// The universally-safe model to recommend pulling. 3B-q4 (~1.9 GB) runs
/// on every Mac; 16 GB+ boxes auto-select a larger model once one is
/// installed (the RAM-aware layer, ADR 0064). Matches the Settings copy
/// button's `ollama pull qwen2.5:3b`.
const RECOMMENDED_PULL: &str = "qwen2.5:3b";

/// The representative mode used to resolve the effective model for the
/// engine-wide status. `normal` is the default dictation mode; its
/// parity default is the 7B a capable box would run.
const REPRESENTATIVE_MODE: &str = "normal";

/// Fallback parity model if the `modes` row can't be read (fresh/partial
/// DB). Only affects the DISPLAYED effective model in that rare state.
const FALLBACK_PARITY_MODEL: &str = "qwen2.5:7b-instruct-q4_K_M";

/// What the UI needs to explain the cleanup state everywhere.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupStatusDto {
    /// `true` iff Ollama answered `GET /api/tags` — the service is up.
    pub ollama_reachable: bool,
    /// `true` iff Ollama is reachable AND a usable cleanup model resolves
    /// (i.e. the effective model is actually installed). When `false`,
    /// dictations are saved raw (passthrough).
    pub cleanup_active: bool,
    /// The RAM-aware effective model that WOULD run, when one resolves.
    /// `None` when cleanup is off (Ollama down, or no suitable model
    /// installed) — i.e. passthrough.
    pub effective_model: Option<String>,
    /// Locally-pulled Ollama model tags (`GET /api/tags`). Empty when
    /// Ollama is unreachable.
    pub installed_models: Vec<String>,
    /// The model tag to suggest the user `ollama pull`. Universally-safe
    /// 3B; the Settings note explains 7B auto-selects on 16 GB+ boxes.
    pub recommended_pull: String,
    /// Human-facing memory tier: `"high"` (>= 16 GiB) or `"small"`.
    /// `None` when no budget signal exists (every non-macOS target, or a
    /// detection failure).
    pub ram_tier: Option<String>,
}

/// Read the representative mode's immutable parity model from `modes`.
/// A read failure collapses to the fallback constant so the command
/// never hard-errors on a partially-migrated DB.
fn representative_configured_model(db: &State<'_, AppStateHandle>) -> String {
    let Ok(conn) = lock_db(db) else {
        return FALLBACK_PARITY_MODEL.to_string();
    };
    conn.query_row(
        "SELECT model_id FROM modes WHERE slug = ?1",
        [REPRESENTATIVE_MODE],
        |r| r.get::<_, String>(0),
    )
    .unwrap_or_else(|_| FALLBACK_PARITY_MODEL.to_string())
}

/// Live cleanup-engine status for the UI. Cheap: one `/api/tags` call +
/// a `sysctl` read (macOS) + one `modes` row.
#[tauri::command]
pub fn cleanup_status(db: State<'_, AppStateHandle>) -> Result<CleanupStatusDto, String> {
    // Reachability + installed tags. Ollama-down is non-fatal.
    let provider = OllamaProvider::new();
    let (installed_models, ollama_reachable) = match provider.list_models() {
        Ok(models) => (models, true),
        Err(_) => (Vec::new(), false),
    };

    let budget = detect_memory_budget();
    let ram_tier = budget.map(|b| ram_tier_label(b.physical_bytes).to_string());

    let configured = representative_configured_model(&db);
    let (cleanup_active, effective_model) =
        resolve_active(ollama_reachable, budget, &configured, &installed_models);

    Ok(CleanupStatusDto {
        ollama_reachable,
        cleanup_active,
        effective_model,
        installed_models,
        recommended_pull: RECOMMENDED_PULL.to_string(),
        ram_tier,
    })
}

/// Human-facing memory-tier label for a physical-byte budget.
fn ram_tier_label(physical_bytes: u64) -> &'static str {
    if physical_bytes >= HIGH_TIER_MIN_BYTES {
        "high"
    } else {
        "small"
    }
}

/// Resolve `(cleanup_active, effective_model)` from reachability + the
/// RAM-aware selection. Pure + platform-agnostic so it is unit-testable
/// everywhere.
///
/// `cleanup_active` requires BOTH that Ollama is reachable AND that the
/// resolved effective model is actually installed — `select_effective_model`
/// returns the parity default when nothing installed fits, so a resolved
/// tag that isn't in `installed` means "no usable model" (passthrough).
fn resolve_active(
    ollama_reachable: bool,
    budget: Option<crate::cleanup::model_select::MemoryBudget>,
    configured: &str,
    installed: &[String],
) -> (bool, Option<String>) {
    if !ollama_reachable {
        return (false, None);
    }
    let effective = select_effective_model(budget, configured, installed, None);
    if installed.iter().any(|m| m == &effective) {
        (true, Some(effective))
    } else {
        (false, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cleanup::model_select::MemoryBudget;

    fn budget(gib: u64) -> Option<MemoryBudget> {
        Some(MemoryBudget {
            physical_bytes: gib * 1024 * 1024 * 1024,
        })
    }

    #[test]
    fn ram_tier_label_splits_at_16gib() {
        assert_eq!(ram_tier_label(8 * 1024 * 1024 * 1024), "small");
        assert_eq!(ram_tier_label(15 * 1024 * 1024 * 1024), "small");
        assert_eq!(ram_tier_label(16 * 1024 * 1024 * 1024), "high");
        assert_eq!(ram_tier_label(64 * 1024 * 1024 * 1024), "high");
    }

    #[test]
    fn ollama_unreachable_is_passthrough() {
        let (active, model) = resolve_active(false, budget(8), "qwen2.5:3b", &[]);
        assert!(!active);
        assert_eq!(model, None);
    }

    #[test]
    fn reachable_but_no_models_installed_is_passthrough() {
        // Reachable, but nothing pulled -> select falls back to the
        // parity default which isn't installed -> not active.
        let (active, model) = resolve_active(true, budget(8), "qwen2.5:7b-instruct-q4_K_M", &[]);
        assert!(!active);
        assert_eq!(model, None);
    }

    #[test]
    fn small_box_with_3b_installed_is_active_on_3b() {
        let installed = vec!["qwen2.5:3b-instruct-q4_K_M".to_string()];
        let (active, model) = resolve_active(
            true,
            budget(8),
            "qwen2.5:7b-instruct-q4_K_M", // parity default won't fit
            &installed,
        );
        assert!(active);
        assert_eq!(model.as_deref(), Some("qwen2.5:3b-instruct-q4_K_M"));
    }

    #[test]
    fn high_box_with_parity_installed_keeps_parity() {
        let installed = vec!["qwen2.5:7b-instruct-q4_K_M".to_string()];
        let (active, model) =
            resolve_active(true, budget(32), "qwen2.5:7b-instruct-q4_K_M", &installed);
        assert!(active);
        assert_eq!(model.as_deref(), Some("qwen2.5:7b-instruct-q4_K_M"));
    }

    #[test]
    fn no_budget_signal_is_identity_when_installed() {
        // Windows path: budget None -> effective == configured. Active iff
        // that configured model is installed.
        let installed = vec!["qwen2.5:7b-instruct-q4_K_M".to_string()];
        let (active, model) = resolve_active(true, None, "qwen2.5:7b-instruct-q4_K_M", &installed);
        assert!(active);
        assert_eq!(model.as_deref(), Some("qwen2.5:7b-instruct-q4_K_M"));
    }
}
