//! Exemplar pool for the ADR 0049 Wave 0.5.2 embeddings classifier.
//!
//! The classifier compares a query segment's embedding to a pool of
//! pre-labeled exemplars and returns the nearest exemplar's
//! `(Category, EntryType)` label. Bootstrap exemplars come from the
//! 32-pair corpus answer keys paired with the model's structured-
//! output bodies (the answer keys are label-only — no per-entry text —
//! so the body text has to come from a structured run that has a
//! per-entry body for the same dictation).
//!
//! ## Pairing rule
//!
//! For each `(answer-key, structured-output)` pair on the same
//! `dictation_id`:
//!
//! - If `answer_key.entries.len() == structured.len()`, treat the two
//!   lists as positionally aligned (`structured[i].body` carries the
//!   text for `answer_key.entries[i]`'s label). This matches the
//!   scorer's sequential matching contract (`scoring/metrics.rs`).
//! - Otherwise the dictation contributes **zero** exemplars — we
//!   can't honestly pair text with label when the segmentation
//!   diverges. Roughly 1-2 of 32 fixtures fall out here in practice
//!   (Phase 0 segmentation rate is ≥93% on multi-item dictations).
//!
//! ## Leave-one-out (LOO)
//!
//! When the query dictation is itself in the pool, we exclude it.
//! Otherwise the classifier would trivially recover its own labels
//! via the dictation's own entries as exemplars, giving an inflated
//! in-sample score that doesn't generalize. `classify_excluding` is
//! the only public predict path because LOO is non-optional for the
//! ADR 0049 Wave 0.5.2 head-to-head methodology to be defensible.

use std::collections::HashMap;
use std::path::Path;

use crate::embeddings::{cosine_similarity, EmbeddingsDispatcher};
use crate::schema::{AnswerKey, Category, Entry, EntryType};

/// One labeled exemplar: text + its ground-truth label + the source
/// dictation id (for LOO exclusion).
#[derive(Debug, Clone)]
pub struct LabeledExemplar {
    pub case_id: String,
    pub text: String,
    pub category: Category,
    pub entry_type: EntryType,
    pub embedding: Vec<f32>,
}

/// In-memory pool of labeled exemplars. Built once per binary run via
/// [`ExemplarPool::build_from`], then queried per-segment.
#[derive(Debug, Clone)]
pub struct ExemplarPool {
    pub embed_model: String,
    pub exemplars: Vec<LabeledExemplar>,
}

impl ExemplarPool {
    /// Build the pool by walking `answer_keys_dir`, finding the matching
    /// `structured_dir/<dictation_id>.json`, and embedding each
    /// positionally-paired `(body, label)` row.
    ///
    /// Junk-bucket answer keys (`is_junk_no_entry_expected`) contribute
    /// nothing — they have zero expected entries, so there's nothing to
    /// label. Dictations whose structured output's entry count doesn't
    /// match the answer key's count also contribute nothing (see the
    /// module-level "Pairing rule").
    pub fn build_from(
        embedder: &dyn EmbeddingsDispatcher,
        embed_model: &str,
        answer_keys_dir: &Path,
        structured_dir: &Path,
    ) -> anyhow::Result<Self> {
        let mut exemplars = Vec::new();
        let mut entries: Vec<_> = std::fs::read_dir(answer_keys_dir)
            .map_err(|e| anyhow::anyhow!("read_dir {answer_keys_dir:?}: {e}"))?
            .filter_map(|e| e.ok())
            .collect();
        entries.sort_by_key(|e| e.file_name());

        for dirent in entries {
            let path = dirent.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let raw = std::fs::read_to_string(&path)
                .map_err(|e| anyhow::anyhow!("read {path:?}: {e}"))?;
            let key: AnswerKey = serde_json::from_str(&raw)
                .map_err(|e| anyhow::anyhow!("parse {path:?} as AnswerKey: {e}"))?;
            if key.is_junk_no_entry_expected || key.entries.is_empty() {
                continue;
            }

            let structured_path = structured_dir.join(format!("{}.json", key.dictation_id));
            if !structured_path.exists() {
                // Not an error — the structured run may not cover every
                // answer key (e.g. a partial run). Just skip.
                continue;
            }
            let s_raw = std::fs::read_to_string(&structured_path)
                .map_err(|e| anyhow::anyhow!("read {structured_path:?}: {e}"))?;
            let structured: Vec<Entry> = serde_json::from_str(&s_raw)
                .map_err(|e| anyhow::anyhow!("parse {structured_path:?} as Vec<Entry>: {e}"))?;

            if structured.len() != key.entries.len() {
                // Segmentation mismatch — can't honestly pair text with
                // label. Skip silently; the build summary logs the
                // skip count for visibility.
                continue;
            }

            for (entry, expected) in structured.iter().zip(key.entries.iter()) {
                let embedding = embedder.embed(embed_model, &entry.body)?;
                exemplars.push(LabeledExemplar {
                    case_id: key.dictation_id.clone(),
                    text: entry.body.clone(),
                    category: expected.category,
                    entry_type: expected.entry_type,
                    embedding,
                });
            }
        }

        Ok(Self {
            embed_model: embed_model.to_string(),
            exemplars,
        })
    }

