//! SCHEMA.md parsers — line-based, deliberately narrow.
//!
//! Split out of `mod.rs` at graduation purely to keep the parent
//! file under the 600-line cap; functions are unchanged from the
//! sandbox. Pulling in a Markdown crate (`pulldown-cmark`) would
//! more than triple the dep footprint for a file we control. YAGNI.
//!
//! Every function returns `Result<_, SchemaError>` against the
//! parent module's error type.

use std::collections::HashMap;

use super::{SchemaError, SUPPORTED_PASSES};

pub(super) fn parse_yaml_int(text: &str, key: &str) -> Result<u32, SchemaError> {
    let raw = parse_yaml_str(text, key)?;
    raw.parse::<u32>()
        .map_err(|e| SchemaError::Parse(format!("`{key}` must be a u32: {e} (got `{raw}`)")))
}

/// Find a `key: value` pair inside any fenced ```yaml block in the
/// document. Returns the value with surrounding whitespace stripped.
pub(super) fn parse_yaml_str(text: &str, key: &str) -> Result<String, SchemaError> {
    let prefix = format!("{key}:");
    let mut in_yaml = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```yaml") {
            in_yaml = true;
            continue;
        }
        if in_yaml && trimmed.starts_with("```") {
            in_yaml = false;
            continue;
        }
        if !in_yaml {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix(&prefix) {
            return Ok(rest.trim().to_string());
        }
    }
    Err(SchemaError::Parse(format!(
        "missing `{key}:` inside any ```yaml block"
    )))
}

/// Parse the bullet list (`- item`) that appears under the given
/// header. Items have surrounding backticks stripped (`` `task` `` → `task`).
/// Stops at the next blank line OR the next `#` heading.
pub(super) fn parse_bullet_list(text: &str, header: &str) -> Result<Vec<String>, SchemaError> {
    let lines: Vec<&str> = text.lines().collect();
    let header_idx = lines
        .iter()
        .position(|l| l.trim() == header)
        .ok_or_else(|| SchemaError::Parse(format!("missing header `{header}`")))?;

    let mut items: Vec<String> = Vec::new();
    let mut started = false;
    for line in &lines[header_idx + 1..] {
        let t = line.trim();
        if t.starts_with("- ") {
            let raw = t.trim_start_matches("- ").trim();
            let cleaned = raw.trim_matches('`').trim().to_string();
            if !cleaned.is_empty() {
                items.push(cleaned);
            }
            started = true;
        } else if started {
            // After the first bullet, any non-bullet line (blank,
            // heading, prose) terminates the list. The sandbox
            // split this into two `else if` arms with identical
            // bodies; clippy flagged the duplication at graduation
            // (clippy::if_same_then_else). The merged single arm is
            // behaviourally identical because the outer
            // `t.starts_with("- ")` already handled the bullet case.
            break;
        }
    }

    if items.is_empty() {
        return Err(SchemaError::Parse(format!(
            "header `{header}` found but no `- bullet` items follow"
        )));
    }
    Ok(items)
}

/// Parse a row of the per-pass-model-defaults table:
/// `| segment | qwen2.5:7b-instruct-q4_K_M | ... |`
pub(super) fn parse_model_default(text: &str, pass: &str) -> Result<String, SchemaError> {
    let needle = format!("`{pass}`");
    let mut in_section = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("## Per-pass model defaults") {
            in_section = true;
            continue;
        }
        if in_section && t.starts_with("## ") {
            break;
        }
        if !in_section {
            continue;
        }
        if !t.starts_with('|') {
            continue;
        }
        if !t.contains(&needle) {
            continue;
        }
        let cols: Vec<&str> = t.split('|').map(str::trim).collect();
        if cols.len() < 3 {
            continue;
        }
        let model = cols[2].trim_matches('`').trim();
        if model.is_empty() {
            continue;
        }
        return Ok(model.to_string());
    }
    Err(SchemaError::Parse(format!(
        "no model-defaults table row for pass `{pass}` inside `## Per-pass model defaults`"
    )))
}

