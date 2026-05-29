//! Loader for the portable `SCHEMA.md` contract (ADR 0049 Move 1).
//!
//! Reads `experimental/kg-validation/SCHEMA.md` at runtime, parses
//! the structured fields the pipeline cares about, and validates
//! `schema_version` against [`EXPECTED_SCHEMA_VERSION`]. Each pass
//! takes its prompt body in via this loader instead of via
//! `include_str!`, so the **schema is the contract** — Rust modules
//! are contract-consumers (Clark schema-driven pattern).
//!
//! Parity contract (Wave 0.5.1 §parity-gate): the runtime prompt
//! string `{prompt_body}{per_pass_context}` must be byte-identical
//! to the pre-refactor `include_str!`-baked equivalent. The prompt
//! files themselves are unchanged; the loader just reads them at
//! startup instead of at compile time. The deterministic `seed=42`
//! corpus run on `qwen2.5:3b-instruct-q4_K_M` must produce identical
//! `structured/*.json` outputs pre- and post-refactor or the refactor
//! is rolled back.
//!
//! Parsing strategy: a small line-based parser tailored to this exact
//! SCHEMA.md format. Pulling in a full Markdown crate (`pulldown-cmark`
//! etc.) would more than triple the sandbox dep footprint for a file
//! we control. YAGNI.

use std::path::{Path, PathBuf};

/// The schema_version this loader speaks. Bump in lockstep with
/// SCHEMA.md whenever a consumer must change to remain compatible.
pub const EXPECTED_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum SchemaError {
    #[error("schema file not found at {0}")]
    NotFound(PathBuf),
    #[error("schema file read failed at {path}: {error}")]
    Io { path: PathBuf, error: String },
    #[error("schema parse failed: {0}")]
    Parse(String),
    #[error(
        "schema_version mismatch: loader expects {expected}, schema declares {found}. Either update SCHEMA.md or rebuild the sandbox."
    )]
    VersionMismatch { expected: u32, found: u32 },
    #[error("prompt file referenced by schema not found at {0}")]
    PromptNotFound(PathBuf),
}

#[derive(Debug, Clone)]
pub struct Schema {
    pub schema_version: u32,
    pub schema_revision: String,
    pub categories: Vec<String>,
    pub entry_types: Vec<String>,
    pub model_defaults: ModelDefaults,
    /// Absolute paths (resolved from schema-file-relative paths at
    /// load time) to each pass's prompt body file.
    pub prompt_paths: PromptPaths,
    /// Cached prompt bodies — read once at load, never re-read.
    pub prompt_bodies: PromptBodies,
}

#[derive(Debug, Clone)]
pub struct ModelDefaults {
    pub segment: String,
    pub classify: String,
    pub extract: String,
}

#[derive(Debug, Clone)]
pub struct PromptPaths {
    pub segment: PathBuf,
    pub classify: PathBuf,
    pub extract: PathBuf,
}

#[derive(Debug, Clone)]
pub struct PromptBodies {
    pub segment: String,
    pub classify: String,
    pub extract: String,
}

impl Schema {
    /// Load and validate the schema. `schema_path` is the path to
    /// `SCHEMA.md`; prompt paths declared inside the schema are
    /// resolved relative to the schema file's parent directory.
    pub fn load(schema_path: &Path) -> Result<Self, SchemaError> {
        if !schema_path.exists() {
            return Err(SchemaError::NotFound(schema_path.to_path_buf()));
        }
        let text = std::fs::read_to_string(schema_path).map_err(|e| SchemaError::Io {
            path: schema_path.to_path_buf(),
            error: e.to_string(),
        })?;
        let parent = schema_path
            .parent()
            .ok_or_else(|| SchemaError::Parse("schema path has no parent directory".into()))?;

        let schema_version = parse_yaml_int(&text, "schema_version")?;
        let schema_revision = parse_yaml_str(&text, "schema_revision")?;
        if schema_version != EXPECTED_SCHEMA_VERSION {
            return Err(SchemaError::VersionMismatch {
                expected: EXPECTED_SCHEMA_VERSION,
                found: schema_version,
            });
        }

        let categories = parse_bullet_list(&text, "### Categories (closed enum)")?;
        let entry_types = parse_bullet_list(&text, "### Entry types (closed enum)")?;

        let model_defaults = ModelDefaults {
            segment: parse_model_default(&text, "segment")?,
            classify: parse_model_default(&text, "classify")?,
            extract: parse_model_default(&text, "extract")?,
        };

        let prompt_paths = PromptPaths {
            segment: parent.join(parse_prompt_path(&text, "segment")?),
            classify: parent.join(parse_prompt_path(&text, "classify")?),
            extract: parent.join(parse_prompt_path(&text, "extract")?),
        };

        let prompt_bodies = PromptBodies {
            segment: read_prompt(&prompt_paths.segment)?,
            classify: read_prompt(&prompt_paths.classify)?,
            extract: read_prompt(&prompt_paths.extract)?,
        };

        Ok(Schema {
            schema_version,
            schema_revision,
            categories,
            entry_types,
            model_defaults,
            prompt_paths,
            prompt_bodies,
        })
    }

    /// Convenience loader for the canonical sandbox location. The
    /// CLI uses this; tests use [`Schema::load`] against a tempdir.
    pub fn load_default() -> Result<Self, SchemaError> {
        let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
        Self::load(&crate_root.join("SCHEMA.md"))
    }
}

// ── Parsers (all private; line-based, deliberately narrow) ──────────

