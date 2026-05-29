//! Loader for the portable `SCHEMA.md` contract (ADR 0049 Move 1).
//!
//! Reads `experimental/kg-validation/SCHEMA.md` at runtime, parses
//! the structured fields the pipeline cares about, and validates
//! `schema_version` against [`EXPECTED_SCHEMA_VERSION`]. Each pass
//! takes its prompt body in via this loader instead of via
//! `include_str!`, so the **schema is the contract** — Rust modules
//! are contract-consumers (Clark schema-driven pattern).
//!
//! ## Model-class calibration profiles (Wave 0.5.1, `mb-4xtd`)
//!
//! Different model families / sizes have different natural priors.
//! A single prompt body behaves differently across models — the
//! Phase 0 extract prompt that ran clean on `qwen2.5:3b` (cautious-
//! by-default) breached the date hard-gate on `qwen2.5:7b`
//! (confident-by-default) at the same fixtures (4 invented dates,
//! seed 42). The portable-contract mission survives this by encoding
//! per-class calibration in the schema itself, not by re-tuning the
//! prompt every model swap (LESSONS P10).
//!
//! At load time the schema declares:
//!   - a `default` prompt body per pass (small-conservative variant),
//!   - zero-or-more `(pass, profile) → file` overrides,
//!   - a `model → profile` assignment table + a default profile
//!     for unknown models.
//!
//! At runtime, callers resolve via [`Schema::prompt_body`]:
//!     `prompt_body(pass, model)` = overrides\[(pass, profile_for(model))\]
//!                                  if present, else defaults\[pass\].
//!
//! ## Parity contract
//!
//! The runtime prompt string `{prompt_body}{per_pass_context}` must
//! be byte-identical to the pre-refactor `include_str!`-baked
//! equivalent. The prompt files themselves are unchanged; the loader
//! just reads them at startup instead of at compile time. Empirically
//! pinned by the `loader_returns_prompt_files_verbatim` test below.
//!
//! Parsing strategy: a small line-based parser tailored to this exact
//! SCHEMA.md format. Pulling in a full Markdown crate (`pulldown-cmark`
//! etc.) would more than triple the sandbox dep footprint for a file
//! we control. YAGNI.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// The schema_version this loader speaks. Bump in lockstep with
/// SCHEMA.md whenever a consumer must change to remain compatible.
pub const EXPECTED_SCHEMA_VERSION: u32 = 1;

/// Profile name used as the implicit default for any unknown model
/// AND as the fallback when a profile doesn't override a pass.
/// Per SCHEMA.md "Default for unknown models: `mid-confident`" —
/// chosen because over-cautious-prompt-on-confident-model just adds
/// a few nulls (loud, cheap) while under-cautious-prompt-on-confident-
/// model invents dates (silent trust erosion).
const DEFAULT_UNKNOWN_MODEL_PROFILE: &str = "mid-confident";

const SUPPORTED_PASSES: &[&str] = &["segment", "classify", "extract", "extract_entities"];

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
    #[error(
        "no prompt body resolved for (pass=`{pass}`, model=`{model}` => profile=`{profile}`); \
         no override row AND no default. SCHEMA.md is malformed."
    )]
    NoPromptForPass {
        pass: String,
        model: String,
        profile: String,
    },
}

