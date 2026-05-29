//! Entity extraction pass — Wave 0.5.4 / `mb-o4ni` / ADR 0049 Move 4.
//!
//! Graduated verbatim from the sandbox under Wave 2 Task 6
//! (`mb-rtk2`) — import-path rewrites only. The five-bucket closed
//! enum + the dedupe-by-`(name, type)` topology stay bit-stable: per
//! Chunk 1's R1 flag (and `docs/knowledge-graph/parity/README.md` §3)
//! the topology must remain compatible with both per-segment AND
//! per-dictation entity-provenance attribution, which Chunk 3's
//! parity probe will exercise.
//!
//! ## Probe-phase decoupling
//!
//! This pass is **NOT wired into the per-dictation pipeline
//! orchestrator** in Phase 1A. It runs as a standalone over the
//! per-segment text the orchestrator already produces. Promotion to
//! in-band is a downstream-wave concern.
//!
//! ## Validation
//!
//! - Each entity's `type` must be one of the five buckets — case
//!   sensitive, lowercased. Unknown types yield a `Parse` error
//!   (via serde's untagged-enum failure) with the raw model output
//!   preserved (parity with `extract.rs` discipline: don't silently
//!   re-coerce).
//! - Each entity's `name` must be non-empty after trimming.
//! - Duplicate `(name, type)` pairs are deduped by the pass (first
//!   occurrence wins, order preserved). The model isn't penalised
//!   for accidentally re-emitting an entity; the scorer compares
//!   sets.

use serde::{Deserialize, Serialize};

use super::super::ollama::{GenerateOptions, OllamaDispatcher};
use super::{strip_json_envelope, PassError};

/// Output of one `extract_entities` call.
///
/// Per the brief, `extract_entities` is **NOT wired into the
/// orchestrator** in Phase 1A (probe-phase decoupling, see the
/// module docstring above). The `dead_code` allow keeps the
/// clippy gate green without lying about the wiring status.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, Serialize)]
pub struct EntityExtraction {
    pub entities: Vec<ExtractedEntity>,
}

/// One extracted entity row. Mirrors the prompt's output schema
/// verbatim so the JSON round-trip is byte-stable.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ExtractedEntity {
    pub name: String,
    #[serde(rename = "type")]
    pub entity_type: EntityType,
    #[serde(default)]
    pub aliases: Vec<String>,
}

/// Five-bucket closed enum. Serde lowercase so the on-the-wire form
/// matches the prompt's instructions exactly. Adding a sixth bucket
/// later is a SCHEMA.md edit + a variant here + a `schema_version`
/// bump (YAGNI says we don't add one until empirical evidence
/// demands it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EntityType {
    /// A specific human (named or referred to by role).
    Person,
    /// A company, team, agency, school, etc.
    Organization,
    /// A concrete physical or digital thing (document, gadget, gift).
    Object,
    /// A geographic or facility location.
    Place,
    /// A named ongoing initiative or body of work.
    Project,
}

impl EntityType {
    /// Lowercase string form — what the prompt emits and what the
    /// scorer matches against. Provided as a method rather than
    /// re-deriving from `Debug` so the wire form is explicit.
    pub fn as_str(self) -> &'static str {
        match self {
            EntityType::Person => "person",
            EntityType::Organization => "organization",
            EntityType::Object => "object",
            EntityType::Place => "place",
            EntityType::Project => "project",
        }
    }
}

