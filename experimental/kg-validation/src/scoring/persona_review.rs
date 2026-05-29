//! Persona Cross-Reference Pass (PCRP) — ADR 0048 §G6.
//!
//! Structured qualitative audit that runs AFTER scoring. Composes a
//! single bounded LLM call per persona, in which the model is given:
//!
//! 1. The persona's notes from `CORPUS_NOTES.md` (FIRST, before any
//!    pipeline output) — the persona-first reading order discipline.
//! 2. The sampled dictations for that persona, with raw text →
//!    answer key → pipeline output → judge verdicts.
//! 3. A structured failure-mode question set.
//! 4. A bias-toward-finding-problems instruction.
//! 5. A required output schema demanding evidence (case ID + quoted
//!    line) for every claim.
//!
//! Output is `runs/<run-id>/PERSONA_REVIEW.md`. The reviewer LLM's
//! free-form output for each persona is concatenated under a
//! per-persona heading. The load-bearing
//! `trust_eroding_failures_count` is parsed from the LLM output so
//! the Wave 6 go/no-go rule (§G6) can be applied mechanically.
//!
//! ## Why this module is the LIGHT in the JVP/PCRP split
//!
//! JVP is the heavy-mechanical layer (5 gates, deterministic
//! counters). PCRP is the qualitative layer — its strength comes
//! from the *prompt discipline* (G6), not from clever Rust code.
//! Most of the value of this module is in [`build_pcrp_prompt`] and
//! the sample-selection rules; the LLM call itself is a thin shim.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::ollama::{GenerateOptions, OllamaDispatcher};
use crate::schema::AnswerKey;
use crate::scoring::metrics::{DictationScore, ScoreReport};

// ────────────────────────────────────────────────────────────────────
// Public types
// ────────────────────────────────────────────────────────────────────

