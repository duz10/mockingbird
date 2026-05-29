//! Entity-quality scorer — Wave 0.5.4 / `mb-o4ni`.
//!
//! Compares the entity-extraction pass's output against a hand-labeled
//! ground-truth subset (`corpus/entity-labels.jsonl`). The primary
//! metric is **per-dictation Jaccard similarity** on the
//! `(name, entity_type)` tuple set, averaged equally across the
//! labeled subset.
//!
//! ## Why per-dictation averaging instead of pooled set Jaccard?
//!
//! Pooled Jaccard (concatenate all extracted entities, concatenate all
//! labels, compute one Jaccard) would let one entity-rich fixture
//! (e.g. the 11-entity `persona-05-case-03`) dominate the metric. The
//! kickoff spec is explicit: "weighted equally per dictation, averaged
//! over the labeled subset." Per-dictation averaging keeps single-
//! entity fixtures load-bearing for the bar.
//!
//! ## Strict vs fuzzy match
//!
//! - **Strict** (primary metric, gates the ≥ 50% bar):
//!   `(name_lowercased, type)` tuple equality. The pass module already
//!   lowercases names; the scorer applies the same normalization
//!   defensively in case a label slipped through with capitalisation.
//! - **Fuzzy sidecar** (observational, does NOT gate):
//!   Levenshtein distance ≤ 2 on the name AND matching type, OR alias
//!   intersection with the label. Reported per-dictation so the
//!   Wave 0.5.4 IAP analysis can see how much of the gap is genuine
//!   miss vs surface-form drift.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::passes::extract_entities::{EntityExtraction, EntityType, ExtractedEntity};

// ────────────────────────────────────────────────────────────────────
// Ground-truth label loading
// ────────────────────────────────────────────────────────────────────

/// One ground-truth entity row in `corpus/entity-labels.jsonl`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EntityLabel {
    pub name: String,
    #[serde(rename = "type")]
    pub entity_type: EntityType,
    #[serde(default)]
    pub aliases: Vec<String>,
}

/// One labeled dictation. Matches the JSONL line schema:
///
/// ```json
/// {"persona_case": "persona-05-case-03",
///  "entities": [{"name":"madison","type":"person","aliases":[]}, ...],
///  "note": "5-segment dictation..."}
/// ```
///
/// `note` is purely documentation (ambiguous-label decisions, scope
/// reasoning). The scorer ignores it.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LabeledDictation {
    pub persona_case: String,
    pub entities: Vec<EntityLabel>,
    #[serde(default)]
    pub note: String,
}

/// Load all labeled dictations from a JSONL file. Lines beginning
/// with `#` and empty lines are skipped. Returns the labels indexed
/// by `persona_case` for cheap lookup at scoring time.
pub fn load_labels(path: &Path) -> anyhow::Result<HashMap<String, LabeledDictation>> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("read {}: {e}", path.display()))?;
    let mut out: HashMap<String, LabeledDictation> = HashMap::new();
    for (lineno, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let dict: LabeledDictation = serde_json::from_str(trimmed).map_err(|e| {
            anyhow::anyhow!(
                "parse {} line {}: {e}\n  text: {trimmed}",
                path.display(),
                lineno + 1
            )
        })?;
        if out.contains_key(&dict.persona_case) {
            anyhow::bail!(
                "duplicate persona_case {:?} at line {} in {}",
                dict.persona_case,
                lineno + 1,
                path.display()
            );
        }
        out.insert(dict.persona_case.clone(), dict);
    }
    Ok(out)
}

// ────────────────────────────────────────────────────────────────────
// Per-dictation scoring
// ────────────────────────────────────────────────────────────────────