impl std::fmt::Display for EntityType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Run the entity-extraction pass on one segment.
///
/// `prompt_body` is the verbatim contents of the per-profile prompt
/// file as resolved by [`super::super::schema_loader::Schema::prompt_body`]
/// for `("extract_entities", model)`. The runtime prompt is
/// `{prompt_body}{per_pass_context_suffix}`, where the suffix is
/// `\n\nSEGMENT:\n{segment}\n` (SCHEMA.md § "Per-pass context
/// suffix").
///
/// **Unwired in Phase 1A** — see the module docstring above for the
/// probe-phase decoupling rationale; the `dead_code` allow signals
/// the intentional gap.
#[allow(dead_code)]
pub fn extract_entities<D: OllamaDispatcher>(
    dispatcher: &D,
    model: &str,
    prompt_body: &str,
    segment: &str,
    options: &GenerateOptions,
) -> Result<EntityExtraction, PassError> {
    let prompt = format!("{prompt_body}\n\nSEGMENT:\n{segment}\n");

    let raw = dispatcher.generate(model, &prompt, None, options)?;
    let candidate = strip_json_envelope(&raw);
    let mut parsed: EntityExtraction =
        serde_json::from_str(candidate).map_err(|e| PassError::Parse {
            pass: "extract_entities",
            error: e.to_string(),
            raw: raw.clone(),
        })?;

    // Validate every row.
    for ent in &parsed.entities {
        if ent.name.trim().is_empty() {
            return Err(PassError::Validation {
                pass: "extract_entities",
                detail: "entity name is empty".to_string(),
                raw,
            });
        }
    }

    // Lowercase + trim every name in-place so downstream scorer
    // comparisons are case-stable. The prompt instructs the model to
    // emit lowercase already, but defensive normalization here means
    // a model that ignores the instruction doesn't silently
    // mismatch the scorer.
    for ent in &mut parsed.entities {
        ent.name = ent.name.trim().to_ascii_lowercase();
        for a in &mut ent.aliases {
            *a = a.trim().to_ascii_lowercase();
        }
    }

    // Dedupe by (name, type), preserving first-seen order. The model
    // isn't penalised for accidental re-emits.
    let mut seen: std::collections::HashSet<(String, EntityType)> =
        std::collections::HashSet::new();
    parsed
        .entities
        .retain(|e| seen.insert((e.name.clone(), e.entity_type)));

    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::super::super::ollama::testing::MockOllama;
    use super::*;

    fn opts() -> GenerateOptions {
        GenerateOptions::default()
    }

    const TEST_PROMPT: &str = "dummy entity extract prompt body";

    #[test]
    fn parses_well_formed_entities() {
        let mock = MockOllama::new().default_response(
            r#"{"entities":[{"name":"madison","type":"person","aliases":[]},{"name":"soccer-cleats","type":"object","aliases":["cleats"]}]}"#,
        );
        let r = extract_entities(
            &mock,
            "m",
            TEST_PROMPT,
            "Madison needs new soccer cleats by Saturday's game.",
            &opts(),
        )
        .unwrap();
        assert_eq!(r.entities.len(), 2);
        assert_eq!(r.entities[0].name, "madison");
        assert_eq!(r.entities[0].entity_type, EntityType::Person);
        assert_eq!(r.entities[1].name, "soccer-cleats");
        assert_eq!(r.entities[1].entity_type, EntityType::Object);
        assert_eq!(r.entities[1].aliases, vec!["cleats"]);
    }

    #[test]
    fn empty_entities_is_valid() {
        let mock = MockOllama::new().default_response(r#"{"entities":[]}"#);
        let r = extract_entities(
            &mock,
            "m",
            TEST_PROMPT,
            "I should start saving more.",
            &opts(),
        )
        .unwrap();
        assert!(r.entities.is_empty());
    }

    #[test]
    fn all_five_buckets_round_trip() {
        let mock = MockOllama::new().default_response(
            r#"{"entities":[
                {"name":"becca","type":"person","aliases":[]},
                {"name":"costco","type":"organization","aliases":[]},
                {"name":"slide-deck","type":"object","aliases":[]},
                {"name":"airport","type":"place","aliases":[]},
                {"name":"docs-migration","type":"project","aliases":[]}
            ]}"#,
        );
        let r = extract_entities(&mock, "m", TEST_PROMPT, "x", &opts()).unwrap();
        assert_eq!(r.entities.len(), 5);
        assert_eq!(r.entities[0].entity_type, EntityType::Person);
        assert_eq!(r.entities[1].entity_type, EntityType::Organization);
        assert_eq!(r.entities[2].entity_type, EntityType::Object);
        assert_eq!(r.entities[3].entity_type, EntityType::Place);
        assert_eq!(r.entities[4].entity_type, EntityType::Project);
    }

    #[test]
    fn rejects_unknown_type() {
        let mock = MockOllama::new()
            .default_response(r#"{"entities":[{"name":"x","type":"event","aliases":[]}]}"#);
        let err = extract_entities(&mock, "m", TEST_PROMPT, "x", &opts()).unwrap_err();
        // Serde's untagged-enum error is reported as a Parse failure;
        // raw output preserved either way (parity with extract.rs
        // discipline).
        let msg = err.to_string();
        assert!(msg.contains("extract_entities"));
        assert!(msg.contains("event"), "raw output must be preserved: {msg}");
    }

    #[test]
    fn rejects_empty_name() {
        let mock = MockOllama::new()
            .default_response(r#"{"entities":[{"name":"   ","type":"person","aliases":[]}]}"#);
        let err = extract_entities(&mock, "m", TEST_PROMPT, "x", &opts()).unwrap_err();
        assert!(err.to_string().contains("entity name is empty"));
    }

    #[test]
    fn lowercases_and_trims_names() {
        let mock = MockOllama::new().default_response(
            r#"{"entities":[{"name":"  Madison ","type":"person","aliases":["  Maddie"]}]}"#,
        );
        let r = extract_entities(&mock, "m", TEST_PROMPT, "x", &opts()).unwrap();
        assert_eq!(r.entities[0].name, "madison");
        assert_eq!(r.entities[0].aliases, vec!["maddie"]);
    }

    #[test]
    fn dedupes_repeated_rows_preserving_order() {
        let mock = MockOllama::new().default_response(
            r#"{"entities":[
                {"name":"becca","type":"person","aliases":[]},
                {"name":"costco","type":"organization","aliases":[]},
                {"name":"becca","type":"person","aliases":[]}
            ]}"#,
        );
        let r = extract_entities(&mock, "m", TEST_PROMPT, "x", &opts()).unwrap();
        assert_eq!(r.entities.len(), 2);
        assert_eq!(r.entities[0].name, "becca");
        assert_eq!(r.entities[1].name, "costco");
    }

    #[test]
    fn same_name_different_type_kept_distinct() {
        let mock = MockOllama::new().default_response(
            r#"{"entities":[
                {"name":"becca","type":"person","aliases":[]},
                {"name":"becca","type":"project","aliases":[]}
            ]}"#,
        );
        let r = extract_entities(&mock, "m", TEST_PROMPT, "x", &opts()).unwrap();
        assert_eq!(r.entities.len(), 2);
    }

    #[test]
    fn strips_markdown_fence_envelope() {
        let mock = MockOllama::new()
            .default_response("```json\n{\"entities\":[{\"name\":\"karen\",\"type\":\"person\",\"aliases\":[]}]}\n```");
        let r = extract_entities(&mock, "m", TEST_PROMPT, "x", &opts()).unwrap();
        assert_eq!(r.entities.len(), 1);
        assert_eq!(r.entities[0].name, "karen");
    }

    #[test]
    fn entity_type_str_round_trip() {
        assert_eq!(EntityType::Person.as_str(), "person");
        assert_eq!(EntityType::Organization.as_str(), "organization");
        assert_eq!(EntityType::Object.as_str(), "object");
        assert_eq!(EntityType::Place.as_str(), "place");
        assert_eq!(EntityType::Project.as_str(), "project");
    }

    #[test]
    fn aliases_default_when_omitted() {
        let mock = MockOllama::new()
            .default_response(r#"{"entities":[{"name":"karen","type":"person"}]}"#);
        let r = extract_entities(&mock, "m", TEST_PROMPT, "x", &opts()).unwrap();
        assert_eq!(r.entities.len(), 1);
        assert!(r.entities[0].aliases.is_empty());
    }
}
