//! Markdown report rendering + content-preservation scoring for the
//! `mode_eval` bin. Split from `main.rs` to keep both files under the
//! 600-line guideline and to put presentation logic in one place.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use mockingbird_lib::cleanup::PREPROCESSOR_VERSION;

use crate::{Fixture, ModeRow, RunResult};

// --------------------------------------------------------------------
// Content-preservation scoring (automated).
// --------------------------------------------------------------------

/// Case-insensitive substring-match per must-preserve term, with optional
/// per-term acceptable-alternative groups. Other quality axes (format-fit,
/// register-fit) are deliberately human-judged.
#[derive(Debug, Clone, Copy)]
pub struct PreservationScore {
    pub matched: usize,
    pub total: usize,
}

impl PreservationScore {
    pub fn pct(&self) -> f32 {
        if self.total == 0 {
            100.0
        } else {
            (self.matched as f32 / self.total as f32) * 100.0
        }
    }
}

/// Score `output` against `must_preserve` terms. `must_preserve_alts` is a
/// list of equivalence groups — for each group, ANY term in the group
/// being present in `output` satisfies EVERY term in the group. This
/// handles legitimate paraphrase under register-lift (formal mode) without
/// weakening literal-preservation for proper nouns / technical terms.
pub fn score_preservation(
    output: &str,
    must_preserve: &[String],
    must_preserve_alts: &[Vec<String>],
) -> PreservationScore {
    let normalised = normalise(output);
    // Build a set of "satisfied" terms via alts groups first.
    let mut satisfied_by_alt: std::collections::HashSet<String> = std::collections::HashSet::new();
    for group in must_preserve_alts {
        let any_present = group.iter().any(|t| normalised.contains(&normalise(t)));
        if any_present {
            for t in group {
                satisfied_by_alt.insert(normalise(t));
            }
        }
    }
    let matched = must_preserve
        .iter()
        .filter(|term| {
            let n = normalise(term);
            normalised.contains(&n) || satisfied_by_alt.contains(&n)
        })
        .count();
    PreservationScore {
        matched,
        total: must_preserve.len(),
    }
}

/// Lowercase, strip markdown/punct, collapse whitespace. So
/// `"Audio Streaming"` matches `"**audio**   streaming."`.
///
/// Hyphens are converted to spaces so `"half-day"` matches `"half day"` —
/// LLM register-lifts frequently add or drop hyphens in compound terms.
pub fn normalise(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c == '-' { ' ' } else { c })
        .filter(|c| !"*_`#[]()<>\"'.,;:!?".contains(*c))
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

// --------------------------------------------------------------------
// Per-mode aggregate.
// --------------------------------------------------------------------

#[derive(Debug, Default, Clone)]
pub struct ModeAggregate {
    pub fixtures_run: usize,
    pub fixtures_errored: usize,
    pub preservation_full: usize,
    pub preservation_partial: usize,
    pub preservation_zero: usize,
    pub sum_preservation_pct: f32,
    pub sum_total_ms: u64,
    pub sum_llm_ms: u64,
    pub max_llm_ms: u64,
}

impl ModeAggregate {
    pub fn record(&mut self, score: PreservationScore, result: &RunResult) {
        self.fixtures_run += 1;
        if result.error.is_some() {
            self.fixtures_errored += 1;
        }
        let pct = score.pct();
        self.sum_preservation_pct += pct;
        if pct >= 99.9 {
            self.preservation_full += 1;
        } else if pct > 0.0 {
            self.preservation_partial += 1;
        } else {
            self.preservation_zero += 1;
        }
        self.sum_total_ms += result.total_ms;
        self.sum_llm_ms += result.llm_ms;
        if result.llm_ms > self.max_llm_ms {
            self.max_llm_ms = result.llm_ms;
        }
    }

    pub fn avg_preservation_pct(&self) -> f32 {
        if self.fixtures_run == 0 {
            0.0
        } else {
            self.sum_preservation_pct / self.fixtures_run as f32
        }
    }
    pub fn avg_llm_ms(&self) -> u64 {
        if self.fixtures_run == 0 {
            0
        } else {
            self.sum_llm_ms / self.fixtures_run as u64
        }
    }
    pub fn avg_total_ms(&self) -> u64 {
        if self.fixtures_run == 0 {
            0
        } else {
            self.sum_total_ms / self.fixtures_run as u64
        }
    }
}

// --------------------------------------------------------------------
// Markdown rendering.
// --------------------------------------------------------------------

pub fn render_report(
    label: &str,
    timestamp: &str,
    modes: &[ModeRow],
    fixtures: &[Fixture],
    runs: &BTreeMap<(String, String), (PreservationScore, RunResult)>,
    aggregates: &BTreeMap<String, ModeAggregate>,
) -> String {
    let mut out = String::with_capacity(64 * 1024);

    writeln!(out, "# Mode-eval report — `{label}`").ok();
    writeln!(
        out,
        "_Generated {timestamp} by `mode_eval` bin (ADR 0024)._\n"
    )
    .ok();

    write_methodology(&mut out, fixtures);
    write_mode_config(&mut out, modes);
    write_summary(&mut out, modes, aggregates);
    write_per_fixture(&mut out, modes, fixtures, runs);

    out
}