    /// Classify `query_embedding` to the nearest exemplar's labels,
    /// excluding any exemplar whose source dictation is `exclude_case_id`.
    ///
    /// Returns `None` if the pool has no exemplars after exclusion
    /// (degenerate; surfaces as a real error in the caller).
    pub fn classify_excluding(
        &self,
        exclude_case_id: &str,
        query_embedding: &[f32],
    ) -> Option<ClassifyResult> {
        let mut best: Option<(f32, &LabeledExemplar)> = None;
        for ex in &self.exemplars {
            if ex.case_id == exclude_case_id {
                continue;
            }
            let sim = cosine_similarity(query_embedding, &ex.embedding);
            if sim.is_nan() {
                continue;
            }
            match best {
                Some((b, _)) if sim <= b => {}
                _ => best = Some((sim, ex)),
            }
        }
        best.map(|(sim, ex)| ClassifyResult {
            category: ex.category,
            entry_type: ex.entry_type,
            similarity: sim,
            source_case_id: ex.case_id.clone(),
        })
    }

    /// Centroid variant of [`Self::classify_excluding`]: compute the
    /// mean embedding per `(category, entry_type)` bucket from the
    /// LOO-filtered pool, then return the bucket whose centroid is
    /// closest to `query_embedding`.
    ///
    /// Smooths over within-label exemplar diversity at the cost of
    /// ignoring multi-modal label distributions. ADR 0049 §Move 2
    /// names this as one of the two reasonable nearest-prototype
    /// strategies; iter-1 used nearest-exemplar, iter-2 swaps to
    /// nearest-centroid to test whether the iter-1 regression was a
    /// kNN-with-k=1 outlier problem rather than an architectural
    /// problem.
    pub fn classify_excluding_by_centroid(
        &self,
        exclude_case_id: &str,
        query_embedding: &[f32],
    ) -> Option<ClassifyResult> {
        // Build per-bucket sum-of-embeddings + counts.
        let mut buckets: HashMap<(Category, EntryType), (Vec<f32>, usize)> = HashMap::new();
        for ex in &self.exemplars {
            if ex.case_id == exclude_case_id {
                continue;
            }
            let key = (ex.category, ex.entry_type);
            let entry = buckets
                .entry(key)
                .or_insert_with(|| (vec![0.0; ex.embedding.len()], 0));
            if entry.0.len() != ex.embedding.len() {
                // dimension drift — skip this exemplar defensively
                continue;
            }
            for i in 0..ex.embedding.len() {
                entry.0[i] += ex.embedding[i];
            }
            entry.1 += 1;
        }

        let mut best: Option<(f32, Category, EntryType)> = None;
        for ((cat, ty), (sum, count)) in &buckets {
            if *count == 0 {
                continue;
            }
            let centroid: Vec<f32> = sum.iter().map(|x| x / *count as f32).collect();
            let sim = cosine_similarity(query_embedding, &centroid);
            if sim.is_nan() {
                continue;
            }
            match best {
                Some((b, _, _)) if sim <= b => {}
                _ => best = Some((sim, *cat, *ty)),
            }
        }
        best.map(|(sim, category, entry_type)| ClassifyResult {
            category,
            entry_type,
            similarity: sim,
            source_case_id: format!("centroid:{:?}-{:?}", category, entry_type),
        })
    }

