//! Per-mode model-override IPC (ADR 0066).
//!
//! The macOS Modes screen needs to show the user the model that will
//! ACTUALLY run for a mode — not the modes-table parity default, which on
//! a memory-constrained Mac is substituted at runtime by the RAM-aware
//! layer (ADR 0064). Pre-0066 the screen showed the configured 7B as a
//! free-text field while the dictation thread silently ran a 3B → the
//! display lied. These commands expose the *effective* model and let the
//! user pin a specific model (an override that bypasses the heuristic),
//! with a clean revert-to-Auto.
//!
//! ## Layering
//!
//! The shipped `modes.model_id` stays IMMUTABLE (Windows parity). User
//! pins live in the separate, additive `mode_model_overrides` table
//! (migration 029). "Auto" = no row = today's behaviour on every
//! platform. The enhanced Modes control that calls [`set_mode_model_override`]
//! / [`clear_mode_model_override`] is isMac-gated in the UI, so Windows
//! never writes the table and its dictation path is byte-identical.
//!
//! [`get_effective_model`] is cross-platform-safe: on non-macOS
//! `detect_memory_budget()` returns `None`, so `effective == configured`
//! and `budgetGb` is `null`.

use serde::Serialize;
use tauri::State;

use crate::cleanup::model_select::{detect_memory_budget, select_effective_model};
use crate::cleanup::OllamaProvider;
use crate::commands::{into_err, lock_db, AppStateHandle};

const BYTES_PER_GIB: u64 = 1024 * 1024 * 1024;

/// What the Modes screen needs to render the model control truthfully.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveModelDto {
    /// The mode's immutable parity default from the `modes` table.
    pub configured: String,
    /// The model that will ACTUALLY run: the user pin if set, else the
    /// RAM-aware auto-selection (macOS) / `configured` (elsewhere).
    pub effective: String,
    /// The user's explicit pin, or `None` for "Auto (RAM-aware)".
    pub override_model: Option<String>,
    /// Detected physical-memory budget in whole GiB (macOS unified
    /// memory). `None` when no budget signal exists (every non-macOS
    /// target, or a detection failure) — i.e. when Auto is a no-op.
    pub budget_gb: Option<u32>,
    /// Whether Ollama answered `/api/tags`. `false` → the dropdown has no
    /// installed models to offer; the UI degrades gracefully.
    pub ollama_reachable: bool,
}

/// Read a mode's immutable parity model from the `modes` table.
fn configured_model(db: &State<'_, AppStateHandle>, slug: &str) -> Result<String, String> {
    let conn = lock_db(db)?;
    conn.query_row("SELECT model_id FROM modes WHERE slug = ?1", [slug], |r| {
        r.get::<_, String>(0)
    })
    .map_err(into_err)
}

/// Read the user's per-mode pin from `mode_model_overrides`, if any.
fn read_override(db: &State<'_, AppStateHandle>, slug: &str) -> Result<Option<String>, String> {
    let conn = lock_db(db)?;
    conn.query_row(
        "SELECT model_id FROM mode_model_overrides WHERE mode_slug = ?1",
        [slug],
        |r| r.get::<_, String>(0),
    )
    .map(Some)
    .or_else(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => Ok(None),
        other => Err(into_err(other)),
    })
}

/// Compute the effective cleanup model for a mode, for display.
///
/// Mirrors the dictation-time resolution exactly via the shared
/// [`select_effective_model`], so the Modes screen can never disagree
/// with what actually runs.
#[tauri::command]
pub fn get_effective_model(
    db: State<'_, AppStateHandle>,
    slug: String,
) -> Result<EffectiveModelDto, String> {
    let configured = configured_model(&db, &slug)?;
    let override_model = read_override(&db, &slug)?;

    // Installed models + reachability. Ollama-down is non-fatal: we
    // still report configured/effective so the screen renders.
    let provider = OllamaProvider::new();
    let (installed, ollama_reachable) = match provider.list_models() {
        Ok(models) => (models, true),
        Err(_) => (Vec::new(), false),
    };

    let budget = detect_memory_budget();
    let budget_gb = budget.map(|b| (b.physical_bytes / BYTES_PER_GIB) as u32);
    let effective =
        select_effective_model(budget, &configured, &installed, override_model.as_deref());

    Ok(EffectiveModelDto {
        configured,
        effective,
        override_model,
        budget_gb,
        ollama_reachable,
    })
}

/// Pin a specific model for a mode (upsert). The pin bypasses the
/// RAM-aware substitution at dictation time.
#[tauri::command]
pub fn set_mode_model_override(
    db: State<'_, AppStateHandle>,
    slug: String,
    model_id: String,
) -> Result<(), String> {
    let conn = lock_db(&db)?;
    conn.execute(
        "INSERT INTO mode_model_overrides (mode_slug, model_id) VALUES (?1, ?2) \
         ON CONFLICT(mode_slug) DO UPDATE SET model_id = excluded.model_id",
        rusqlite::params![slug, model_id],
    )
    .map_err(into_err)?;
    tracing::info!(mode = %slug, model = %model_id, "set per-mode model override (ADR 0066)");
    Ok(())
}

/// Clear a mode's pin → revert to "Auto" (RAM-aware) behaviour.
#[tauri::command]
pub fn clear_mode_model_override(
    db: State<'_, AppStateHandle>,
    slug: String,
) -> Result<(), String> {
    let conn = lock_db(&db)?;
    conn.execute(
        "DELETE FROM mode_model_overrides WHERE mode_slug = ?1",
        [&slug],
    )
    .map_err(into_err)?;
    tracing::info!(mode = %slug, "cleared per-mode model override → Auto (ADR 0066)");
    Ok(())
}
