//! Loader for the portable `SCHEMA.md` contract (ADR 0049 Move 1),
//! graduated from the sandbox under Wave 2 Task 2 (`mb-6kon`).
//!
//! ## What changed at graduation (binding parameter D2)
//!
//! - **Bundled assets via `include_str!`.** Pre-graduation the loader
//!   read `experimental/kg-validation/SCHEMA.md` and the `prompts/*.md`
//!   files at runtime via `env!("CARGO_MANIFEST_DIR")`. In production
//!   the schema + prompts live next to the binary, so they're baked
//!   in at compile time from `src-tauri/src/kg/assets/`.
//!
//! - **Env-var override: `MOCKINGBIRD_KG_SCHEMA_DIR`.** When set, the
//!   loader reads SCHEMA.md + `prompts/*.md` from that directory
//!   instead of the bundled copy. Useful for prompt iteration without
//!   rebuilding the binary.
//!
//! - **Either / or, never merge (per Chunk 1 R2 flag).** The loader
//!   picks ONE source — env if the env-dir is complete, else bundled —
//!   and emits a `tracing::info!` line at load naming which source
//!   won. If the env dir is set but a referenced prompt is missing,
//!   the loader errors out; it does NOT silently fall back to bundled,
//!   because a half-merged prompt set would invalidate the
//!   parity-fixture contract.
//!
//! - **`anyhow` → `thiserror`** (binding parameter D3): the loader's
//!   error enum is unchanged in variant shape, just re-rooted on
//!   `thiserror::Error`.
//!
//! ## File layout (split for the 600-line cap)
//!
//! - `mod.rs` (this file) — `Schema`, `SchemaSource`, `SchemaError`,
//!   loader entry points, bundled-assets table.
//! - `parsers.rs` — the line-based SCHEMA.md parsers (unchanged from
//!   the sandbox apart from re-rooting on `super::SchemaError`).
//! - `tests.rs` — the loader's tests.
//!
//! ## Parity contract
//!
//! The runtime prompt string `{prompt_body}{per_pass_context}` must
//! be byte-identical to the sandbox's `include_str!`-baked equivalent.
//! The bundled `assets/prompts/*.md` files are exact byte copies of
//! the sandbox `prompts/*.md`. The `loader_returns_default_prompt_files_verbatim`
//! test pins this against the bundled copy.

mod parsers;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use thiserror::Error;

/// The schema_version this loader speaks. Bump in lockstep with
/// SCHEMA.md whenever a consumer must change to remain compatible.
pub const EXPECTED_SCHEMA_VERSION: u32 = 1;

/// Env var that, when set, points at a directory containing
/// `SCHEMA.md` + `prompts/*.md`. Overrides the bundled assets.
pub const SCHEMA_DIR_ENV: &str = "MOCKINGBIRD_KG_SCHEMA_DIR";

/// Profile name used as the implicit default for any unknown model
/// AND as the fallback when a profile doesn't override a pass.
pub(super) const DEFAULT_UNKNOWN_MODEL_PROFILE: &str = "mid-confident";

pub(super) const SUPPORTED_PASSES: &[&str] =
    &["segment", "classify", "extract", "extract_entities"];

// ── Bundled assets (binding parameter D2) ───────────────────────────
//
// `include_str!` paths are relative to THIS source file
// (`src-tauri/src/kg/schema_loader/mod.rs`), so they resolve into
// `src-tauri/src/kg/assets/`.

pub(super) const BUNDLED_SCHEMA: &str = include_str!("../assets/SCHEMA.md");

/// Lookup table for the bundled prompt bodies. Keyed by the
/// schema-declared relative path (e.g. `prompts/segment.md`). If
/// SCHEMA.md adds a new prompt file, the match below MUST grow a
/// new arm or `Schema::load_bundled` errors with
/// [`SchemaError::PromptNotFound`].
pub(super) fn bundled_prompt(relpath: &str) -> Option<&'static str> {
    match relpath {
        "prompts/segment.md" => Some(include_str!("../assets/prompts/segment.md")),
        "prompts/classify.md" => Some(include_str!("../assets/prompts/classify.md")),
        "prompts/extract.md" => Some(include_str!("../assets/prompts/extract.md")),
        "prompts/extract.mid-confident.md" => {
            Some(include_str!("../assets/prompts/extract.mid-confident.md"))
        }
        "prompts/extract.closed-vocab.mid-confident.md" => Some(include_str!(
            "../assets/prompts/extract.closed-vocab.mid-confident.md"
        )),
        "prompts/extract_entities.md" => {
            Some(include_str!("../assets/prompts/extract_entities.md"))
        }
        "prompts/extract_entities.mid-confident.md" => Some(include_str!(
            "../assets/prompts/extract_entities.mid-confident.md"
        )),
        _ => None,
    }
}