    /// Distinct source dictation count — useful for build-time logging.
    pub fn distinct_case_count(&self) -> usize {
        let mut seen = HashMap::new();
        for ex in &self.exemplars {
            *seen.entry(ex.case_id.as_str()).or_insert(0_usize) += 1;
        }
        seen.len()
    }
}

/// Outcome of one classification: predicted labels + similarity to the
/// matched exemplar + which case the matched exemplar came from
/// (for debugging / analysis).
#[derive(Debug, Clone, PartialEq)]
pub struct ClassifyResult {
    pub category: Category,
    pub entry_type: EntryType,
    pub similarity: f32,
    pub source_case_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embeddings::testing::MockEmbedder;

    #[test]
    fn classify_picks_nearest_exemplar() {
        let pool = ExemplarPool {
            embed_model: "mock".to_string(),
            exemplars: vec![
                LabeledExemplar {
                    case_id: "c1".to_string(),
                    text: "task one".to_string(),
                    category: Category::Personal,
                    entry_type: EntryType::Task,
                    embedding: vec![1.0, 0.0, 0.0],
                },
                LabeledExemplar {
                    case_id: "c2".to_string(),
                    text: "idea one".to_string(),
                    category: Category::Professional,
                    entry_type: EntryType::Idea,
                    embedding: vec![0.0, 1.0, 0.0],
                },
            ],
        };

        // Query close to c2's vector.
        let result = pool
            .classify_excluding("nothing-matches", &[0.0, 1.0, 0.0])
            .expect("non-empty pool");
        assert_eq!(result.category, Category::Professional);
        assert_eq!(result.entry_type, EntryType::Idea);
        assert_eq!(result.source_case_id, "c2");
        assert!((result.similarity - 1.0).abs() < 1e-6);
    }

    #[test]
    fn classify_excludes_matching_case_id() {
        let pool = ExemplarPool {
            embed_model: "mock".to_string(),
            exemplars: vec![
                // Closest in vector space, but EXCLUDED — its case_id
                // matches the query.
                LabeledExemplar {
                    case_id: "self".to_string(),
                    text: "self".to_string(),
                    category: Category::Personal,
                    entry_type: EntryType::Task,
                    embedding: vec![1.0, 0.0],
                },
                // Second-best, used after exclusion.
                LabeledExemplar {
                    case_id: "other".to_string(),
                    text: "other".to_string(),
                    category: Category::Objective,
                    entry_type: EntryType::Note,
                    embedding: vec![0.9, 0.4],
                },
            ],
        };
        let result = pool.classify_excluding("self", &[1.0, 0.0]).unwrap();
        assert_eq!(result.source_case_id, "other");
        assert_eq!(result.category, Category::Objective);
        assert_eq!(result.entry_type, EntryType::Note);
    }

    #[test]
    fn classify_returns_none_when_all_excluded() {
        let pool = ExemplarPool {
            embed_model: "mock".to_string(),
            exemplars: vec![LabeledExemplar {
                case_id: "only".to_string(),
                text: "only".to_string(),
                category: Category::Personal,
                entry_type: EntryType::Task,
                embedding: vec![1.0],
            }],
        };
        assert!(pool.classify_excluding("only", &[1.0]).is_none());
    }

    #[test]
    fn classify_skips_nan_similarity_rows() {
        // A mismatched-length exemplar returns NaN cosine, which the
        // classifier must skip rather than treat as "best so far".
        let pool = ExemplarPool {
            embed_model: "mock".to_string(),
            exemplars: vec![
                LabeledExemplar {
                    case_id: "bad-dim".to_string(),
                    text: "wrong-dim".to_string(),
                    category: Category::Personal,
                    entry_type: EntryType::Task,
                    embedding: vec![1.0, 0.0, 0.0], // 3D
                },
                LabeledExemplar {
                    case_id: "good".to_string(),
                    text: "right-dim".to_string(),
                    category: Category::Professional,
                    entry_type: EntryType::Idea,
                    embedding: vec![0.5, 0.5], // 2D, matches query
                },
            ],
        };
        let result = pool.classify_excluding("none", &[0.5, 0.5]).unwrap();
        assert_eq!(result.source_case_id, "good");
    }