/// One scored dictation. The primary `jaccard` field is what feeds
/// the corpus-average bar; `fuzzy_jaccard` is observational sidecar
/// (Levenshtein ≤ 2 + alias-match relaxation).
#[derive(Debug, Clone, Serialize)]
pub struct EntityQualityEntry {
    pub persona_case: String,
    pub extracted: Vec<ExtractedEntity>,
    pub labeled: Vec<EntityLabel>,
    /// Strict Jaccard on `(name, type)` tuples — primary metric.
    pub jaccard: f64,
    /// Observational: Levenshtein ≤ 2 OR alias match. Always ≥
    /// `jaccard`. Reported per-dictation only.
    pub fuzzy_jaccard: f64,
    /// Tuples in `labeled` not matched strictly. For Wave 0.5.4 IAP
    /// diagnostics.
    pub missed: Vec<(String, EntityType)>,
    /// Tuples in `extracted` not in `labeled`. The model emitting
    /// extras the label set doesn't include — could be legitimate
    /// (label is conservative) or fabrication.
    pub extra: Vec<(String, EntityType)>,
}

/// Corpus-level scorecard for entity quality.
#[derive(Debug, Clone, Serialize)]
pub struct EntityQualityScore {
    pub labeled_subset_size: usize,
    pub scored_count: usize,
    pub corpus_average_jaccard: f64,
    pub corpus_average_fuzzy_jaccard: f64,
    pub per_entry: Vec<EntityQualityEntry>,
}

impl EntityQualityScore {
    /// The ADR 0049 / Wave 0.5.4 bar. ≥ 50% advances entity layer
    /// to v1 (conditional on Wave 0.5.6 review); < 50% triggers v1
    /// tags-only fallback (open-vocab + synonym-map + new-tag-request
    /// growth loop, the v1.1 deferral named in ADR 0049).
    pub fn meets_bar(&self) -> bool {
        self.corpus_average_jaccard >= 0.50
    }
}

/// Stability metric across two seeds. Per the kickoff: entity-quality
/// stability seed 42 vs seed 137 ≥ 75% Jaccard on the EXTRACTED entity
/// sets. This is set-Jaccard on the `(name, type)` tuples, per
/// dictation, averaged.
pub fn stability_jaccard(
    extracted_a: &HashMap<String, EntityExtraction>,
    extracted_b: &HashMap<String, EntityExtraction>,
) -> f64 {
    let cases: BTreeSet<&String> = extracted_a.keys().chain(extracted_b.keys()).collect();
    if cases.is_empty() {
        return 1.0;
    }
    let mut sum = 0.0;
    for case in &cases {
        let a = extracted_a
            .get(*case)
            .map(|e| tuple_set_from_extracted(&e.entities))
            .unwrap_or_default();
        let b = extracted_b
            .get(*case)
            .map(|e| tuple_set_from_extracted(&e.entities))
            .unwrap_or_default();
        sum += set_jaccard(&a, &b);
    }
    sum / cases.len() as f64
}

/// Score one corpus-wide run. `extracted` is the model output indexed
/// by `persona_case` (matching the JSONL key); `labels` is the loaded
/// ground truth. Dictations in `labels` without a corresponding
/// `extracted` entry are scored as `jaccard = 0.0` (model failed to
/// run — failure to produce is a miss, not an exclusion).
pub fn score_entity_quality(
    extracted: &HashMap<String, EntityExtraction>,
    labels: &HashMap<String, LabeledDictation>,
) -> EntityQualityScore {
    let mut per_entry = Vec::with_capacity(labels.len());
    let mut jaccard_sum = 0.0;
    let mut fuzzy_sum = 0.0;

    // Sort keys so the output is stable across runs (HashMap
    // iteration order is randomised).
    let mut keys: Vec<&String> = labels.keys().collect();
    keys.sort();

    for k in keys {
        let label = &labels[k];
        let empty = EntityExtraction::default();
        let actual = extracted.get(k).unwrap_or(&empty);

        let label_tuples = tuple_set_from_labels(&label.entities);
        let actual_tuples = tuple_set_from_extracted(&actual.entities);

        let jaccard = set_jaccard(&label_tuples, &actual_tuples);
        let fuzzy_jaccard = fuzzy_jaccard_score(&label.entities, &actual.entities);

        let missed: Vec<(String, EntityType)> =
            label_tuples.difference(&actual_tuples).cloned().collect();
        let extra: Vec<(String, EntityType)> =
            actual_tuples.difference(&label_tuples).cloned().collect();

        jaccard_sum += jaccard;
        fuzzy_sum += fuzzy_jaccard;

        per_entry.push(EntityQualityEntry {
            persona_case: k.to_string(),
            extracted: actual.entities.clone(),
            labeled: label.entities.clone(),
            jaccard,
            fuzzy_jaccard,
            missed,
            extra,
        });
    }

    let n = per_entry.len();
    let corpus_average_jaccard = if n == 0 { 0.0 } else { jaccard_sum / n as f64 };
    let corpus_average_fuzzy_jaccard = if n == 0 { 0.0 } else { fuzzy_sum / n as f64 };

    EntityQualityScore {
        labeled_subset_size: labels.len(),
        scored_count: n,
        corpus_average_jaccard,
        corpus_average_fuzzy_jaccard,
        per_entry,
    }
}

