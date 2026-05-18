//! Modes editor for the UI. The `modes` table is queried directly
//! since the per-row repo (Phase 4 didn't add one — modes are mostly
//! read-only from the orchestrator's perspective).

use serde::Deserialize;
use tauri::State;

use crate::commands::types::ModeDto;
use crate::commands::{into_err, lock_db, AppStateHandle};

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ModePatch {
    pub enabled: Option<bool>,
    pub model_id: Option<String>,
    pub provider: Option<String>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<i64>,
    pub hotkey: Option<String>,
}

#[tauri::command]
pub fn list_modes(db: State<'_, AppStateHandle>) -> Result<Vec<ModeDto>, String> {
    let conn = lock_db(&db)?;
    // Two SQL↔schema name mismatches handled inline here:
    //   - `label` is the DTO field; `display_name` is the schema column
    //     (migrations/001_initial.sql). Migrations are append-only
    //     post-Phase-1-seal so we alias on the SELECT side.
    //   - `prompt_version` is `String` on the DTO; `prompts.version` is
    //     `INTEGER` on the schema. SQLite will return Integer for the
    //     raw column, which fails rusqlite's String column-type check
    //     ("Invalid column type Integer at index: 8"). Concatenating
    //     `'v' || p.version` forces TEXT affinity and produces the
    //     human-readable form (v1, v2, …) the UI already expects —
    //     same shape as the `'v1'` literal fallback.
    let mut stmt = conn
        .prepare(
            "SELECT m.slug, m.display_name AS label, m.enabled, m.model_id, m.provider, \
                    m.temperature, m.max_tokens, m.hotkey, COALESCE('v' || p.version, 'v1') \
             FROM modes m \
             LEFT JOIN prompts p ON p.id = m.prompt_id \
             ORDER BY m.id ASC",
        )
        .map_err(into_err)?;
    let mapped = stmt
        .query_map([], |r: &rusqlite::Row<'_>| -> rusqlite::Result<ModeDto> {
            Ok(ModeDto {
                slug: r.get(0)?,
                label: r.get(1)?,
                enabled: r.get::<_, i64>(2)? != 0,
                model_id: r.get(3)?,
                provider: r.get(4)?,
                temperature: r.get(5)?,
                max_tokens: r.get(6)?,
                hotkey: r.get(7)?,
                prompt_version: r.get(8)?,
            })
        })
        .map_err(into_err)?;
    let mut out = Vec::new();
    for r in mapped {
        out.push(r.map_err(into_err)?);
    }
    Ok(out)
}

#[tauri::command]
pub fn update_mode(
    db: State<'_, AppStateHandle>,
    slug: String,
    patch: ModePatch,
) -> Result<(), String> {
    let conn = lock_db(&db)?;
    // Build a single UPDATE with only the changed columns. Keeps the
    // audit trigger trail tight (one history row per call, not one
    // per field). All branches use the same parameter ordering so
    // there's no clever string-build risk.
    let mut sets: Vec<String> = Vec::new();
    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(v) = patch.enabled {
        sets.push("enabled = ?".into());
        params_vec.push(Box::new(i64::from(v)));
    }
    if let Some(v) = patch.model_id {
        sets.push("model_id = ?".into());
        params_vec.push(Box::new(v));
    }
    if let Some(v) = patch.provider {
        sets.push("provider = ?".into());
        params_vec.push(Box::new(v));
    }
    if let Some(v) = patch.temperature {
        sets.push("temperature = ?".into());
        params_vec.push(Box::new(v));
    }
    if let Some(v) = patch.max_tokens {
        sets.push("max_tokens = ?".into());
        params_vec.push(Box::new(v));
    }
    if let Some(v) = patch.hotkey {
        sets.push("hotkey = ?".into());
        params_vec.push(Box::new(v));
    }
    if sets.is_empty() {
        return Ok(()); // no-op
    }
    params_vec.push(Box::new(slug));
    let sql = format!("UPDATE modes SET {} WHERE slug = ?", sets.join(", "));
    let refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|b| b.as_ref()).collect();
    conn.execute(&sql, refs.as_slice()).map_err(into_err)?;
    Ok(())
}