fn parse_yaml_int(text: &str, key: &str) -> Result<u32, SchemaError> {
    let raw = parse_yaml_str(text, key)?;
    raw.parse::<u32>()
        .map_err(|e| SchemaError::Parse(format!("`{key}` must be a u32: {e} (got `{raw}`)")))
}

/// Find a `key: value` pair inside any fenced ```yaml block in the
/// document. Returns the value with surrounding whitespace stripped.
fn parse_yaml_str(text: &str, key: &str) -> Result<String, SchemaError> {
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
fn parse_bullet_list(text: &str, header: &str) -> Result<Vec<String>, SchemaError> {
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
            // Strip surrounding backticks if present.
            let cleaned = raw.trim_matches('`').trim().to_string();
            if !cleaned.is_empty() {
                items.push(cleaned);
            }
            started = true;
        } else if started && (t.is_empty() || t.starts_with('#')) {
            break;
        } else if started && !t.starts_with("- ") {
            // Allow blank lines AFTER list start to terminate it.
            // A non-blank non-bullet inside the list (a stray
            // paragraph) is treated as the terminator too.
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
fn parse_model_default(text: &str, pass: &str) -> Result<String, SchemaError> {
    let needle = format!("`{pass}`");
    for line in text.lines() {
        let t = line.trim();
        if !t.starts_with('|') {
            continue;
        }
        if !t.contains(&needle) {
            continue;
        }
        let cols: Vec<&str> = t.split('|').map(str::trim).collect();
        // cols[0] is empty (leading `|`), cols[1] is the pass cell,
        // cols[2] is the model cell.
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
        "no model-defaults table row for pass `{pass}`"
    )))
}

/// Parse the `| pass | prompts/segment.md |` rows.
fn parse_prompt_path(text: &str, pass: &str) -> Result<String, SchemaError> {
    let needle = format!("`{pass}`");
    let mut in_prompts_section = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("## Pass prompts") {
            in_prompts_section = true;
            continue;
        }
        if in_prompts_section && t.starts_with("## ") {
            // Left the section without finding it.
            break;
        }
        if !in_prompts_section {
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
        "no prompt-path table row for pass `{pass}` inside `## Pass prompts`"
    )))
}

fn read_prompt(path: &Path) -> Result<String, SchemaError> {
    if !path.exists() {
        return Err(SchemaError::PromptNotFound(path.to_path_buf()));
    }
    std::fs::read_to_string(path).map_err(|e| SchemaError::Io {
        path: path.to_path_buf(),
        error: e.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bedrock test: the real on-disk SCHEMA.md loads cleanly.
    #[test]
    fn real_schema_loads() {
        let s = Schema::load_default().expect("load default schema");
        assert_eq!(s.schema_version, EXPECTED_SCHEMA_VERSION);
        assert_eq!(
            s.categories,
            vec![
                "personal".to_string(),
                "professional".into(),
                "objective".into()
            ]
        );
        assert_eq!(
            s.entry_types,
            vec![
                "task".to_string(),
                "research".into(),
                "idea".into(),
                "note".into(),
                "reference".into()
            ]
        );
        assert_eq!(s.model_defaults.segment, "qwen2.5:7b-instruct-q4_K_M");
        assert_eq!(s.model_defaults.classify, "qwen2.5:7b-instruct-q4_K_M");
        assert_eq!(s.model_defaults.extract, "qwen2.5:7b-instruct-q4_K_M");
        assert!(s.prompt_paths.segment.ends_with("segment.md"));
        assert!(!s.prompt_bodies.segment.is_empty());
        assert!(!s.prompt_bodies.classify.is_empty());
        assert!(!s.prompt_bodies.extract.is_empty());
    }

    /// **Parity contract pin.** The prompt body the loader returns
    /// MUST equal the file content `include_str!` previously baked
    /// in — i.e. the on-disk `prompts/*.md` file content. If a
    /// future loader change accidentally trims trailing whitespace
    /// or normalizes line endings, this test fails BEFORE a parity-
    /// gate corpus run does.
    #[test]
    fn loader_returns_prompt_files_verbatim() {
        let s = Schema::load_default().unwrap();
        let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));

        for (pass, body) in [
            ("segment", &s.prompt_bodies.segment),
            ("classify", &s.prompt_bodies.classify),
            ("extract", &s.prompt_bodies.extract),
        ] {
            let expected =
                std::fs::read_to_string(crate_root.join("prompts").join(format!("{pass}.md")))
                    .unwrap();
            assert_eq!(
                body, &expected,
                "schema-loaded {pass} prompt diverges from prompts/{pass}.md verbatim"
            );
        }
    }

    #[test]
    fn version_mismatch_is_rejected() {
        let tmp = std::env::temp_dir().join(format!(
            "kg-schema-bad-version-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let schema_path = tmp.join("SCHEMA.md");
        std::fs::write(
            &schema_path,
            "```yaml\nschema_version: 999\nschema_revision: x\n```\n",
        )
        .unwrap();
        let err = Schema::load(&schema_path).unwrap_err();
        assert!(matches!(err, SchemaError::VersionMismatch { .. }), "{err}");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn missing_file_yields_not_found() {
        let bogus = std::env::temp_dir().join("nope-schema.md");
        let _ = std::fs::remove_file(&bogus);
        let err = Schema::load(&bogus).unwrap_err();
        assert!(matches!(err, SchemaError::NotFound(_)), "{err}");
    }
}