// ────────────────────────────────────────────────────────────────────
// Helpers
// ────────────────────────────────────────────────────────────────────

fn tuple_set_from_labels(rows: &[EntityLabel]) -> BTreeSet<(String, EntityType)> {
    rows.iter()
        .map(|r| (r.name.trim().to_ascii_lowercase(), r.entity_type))
        .collect()
}

fn tuple_set_from_extracted(rows: &[ExtractedEntity]) -> BTreeSet<(String, EntityType)> {
    rows.iter()
        .map(|r| (r.name.trim().to_ascii_lowercase(), r.entity_type))
        .collect()
}

fn set_jaccard(a: &BTreeSet<(String, EntityType)>, b: &BTreeSet<(String, EntityType)>) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let intersect = a.intersection(b).count();
    let union = a.union(b).count();
    if union == 0 {
        return 0.0;
    }
    intersect as f64 / union as f64
}

/// Fuzzy Jaccard: a label tuple counts as matched if either
///   (a) it appears strictly in the extracted set, OR
///   (b) some extracted row of the same type has Levenshtein ≤ 2 to
///       the label name, OR
///   (c) some extracted row's name is in the label's `aliases` list.
///
/// Symmetric on the extracted side. The numerator is the matched
/// label rows; the denominator is `|label ∪ extracted|` after the
/// fuzzy-relaxation collapses paired rows. This is intentionally a
/// SIDECAR metric — we never gate on it.
fn fuzzy_jaccard_score(label: &[EntityLabel], actual: &[ExtractedEntity]) -> f64 {
    if label.is_empty() && actual.is_empty() {
        return 1.0;
    }
    let mut matched_label = vec![false; label.len()];
    let mut matched_actual = vec![false; actual.len()];
    for (li, l) in label.iter().enumerate() {
        let l_name = l.name.trim().to_ascii_lowercase();
        for (ai, a) in actual.iter().enumerate() {
            if matched_actual[ai] {
                continue;
            }
            if a.entity_type != l.entity_type {
                continue;
            }
            let a_name = a.name.trim().to_ascii_lowercase();
            let strict = a_name == l_name;
            let alias_hit = l
                .aliases
                .iter()
                .any(|x| x.trim().to_ascii_lowercase() == a_name)
                || a.aliases
                    .iter()
                    .any(|x| x.trim().to_ascii_lowercase() == l_name);
            let lev_hit = levenshtein(&a_name, &l_name) <= 2;
            if strict || alias_hit || lev_hit {
                matched_label[li] = true;
                matched_actual[ai] = true;
                break;
            }
        }
    }
    let matched_count = matched_label.iter().filter(|b| **b).count();
    let unmatched_label = label.len() - matched_count;
    let unmatched_actual = actual
        .iter()
        .enumerate()
        .filter(|(i, _)| !matched_actual[*i])
        .count();
    let union = matched_count + unmatched_label + unmatched_actual;
    if union == 0 {
        return 1.0;
    }
    matched_count as f64 / union as f64
}