#[derive(Debug, Error)]
pub enum SchemaError {
    #[error("schema file not found at {0}")]
    NotFound(PathBuf),
    #[error("schema file read failed at {path}: {error}")]
    Io { path: PathBuf, error: String },
    #[error("schema parse failed: {0}")]
    Parse(String),
    #[error(
        "schema_version mismatch: loader expects {expected}, schema declares {found}. Either update SCHEMA.md or rebuild the binary."
    )]
    VersionMismatch { expected: u32, found: u32 },
    #[error(
        "prompt {relpath:?} referenced by schema not found (loaded_from: {loaded_from}); \
         check `MOCKINGBIRD_KG_SCHEMA_DIR` if set, or that the bundled prompt list in \
         `schema_loader::bundled_prompt` covers it"
    )]
    PromptNotFound {
        relpath: String,
        /// Human-readable description of where the loader tried to
        /// read the prompt from. NOT named `source` because
        /// `thiserror` reserves that field name for upstream
        /// `std::error::Error` causes.
        loaded_from: String,
    },
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

/// Where the in-memory schema + prompts came from. Recorded once at
/// load and reported via `tracing::info!` so the parity-probe report
/// can show "this run used the env-override at /tmp/foo" vs "this
/// run used the bundled assets" without grepping logs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaSource {
    Bundled,
    EnvOverride(PathBuf),
}

#[derive(Debug, Clone)]
pub struct Schema {
    pub schema_version: u32,
    pub schema_revision: String,
    pub categories: Vec<String>,
    pub entry_types: Vec<String>,
    pub model_defaults: ModelDefaults,
    /// Which source the schema came from (bundled vs env-override).
    pub source: SchemaSource,
    /// `model_name → profile_name`. A model not present here uses
    /// [`DEFAULT_UNKNOWN_MODEL_PROFILE`].
    pub profile_assignments: HashMap<String, String>,
    /// Default prompt path per pass — schema-relative string from
    /// SCHEMA.md (`prompts/segment.md` etc.), NOT a resolved
    /// absolute path. Kept for diagnostic output.
    pub default_prompt_paths: HashMap<String, String>,
    /// Profile-specific overrides: `(pass, profile) → schema-relative path`.
    pub override_prompt_paths: HashMap<(String, String), String>,
    /// Cached default prompt bodies — read once at load.
    pub(super) default_prompt_bodies: HashMap<String, String>,
    /// Cached override prompt bodies — read once at load.
    override_prompt_bodies: HashMap<(String, String), String>,
    /// Closed canonical tag vocabulary (Wave 0.5.3 / `mb-rzpd`).
    canonical_tag_vocabulary: HashSet<String>,
    canonical_tag_vocabulary_ordered: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ModelDefaults {
    pub segment: String,
    pub classify: String,
    pub extract: String,
    /// Wave 0.5.4 / `mb-o4ni`. New entity-extraction pass.
    pub extract_entities: String,
}

impl Schema {
    /// Load the schema for normal production use.
    ///
    /// Source selection (per binding parameter D2 + Chunk 1 R2):
    ///
    /// 1. If `MOCKINGBIRD_KG_SCHEMA_DIR` is set, the loader reads
    ///    `SCHEMA.md` + all referenced `prompts/*.md` from that
    ///    directory. Any missing file is a hard error (not a fall-back
    ///    to bundled).
    /// 2. Otherwise the bundled assets are used (`include_str!`).
    ///
    /// Emits one `tracing::info!` line naming the winning source.
    pub fn load_default() -> Result<Self, SchemaError> {
        if let Ok(env_dir) = std::env::var(SCHEMA_DIR_ENV) {
            let env_path = PathBuf::from(env_dir);
            let schema_path = env_path.join("SCHEMA.md");
            tracing::info!(
                target: "kg::schema",
                source = "env-override",
                env_var = SCHEMA_DIR_ENV,
                path = %schema_path.display(),
                "loading SCHEMA.md from env-override directory"
            );
            return Self::load_from_dir(&env_path);
        }
        tracing::info!(
            target: "kg::schema",
            source = "bundled",
            "loading bundled SCHEMA.md (no MOCKINGBIRD_KG_SCHEMA_DIR set)"
        );
        Self::load_bundled()
    }