/// One sampled dictation, with everything the reviewer needs to
/// audit it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PcrpSample {
    pub dictation_id: String,
    pub persona_id: String,
    pub raw_dictation: String,
    pub answer_key_json: String,
    pub pipeline_output_json: String,
    pub per_metric_pass_flags: PassFlags,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PassFlags {
    pub segmentation_correct: bool,
    pub category_all_correct: bool,
    pub entry_type_all_correct: bool,
    pub no_invented_dates: bool,
    pub junk_correct: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PcrpReport {
    pub run_id: String,
    pub reviewer_model: String,
    pub per_persona_markdown: Vec<PersonaSection>,
    /// The load-bearing aggregate. Extracted by counting the bullet
    /// items in each persona's "Top trust-eroding failures" section.
    pub trust_eroding_failures_count: usize,
    pub trust_building_wins_count: usize,
    pub samples: Vec<PcrpSample>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PersonaSection {
    pub persona_id: String,
    pub heading: String,
    pub markdown_body: String,
    pub trust_eroding_failures: usize,
    pub trust_building_wins: usize,
}

// ────────────────────────────────────────────────────────────────────
// Configuration
// ────────────────────────────────────────────────────────────────────

pub struct PcrpConfig {
    pub run_id: String,
    pub reviewer_model: String,
    pub options: GenerateOptions,
    /// The persona-notes block from `CORPUS_NOTES.md`. Keyed by
    /// `persona_id` (e.g. `"persona-01"`).
    pub persona_notes: HashMap<String, String>,
    /// Selected samples — output of [`select_samples`].
    pub samples: Vec<PcrpSample>,
}

// ────────────────────────────────────────────────────────────────────
// Sample selection — deterministic, weighted
// ────────────────────────────────────────────────────────────────────

/// Sample selection per ADR 0048 §G6 + dispatch lock #5:
///
/// - ≥ 1 dictation per persona (covers all 6 personas)
/// - additional multi-item rambler / ambiguous-category / no-date cases
/// - **always include** `persona-05-case-03` (5-item peak-hard)
/// - **always include** `persona-01-case-05` + `persona-05-case-05` (junk —
///   the false-positive trap: a clean-empty pipeline output must not
///   produce a trust-eroding finding)
/// - **always include** ≥ 3 quantitatively-PASSED cases (G6
///   confirmation-bias guardrail)
///
/// Total target: 12–15 samples. Deterministic given the inputs (no
/// randomness; we order by dictation_id throughout).
pub fn select_samples(
    score: &ScoreReport,
    answer_keys: &HashMap<String, AnswerKey>,
    raw_dictations: &HashMap<String, String>,
    pipeline_output: &HashMap<String, String>,
) -> Vec<PcrpSample> {
    let mut selected: Vec<String> = Vec::new();
    let mut chosen: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Required cases (peak-hard + both junk).
    for required in [
        "persona-05-case-03",
        "persona-01-case-05",
        "persona-05-case-05",
    ] {
        if answer_keys.contains_key(required) && chosen.insert(required.to_string()) {
            selected.push(required.to_string());
        }
    }

    // 1 per persona. Order by dictation_id for determinism.
    let mut per_persona_pick: HashMap<String, String> = HashMap::new();
    let mut ids_sorted: Vec<&String> = answer_keys.keys().collect();
    ids_sorted.sort();
    for id in &ids_sorted {
        if let Some(p) = persona_id_of(id) {
            per_persona_pick.entry(p).or_insert_with(|| (*id).clone());
        }
    }
    let mut per_persona_keys: Vec<&String> = per_persona_pick.values().collect();
    per_persona_keys.sort();
    for id in per_persona_keys {
        if chosen.insert(id.clone()) {
            selected.push(id.clone());
        }
    }

    // Hard-case bias: multi-item (count ≥ 2) and no-date cases (all
    // expected `due_iso` are None AND not junk). Add until we hit ~13.
    let target = 13usize;
    for id in &ids_sorted {
        if selected.len() >= target {
            break;
        }
        let key = &answer_keys[*id];
        if key.is_junk_no_entry_expected {
            continue;
        }
        let is_multi = key.entries.len() >= 2;
        let is_no_date = !key.entries.is_empty() && key.entries.iter().all(|e| e.due_iso.is_none());
        if (is_multi || is_no_date) && chosen.insert((*id).clone()) {
            selected.push((*id).clone());
        }
    }

    // Confirmation-bias guardrail: ensure ≥ 3 metric-PASSED cases in
    // the selection. A "passed" case is one whose DictationScore has
    // segmentation_correct AND zero invented dates AND every per-entry
    // category+type correct. We don't lean on the tag metric here —
    // tag judge isn't always available at PCRP time.
    let passed_ids: Vec<String> = score
        .per_dictation
        .iter()
        .filter(|d| is_metric_passed(d))
        .map(|d| d.dictation_id.clone())
        .collect();
    let already_passed_in_selection = selected.iter().filter(|id| passed_ids.contains(id)).count();
    if already_passed_in_selection < 3 {
        let mut need = 3 - already_passed_in_selection;
        for id in passed_ids {
            if need == 0 {
                break;
            }
            if chosen.insert(id.clone()) {
                selected.push(id);
                need -= 1;
            }
        }
    }

    selected.sort();
    selected
        .into_iter()
        .filter_map(|id| build_sample(&id, score, answer_keys, raw_dictations, pipeline_output))
        .collect()
}

fn is_metric_passed(d: &DictationScore) -> bool {
    if d.is_junk {
        return d.junk_correct.unwrap_or(false);
    }
    if !d.segmentation_correct {
        return false;
    }
    if d.invented_dates > 0 {
        return false;
    }
    d.per_entry
        .iter()
        .all(|p| p.category_correct && p.entry_type_correct)
}

fn build_sample(
    id: &str,
    score: &ScoreReport,
    answer_keys: &HashMap<String, AnswerKey>,
    raw_dictations: &HashMap<String, String>,
    pipeline_output: &HashMap<String, String>,
) -> Option<PcrpSample> {
    let key = answer_keys.get(id)?;
    let raw = raw_dictations.get(id)?.clone();
    let out = pipeline_output.get(id)?.clone();
    let answer_json = serde_json::to_string_pretty(key).ok()?;
    let dictation_score = score.per_dictation.iter().find(|d| d.dictation_id == id);
    let flags = PassFlags {
        segmentation_correct: dictation_score
            .map(|d| d.segmentation_correct)
            .unwrap_or(false),
        category_all_correct: dictation_score
            .map(|d| d.per_entry.iter().all(|p| p.category_correct))
            .unwrap_or(false),
        entry_type_all_correct: dictation_score
            .map(|d| d.per_entry.iter().all(|p| p.entry_type_correct))
            .unwrap_or(false),
        no_invented_dates: dictation_score
            .map(|d| d.invented_dates == 0)
            .unwrap_or(true),
        junk_correct: dictation_score.and_then(|d| d.junk_correct),
    };
    Some(PcrpSample {
        dictation_id: id.to_string(),
        persona_id: persona_id_of(id).unwrap_or_else(|| "unknown".to_string()),
        raw_dictation: raw,
        answer_key_json: answer_json,
        pipeline_output_json: out,
        per_metric_pass_flags: flags,
    })
}

fn persona_id_of(dictation_id: &str) -> Option<String> {
    // dictation ids look like "persona-NN-case-MM"; persona id is the
    // first two hyphen-segments.
    let parts: Vec<&str> = dictation_id.splitn(4, '-').collect();
    if parts.len() >= 2 && parts[0] == "persona" {
        Some(format!("{}-{}", parts[0], parts[1]))
    } else {
        None
    }
}

// ────────────────────────────────────────────────────────────────────
// Prompt assembly + LLM call
// ────────────────────────────────────────────────────────────────────

/// Build the per-persona PCRP prompt. The discipline rules from
/// ADR 0048 §G6 are baked in literally: persona notes FIRST, then
/// the sampled dictations + answer key + pipeline output + flags,
/// then the structured failure-mode prompts + bias-toward-problems
/// instruction + required output schema.
pub fn build_pcrp_prompt(persona_id: &str, persona_notes: &str, samples: &[PcrpSample]) -> String {
    let mut s = String::new();
    s.push_str("You are auditing a small-local-LLM pipeline that turns rambling personal\n");
    s.push_str("voice memos into structured knowledge-graph entries. Your job is QUALITATIVE\n");
    s.push_str("REVIEW. Quantitative scoring already ran; you exist to catch the failures\n");
    s.push_str("that the metrics blindspot.\n\n");

    // Step 1 — persona-first reading order.
    s.push_str(&format!(
        "STEP 1 — Read the persona notes BEFORE looking at any pipeline output.\n\n\
        PERSONA: {persona_id}\n\n\
        PERSONA NOTES:\n{persona_notes}\n\n"
    ));

    // Step 2 — the samples.
    s.push_str("STEP 2 — Now and ONLY now look at the pipeline outputs.\n\n");
    for sample in samples {
        s.push_str(&format!("──── CASE {} ────\n", sample.dictation_id));
        s.push_str("RAW DICTATION (what the user spoke):\n");
        s.push_str(&sample.raw_dictation);
        s.push_str("\n\nANSWER KEY (hand-authored ground truth):\n");
        s.push_str(&sample.answer_key_json);
        s.push_str("\n\nPIPELINE OUTPUT (what the LLM pipeline produced):\n");
        s.push_str(&sample.pipeline_output_json);
        s.push_str("\n\nQUANTITATIVE FLAGS:\n");
        s.push_str(&format!(
            "  segmentation_correct: {}\n  category_all_correct: {}\n  entry_type_all_correct: {}\n  no_invented_dates: {}\n",
            sample.per_metric_pass_flags.segmentation_correct,
            sample.per_metric_pass_flags.category_all_correct,
            sample.per_metric_pass_flags.entry_type_all_correct,
            sample.per_metric_pass_flags.no_invented_dates
        ));
        if let Some(jc) = sample.per_metric_pass_flags.junk_correct {
            s.push_str(&format!("  junk_correct: {jc}\n"));
        }
        s.push('\n');
    }

    // Step 3 — structured failure modes.
    s.push_str("STEP 3 — Look for these SPECIFIC failure shapes (not general impressions):\n\n");
    s.push_str(
        "  - Hallucinated dates: pipeline emitted a due_iso when the user didn't mention timing.\n",
    );
    s.push_str(
        "  - Weird tags: open-vocabulary tags that wouldn't help search/filing for THIS persona.\n",
    );
    s.push_str(
        "  - Miscategorized obvious cases: clearly-professional content tagged personal, etc.\n",
    );
    s.push_str(
        "  - Titles misrepresenting intent: title doesn't match what the user actually said.\n",
    );
    s.push_str("  - Over-formalization: casual speech rendered as stilted boardroom prose.\n");
    s.push_str("  - Wrong type: idea filed as task, task filed as note, etc.\n");
    s.push_str("  - Over-splitting: one item segmented into multiple entries.\n");
    s.push_str("  - Under-splitting: distinct items glued together into one entry.\n\n");

    // Step 4 — bias toward problems + confirmation-bias guardrail.
    s.push_str("STEP 4 — IMPORTANT DISCIPLINE:\n\n");
    s.push_str("  - Default assumption: at this model scale, problems exist. If you find none, look harder, especially at multi-item and ambiguous-category cases.\n");
    s.push_str("  - Every claim MUST cite a specific case ID + a quoted line from the pipeline output. No abstract observations.\n");
    s.push_str("  - At least 3 of the cases above quantitatively PASSED on the metrics. If you find those 3 cases are also qualitatively bad, that is a METRIC BLINDSPOT finding worth flagging. If you find them all unanimously great, look again — that is the rubber-stamp pattern.\n\n");

    // Step 5 — required output schema.
    s.push_str(
        "STEP 5 — Output EXACTLY this Markdown structure (no extra prose before or after):\n\n",
    );
    s.push_str(&format!("## Persona {persona_id}\n\n"));
    s.push_str("### Summary\n\n<2-4 sentences: does this pipeline produce output this persona would trust as their filing system?>\n\n");
    s.push_str("### Top trust-eroding failures\n\n");
    s.push_str(
        "- <case-id>: <one-sentence claim> — evidence: \"<quoted line from pipeline output>\"\n",
    );
    s.push_str("- <case-id>: ...\n");
    s.push_str("- <case-id>: ...\n\n");
    s.push_str("### Top trust-building wins\n\n");
    s.push_str(
        "- <case-id>: <one-sentence claim> — evidence: \"<quoted line from pipeline output>\"\n",
    );
    s.push_str("- <case-id>: ...\n");
    s.push_str("- <case-id>: ...\n\n");
    s.push_str("### Metric blindspots observed\n\n<bullet list of cases where qualitative reading disagrees with quantitative flags; empty if none>\n");

    s
}

/// Run PCRP. For each persona present in the samples, build the
/// prompt, dispatch one LLM call, parse the bullet counts, and
/// concatenate into a [`PcrpReport`].
pub fn run_pcrp<D: OllamaDispatcher>(
    dispatcher: &D,
    config: PcrpConfig,
) -> anyhow::Result<PcrpReport> {
    // Group samples by persona.
    let mut by_persona: HashMap<String, Vec<PcrpSample>> = HashMap::new();
    for s in &config.samples {
        by_persona
            .entry(s.persona_id.clone())
            .or_default()
            .push(s.clone());
    }
    let mut persona_ids: Vec<String> = by_persona.keys().cloned().collect();
    persona_ids.sort();

    let mut sections: Vec<PersonaSection> = Vec::new();
    let mut total_trust_eroding = 0usize;
    let mut total_trust_building = 0usize;

    for pid in &persona_ids {
        let samples_for_persona = by_persona.get(pid).cloned().unwrap_or_default();
        let notes = config
            .persona_notes
            .get(pid)
            .cloned()
            .unwrap_or_else(|| format!("(no persona notes found for {pid})"));
        let prompt = build_pcrp_prompt(pid, &notes, &samples_for_persona);
        let body = dispatcher
            .generate(&config.reviewer_model, &prompt, None, &config.options)
            .map_err(|e| anyhow::anyhow!("PCRP call for {pid} failed: {e}"))?;

        let (te, tb) = count_findings(&body);
        total_trust_eroding += te;
        total_trust_building += tb;
        sections.push(PersonaSection {
            persona_id: pid.clone(),
            heading: format!("Persona {pid}"),
            markdown_body: body,
            trust_eroding_failures: te,
            trust_building_wins: tb,
        });
    }

    Ok(PcrpReport {
        run_id: config.run_id,
        reviewer_model: config.reviewer_model,
        per_persona_markdown: sections,
        trust_eroding_failures_count: total_trust_eroding,
        trust_building_wins_count: total_trust_building,
        samples: config.samples,
    })
}

/// Tiny bullet counter over the persona section markdown. Looks for
/// the headings "Top trust-eroding failures" and "Top trust-building
/// wins" then counts bullets in the block after each heading until
/// the next `###` heading (or end of string).
pub fn count_findings(body: &str) -> (usize, usize) {
    let te = count_bullets_in_section(body, "trust-eroding failures");
    let tb = count_bullets_in_section(body, "trust-building wins");
    (te, tb)
}

fn count_bullets_in_section(body: &str, section_marker: &str) -> usize {
    let lower = body.to_ascii_lowercase();
    let needle = section_marker.to_ascii_lowercase();
    let Some(hpos) = lower.find(&needle) else {
        return 0;
    };
    // Skip past the heading line.
    let after_heading = match body[hpos..].find('\n') {
        Some(off) => &body[hpos + off + 1..],
        None => return 0,
    };
    let end_of_section = after_heading.find("\n###").unwrap_or(after_heading.len());
    let block = &after_heading[..end_of_section];
    block
        .lines()
        .filter(|l| {
            let t = l.trim();
            // A real bullet: leading `-` followed by at least one
            // non-`<` character (so the literal placeholder
            // `- <case-id>: ...` in the schema example doesn't get
            // counted as a finding).
            t.starts_with('-')
                && t.len() > 2
                && !t.trim_start_matches('-').trim_start().starts_with('<')
        })
        .count()
}

/// Load `CORPUS_NOTES.md` and slice it into a per-persona map. The
/// file uses `### Persona NN — <descriptor>` headings; we collect
/// everything between one such heading and the next.
pub fn load_persona_notes(corpus_notes_path: &Path) -> anyhow::Result<HashMap<String, String>> {
    let text = std::fs::read_to_string(corpus_notes_path)
        .map_err(|e| anyhow::anyhow!("read CORPUS_NOTES {}: {e}", corpus_notes_path.display()))?;
    let mut out: HashMap<String, String> = HashMap::new();
    let mut current_id: Option<String> = None;
    let mut buf = String::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("### Persona ") {
            // Flush the in-progress persona block.
            if let Some(id) = current_id.take() {
                out.insert(id, std::mem::take(&mut buf));
            }
            // New heading — derive persona id from leading number.
            let two_digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            if !two_digits.is_empty() {
                current_id = Some(format!("persona-{two_digits}"));
            }
            continue;
        }
        if current_id.is_some() {
            buf.push_str(line);
            buf.push('\n');
        }
    }
    if let Some(id) = current_id.take() {
        out.insert(id, buf);
    }
    Ok(out)
}

/// Render a fully-assembled [`PcrpReport`] as the on-disk
/// `PERSONA_REVIEW.md` file.
pub fn render_markdown(report: &PcrpReport) -> String {
    let mut s = String::new();
    s.push_str("# PCRP — Persona Cross-Reference Pass\n\n");
    s.push_str(&format!("- run_id: `{}`\n", report.run_id));
    s.push_str(&format!("- reviewer_model: `{}`\n", report.reviewer_model));
    s.push_str(&format!(
        "- trust_eroding_failures_count: **{}**\n",
        report.trust_eroding_failures_count
    ));
    s.push_str(&format!(
        "- trust_building_wins_count: **{}**\n\n",
        report.trust_building_wins_count
    ));
    s.push_str("---\n\n");
    for section in &report.per_persona_markdown {
        s.push_str(&section.markdown_body);
        s.push_str("\n\n---\n\n");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ollama::testing::MockOllama;
    use crate::schema::{Category, EntryType, ExpectedEntry};

    fn ak(id: &str, n: usize, junk: bool, entries: Vec<ExpectedEntry>) -> AnswerKey {
        AnswerKey {
            dictation_id: id.into(),
            expected_entry_count: n,
            entries,
            is_junk_no_entry_expected: junk,
        }
    }

    fn ee(cat: Category, t: EntryType, due: Option<&str>) -> ExpectedEntry {
        ExpectedEntry {
            category: cat,
            entry_type: t,
            due_iso: due.map(str::to_string),
            acceptable_topic_tag_sets: vec![vec!["x".into()]],
        }
    }

    fn ds(id: &str, seg_ok: bool, invented: usize, is_junk: bool) -> DictationScore {
        DictationScore {
            dictation_id: id.into(),
            expected_entry_count: 1,
            actual_entry_count: if is_junk { 0 } else { 1 },
            segmentation_correct: seg_ok,
            is_junk,
            junk_correct: if is_junk { Some(true) } else { None },
            per_entry: Vec::new(),
            invented_dates: invented,
        }
    }

    fn empty_score(per_dictation: Vec<DictationScore>) -> ScoreReport {
        ScoreReport {
            run_id: "t".into(),
            total_dictations: per_dictation.len(),
            graded_dictations: per_dictation.len(),
            ungradable_dictations: vec![],
            match_algorithm: "test",
            per_metric: crate::scoring::metrics::PerMetric {
                clean_single_item_correct: crate::scoring::metrics::Ratio::new(0, 0),
                segmentation_correct: crate::scoring::metrics::Ratio::new(0, 0),
                category_correct: crate::scoring::metrics::Ratio::new(0, 0),
                entry_type_correct: crate::scoring::metrics::Ratio::new(0, 0),
                invented_dates_count: 0,
                tag_variant_collapse_correct: crate::scoring::metrics::Ratio::new(0, 0),
                junk_correct: crate::scoring::metrics::Ratio::new(0, 0),
            },
            per_dictation,
            stability_vs: None,
            stability: None,
            tag_collapse: None,
        }
    }

    #[test]
    fn persona_id_extraction() {
        assert_eq!(
            persona_id_of("persona-05-case-03"),
            Some("persona-05".into())
        );
        assert_eq!(
            persona_id_of("persona-01-case-05"),
            Some("persona-01".into())
        );
        assert_eq!(persona_id_of("random-thing"), None);
    }

    #[test]
    fn select_samples_includes_required_cases_and_one_per_persona() {
        let mut ak_map: HashMap<String, AnswerKey> = HashMap::new();
        let mut raws: HashMap<String, String> = HashMap::new();
        let mut outs: HashMap<String, String> = HashMap::new();
        let mut scores: Vec<DictationScore> = Vec::new();

        let make = |id: &str, junk: bool| {
            let n = if junk { 0 } else { 1 };
            (
                ak(
                    id,
                    n,
                    junk,
                    if junk {
                        vec![]
                    } else {
                        vec![ee(Category::Personal, EntryType::Task, None)]
                    },
                ),
                "raw text".to_string(),
                "[]".to_string(),
            )
        };

        for id in [
            "persona-01-case-01",
            "persona-01-case-05",
            "persona-02-case-01",
            "persona-03-case-01",
            "persona-04-case-01",
            "persona-05-case-01",
            "persona-05-case-03",
            "persona-05-case-05",
            "persona-06-case-01",
        ] {
            let junk = id == "persona-01-case-05" || id == "persona-05-case-05";
            let (k, r, o) = make(id, junk);
            ak_map.insert(id.to_string(), k);
            raws.insert(id.to_string(), r);
            outs.insert(id.to_string(), o);
            scores.push(ds(id, true, 0, junk));
        }
        let score = empty_score(scores);
        let samples = select_samples(&score, &ak_map, &raws, &outs);
        let ids: Vec<&str> = samples.iter().map(|s| s.dictation_id.as_str()).collect();
        assert!(ids.contains(&"persona-05-case-03"));
        assert!(ids.contains(&"persona-01-case-05"));
        assert!(ids.contains(&"persona-05-case-05"));
        for p in [
            "persona-01",
            "persona-02",
            "persona-03",
            "persona-04",
            "persona-05",
            "persona-06",
        ] {
            assert!(
                samples.iter().any(|s| s.persona_id == p),
                "missing persona {p} in selection: {ids:?}"
            );
        }
    }

    #[test]
    fn build_pcrp_prompt_has_persona_notes_first_then_samples_then_schema() {
        let samples = vec![PcrpSample {
            dictation_id: "persona-01-case-01".into(),
            persona_id: "persona-01".into(),
            raw_dictation: "raw line about a thing".into(),
            answer_key_json: "{\"k\":1}".into(),
            pipeline_output_json: "[]".into(),
            per_metric_pass_flags: PassFlags {
                segmentation_correct: true,
                category_all_correct: true,
                entry_type_all_correct: true,
                no_invented_dates: true,
                junk_correct: None,
            },
        }];
        let prompt = build_pcrp_prompt("persona-01", "PERSONA NOTES BODY", &samples);
        let pn = prompt.find("PERSONA NOTES BODY").expect("notes present");
        let raw = prompt.find("raw line about a thing").expect("raw present");
        let schema = prompt
            .find("Top trust-eroding failures")
            .expect("schema present");
        assert!(pn < raw, "persona notes must precede samples");
        assert!(raw < schema, "samples must precede output schema");
        // Discipline cues present.
        assert!(prompt.contains("Default assumption"));
        assert!(prompt.contains("evidence"));
    }

    #[test]
    fn count_findings_ignores_literal_placeholders() {
        // This is the exact schema we hand the model — without
        // filtering out the `<case-id>` placeholders the counter
        // would always report 3+3.
        let body = "\
### Top trust-eroding failures

- <case-id>: <claim> — evidence: \"<quoted>\"
- <case-id>: ...

### Top trust-building wins

- <case-id>: ...
";
        let (te, tb) = count_findings(body);
        assert_eq!((te, tb), (0, 0));
    }

    #[test]
    fn count_findings_counts_real_bullets() {
        let body = "\
### Top trust-eroding failures

- persona-03-case-02: title misrepresents intent — evidence: \"Quarterly review of nothing\"
- persona-05-case-03: invented date — evidence: \"due_iso: 2026-06-22\"

### Top trust-building wins

- persona-01-case-01: clean single-item parse — evidence: \"category: personal\"
";
        let (te, tb) = count_findings(body);
        assert_eq!((te, tb), (2, 1));
    }

    #[test]
    fn run_pcrp_invokes_dispatcher_once_per_persona() {
        let samples = vec![
            PcrpSample {
                dictation_id: "persona-01-case-01".into(),
                persona_id: "persona-01".into(),
                raw_dictation: "r1".into(),
                answer_key_json: "{}".into(),
                pipeline_output_json: "[]".into(),
                per_metric_pass_flags: PassFlags {
                    segmentation_correct: true,
                    category_all_correct: true,
                    entry_type_all_correct: true,
                    no_invented_dates: true,
                    junk_correct: None,
                },
            },
            PcrpSample {
                dictation_id: "persona-02-case-01".into(),
                persona_id: "persona-02".into(),
                raw_dictation: "r2".into(),
                answer_key_json: "{}".into(),
                pipeline_output_json: "[]".into(),
                per_metric_pass_flags: PassFlags {
                    segmentation_correct: true,
                    category_all_correct: true,
                    entry_type_all_correct: true,
                    no_invented_dates: true,
                    junk_correct: None,
                },
            },
        ];
        let mock = MockOllama::new().default_response(
            "## Persona persona-01\n\n### Summary\nok\n\n### Top trust-eroding failures\n\n- persona-01-case-01: a thing — evidence: \"x\"\n\n### Top trust-building wins\n\n- persona-01-case-01: another thing — evidence: \"y\"\n\n### Metric blindspots observed\n\nnone\n",
        );
        let mut persona_notes = HashMap::new();
        persona_notes.insert("persona-01".into(), "P1 notes".into());
        persona_notes.insert("persona-02".into(), "P2 notes".into());
        let cfg = PcrpConfig {
            run_id: "test".into(),
            reviewer_model: "judge".into(),
            options: GenerateOptions::default(),
            persona_notes,
            samples,
        };
        let report = run_pcrp(&mock, cfg).unwrap();
        assert_eq!(report.per_persona_markdown.len(), 2);
        assert_eq!(mock.calls().len(), 2);
        // Each persona's response had one finding of each kind.
        assert_eq!(report.trust_eroding_failures_count, 2);
        assert_eq!(report.trust_building_wins_count, 2);
        // Markdown render is non-empty and contains the count.
        let md = render_markdown(&report);
        assert!(md.contains("trust_eroding_failures_count: **2**"));
    }
}