/// Tiny Levenshtein. ~30 LoC and no new dep — pulling `strsim` for
/// one call site fails YAGNI. O(|a| * |b|) time / O(min(|a|, |b|))
/// space (single-row DP).
fn levenshtein(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    if a_chars.is_empty() {
        return b_chars.len();
    }
    if b_chars.is_empty() {
        return a_chars.len();
    }
    // Ensure b is the shorter axis so the row vector stays minimal.
    let (s, t) = if a_chars.len() < b_chars.len() {
        (&b_chars, &a_chars)
    } else {
        (&a_chars, &b_chars)
    };
    let mut prev: Vec<usize> = (0..=t.len()).collect();
    let mut curr: Vec<usize> = vec![0; t.len() + 1];
    for i in 1..=s.len() {
        curr[0] = i;
        for j in 1..=t.len() {
            let cost = if s[i - 1] == t[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[t.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::passes::extract_entities::EntityType;

    fn ext(name: &str, t: EntityType) -> ExtractedEntity {
        ExtractedEntity {
            name: name.to_string(),
            entity_type: t,
            aliases: vec![],
        }
    }

    fn lab(name: &str, t: EntityType) -> EntityLabel {
        EntityLabel {
            name: name.to_string(),
            entity_type: t,
            aliases: vec![],
        }
    }

    fn dict(case: &str, entities: Vec<EntityLabel>) -> LabeledDictation {
        LabeledDictation {
            persona_case: case.to_string(),
            entities,
            note: String::new(),
        }
    }

    #[test]
    fn perfect_match_is_jaccard_one() {
        let labels: HashMap<String, LabeledDictation> = [(
            "p1".to_string(),
            dict(
                "p1",
                vec![
                    lab("madison", EntityType::Person),
                    lab("costco", EntityType::Organization),
                ],
            ),
        )]
        .into_iter()
        .collect();
        let extracted: HashMap<String, EntityExtraction> = [(
            "p1".to_string(),
            EntityExtraction {
                entities: vec![
                    ext("madison", EntityType::Person),
                    ext("costco", EntityType::Organization),
                ],
            },
        )]
        .into_iter()
        .collect();
        let s = score_entity_quality(&extracted, &labels);
        assert!((s.corpus_average_jaccard - 1.0).abs() < 1e-9);
        assert_eq!(s.per_entry[0].missed.len(), 0);
        assert_eq!(s.per_entry[0].extra.len(), 0);
    }

    #[test]
    fn missing_dictation_scores_as_zero_not_excluded() {
        // Label has p1, extracted has nothing. Failure to produce is
        // a miss, not exclusion (otherwise the model could game the
        // metric by emitting empty everywhere).
        let labels: HashMap<String, LabeledDictation> = [(
            "p1".to_string(),
            dict("p1", vec![lab("becca", EntityType::Person)]),
        )]
        .into_iter()
        .collect();
        let extracted: HashMap<String, EntityExtraction> = HashMap::new();
        let s = score_entity_quality(&extracted, &labels);
        assert_eq!(s.scored_count, 1);
        assert!((s.corpus_average_jaccard - 0.0).abs() < 1e-9);
        assert_eq!(
            s.per_entry[0].missed,
            vec![("becca".to_string(), EntityType::Person)]
        );
    }

    #[test]
    fn type_mismatch_is_a_miss() {
        // Same surface name, different type → not a match.
        let labels: HashMap<String, LabeledDictation> = [(
            "p1".to_string(),
            dict("p1", vec![lab("becca", EntityType::Person)]),
        )]
        .into_iter()
        .collect();
        let extracted: HashMap<String, EntityExtraction> = [(
            "p1".to_string(),
            EntityExtraction {
                entities: vec![ext("becca", EntityType::Project)],
            },
        )]
        .into_iter()
        .collect();
        let s = score_entity_quality(&extracted, &labels);
        assert!((s.corpus_average_jaccard - 0.0).abs() < 1e-9);
        assert_eq!(s.per_entry[0].missed.len(), 1);
        assert_eq!(s.per_entry[0].extra.len(), 1);
    }

    #[test]
    fn per_dictation_average_equal_weight() {
        // Two dictations: one perfect (1.0), one zero. Average must
        // be 0.5 — NOT pooled (which would weight by entity count).
        let labels: HashMap<String, LabeledDictation> = [
            (
                "p1".to_string(),
                dict("p1", vec![lab("a", EntityType::Person)]),
            ),
            (
                "p2".to_string(),
                dict(
                    "p2",
                    vec![
                        lab("x", EntityType::Person),
                        lab("y", EntityType::Object),
                        lab("z", EntityType::Place),
                    ],
                ),
            ),
        ]
        .into_iter()
        .collect();
        let extracted: HashMap<String, EntityExtraction> = [
            (
                "p1".to_string(),
                EntityExtraction {
                    entities: vec![ext("a", EntityType::Person)],
                },
            ),
            ("p2".to_string(), EntityExtraction { entities: vec![] }),
        ]
        .into_iter()
        .collect();
        let s = score_entity_quality(&extracted, &labels);
        assert!(
            (s.corpus_average_jaccard - 0.5).abs() < 1e-9,
            "got {}",
            s.corpus_average_jaccard
        );
    }

    #[test]
    fn fuzzy_jaccard_relaxes_levenshtein_2() {
        let label = vec![lab("hendersons", EntityType::Person)];
        let actual = vec![ext("henderson", EntityType::Person)];
        let fuzz = fuzzy_jaccard_score(&label, &actual);
        assert!(
            (fuzz - 1.0).abs() < 1e-9,
            "single-char drop is lev=1, should match"
        );
        // Strict should NOT match
        let lset = tuple_set_from_labels(&label);
        let aset = tuple_set_from_extracted(&actual);
        assert!((set_jaccard(&lset, &aset) - 0.0).abs() < 1e-9);
    }

    #[test]
    fn fuzzy_jaccard_alias_match() {
        let label = vec![EntityLabel {
            name: "mrs-chen".to_string(),
            entity_type: EntityType::Person,
            aliases: vec!["chen".to_string()],
        }];
        let actual = vec![ext("chen", EntityType::Person)];
        let fuzz = fuzzy_jaccard_score(&label, &actual);
        assert!((fuzz - 1.0).abs() < 1e-9);
    }

    #[test]
    fn fuzzy_jaccard_type_must_match_even_under_relaxation() {
        let label = vec![lab("hendersons", EntityType::Person)];
        let actual = vec![ext("henderson", EntityType::Organization)];
        let fuzz = fuzzy_jaccard_score(&label, &actual);
        // Lev=1 on name but wrong type → no match. Each side counts
        // as unmatched ⇒ 0 / (0+1+1) = 0.0.
        assert!((fuzz - 0.0).abs() < 1e-9);
    }

    #[test]
    fn meets_bar_at_50pct_inclusive() {
        let s = EntityQualityScore {
            labeled_subset_size: 2,
            scored_count: 2,
            corpus_average_jaccard: 0.50,
            corpus_average_fuzzy_jaccard: 0.55,
            per_entry: vec![],
        };
        assert!(s.meets_bar());
    }

    #[test]
    fn meets_bar_rejects_below_threshold() {
        let s = EntityQualityScore {
            labeled_subset_size: 2,
            scored_count: 2,
            corpus_average_jaccard: 0.499,
            corpus_average_fuzzy_jaccard: 0.55,
            per_entry: vec![],
        };
        assert!(!s.meets_bar());
    }

    #[test]
    fn stability_jaccard_identical_runs_is_one() {
        let a: HashMap<String, EntityExtraction> = [(
            "p1".to_string(),
            EntityExtraction {
                entities: vec![ext("becca", EntityType::Person)],
            },
        )]
        .into_iter()
        .collect();
        let b = a.clone();
        assert!((stability_jaccard(&a, &b) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn stability_jaccard_half_overlap() {
        let a: HashMap<String, EntityExtraction> = [(
            "p1".to_string(),
            EntityExtraction {
                entities: vec![
                    ext("becca", EntityType::Person),
                    ext("costco", EntityType::Organization),
                ],
            },
        )]
        .into_iter()
        .collect();
        let b: HashMap<String, EntityExtraction> = [(
            "p1".to_string(),
            EntityExtraction {
                entities: vec![ext("becca", EntityType::Person)],
            },
        )]
        .into_iter()
        .collect();
        assert!((stability_jaccard(&a, &b) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn levenshtein_basics() {
        assert_eq!(levenshtein("", ""), 0);
        assert_eq!(levenshtein("a", ""), 1);
        assert_eq!(levenshtein("", "abc"), 3);
        assert_eq!(levenshtein("kitten", "sitting"), 3);
        assert_eq!(levenshtein("henderson", "hendersons"), 1);
        assert_eq!(levenshtein("becca", "becka"), 1);
    }

    #[test]
    fn load_labels_round_trips_real_jsonl() {
        let tmp =
            std::env::temp_dir().join(format!("entity-labels-test-{}.jsonl", std::process::id(),));
        std::fs::write(
            &tmp,
            r#"{"persona_case":"p1","entities":[{"name":"becca","type":"person","aliases":[]}],"note":""}
# this comment line should be skipped
{"persona_case":"p2","entities":[{"name":"costco","type":"organization","aliases":[]}]}
"#,
        )
        .unwrap();
        let labels = load_labels(&tmp).unwrap();
        let _ = std::fs::remove_file(&tmp);
        assert_eq!(labels.len(), 2);
        assert_eq!(labels["p1"].entities[0].name, "becca");
        assert_eq!(
            labels["p2"].entities[0].entity_type,
            EntityType::Organization
        );
    }

    #[test]
    fn load_labels_rejects_duplicate_persona_case() {
        let tmp = std::env::temp_dir().join(format!(
            "entity-labels-dup-test-{}.jsonl",
            std::process::id(),
        ));
        std::fs::write(
            &tmp,
            r#"{"persona_case":"p1","entities":[]}
{"persona_case":"p1","entities":[]}
"#,
        )
        .unwrap();
        let err = load_labels(&tmp).unwrap_err();
        let _ = std::fs::remove_file(&tmp);
        assert!(format!("{err}").contains("duplicate persona_case"));
    }

    #[test]
    fn load_labels_loads_corpus_labels_file() {
        // Safety-net test against the real corpus file. If the
        // hand-labeled `corpus/entity-labels.jsonl` parses cleanly +
        // contains the expected number of dictations, the Wave 0.5.4
        // ground-truth contract is intact.
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("corpus")
            .join("entity-labels.jsonl");
        if !path.exists() {
            // Labels file not present in this sandbox build (e.g.
            // during early scaffolding). Test trivially passes —
            // the file load happens in the runtime caller.
            return;
        }
        let labels = load_labels(&path).expect("labels JSONL must parse");
        assert!(
            !labels.is_empty(),
            "entity-labels.jsonl must contain at least one labeled dictation"
        );
        // Sanity: every label has at least one entity OR the empty
        // entities list is intentional (a dictation labeled as "no
        // entities here"). Wave 0.5.4 ships with all rows non-empty
        // by construction.
        for (case, dict) in &labels {
            // ASCII discipline (LESSONS bd-trap pattern carries over):
            // name strings are ASCII-lowercase.
            for ent in &dict.entities {
                assert!(
                    ent.name
                        .chars()
                        .all(|c| c.is_ascii() && !c.is_ascii_uppercase()),
                    "entity {:?} in {case} has non-ASCII or uppercase characters",
                    ent.name
                );
            }
        }
    }
}