#[derive(Debug, Clone)]
pub struct Schema {
    pub schema_version: u32,
    pub schema_revision: String,
    pub categories: Vec<String>,
    pub entry_types: Vec<String>,
    pub model_defaults: ModelDefaults,
    /// `model_name → profile_name`. A model not present here uses
    /// [`DEFAULT_UNKNOWN_MODEL_PROFILE`].
    pub profile_assignments: HashMap<String, String>,
    /// Default prompt path per pass (the small-conservative variant).
    pub default_prompt_paths: HashMap<String, PathBuf>,
    /// Profile-specific overrides: `(pass, profile) → path`.
    pub override_prompt_paths: HashMap<(String, String), PathBuf>,
    /// Cached default prompt bodies — read once at load.
    default_prompt_bodies: HashMap<String, String>,
    /// Cached override prompt bodies — read once at load.
    override_prompt_bodies: HashMap<(String, String), String>,
    /// Closed canonical tag vocabulary (Wave 0.5.3 / `mb-rzpd`).
    /// Empty when the schema declares `status: open` for the
    /// vocabulary; populated when status is `closed`.
    canonical_tag_vocabulary: HashSet<String>,
    /// Ordered form for human-readable export (e.g. test reporting).
    canonical_tag_vocabulary_ordered: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ModelDefaults {
    pub segment: String,
    pub classify: String,
    pub extract: String,
    /// Wave 0.5.4 / `mb-o4ni`. New entity-extraction pass; runs as a
    /// standalone probe over per-segment artifacts (decoupled from the
    /// production pipeline orchestrator for the probe phase). Promotion
    /// to in-band depends on the Wave 0.5.4 ≥50% bar + Wave 0.5.6
    /// REPORT acceptance.
    pub extract_entities: String,
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

        // Wave 0.5.3: closed canonical tag vocabulary. The vocab
        // section header is a level-4 `#### Vocabulary list` so it
        // sits cleanly inside the level-3 `### Canonical tag
        // vocabulary (...)` section. Missing-and-empty is OK (older
        // schema revisions don't carry it); missing-with-status-closed
        // would be a malformed schema, but we don't enforce that
        // here — the loader treats absence as "open vocab".
        let canonical_tag_vocabulary_ordered =
            parse_bullet_list(&text, "#### Vocabulary list").unwrap_or_default();
        let canonical_tag_vocabulary: HashSet<String> =
            canonical_tag_vocabulary_ordered.iter().cloned().collect();

        let model_defaults = ModelDefaults {
            segment: parse_model_default(&text, "segment")?,
            classify: parse_model_default(&text, "classify")?,
            extract: parse_model_default(&text, "extract")?,
            extract_entities: parse_model_default(&text, "extract_entities")?,
        };

        let profile_assignments = parse_profile_assignments(&text)?;

        // Default prompt paths — required for every pass.
        let mut default_prompt_paths: HashMap<String, PathBuf> = HashMap::new();
        for pass in SUPPORTED_PASSES {
            let rel = parse_default_prompt_path(&text, pass)?;
            default_prompt_paths.insert((*pass).to_string(), parent.join(rel));
        }

        // Profile overrides — zero or more rows; missing is fine.
        let override_prompt_paths = parse_override_prompt_paths(&text, parent)?;

        // Read the bodies eagerly. A missing prompt file is a load-
        // time error so we don't surprise the runner mid-corpus.
        let mut default_prompt_bodies: HashMap<String, String> = HashMap::new();
        for (pass, path) in &default_prompt_paths {
            default_prompt_bodies.insert(pass.clone(), read_prompt(path)?);
        }
        let mut override_prompt_bodies: HashMap<(String, String), String> = HashMap::new();
        for (key, path) in &override_prompt_paths {
            override_prompt_bodies.insert(key.clone(), read_prompt(path)?);
        }

        Ok(Schema {
            schema_version,
            schema_revision,
            categories,
            entry_types,
            model_defaults,
            profile_assignments,
            default_prompt_paths,
            override_prompt_paths,
            default_prompt_bodies,
            override_prompt_bodies,
            canonical_tag_vocabulary,
            canonical_tag_vocabulary_ordered,
        })
    }

    /// Closed canonical tag vocabulary as a fast-lookup set. Empty
    /// when the schema revision predates Wave 0.5.3 (open vocab).
    pub fn canonical_tag_vocabulary(&self) -> &HashSet<String> {
        &self.canonical_tag_vocabulary
    }

    /// Closed canonical tag vocabulary as the ordered list it
    /// appears in SCHEMA.md. For reporting / debugging only — the
    /// validator uses [`Self::canonical_tag_vocabulary`].
    pub fn canonical_tag_vocabulary_ordered(&self) -> &[String] {
        &self.canonical_tag_vocabulary_ordered
    }

    /// True when this schema declares a closed canonical tag
    /// vocabulary (Wave 0.5.3+). False for open-vocab schemas.
    pub fn has_closed_tag_vocabulary(&self) -> bool {
        !self.canonical_tag_vocabulary.is_empty()
    }

    /// Convenience loader for the canonical sandbox location. The
    /// CLI uses this; tests use [`Schema::load`] against a tempdir.
    pub fn load_default() -> Result<Self, SchemaError> {
        let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
        Self::load(&crate_root.join("SCHEMA.md"))
    }

