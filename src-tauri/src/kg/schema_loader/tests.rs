//! Tests for the schema loader. Split out of `mod.rs` to keep the
//! parent file under the 600-line cap. Imports are scoped to the
//! parent module's `pub(super)` surface only — these tests do not
//! reach into the private parsers module directly.

use std::collections::HashSet;
use std::path::PathBuf;

use std::sync::Mutex;

use super::{bundled_prompt, EXPECTED_SCHEMA_VERSION, SCHEMA_DIR_ENV};
use super::{Schema, SchemaError, SchemaSource, BUNDLED_SCHEMA, DEFAULT_UNKNOWN_MODEL_PROFILE};

/// Serializes tests that mutate `MOCKINGBIRD_KG_SCHEMA_DIR`. Cargo's
/// default test runner is multi-threaded; without this guard a parallel
/// test that reads the env var could race with one that's mid-set/unset.
/// Acquire BEFORE constructing an `EnvGuard`, hold across all
/// `Schema::load_default()` calls in the test.
static SCHEMA_ENV_LOCK: Mutex<()> = Mutex::new(());

/// RAII guard: sets an env var on construction, restores the prior
/// value (or removes the var if it was unset) on drop — even on panic.
struct EnvGuard {
    key: &'static str,
    prior: Option<String>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let prior = std::env::var(key).ok();
        // SAFETY: tests holding SCHEMA_ENV_LOCK serialize env mutation;
        // see the lock's docstring above.
        std::env::set_var(key, value);
        EnvGuard { key, prior }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.prior {
            Some(v) => std::env::set_var(self.key, v),
            None => std::env::remove_var(self.key),
        }
    }
}

/// The bedrock test: the bundled SCHEMA.md loads cleanly.
#[test]
fn bundled_schema_loads() {
    let s = Schema::load_bundled().expect("load bundled schema");
    assert_eq!(s.schema_version, EXPECTED_SCHEMA_VERSION);
    assert_eq!(s.source, SchemaSource::Bundled);
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
    let s = Schema::load_bundled().unwrap();
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
    assert_eq!(
        s.profile_for("brand-new-model:13b"),
        DEFAULT_UNKNOWN_MODEL_PROFILE
    );
}

/// 3b-class model gets the small-conservative (default) extract
/// prompt — byte-identical to the bundled `prompts/extract.md`.
#[test]
fn small_conservative_extract_uses_default_prompt() {
    let s = Schema::load_bundled().unwrap();
    let body = s
        .prompt_body("extract", "qwen2.5:3b-instruct-q4_K_M")
        .unwrap();
    let expected = include_str!("../assets/prompts/extract.md");
    assert_eq!(body, expected);
}

/// 7b-class model gets the closed-vocab mid-confident extract prompt
/// (Wave 0.5.3 override) — byte-identical to
/// `prompts/extract.closed-vocab.mid-confident.md`.
#[test]
fn mid_confident_extract_uses_override_prompt() {
    let s = Schema::load_bundled().unwrap();
    let body = s
        .prompt_body("extract", "qwen2.5:7b-instruct-q4_K_M")
        .unwrap();
    let expected = include_str!("../assets/prompts/extract.closed-vocab.mid-confident.md");
    assert_eq!(body, expected);
}

/// Phase 1A (`mb-cskk`): the bundled SCHEMA.md ships the **v1 slice**
/// per ADR 0049 amendment A2 — open-vocab tags only, NO
/// `#### Vocabulary list` section. The closed-vocab Rust wiring
/// (`tag_validator.rs::validate_tags` + `synonyms.rs`) remains
/// graduated as the v1.1 starting point; coverage on the closed-vocab
/// pipeline path lives in
/// [`closed_vocab_path_still_active_via_env_override`] below.
///
/// (Wave 0.5.3 held a 228-entry closed vocab here. Stripped
/// during the Phase 1A Wave 3 parity-gate fix.)
#[test]
fn bundled_schema_is_open_vocab() {
    let s = Schema::load_bundled().unwrap();
    assert!(
        !s.has_closed_tag_vocabulary(),
        "bundled SCHEMA.md must be open-vocab per ADR 0049 A2 — \
         the `#### Vocabulary list` section was re-introduced without \
         an ADR amendment"
    );
    assert_eq!(s.canonical_tag_vocabulary().len(), 0);
    assert!(s.canonical_tag_vocabulary_ordered().is_empty());
    assert_eq!(s.schema_revision, "phase-1a-v1-open-vocab");
}