    #[test]
    fn build_from_skips_junk_keys() {
        // Build a tiny in-memory pair: one junk key + one real key.
        // The junk key must contribute zero exemplars.
        let tmp = tempfile::tempdir().expect("tempdir");
        let ak = tmp.path().join("answer-keys");
        let st = tmp.path().join("structured");
        std::fs::create_dir(&ak).unwrap();
        std::fs::create_dir(&st).unwrap();

        std::fs::write(
            ak.join("persona-junk.json"),
            r#"{
              "dictation_id": "persona-junk",
              "expected_entry_count": 0,
              "is_junk_no_entry_expected": true,
              "entries": []
            }"#,
        )
        .unwrap();

        std::fs::write(
            ak.join("persona-real.json"),
            r#"{
              "dictation_id": "persona-real",
              "expected_entry_count": 1,
              "is_junk_no_entry_expected": false,
              "entries": [
                {
                  "category": "personal",
                  "entry_type": "task",
                  "due_iso": null,
                  "acceptable_topic_tag_sets": [["errand"]]
                }
              ]
            }"#,
        )
        .unwrap();

        std::fs::write(
            st.join("persona-real.json"),
            r#"[
              {
                "title": "Pick up bread",
                "category": "professional",
                "entry_type": "idea",
                "topic_tags": ["errand"],
                "captured_iso": "2026-06-14T08:00:00Z",
                "body": "Pick up bread on the way home."
              }
            ]"#,
        )
        .unwrap();

        let embedder = MockEmbedder::new().with("Pick up bread on the way home.", vec![1.0, 0.0]);

        let pool =
            ExemplarPool::build_from(&embedder, "mock", &ak, &st).expect("build_from succeeds");

        assert_eq!(pool.exemplars.len(), 1, "junk key contributed nothing");
        assert_eq!(pool.exemplars[0].case_id, "persona-real");
        // Note: the structured row had category=professional/idea, but
        // the answer key says personal/task — the pool MUST trust the
        // answer key (label), not the structured output (was 7b's
        // prediction, the thing we're trying to replace).
        assert_eq!(pool.exemplars[0].category, Category::Personal);
        assert_eq!(pool.exemplars[0].entry_type, EntryType::Task);
    }

    #[test]
    fn build_from_skips_segmentation_mismatch() {
        // answer-key has 2 entries, structured has 1 entry → skip.
        let tmp = tempfile::tempdir().expect("tempdir");
        let ak = tmp.path().join("answer-keys");
        let st = tmp.path().join("structured");
        std::fs::create_dir(&ak).unwrap();
        std::fs::create_dir(&st).unwrap();

        std::fs::write(
            ak.join("persona-mismatch.json"),
            r#"{
              "dictation_id": "persona-mismatch",
              "expected_entry_count": 2,
              "is_junk_no_entry_expected": false,
              "entries": [
                {
                  "category": "personal",
                  "entry_type": "task",
                  "due_iso": null,
                  "acceptable_topic_tag_sets": [["a"]]
                },
                {
                  "category": "personal",
                  "entry_type": "idea",
                  "due_iso": null,
                  "acceptable_topic_tag_sets": [["b"]]
                }
              ]
            }"#,
        )
        .unwrap();

        std::fs::write(
            st.join("persona-mismatch.json"),
            r#"[
              {
                "title": "only one",
                "category": "personal",
                "entry_type": "task",
                "topic_tags": ["a"],
                "captured_iso": "2026-06-14T08:00:00Z",
                "body": "Only one entry produced."
              }
            ]"#,
        )
        .unwrap();

        let embedder = MockEmbedder::new();
        let pool = ExemplarPool::build_from(&embedder, "mock", &ak, &st).unwrap();
        assert_eq!(pool.exemplars.len(), 0);
        assert_eq!(embedder.calls().len(), 0, "no embed calls for skip");
    }
}