/// Parse the default `| pass | prompts/segment.md |` rows in the
/// `### Default prompt body per pass` table.
pub(super) fn parse_default_prompt_path(text: &str, pass: &str) -> Result<String, SchemaError> {
    let needle = format!("`{pass}`");
    let mut in_table = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("### Default prompt body per pass") {
            in_table = true;
            continue;
        }
        if in_table && (t.starts_with("### ") || t.starts_with("## ")) {
            break;
        }
        if !in_table {
            continue;
        }
        if !t.starts_with('|') || !t.contains(&needle) {
            continue;
        }
        let cols: Vec<&str> = t.split('|').map(str::trim).collect();
        if cols.len() < 3 {
            continue;
        }
        let path = cols[2].trim_matches('`').trim();
        if path.is_empty() {
            continue;
        }
        return Ok(path.to_string());
    }
    Err(SchemaError::Parse(format!(
        "no default-prompt-path row for pass `{pass}` inside `### Default prompt body per pass`"
    )))
}

/// Parse zero-or-more rows of the `### Profile-specific prompt
/// overrides` table: `| pass | profile | prompts/extract.mid.md |`.
pub(super) fn parse_override_prompt_paths(
    text: &str,
) -> Result<HashMap<(String, String), String>, SchemaError> {
    let mut out: HashMap<(String, String), String> = HashMap::new();
    let mut in_table = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("### Profile-specific prompt overrides") {
            in_table = true;
            continue;
        }
        if in_table && (t.starts_with("### ") || t.starts_with("## ")) {
            break;
        }
        if !in_table {
            continue;
        }
        if !t.starts_with('|') {
            continue;
        }
        if t.contains("Pass") && t.contains("Profile") {
            continue;
        }
        if t.starts_with("|---") || t.starts_with("|--") {
            continue;
        }
        let cols: Vec<&str> = t.split('|').map(str::trim).collect();
        if cols.len() < 4 {
            continue;
        }
        let pass = cols[1].trim_matches('`').trim();
        let profile = cols[2].trim_matches('`').trim();
        let path = cols[3].trim_matches('`').trim();
        if pass.is_empty() || profile.is_empty() || path.is_empty() {
            continue;
        }
        if !SUPPORTED_PASSES.contains(&pass) {
            return Err(SchemaError::Parse(format!(
                "override row references unknown pass `{pass}`"
            )));
        }
        out.insert((pass.to_string(), profile.to_string()), path.to_string());
    }
    Ok(out)
}

/// Parse the `### Profile assignment` table mapping model name →
/// profile name. Returns the (model_name → profile_name) map.
pub(super) fn parse_profile_assignments(
    text: &str,
) -> Result<HashMap<String, String>, SchemaError> {
    let mut out: HashMap<String, String> = HashMap::new();
    let mut in_table = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("### Profile assignment") {
            in_table = true;
            continue;
        }
        if in_table && (t.starts_with("### ") || t.starts_with("## ") || t.starts_with("---")) {
            break;
        }
        if !in_table {
            continue;
        }
        if !t.starts_with('|') {
            continue;
        }
        if t.contains("Model") && t.contains("Profile") {
            continue;
        }
        if t.starts_with("|---") || t.starts_with("|--") {
            continue;
        }
        let cols: Vec<&str> = t.split('|').map(str::trim).collect();
        if cols.len() < 3 {
            continue;
        }
        let model = cols[1].trim_matches('`').trim();
        let profile = cols[2].trim_matches('`').trim();
        if model.is_empty() || profile.is_empty() {
            continue;
        }
        out.insert(model.to_string(), profile.to_string());
    }
    if out.is_empty() {
        return Err(SchemaError::Parse(
            "`### Profile assignment` table missing or empty".into(),
        ));
    }
    Ok(out)
}
