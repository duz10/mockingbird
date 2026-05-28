//! Per-dictation, per-metric scorer (spec §8.2).
//!
//! Inputs:
//! - A directory of structured pipeline outputs (`runs/<id>/structured/*.json`)
//! - The matching corpus answer keys (`corpus/answer-keys/*.json`)
//! - An optional tag-equivalence judge (the LLM-mediated metric)
//!
//! Output: a [`ScoreReport`] that maps cleanly to spec §8.4
//! thresholds and gets persisted to `runs/<run-id>/SCORE.json` plus
//! a human-readable `SCORE_SUMMARY.md`.
//!
//! ## Match algorithm
//!
//! For each dictation we compute the per-metric ratios by walking
//! the answer-key entries and matching them to pipeline entries
//! **sequentially** (entry-0 ↔ entry-0, etc.). This is the simplest
//! choice and is documented in `ScoreReport::match_algorithm`: a
//! bipartite-by-similarity match would be marginally better when
//! segment ordering disagrees, but Wave-3 baseline scoring is more
//! valuable than scoring sophistication, and the segmenter does
//! preserve dictation order in practice. If this becomes a
//! systematic blindspot, swap to bipartite later — the report
//! field makes the choice auditable.
//!
//! ## Junk handling
//!
//! Junk dictations (`is_junk_no_entry_expected=true`) are scored in
//! their own `junk_correct` bucket and do NOT feed the other
//! per-metric ratios. The pipeline succeeds when it emits zero
//! entries; emitting any entry is a junk-handling failure.

use std::collections::HashMap;
use std::path::Path;

use serde::Serialize;

use crate::ollama::{GenerateOptions, OllamaDispatcher};
use crate::schema::{AnswerKey, Category, Entry, EntryType, ExpectedEntry};
use crate::scoring::judge::{
    judge_tag_equivalence, TagEquivalence, TagJudgeRequest, TagJudgeVerdict,
};

/// Numerator / denominator + cached percentage. Percentage is `0.0`
/// when denominator is `0` (no opportunity to be wrong).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Ratio {
    pub numerator: usize,
    pub denominator: usize,
    pub percentage: f64,
}

impl Ratio {
    pub fn new(numerator: usize, denominator: usize) -> Self {
        let percentage = if denominator == 0 {
            0.0
        } else {
            100.0 * numerator as f64 / denominator as f64
        };
        Self {
            numerator,
            denominator,
            percentage,
        }
    }
}

/// Per-metric aggregate rolled up from every dictation in the run.
#[derive(Debug, Clone, Serialize)]
pub struct PerMetric {
    pub clean_single_item_correct: Ratio,
    pub segmentation_correct: Ratio,
    pub category_correct: Ratio,
    pub entry_type_correct: Ratio,
    pub invented_dates_count: usize,
    pub tag_variant_collapse_correct: Ratio,
    pub junk_correct: Ratio,
}