    /// Resolve `model_name` to a profile via the assignment table,
    /// falling back to [`DEFAULT_UNKNOWN_MODEL_PROFILE`].
    pub fn profile_for(&self, model: &str) -> &str {
        self.profile_assignments
            .get(model)
            .map(String::as_str)
            .unwrap_or(DEFAULT_UNKNOWN_MODEL_PROFILE)
    }

    /// Resolve the prompt body for a pass given the running model.
    /// Tries `(pass, profile_for(model))` first; falls back to the
    /// default-pass body.
    ///
    /// Returns [`SchemaError::NoPromptForPass`] only if BOTH lookups
    /// miss — at load time the parser already required a default-row
    /// for every supported pass, so the fallback is effectively
    /// guaranteed for `pass ∈ SUPPORTED_PASSES`.
    pub fn prompt_body(&self, pass: &str, model: &str) -> Result<&str, SchemaError> {
        let profile = self.profile_for(model);
        if let Some(body) = self
            .override_prompt_bodies
            .get(&(pass.to_string(), profile.to_string()))
        {
            return Ok(body.as_str());
        }
        if let Some(body) = self.default_prompt_bodies.get(pass) {
            return Ok(body.as_str());
        }
        Err(SchemaError::NoPromptForPass {
            pass: pass.to_string(),
            model: model.to_string(),
            profile: profile.to_string(),
        })
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
    // The table lives under `## Per-pass model defaults` and is the
    // only table whose first column is a backtick-quoted pass name
    // AND whose header line contains the literal "Default model".
    // We anchor on the section header to avoid matching the (similar-
    // shaped) prompt-path tables further down the file.
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
        "no model-defaults table row for pass `{pass}` inside `## Per-pass model defaults`"
    )))
}

/// Parse the default `| pass | prompts/segment.md |` rows in the
/// `### Default prompt body per pass` table.
fn parse_default_prompt_path(text: &str, pass: &str) -> Result<String, SchemaError> {
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
fn parse_override_prompt_paths(
    text: &str,
    parent: &Path,
) -> Result<HashMap<(String, String), PathBuf>, SchemaError> {
    let mut out: HashMap<(String, String), PathBuf> = HashMap::new();
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
        // Skip header + delimiter rows.
        if t.contains("Pass") && t.contains("Profile") {
            continue;
        }
        if t.starts_with("|---") || t.starts_with("|--") {
            continue;
        }
        let cols: Vec<&str> = t.split('|').map(str::trim).collect();
        // | _ | pass | profile | path | _ |  ⇒ len == 5 typically.
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
        out.insert((pass.to_string(), profile.to_string()), parent.join(path));
    }
    Ok(out)
}