/// Phase 1A (`mb-cskk`): closed-vocab pipeline path stays honest under
/// env-override. The v1.1-deferred Move 3 wiring lives in production
/// code as dead-but-tested; this test exercises it by writing a
/// minimal SCHEMA.md with a `#### Vocabulary list` section to a temp
/// dir, pointing `MOCKINGBIRD_KG_SCHEMA_DIR` at it, and asserting
/// `Schema::load_default()` parses the closed vocab + flips
/// `has_closed_tag_vocabulary() → true`.
///
/// Without this test, future drift in `tag_validator.rs` or
/// `parsers::parse_bullet_list` could silently break the v1.1
/// starting state and no other test would catch it.
#[test]
fn closed_vocab_path_still_active_via_env_override() {
    let _lock = SCHEMA_ENV_LOCK.lock().expect("acquire schema env lock");

    let dir = unique_tmp_dir("kg-schema-closed-vocab-env");
    std::fs::create_dir_all(&dir).unwrap();
    let schema = "\
```yaml
schema_version: 1
schema_revision: test-closed-vocab
```

#### Vocabulary list

- `alpha`
- `beta`
- `gamma`
";
    std::fs::write(dir.join("SCHEMA.md"), schema).unwrap();

    let _guard = EnvGuard::set(SCHEMA_DIR_ENV, dir.to_str().unwrap());
    let s = Schema::load_default().expect("load env-override schema");

    assert_eq!(s.source, SchemaSource::EnvOverride(dir.clone()));
    assert!(
        s.has_closed_tag_vocabulary(),
        "env-override SCHEMA.md with `#### Vocabulary list` section did not flip the flag"
    );
    let vocab: HashSet<String> = s.canonical_tag_vocabulary().clone();
    assert_eq!(vocab.len(), 3);
    for tag in ["alpha", "beta", "gamma"] {
        assert!(
            vocab.contains(tag),
            "missing `{tag}` from env-override vocab"
        );
    }
    let ordered = s.canonical_tag_vocabulary_ordered();
    assert_eq!(
        ordered,
        &["alpha".to_string(), "beta".into(), "gamma".into()]
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// Passes WITHOUT a profile override fall back to the default
/// regardless of model.
#[test]
fn unoverridden_passes_share_default_across_profiles() {
    let s = Schema::load_bundled().unwrap();
    for pass in ["segment", "classify"] {
        let small = s.prompt_body(pass, "qwen2.5:3b-instruct-q4_K_M").unwrap();
        let mid = s.prompt_body(pass, "qwen2.5:7b-instruct-q4_K_M").unwrap();
        assert_eq!(small, mid, "{pass} body diverged across profiles");
    }
}

/// **Parity contract pin.** The prompt body the loader returns for a
/// 3b-class model MUST equal the bundled file content `include_str!`
/// baked in. If a future loader change accidentally trims trailing
/// whitespace or normalizes line endings, this test fails BEFORE a
/// parity-gate corpus run does.
#[test]
fn loader_returns_default_prompt_files_verbatim() {
    let s = Schema::load_bundled().unwrap();
    let small_model = "qwen2.5:3b-instruct-q4_K_M";

    let expected_segment = include_str!("../assets/prompts/segment.md");
    let expected_classify = include_str!("../assets/prompts/classify.md");
    let expected_extract = include_str!("../assets/prompts/extract.md");

    assert_eq!(
        s.prompt_body("segment", small_model).unwrap(),
        expected_segment
    );
    assert_eq!(
        s.prompt_body("classify", small_model).unwrap(),
        expected_classify
    );
    assert_eq!(
        s.prompt_body("extract", small_model).unwrap(),
        expected_extract
    );
}

#[test]
fn version_mismatch_is_rejected() {
    let tmp = unique_tmp_dir("kg-schema-bad-version");
    std::fs::create_dir_all(&tmp).unwrap();
    let schema_path = tmp.join("SCHEMA.md");
    std::fs::write(
        &schema_path,
        "```yaml\nschema_version: 999\nschema_revision: x\n```\n",
    )
    .unwrap();
    let err = Schema::load_from_dir(&tmp).unwrap_err();
    assert!(matches!(err, SchemaError::VersionMismatch { .. }), "{err}");
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn missing_file_yields_not_found() {
    let bogus = unique_tmp_dir("kg-schema-missing");
    let _ = std::fs::remove_dir_all(&bogus);
    let err = Schema::load_from_dir(&bogus).unwrap_err();
    assert!(matches!(err, SchemaError::NotFound(_)), "{err}");
}

/// `load_from_dir` round-trips a copy of the bundled tree: writing
/// the bundled SCHEMA.md + every referenced prompt to a tmpdir and
/// pointing the loader at it produces byte-identical prompt bodies
/// to `load_bundled()`. This is what the env-override path will look
/// like in real use.
#[test]
fn env_override_round_trips_against_bundled() {
    let dir = unique_tmp_dir("kg-schema-env-roundtrip");
    std::fs::create_dir_all(dir.join("prompts")).unwrap();
    std::fs::write(dir.join("SCHEMA.md"), BUNDLED_SCHEMA).unwrap();

    let prompt_relpaths = [
        "prompts/segment.md",
        "prompts/classify.md",
        "prompts/extract.md",
        "prompts/extract.mid-confident.md",
        "prompts/extract.closed-vocab.mid-confident.md",
        "prompts/extract_entities.md",
        "prompts/extract_entities.mid-confident.md",
    ];
    for relpath in prompt_relpaths {
        let body = bundled_prompt(relpath).expect("bundled prompt present");
        std::fs::write(dir.join(relpath), body).unwrap();
    }

    let from_dir = Schema::load_from_dir(&dir).expect("load env-override");
    let bundled = Schema::load_bundled().expect("load bundled");
    assert_eq!(from_dir.source, SchemaSource::EnvOverride(dir.clone()));
    assert_eq!(bundled.source, SchemaSource::Bundled);

    for model in ["qwen2.5:3b-instruct-q4_K_M", "qwen2.5:7b-instruct-q4_K_M"] {
        for pass in ["segment", "classify", "extract", "extract_entities"] {
            let a = from_dir.prompt_body(pass, model).unwrap();
            let b = bundled.prompt_body(pass, model).unwrap();
            assert_eq!(
                a, b,
                "env-override vs bundled diverged for ({pass}, {model})"
            );
        }
    }

    let _ = std::fs::remove_dir_all(&dir);
}

/// Env override with a missing prompt file is a HARD error — the
/// loader must NOT silently fall back to bundled (per R2).
#[test]
fn env_override_with_missing_prompt_errors_hard() {
    let dir = unique_tmp_dir("kg-schema-env-partial");
    std::fs::create_dir_all(dir.join("prompts")).unwrap();
    std::fs::write(dir.join("SCHEMA.md"), BUNDLED_SCHEMA).unwrap();
    // Intentionally only write ONE of the seven referenced prompts.
    std::fs::write(
        dir.join("prompts/segment.md"),
        bundled_prompt("prompts/segment.md").unwrap(),
    )
    .unwrap();

    let err = Schema::load_from_dir(&dir).unwrap_err();
    assert!(matches!(err, SchemaError::PromptNotFound { .. }), "{err}");

    let _ = std::fs::remove_dir_all(&dir);
}

fn unique_tmp_dir(prefix: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "{prefix}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ))
}