/// One dictation's contribution to the score, kept around so the
/// SCORE_SUMMARY.md can print failure examples and the PCRP can
/// reference specific cases.
#[derive(Debug, Clone, Serialize)]
pub struct DictationScore {
    pub dictation_id: String,
    pub expected_entry_count: usize,
    pub actual_entry_count: usize,
    pub segmentation_correct: bool,
    pub is_junk: bool,
    pub junk_correct: Option<bool>,
    /// Per-entry results, length = min(expected, actual). Indices align
    /// with answer-key order.
    pub per_entry: Vec<PerEntryScore>,
    pub invented_dates: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct PerEntryScore {
    pub category_correct: bool,
    pub entry_type_correct: bool,
    /// `Some(true)` matched, `Some(false)` mismatched, `None` when both
    /// sides absent (no opportunity to grade — does not feed the date
    /// ratio).
    pub date_match: Option<bool>,
    pub date_invented: bool,
    /// `Some(true)` judge says equivalent to one of the acceptable
    /// sets, `Some(false)` not equivalent to any, `None` when no
    /// judge was provided.
    pub tag_equivalent: Option<bool>,
    pub expected: ExpectedEntrySnapshot,
    pub actual: ActualEntrySnapshot,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExpectedEntrySnapshot {
    pub category: Category,
    pub entry_type: EntryType,
    pub due_iso: Option<String>,
    pub acceptable_topic_tag_sets: Vec<Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ActualEntrySnapshot {
    pub category: Category,
    pub entry_type: EntryType,
    pub due_iso: Option<String>,
    pub topic_tags: Vec<String>,
}

/// Optional run-pair stability data per spec §8.5.
#[derive(Debug, Clone, Serialize)]
pub struct StabilityReport {
    pub vs_run_id: String,
    pub segmentation_agreement: Ratio,
    pub category_agreement: Ratio,
    pub entry_type_agreement: Ratio,
    pub date_agreement: Ratio,
    pub tag_set_exact_agreement: Ratio,
    pub total_compared_dictations: usize,
    pub total_compared_entries: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScoreReport {
    pub run_id: String,
    pub total_dictations: usize,
    pub graded_dictations: usize,
    pub ungradable_dictations: Vec<String>,
    pub match_algorithm: &'static str,
    pub per_metric: PerMetric,
    pub per_dictation: Vec<DictationScore>,
    pub stability_vs: Option<String>,
    pub stability: Option<StabilityReport>,
}

/// Compute the score for a single run.
///
/// `tag_judge` is optional: when `None`, the tag metric is not
/// populated (its `denominator` is 0 and `percentage` is 0.0). This
/// supports the `--skip-jvp`-style dev path where the JVP has
/// already halted and we still want the structural scoring numbers.
pub fn score_run<D: OllamaDispatcher>(
    run_id: &str,
    structured_dir: &Path,
    answer_keys_dir: &Path,
    tag_judge: Option<TagJudgeContext<'_, D>>,
) -> anyhow::Result<ScoreReport> {
    let pipeline_entries = load_pipeline_entries(structured_dir)?;
    let answer_keys = load_answer_keys(answer_keys_dir)?;

    let mut graded: Vec<(String, AnswerKey, Vec<Entry>)> = Vec::new();
    let mut ungradable: Vec<String> = Vec::new();

    for (id, key) in &answer_keys {
        match pipeline_entries.get(id) {
            Some(actual) => graded.push((id.clone(), key.clone(), actual.clone())),
            None => ungradable.push(id.clone()),
        }
    }
    graded.sort_by(|a, b| a.0.cmp(&b.0));
    ungradable.sort();

    // Counters for the aggregate ratios.
    let mut clean_single_n = 0usize;
    let mut clean_single_d = 0usize;
    let mut seg_n = 0usize;
    let mut seg_d = 0usize;
    let mut cat_n = 0usize;
    let mut cat_d = 0usize;
    let mut type_n = 0usize;
    let mut type_d = 0usize;
    let mut invented_dates = 0usize;
    let mut tag_n = 0usize;
    let mut tag_d = 0usize;
    let mut junk_n = 0usize;
    let mut junk_d = 0usize;

    let mut per_dictation: Vec<DictationScore> = Vec::new();

    for (id, key, actual) in &graded {
        let n_expected = key.entries.len();
        let n_actual = actual.len();

        // ── Junk bucket: scored independently. ────────────────────
        if key.is_junk_no_entry_expected {
            junk_d += 1;
            let ok = n_actual == 0;
            if ok {
                junk_n += 1;
            }
            per_dictation.push(DictationScore {
                dictation_id: id.clone(),
                expected_entry_count: n_expected,
                actual_entry_count: n_actual,
                segmentation_correct: ok,
                is_junk: true,
                junk_correct: Some(ok),
                per_entry: Vec::new(),
                invented_dates: 0,
            });
            continue;
        }

        // ── Clean single-item floor (~100% target). ───────────────
        let is_clean_single = n_expected == 1;
        let seg_ok = n_expected == n_actual;
        if is_clean_single {
            clean_single_d += 1;
            if seg_ok && all_entry_metrics_match(&key.entries, actual, tag_judge.as_ref()) {
                clean_single_n += 1;
            }
        }

        // ── Segmentation metric: only multi-item cases count. ─────
        if n_expected >= 2 {
            seg_d += 1;
            if seg_ok {
                seg_n += 1;
            }
        }

        // ── Per-entry: walk min(expected, actual) sequentially. ───
        let pairs = key.entries.iter().zip(actual.iter());
        let mut per_entry: Vec<PerEntryScore> = Vec::new();
        let mut dictation_invented = 0usize;
        for (expected, actual_entry) in pairs {
            cat_d += 1;
            type_d += 1;
            let category_correct = expected.category == actual_entry.category;
            let entry_type_correct = expected.entry_type == actual_entry.entry_type;
            if category_correct {
                cat_n += 1;
            }
            if entry_type_correct {
                type_n += 1;
            }

            // Date: invented date is the spec §8.4 hard gate — answer
            // key says None but pipeline emitted Some.
            let date_invented = expected.due_iso.is_none() && actual_entry.due_iso.is_some();
            if date_invented {
                invented_dates += 1;
                dictation_invented += 1;
            }
            let date_match = match (&expected.due_iso, &actual_entry.due_iso) {
                (None, None) => None,
                (Some(a), Some(b)) => Some(a == b),
                _ => Some(false),
            };

            // Tag metric — judge-mediated. Only counted when the
            // judge is wired AND the answer key offers at least one
            // acceptable set.
            let tag_equivalent = if let Some(ref ctx) = tag_judge {
                if expected.acceptable_topic_tag_sets.is_empty() {
                    None
                } else {
                    tag_d += 1;
                    let any_match = expected.acceptable_topic_tag_sets.iter().any(|candidate| {
                        let req = TagJudgeRequest {
                            tags_a: actual_entry.topic_tags.clone(),
                            tags_b: candidate.clone(),
                        };
                        match judge_tag_equivalence(ctx.dispatcher, ctx.model, &req, ctx.options) {
                            Ok(v) => {
                                if let Some(sink) = ctx.verdict_sink {
                                    sink.borrow_mut().push(RecordedJudgeCall {
                                        dictation_id: id.clone(),
                                        entry_index: per_entry.len(),
                                        tags_a: req.tags_a.clone(),
                                        tags_b: req.tags_b.clone(),
                                        verdict: v.clone(),
                                    });
                                }
                                v.verdict == TagEquivalence::Equivalent
                            }
                            Err(_) => false,
                        }
                    });
                    if any_match {
                        tag_n += 1;
                    }
                    Some(any_match)
                }
            } else {
                None
            };

            per_entry.push(PerEntryScore {
                category_correct,
                entry_type_correct,
                date_match,
                date_invented,
                tag_equivalent,
                expected: snapshot_expected(expected),
                actual: snapshot_actual(actual_entry),
            });
        }

        per_dictation.push(DictationScore {
            dictation_id: id.clone(),
            expected_entry_count: n_expected,
            actual_entry_count: n_actual,
            segmentation_correct: seg_ok,
            is_junk: false,
            junk_correct: None,
            per_entry,
            invented_dates: dictation_invented,
        });
    }

    let per_metric = PerMetric {
        clean_single_item_correct: Ratio::new(clean_single_n, clean_single_d),
        segmentation_correct: Ratio::new(seg_n, seg_d),
        category_correct: Ratio::new(cat_n, cat_d),
        entry_type_correct: Ratio::new(type_n, type_d),
        invented_dates_count: invented_dates,
        tag_variant_collapse_correct: Ratio::new(tag_n, tag_d),
        junk_correct: Ratio::new(junk_n, junk_d),
    };

    Ok(ScoreReport {
        run_id: run_id.to_string(),
        total_dictations: answer_keys.len(),
        graded_dictations: graded.len(),
        ungradable_dictations: ungradable,
        match_algorithm: "sequential (answer-key index ↔ pipeline index)",
        per_metric,
        per_dictation,
        stability_vs: None,
        stability: None,
    })
}

/// Compute spec §8.5 stability findings: same corpus, two runs,
/// agree on each metric? Match is by `dictation_id`. Per-entry
/// comparisons are sequential, same convention as the within-run
/// scorer.
pub fn compute_stability(
    a_structured: &Path,
    b_structured: &Path,
    vs_run_id: &str,
) -> anyhow::Result<StabilityReport> {
    let a = load_pipeline_entries(a_structured)?;
    let b = load_pipeline_entries(b_structured)?;

    let mut shared_ids: Vec<&String> = a.keys().filter(|k| b.contains_key(*k)).collect();
    shared_ids.sort();

    let mut seg_n = 0usize;
    let mut seg_d = 0usize;
    let mut cat_n = 0usize;
    let mut cat_d = 0usize;
    let mut type_n = 0usize;
    let mut type_d = 0usize;
    let mut date_n = 0usize;
    let mut date_d = 0usize;
    let mut tag_n = 0usize;
    let mut tag_d = 0usize;
    let mut total_entries = 0usize;

    for id in &shared_ids {
        let ae = a.get(*id).unwrap();
        let be = b.get(*id).unwrap();
        seg_d += 1;
        if ae.len() == be.len() {
            seg_n += 1;
        }
        for (ax, bx) in ae.iter().zip(be.iter()) {
            cat_d += 1;
            if ax.category == bx.category {
                cat_n += 1;
            }
            type_d += 1;
            if ax.entry_type == bx.entry_type {
                type_n += 1;
            }
            date_d += 1;
            if ax.due_iso == bx.due_iso {
                date_n += 1;
            }
            tag_d += 1;
            if tag_set_equal(&ax.topic_tags, &bx.topic_tags) {
                tag_n += 1;
            }
            total_entries += 1;
        }
    }

    Ok(StabilityReport {
        vs_run_id: vs_run_id.to_string(),
        segmentation_agreement: Ratio::new(seg_n, seg_d),
        category_agreement: Ratio::new(cat_n, cat_d),
        entry_type_agreement: Ratio::new(type_n, type_d),
        date_agreement: Ratio::new(date_n, date_d),
        tag_set_exact_agreement: Ratio::new(tag_n, tag_d),
        total_compared_dictations: shared_ids.len(),
        total_compared_entries: total_entries,
    })
}

// ────────────────────────────────────────────────────────────────────
// Helpers
// ────────────────────────────────────────────────────────────────────

/// Carries the LLM judge dispatcher + model + options into the scorer.
/// The optional `verdict_sink` records every judge call so the JVP
/// gates can audit the run after-the-fact without re-issuing calls.
pub struct TagJudgeContext<'a, D: OllamaDispatcher> {
    pub dispatcher: &'a D,
    pub model: &'a str,
    pub options: &'a GenerateOptions,
    pub verdict_sink: Option<&'a std::cell::RefCell<Vec<RecordedJudgeCall>>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecordedJudgeCall {
    pub dictation_id: String,
    pub entry_index: usize,
    pub tags_a: Vec<String>,
    pub tags_b: Vec<String>,
    pub verdict: TagJudgeVerdict,
}

fn snapshot_expected(e: &ExpectedEntry) -> ExpectedEntrySnapshot {
    ExpectedEntrySnapshot {
        category: e.category,
        entry_type: e.entry_type,
        due_iso: e.due_iso.clone(),
        acceptable_topic_tag_sets: e.acceptable_topic_tag_sets.clone(),
    }
}

fn snapshot_actual(e: &Entry) -> ActualEntrySnapshot {
    ActualEntrySnapshot {
        category: e.category,
        entry_type: e.entry_type,
        due_iso: e.due_iso.clone(),
        topic_tags: e.topic_tags.clone(),
    }
}

/// Used by the clean-single-item floor — a clean single is "correct"
/// only when segmentation, category, type, and date all match.
fn all_entry_metrics_match<D: OllamaDispatcher>(
    expected: &[ExpectedEntry],
    actual: &[Entry],
    _judge: Option<&TagJudgeContext<'_, D>>,
) -> bool {
    if expected.len() != actual.len() {
        return false;
    }
    for (e, a) in expected.iter().zip(actual.iter()) {
        if e.category != a.category || e.entry_type != a.entry_type || e.due_iso != a.due_iso {
            return false;
        }
    }
    true
}

fn tag_set_equal(a: &[String], b: &[String]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    for t in a {
        if !b.contains(t) {
            return false;
        }
    }
    true
}

fn load_pipeline_entries(dir: &Path) -> anyhow::Result<HashMap<String, Vec<Entry>>> {
    let mut out: HashMap<String, Vec<Entry>> = HashMap::new();
    if !dir.exists() {
        anyhow::bail!("structured dir does not exist: {}", dir.display());
    }
    for entry in std::fs::read_dir(dir)
        .map_err(|e| anyhow::anyhow!("read structured dir {}: {e}", dir.display()))?
    {
        let entry = entry?;
        let p = entry.path();
        if p.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let id = p
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| anyhow::anyhow!("bad filename: {}", p.display()))?
            .to_string();
        let text = std::fs::read_to_string(&p)
            .map_err(|e| anyhow::anyhow!("read {}: {e}", p.display()))?;
        let entries: Vec<Entry> = serde_json::from_str(&text)
            .map_err(|e| anyhow::anyhow!("parse {}: {e}", p.display()))?;
        out.insert(id, entries);
    }
    Ok(out)
}

fn load_answer_keys(dir: &Path) -> anyhow::Result<HashMap<String, AnswerKey>> {
    let mut out: HashMap<String, AnswerKey> = HashMap::new();
    if !dir.exists() {
        anyhow::bail!("answer-keys dir does not exist: {}", dir.display());
    }
    for entry in std::fs::read_dir(dir)
        .map_err(|e| anyhow::anyhow!("read answer-keys dir {}: {e}", dir.display()))?
    {
        let entry = entry?;
        let p = entry.path();
        if p.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let text = std::fs::read_to_string(&p)
            .map_err(|e| anyhow::anyhow!("read {}: {e}", p.display()))?;
        let key: AnswerKey = serde_json::from_str(&text)
            .map_err(|e| anyhow::anyhow!("parse {}: {e}", p.display()))?;
        out.insert(key.dictation_id.clone(), key);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ollama::testing::MockOllama;
    use crate::schema::Status;

    fn tmp() -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!(
            "kg-metrics-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn write_structured(dir: &Path, id: &str, entries: &[Entry]) {
        std::fs::create_dir_all(dir).unwrap();
        let p = dir.join(format!("{id}.json"));
        std::fs::write(&p, serde_json::to_string_pretty(entries).unwrap()).unwrap();
    }

    fn write_key(dir: &Path, id: &str, key: &AnswerKey) {
        std::fs::create_dir_all(dir).unwrap();
        let p = dir.join(format!("{id}.json"));
        std::fs::write(&p, serde_json::to_string_pretty(key).unwrap()).unwrap();
    }

    fn entry(cat: Category, t: EntryType, due: Option<&str>, tags: &[&str]) -> Entry {
        Entry {
            title: "T".into(),
            category: cat,
            entry_type: t,
            status: if matches!(t, EntryType::Task) {
                Some(Status::Todo)
            } else {
                None
            },
            topic_tags: tags.iter().map(|s| s.to_string()).collect(),
            due_iso: due.map(str::to_string),
            captured_iso: "2026-06-14T08:00:00Z".into(),
            body: "B".into(),
        }
    }

    fn expected(
        cat: Category,
        t: EntryType,
        due: Option<&str>,
        tag_sets: &[&[&str]],
    ) -> ExpectedEntry {
        ExpectedEntry {
            category: cat,
            entry_type: t,
            due_iso: due.map(str::to_string),
            acceptable_topic_tag_sets: tag_sets
                .iter()
                .map(|s| s.iter().map(|t| t.to_string()).collect())
                .collect(),
        }
    }

    #[test]
    fn clean_single_all_correct_no_judge() {
        let root = tmp();
        let s = root.join("structured");
        let k = root.join("keys");
        write_structured(
            &s,
            "p1",
            &[entry(
                Category::Personal,
                EntryType::Task,
                Some("2026-06-15"),
                &["kid"],
            )],
        );
        write_key(
            &k,
            "p1",
            &AnswerKey {
                dictation_id: "p1".into(),
                expected_entry_count: 1,
                entries: vec![expected(
                    Category::Personal,
                    EntryType::Task,
                    Some("2026-06-15"),
                    &[&["kid"]],
                )],
                is_junk_no_entry_expected: false,
            },
        );

        let report =
            score_run::<MockOllama>("test", &s, &k, None::<TagJudgeContext<'_, MockOllama>>)
                .unwrap();
        assert_eq!(report.per_metric.clean_single_item_correct.numerator, 1);
        assert_eq!(report.per_metric.clean_single_item_correct.denominator, 1);
        assert_eq!(report.per_metric.invented_dates_count, 0);
        // No judge -> tag metric inert.
        assert_eq!(
            report.per_metric.tag_variant_collapse_correct.denominator,
            0
        );
    }

    #[test]
    fn invented_date_is_counted() {
        let root = tmp();
        let s = root.join("structured");
        let k = root.join("keys");
        write_structured(
            &s,
            "p1",
            &[entry(
                Category::Personal,
                EntryType::Task,
                Some("2026-06-15"),
                &["x"],
            )],
        );
        write_key(
            &k,
            "p1",
            &AnswerKey {
                dictation_id: "p1".into(),
                expected_entry_count: 1,
                entries: vec![expected(
                    Category::Personal,
                    EntryType::Task,
                    None,
                    &[&["x"]],
                )],
                is_junk_no_entry_expected: false,
            },
        );
        let report =
            score_run::<MockOllama>("test", &s, &k, None::<TagJudgeContext<'_, MockOllama>>)
                .unwrap();
        assert_eq!(report.per_metric.invented_dates_count, 1);
        assert_eq!(report.per_dictation[0].invented_dates, 1);
    }

    #[test]
    fn junk_correct_when_empty() {
        let root = tmp();
        let s = root.join("structured");
        let k = root.join("keys");
        write_structured(&s, "j1", &[]);
        write_key(
            &k,
            "j1",
            &AnswerKey {
                dictation_id: "j1".into(),
                expected_entry_count: 0,
                entries: vec![],
                is_junk_no_entry_expected: true,
            },
        );
        let report =
            score_run::<MockOllama>("test", &s, &k, None::<TagJudgeContext<'_, MockOllama>>)
                .unwrap();
        assert_eq!(report.per_metric.junk_correct.numerator, 1);
        assert_eq!(report.per_metric.junk_correct.denominator, 1);
        // Junk does NOT contribute to clean_single or segmentation.
        assert_eq!(report.per_metric.clean_single_item_correct.denominator, 0);
        assert_eq!(report.per_metric.segmentation_correct.denominator, 0);
    }

    #[test]
    fn junk_failure_when_pipeline_emits_entries() {
        let root = tmp();
        let s = root.join("structured");
        let k = root.join("keys");
        write_structured(
            &s,
            "j1",
            &[entry(Category::Personal, EntryType::Task, None, &["x"])],
        );
        write_key(
            &k,
            "j1",
            &AnswerKey {
                dictation_id: "j1".into(),
                expected_entry_count: 0,
                entries: vec![],
                is_junk_no_entry_expected: true,
            },
        );
        let report =
            score_run::<MockOllama>("test", &s, &k, None::<TagJudgeContext<'_, MockOllama>>)
                .unwrap();
        assert_eq!(report.per_metric.junk_correct.numerator, 0);
        assert_eq!(report.per_metric.junk_correct.denominator, 1);
    }

    #[test]
    fn segmentation_metric_only_counts_multi_item_keys() {
        let root = tmp();
        let s = root.join("structured");
        let k = root.join("keys");
        // Multi-item key — counts.
        write_structured(
            &s,
            "m1",
            &[
                entry(Category::Personal, EntryType::Task, None, &["a"]),
                entry(Category::Personal, EntryType::Task, None, &["b"]),
            ],
        );
        write_key(
            &k,
            "m1",
            &AnswerKey {
                dictation_id: "m1".into(),
                expected_entry_count: 2,
                entries: vec![
                    expected(Category::Personal, EntryType::Task, None, &[&["a"]]),
                    expected(Category::Personal, EntryType::Task, None, &[&["b"]]),
                ],
                is_junk_no_entry_expected: false,
            },
        );
        // Single-item key — does NOT count toward segmentation_correct.
        write_structured(
            &s,
            "s1",
            &[entry(Category::Personal, EntryType::Task, None, &["c"])],
        );
        write_key(
            &k,
            "s1",
            &AnswerKey {
                dictation_id: "s1".into(),
                expected_entry_count: 1,
                entries: vec![expected(
                    Category::Personal,
                    EntryType::Task,
                    None,
                    &[&["c"]],
                )],
                is_junk_no_entry_expected: false,
            },
        );

        let report =
            score_run::<MockOllama>("test", &s, &k, None::<TagJudgeContext<'_, MockOllama>>)
                .unwrap();
        assert_eq!(report.per_metric.segmentation_correct.denominator, 1);
        assert_eq!(report.per_metric.segmentation_correct.numerator, 1);
    }

    #[test]
    fn tag_judge_called_when_provided() {
        let root = tmp();
        let s = root.join("structured");
        let k = root.join("keys");
        write_structured(
            &s,
            "t1",
            &[entry(
                Category::Personal,
                EntryType::Task,
                None,
                &["car-repair", "auto"],
            )],
        );
        write_key(
            &k,
            "t1",
            &AnswerKey {
                dictation_id: "t1".into(),
                expected_entry_count: 1,
                entries: vec![expected(
                    Category::Personal,
                    EntryType::Task,
                    None,
                    &[&["car-repair", "auto-maintenance"]],
                )],
                is_junk_no_entry_expected: false,
            },
        );

        let mock = MockOllama::new().default_response(
            "REASONING: both name the same car repair note for filing purposes\nVERDICT: equivalent",
        );
        let opts = GenerateOptions::default();
        let sink = std::cell::RefCell::new(Vec::new());
        let ctx = TagJudgeContext {
            dispatcher: &mock,
            model: "judge",
            options: &opts,
            verdict_sink: Some(&sink),
        };
        let report = score_run("test", &s, &k, Some(ctx)).unwrap();

        assert_eq!(report.per_metric.tag_variant_collapse_correct.numerator, 1);
        assert_eq!(
            report.per_metric.tag_variant_collapse_correct.denominator,
            1
        );
        // Sink recorded the call.
        assert_eq!(sink.borrow().len(), 1);
    }

    #[test]
    fn stability_perfect_when_runs_identical() {
        let root = tmp();
        let a = root.join("a");
        let b = root.join("b");
        let e = vec![entry(
            Category::Personal,
            EntryType::Task,
            Some("2026-06-15"),
            &["x"],
        )];
        write_structured(&a, "d1", &e);
        write_structured(&b, "d1", &e);
        let s = compute_stability(&a, &b, "b").unwrap();
        assert_eq!(s.segmentation_agreement.percentage, 100.0);
        assert_eq!(s.category_agreement.percentage, 100.0);
        assert_eq!(s.tag_set_exact_agreement.percentage, 100.0);
        assert_eq!(s.total_compared_dictations, 1);
    }

    #[test]
    fn stability_flags_disagreement() {
        let root = tmp();
        let a = root.join("a");
        let b = root.join("b");
        write_structured(
            &a,
            "d1",
            &[entry(
                Category::Personal,
                EntryType::Task,
                Some("2026-06-15"),
                &["x"],
            )],
        );
        write_structured(
            &b,
            "d1",
            // Different category + date + tag.
            &[entry(
                Category::Professional,
                EntryType::Task,
                Some("2026-06-22"),
                &["y"],
            )],
        );
        let s = compute_stability(&a, &b, "b").unwrap();
        assert_eq!(s.category_agreement.numerator, 0);
        assert_eq!(s.date_agreement.numerator, 0);
        assert_eq!(s.tag_set_exact_agreement.numerator, 0);
        assert_eq!(s.entry_type_agreement.numerator, 1); // type still matches
    }
}