/// Parse the `### Profile assignment` table mapping model name →
/// profile name. Returns the (model_name → profile_name) map.
fn parse_profile_assignments(text: &str) -> Result<HashMap<String, String>, SchemaError> {
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

        // Default paths populated for every supported pass.
        for pass in ["segment", "classify", "extract"] {
            assert!(
                s.default_prompt_paths.contains_key(pass),
                "missing default for {pass}"
            );
            assert!(
                s.default_prompt_bodies
                    .get(pass)
                    .map(|b| !b.is_empty())
                    .unwrap_or(false),
                "empty default body for {pass}"
            );
        }
    }

    /// Profile assignment table parses + maps the known models.
    #[test]
    fn profile_assignments_parse() {
        let s = Schema::load_default().unwrap();
        assert_eq!(
            s.profile_for("qwen2.5:3b-instruct-q4_K_M"),
            "small-conservative"
        );
        assert_eq!(s.profile_for("qwen2.5:7b-instruct-q4_K_M"), "mid-confident");
        assert_eq!(s.profile_for("gemma2:9b"), "mid-confident");
        assert_eq!(
            s.profile_for("llama3.1:8b-instruct-q4_K_M"),
            "mid-confident"
        );
        // Unknown models fall back to the documented default.
        assert_eq!(
            s.profile_for("brand-new-model:13b"),
            DEFAULT_UNKNOWN_MODEL_PROFILE
        );
    }

    /// 3b-class model gets the small-conservative (default) extract
    /// prompt — byte-identical to `prompts/extract.md`.
    #[test]
    fn small_conservative_extract_uses_default_prompt() {
        let s = Schema::load_default().unwrap();
        let body = s
            .prompt_body("extract", "qwen2.5:3b-instruct-q4_K_M")
            .unwrap();
        let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let expected = std::fs::read_to_string(crate_root.join("prompts").join("extract.md"))
            .expect("read prompts/extract.md");
        assert_eq!(body, expected);
    }

    /// 7b-class model gets the closed-vocab mid-confident extract
    /// prompt (Wave 0.5.3 override) — byte-identical to
    /// `prompts/extract.closed-vocab.mid-confident.md`.
    #[test]
    fn mid_confident_extract_uses_override_prompt() {
        let s = Schema::load_default().unwrap();
        let body = s
            .prompt_body("extract", "qwen2.5:7b-instruct-q4_K_M")
            .unwrap();
        let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let expected = std::fs::read_to_string(
            crate_root
                .join("prompts")
                .join("extract.closed-vocab.mid-confident.md"),
        )
        .expect("read prompts/extract.closed-vocab.mid-confident.md");
        assert_eq!(body, expected);
    }

    /// Wave 0.5.3: SCHEMA.md ships a 228-entry closed canonical tag
    /// vocabulary. The set is exposed for the validator and a
    /// representative spot-check of corpus-grounded canonicals +
    /// domain pads is intact.
    #[test]
    fn canonical_tag_vocabulary_loads() {
        let s = Schema::load_default().unwrap();
        assert!(s.has_closed_tag_vocabulary());
        let vocab = s.canonical_tag_vocabulary();
        assert_eq!(
            vocab.len(),
            228,
            "vocab size drift; bullets in SCHEMA.md changed?"
        );

        // Corpus-grounded canonicals from synonym-map v1.1.
        for tag in [
            "daycare",
            "kid",
            "car-repair",
            "olivia",
            "permission-slip",
            "q3",
            "venmo",
        ] {
            assert!(
                vocab.contains(tag),
                "missing corpus canonical `{tag}` from closed vocab"
            );
        }

        // Domain pads added in Wave 0.5.3.
        for tag in [
            "call",
            "follow-up",
            "medication",
            "dmv",
            "flight",
            "deadline",
            "subscription",
        ] {
            assert!(
                vocab.contains(tag),
                "missing domain pad `{tag}` from closed vocab"
            );
        }

        // Ordered form has the same length as the set + zero
        // duplicates after collection. We do NOT pin Rust-ordinal
        // sort here because the schema list is authored against
        // culture-aware (en-US) sort — which orders `budgeting`
        // before `budget-revision`, whereas Rust ordinal sort would
        // order `budget-revision` first because `-`(45) < `e`(101).
        // Both orderings are "alphabetical" by reasonable
        // definitions; we accept either and just verify there are
        // no dupes.
        let ordered = s.canonical_tag_vocabulary_ordered();
        assert_eq!(ordered.len(), 228);
        let unique: HashSet<&String> = ordered.iter().collect();
        assert_eq!(unique.len(), 228, "vocab list contains duplicates");
    }

    /// Passes WITHOUT a profile override fall back to the default
    /// regardless of model — both 3b and 7b should see the same
    /// segment + classify bodies (we only override `extract`).
    #[test]
    fn unoverridden_passes_share_default_across_profiles() {
        let s = Schema::load_default().unwrap();
        for pass in ["segment", "classify"] {
            let small = s.prompt_body(pass, "qwen2.5:3b-instruct-q4_K_M").unwrap();
            let mid = s.prompt_body(pass, "qwen2.5:7b-instruct-q4_K_M").unwrap();
            assert_eq!(small, mid, "{pass} body diverged across profiles");
        }
    }

    /// **Parity contract pin.** The prompt body the loader returns
    /// for a 3b-class model MUST equal the file content `include_str!`
    /// previously baked in — i.e. the on-disk `prompts/*.md` file
    /// content. If a future loader change accidentally trims trailing
    /// whitespace or normalizes line endings, this test fails BEFORE
    /// a parity-gate corpus run does.
    #[test]
    fn loader_returns_default_prompt_files_verbatim() {
        let s = Schema::load_default().unwrap();
        let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let small_model = "qwen2.5:3b-instruct-q4_K_M";

        for pass in ["segment", "classify", "extract"] {
            let body = s.prompt_body(pass, small_model).unwrap();
            let expected =
                std::fs::read_to_string(crate_root.join("prompts").join(format!("{pass}.md")))
                    .unwrap();
            assert_eq!(
                body, expected,
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