    /// Load from a runtime directory (test helper + env-override
    /// path). Layout: `dir/SCHEMA.md` + `dir/prompts/*.md`.
    pub fn load_from_dir(dir: &Path) -> Result<Self, SchemaError> {
        let schema_path = dir.join("SCHEMA.md");
        if !schema_path.exists() {
            return Err(SchemaError::NotFound(schema_path));
        }
        let text = std::fs::read_to_string(&schema_path).map_err(|e| SchemaError::Io {
            path: schema_path.clone(),
            error: e.to_string(),
        })?;
        Self::parse_and_resolve(
            &text,
            |relpath| {
                let p = dir.join(relpath);
                if !p.exists() {
                    return Err(SchemaError::PromptNotFound {
                        relpath: relpath.to_string(),
                        loaded_from: format!("filesystem: {}", p.display()),
                    });
                }
                std::fs::read_to_string(&p).map_err(|e| SchemaError::Io {
                    path: p,
                    error: e.to_string(),
                })
            },
            SchemaSource::EnvOverride(dir.to_path_buf()),
        )
    }

    /// Load from the bundled `include_str!` assets.
    pub fn load_bundled() -> Result<Self, SchemaError> {
        Self::parse_and_resolve(
            BUNDLED_SCHEMA,
            |relpath| {
                bundled_prompt(relpath).map(str::to_string).ok_or_else(|| {
                    SchemaError::PromptNotFound {
                        relpath: relpath.to_string(),
                        loaded_from: "bundled (include_str)".to_string(),
                    }
                })
            },
            SchemaSource::Bundled,
        )
    }

    /// Shared parser + prompt-resolution path. The `read_prompt`
    /// closure abstracts where the prompt body comes from (filesystem
    /// vs `include_str!` table) so the parser logic stays one
    /// implementation.
    fn parse_and_resolve<F>(
        text: &str,
        mut read_prompt: F,
        source: SchemaSource,
    ) -> Result<Self, SchemaError>
    where
        F: FnMut(&str) -> Result<String, SchemaError>,
    {
        let schema_version = parsers::parse_yaml_int(text, "schema_version")?;
        let schema_revision = parsers::parse_yaml_str(text, "schema_revision")?;
        if schema_version != EXPECTED_SCHEMA_VERSION {
            return Err(SchemaError::VersionMismatch {
                expected: EXPECTED_SCHEMA_VERSION,
                found: schema_version,
            });
        }

        let categories = parsers::parse_bullet_list(text, "### Categories (closed enum)")?;
        let entry_types = parsers::parse_bullet_list(text, "### Entry types (closed enum)")?;

        // Wave 0.5.3 canonical tag vocab — absence is OK (open vocab).
        let canonical_tag_vocabulary_ordered =
            parsers::parse_bullet_list(text, "#### Vocabulary list").unwrap_or_default();
        let canonical_tag_vocabulary: HashSet<String> =
            canonical_tag_vocabulary_ordered.iter().cloned().collect();

        let model_defaults = ModelDefaults {
            segment: parsers::parse_model_default(text, "segment")?,
            classify: parsers::parse_model_default(text, "classify")?,
            extract: parsers::parse_model_default(text, "extract")?,
            extract_entities: parsers::parse_model_default(text, "extract_entities")?,
        };

        let profile_assignments = parsers::parse_profile_assignments(text)?;

        let mut default_prompt_paths: HashMap<String, String> = HashMap::new();
        for pass in SUPPORTED_PASSES {
            let rel = parsers::parse_default_prompt_path(text, pass)?;
            default_prompt_paths.insert((*pass).to_string(), rel);
        }

        let override_prompt_paths = parsers::parse_override_prompt_paths(text)?;

        // Resolve every body eagerly. A missing prompt is a load-time
        // error so we don't surprise a runner mid-corpus.
        let mut default_prompt_bodies: HashMap<String, String> = HashMap::new();
        for (pass, relpath) in &default_prompt_paths {
            default_prompt_bodies.insert(pass.clone(), read_prompt(relpath)?);
        }
        let mut override_prompt_bodies: HashMap<(String, String), String> = HashMap::new();
        for (key, relpath) in &override_prompt_paths {
            override_prompt_bodies.insert(key.clone(), read_prompt(relpath)?);
        }

        Ok(Schema {
            schema_version,
            schema_revision,
            categories,
            entry_types,
            model_defaults,
            source,
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
    /// when the schema declares no vocab.
    pub fn canonical_tag_vocabulary(&self) -> &HashSet<String> {
        &self.canonical_tag_vocabulary
    }

    /// Ordered list of canonical tags as authored in SCHEMA.md.
    pub fn canonical_tag_vocabulary_ordered(&self) -> &[String] {
        &self.canonical_tag_vocabulary_ordered
    }

    /// True when this schema declares a closed canonical tag
    /// vocabulary (Wave 0.5.3+).
    pub fn has_closed_tag_vocabulary(&self) -> bool {
        !self.canonical_tag_vocabulary.is_empty()
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

#[cfg(test)]
mod tests;