fn write_methodology(out: &mut String, fixtures: &[Fixture]) {
    writeln!(out, "## Methodology\n").ok();
    writeln!(
        out,
        "- Pipeline: `Preprocessor` ({PREPROCESSOR_VERSION}) → DB prompt + dictionary + few-shot → \
         `OllamaProvider` against `localhost:11434`."
    )
    .ok();
    writeln!(
        out,
        "- Fixtures: {} cases across {} categories (see `src-tauri/eval/baseline.json`).",
        fixtures.len(),
        fixtures
            .iter()
            .map(|f| f.category.as_str())
            .collect::<std::collections::BTreeSet<_>>()
            .len()
    )
    .ok();
    writeln!(
        out,
        "- **Preservation** is automated (substring match on `must_preserve`). \
         **Format-fit** and **register-fit** are human-judged from the diffs below."
    )
    .ok();
    writeln!(
        out,
        "- A mode hits **badass** when ≥100% preservation, ≥90% format-fit ≥1, ≥70% format-fit =2 \
         (per ADR 0024). Numbers below give the human reviewer a quick read.\n"
    )
    .ok();
}

fn write_mode_config(out: &mut String, modes: &[ModeRow]) {
    writeln!(out, "## Mode configuration\n").ok();
    writeln!(
        out,
        "| Mode | Display | Model | Temp | Max tok | Prompt ver |"
    )
    .ok();
    writeln!(out, "|---|---|---|---|---|---|").ok();
    for m in modes {
        writeln!(
            out,
            "| `{}` | {} | `{}` | {} | {} | v{} |",
            m.slug, m.display_name, m.model_id, m.temperature, m.max_tokens, m.prompt_version
        )
        .ok();
    }
    writeln!(out).ok();
}

fn write_summary(
    out: &mut String,
    modes: &[ModeRow],
    aggregates: &BTreeMap<String, ModeAggregate>,
) {
    writeln!(out, "## Summary\n").ok();
    writeln!(
        out,
        "| Mode | N | Errors | Preserve avg | Full ✅ | Partial ⚠️ | Zero ❌ | avg LLM ms | avg total ms | max LLM ms |"
    )
    .ok();
    writeln!(out, "|---|---|---|---|---|---|---|---|---|---|").ok();
    for m in modes {
        let agg = aggregates.get(&m.slug).cloned().unwrap_or_default();
        writeln!(
            out,
            "| `{}` | {} | {} | {:.1}% | {} | {} | {} | {} | {} | {} |",
            m.slug,
            agg.fixtures_run,
            agg.fixtures_errored,
            agg.avg_preservation_pct(),
            agg.preservation_full,
            agg.preservation_partial,
            agg.preservation_zero,
            agg.avg_llm_ms(),
            agg.avg_total_ms(),
            agg.max_llm_ms,
        )
        .ok();
    }
    writeln!(out).ok();
}

fn write_per_fixture(
    out: &mut String,
    modes: &[ModeRow],
    fixtures: &[Fixture],
    runs: &BTreeMap<(String, String), (PreservationScore, RunResult)>,
) {
    writeln!(out, "## Per-fixture detail\n").ok();
    for fx in fixtures {
        writeln!(out, "### `{}` — {} ({})\n", fx.id, fx.category, fx.length).ok();
        writeln!(out, "- **Intent:** {}", fx.intent).ok();
        writeln!(
            out,
            "- **Must preserve ({}):** {}",
            fx.must_preserve.len(),
            fx.must_preserve
                .iter()
                .map(|t| format!("`{t}`"))
                .collect::<Vec<_>>()
                .join(", ")
        )
        .ok();
        writeln!(out, "- **Raw STT:**\n  ```\n  {}\n  ```", fx.raw).ok();
        // Preprocessor output is identical across modes — show once.
        if let Some((_, r)) = runs.iter().find(|((fid, _), _)| fid == &fx.id) {
            writeln!(
                out,
                "- **Preprocessed** ({}ms, fillers={}, stutters={}, self-corr={}, cues={}): \n  ```\n  {}\n  ```",
                r.1.preprocess_ms,
                r.1.fillers_stripped,
                r.1.stutters_collapsed,
                r.1.self_corrections,
                r.1.cues_rendered,
                r.1.preprocessed
            )
            .ok();
        }
        writeln!(out).ok();

        for m in modes {
            let key = (fx.id.clone(), m.slug.clone());
            if let Some((score, r)) = runs.get(&key) {
                write_one_mode_block(out, m, fx, score, r);
            }
        }
        writeln!(out, "---\n").ok();
    }
}

fn write_one_mode_block(
    out: &mut String,
    m: &ModeRow,
    fx: &Fixture,
    score: &PreservationScore,
    r: &RunResult,
) {
    let badge = if r.error.is_some() {
        "🛑"
    } else if score.pct() >= 99.9 {
        "✅"
    } else if score.pct() > 0.0 {
        "⚠️"
    } else {
        "❌"
    };
    writeln!(
        out,
        "#### `{}` {} — preservation {}/{} ({:.0}%), LLM {}ms, total {}ms",
        m.slug,
        badge,
        score.matched,
        score.total,
        score.pct(),
        r.llm_ms,
        r.total_ms
    )
    .ok();
    if let Some(err) = &r.error {
        writeln!(out, "\n> 🛑 **error:** {err}\n").ok();
    }
    if let Some(hint) = fx.mode_hints.get(&m.slug) {
        writeln!(out, "\n> _hint: {hint}_\n").ok();
    }
    writeln!(out, "```\n{}\n```\n", r.output).ok();

    // Reproduce the scorer's alt-equivalence logic so "missed" only lists
    // terms whose ENTIRE equivalence group is absent. Otherwise a formal
    // paraphrase scored as a pass would still appear in the missed list.
    let output_norm = normalise(&r.output);
    let mut satisfied_by_alt: std::collections::HashSet<String> = std::collections::HashSet::new();
    for group in &fx.must_preserve_alts {
        if group.iter().any(|t| output_norm.contains(&normalise(t))) {
            for t in group {
                satisfied_by_alt.insert(normalise(t));
            }
        }
    }
    let missed: Vec<&str> = fx
        .must_preserve
        .iter()
        .filter(|t| {
            let n = normalise(t);
            !output_norm.contains(&n) && !satisfied_by_alt.contains(&n)
        })
        .map(|s| s.as_str())
        .collect();
    if !missed.is_empty() {
        writeln!(
            out,
            "_missed must-preserve:_ {}\n",
            missed
                .iter()
                .map(|t| format!("`{t}`"))
                .collect::<Vec<_>>()
                .join(", ")
        )
        .ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalise_strips_punct_and_collapses_ws() {
        assert_eq!(normalise("Hello, **WORLD**!"), "hello world");
        assert_eq!(normalise("  audio   streaming  "), "audio streaming");
    }

    #[test]
    fn normalise_treats_hyphen_as_space() {
        // Half-day vs half day — the LLM frequently adds hyphens to
        // compound terms under register-lift. They're the same content.
        assert_eq!(normalise("half-day"), normalise("half day"));
        assert_eq!(normalise("state-of-the-art"), "state of the art");
    }

    #[test]
    fn preservation_full_match() {
        let s = score_preservation(
            "Apples, eggs, and milk.",
            &svec(&["apples", "eggs", "milk"]),
            &[],
        );
        assert_eq!(s.matched, 3);
        assert_eq!(s.total, 3);
        assert!((s.pct() - 100.0).abs() < 0.01);
    }

    #[test]
    fn preservation_partial_match() {
        let s = score_preservation("apples and milk", &svec(&["apples", "eggs", "milk"]), &[]);
        assert_eq!(s.matched, 2);
        assert!((s.pct() - 66.6666).abs() < 0.1);
    }

    #[test]
    fn preservation_handles_markdown_decoration() {
        let s = score_preservation(
            "**Audio streaming** is ready.",
            &svec(&["audio streaming"]),
            &[],
        );
        assert_eq!(s.matched, 1);
    }

    #[test]
    fn preservation_handles_hyphen_in_output() {
        // 36_emphasis_long case: must_preserve="half day of work";
        // model output is "half-day of work". Should still count.
        let s = score_preservation(
            "It is like a half-day of work.",
            &svec(&["half day of work"]),
            &[],
        );
        assert_eq!(s.matched, 1, "hyphenated compound should normalise");
    }

    #[test]
    fn preservation_alts_satisfies_original_term() {
        // Formal register-lift: "bad" → "poor" is semantically fine.
        // If we declare these equivalent, scoring should pass.
        let s = score_preservation(
            "The UX is significantly poor.",
            &svec(&["bad"]),
            &[svec(&["bad", "poor", "subpar"])],
        );
        assert_eq!(s.matched, 1);
    }

    #[test]
    fn preservation_alts_do_not_create_false_positives() {
        // If neither the original nor any alt is present, no match.
        let s = score_preservation(
            "The deploy went smoothly.",
            &svec(&["bad"]),
            &[svec(&["bad", "poor", "subpar"])],
        );
        assert_eq!(s.matched, 0);
    }

    #[test]
    fn empty_must_preserve_is_full_score() {
        let s = score_preservation("anything", &[], &[]);
        assert_eq!(s.matched, 0);
        assert_eq!(s.total, 0);
        assert!((s.pct() - 100.0).abs() < 0.01);
    }

    fn svec(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|s| s.to_string()).collect()
    }
}
