//! Phase 0 Knowledge Graph schema types.
//!
//! These types are deliberately minimal — they are exactly the shape
//! the pipeline must emit (spec §7) and the answer-key authors hand-fill
//! at corpus authoring time (spec §6.1). The v1 schema layered on top
//! of these will live elsewhere; Phase 0 only needs enough of the model
//! to score correctness.
//!
//! ## Design notes
//!
//! - **`due_iso: Option<String>`** — the type-level enforcement of the
//!   spec §8.4 "0 invented dates" hard gate. An absent date is
//!   `None`, not the empty string and not a default-falsy "unknown".
//!   The `due_iso_none_survives_round_trip` test pins this contract.
//! - **`status: Option<Status>`** on [`Entry`] is `skip_serializing_if =
//!   "Option::is_none"` — non-task entries must emit YAML frontmatter
//!   *without* a `status:` key at all (spec §7.3: "Status — for `task`
//!   type … Omit for non-tasks"). The `status_omitted_when_none`
//!   test pins this.
//! - **`ExpectedEntry::due_iso`** is **not** skip-serialized — the
//!   answer key intentionally carries `due_iso: null` as a visible,
//!   explicit "this dictation should produce no due date." Hiding it
//!   would defeat the purpose of the answer key as ground truth.
//! - Tag normalization (lowercase / hyphen / singular per spec §7.2)
//!   is **not** implemented here — that's `passes/normalize.rs` in
//!   Wave 2. Schema stays a pure data shell.

use serde::{Deserialize, Serialize};

/// Layer 1 of the spec §7.2 three-layer tag system — controlled,
/// pick exactly one. Closed set; the coarse filter the user leans
/// on most.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Personal,
    Professional,
    Objective,
}

/// Layer 2 of the spec §7.2 three-layer tag system — controlled,
/// small stable set. Drives downstream behavior (a `Task` gets a
/// status, a `Note` does not).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryType {
    Task,
    Research,
    Idea,
    Note,
    Reference,
}

/// Status of a task entry. Only meaningful when `EntryType == Task`;
/// omitted entirely (not even a `null` value) on `Entry` for any
/// other type — see [`Entry::status`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Todo,
    Doing,
    Done,
}

/// A single structured Knowledge Graph entry as produced by the
/// pipeline. This is what gets emitted as YAML frontmatter (plus
/// `body` as the markdown body) into a file in `<vault>/Knowledge
/// Graph/Entries/` in v1.
///
/// All fields are *inferred* — the user is never asked to fill any
/// of these (spec §7.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Short, human-readable; generated from content.
    pub title: String,

    /// Layer 1 of the tag system.
    pub category: Category,

    /// Layer 2 of the tag system.
    pub entry_type: EntryType,

    /// Only meaningful for `EntryType::Task`. Serialized as a `status:`
    /// key only when `Some(_)` — for non-task entries the key is
    /// absent from the emitted YAML.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<Status>,

    /// Layer 3 of the tag system — open-vocabulary, LLM-generated,
    /// normalized (lowercase / hyphenated / singular) by the
    /// normalize pass.
    pub topic_tags: Vec<String>,

    /// ISO-8601 due date when, and only when, the user mentioned
    /// timing. `None` is the honest answer when no timing was
    /// mentioned; an invented `Some(_)` is the spec §8.4 hard-gate
    /// failure. Skip-serialized so the absence is also absent from
    /// the YAML.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due_iso: Option<String>,

    /// When the dictation was captured. ISO-8601.
    pub captured_iso: String,

    /// Cleaned transcript body. v1 also embeds a link back to the
    /// raw audio + transcript under `History/`; that wiring is a v1
    /// concern, not Phase 0.
    pub body: String,
}

/// One *expected* entry inside an [`AnswerKey`]. Authored by hand at
/// corpus-construction time (spec §6.1).
///
/// `acceptable_topic_tag_sets` is a `Vec<Vec<String>>` to capture the
/// spec §8.3 reality: tag correctness is partly subjective, so the
/// answer key may legitimately list multiple acceptable tag sets
/// (e.g. `[["car-repair", "auto"], ["car-repair"], ["auto-maintenance"]]`).
/// The scorer accepts if the pipeline's normalized tag set is set-equal
/// to *any* of the listed alternatives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpectedEntry {
    pub category: Category,
    pub entry_type: EntryType,
    /// The expected due date for this entry, or `None` when the
    /// dictation mentions no timing. This field is *not*
    /// skip-serialized — `due_iso: null` in an answer key is a
    /// load-bearing explicit assertion ("the pipeline must leave
    /// this empty"), not noise to be hidden.
    pub due_iso: Option<String>,
    pub acceptable_topic_tag_sets: Vec<Vec<String>>,
}

/// The hand-authored ground truth for a single dictation. The single
/// most important methodology rule in Phase 0 (spec §6.1): the
/// answer key is authored at corpus-construction time, *not* generated
/// by the model under test. If the same model produces and grades,
/// nothing is learned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnswerKey {
    /// Stable id of the dictation this key grades.
    pub dictation_id: String,

    /// How many distinct entries the dictation *should* split into.
    /// `0` when `is_junk_no_entry_expected` is true.
    pub expected_entry_count: usize,

    /// One element per expected entry, in any order. The scorer
    /// matches by content, not by index.
    pub entries: Vec<ExpectedEntry>,

    /// True for the "junk / no real content" difficulty bucket
    /// (spec §6.2). When true, the pipeline must emit zero entries
    /// (or a flagged-for-review marker, per spec §6.1).
    pub is_junk_no_entry_expected: bool,
}

// ────────────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────────────
//
// These tests are the type-level guard for the spec §8.4 thresholds.
// In particular, `due_iso_none_survives_round_trip` is the mechanical
// enforcement of the "0 invented dates" hard gate: it would be
// possible for a sloppy serde derive to coerce a `None` due date into
// an empty string on round-trip, and then the scorer would silently
// score that as "the pipeline emitted a date." The test prevents that
// failure mode at the type level.

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entry() -> Entry {
        Entry {
            title: "Pick up dry cleaning".to_string(),
            category: Category::Personal,
            entry_type: EntryType::Task,
            status: Some(Status::Todo),
            topic_tags: vec!["errand".to_string(), "household".to_string()],
            due_iso: Some("2026-06-01".to_string()),
            captured_iso: "2026-05-28T10:15:00Z".to_string(),
            body: "Pick up the dry cleaning from Maple before Friday.".to_string(),
        }
    }

    fn sample_answer_key_two_entries() -> AnswerKey {
        AnswerKey {
            dictation_id: "persona-caregiver-002".to_string(),
            expected_entry_count: 2,
            entries: vec![
                ExpectedEntry {
                    category: Category::Personal,
                    entry_type: EntryType::Task,
                    due_iso: Some("2026-06-01".to_string()),
                    acceptable_topic_tag_sets: vec![
                        vec!["errand".to_string(), "household".to_string()],
                        vec!["chore".to_string()],
                    ],
                },
                ExpectedEntry {
                    category: Category::Personal,
                    entry_type: EntryType::Idea,
                    due_iso: None,
                    acceptable_topic_tag_sets: vec![vec!["gift".to_string()]],
                },
            ],
            is_junk_no_entry_expected: false,
        }
    }

    #[test]
    fn answer_key_json_round_trip() {
        let original = sample_answer_key_two_entries();
        let json = serde_json::to_string(&original).expect("serialize answer key");
        let parsed: AnswerKey = serde_json::from_str(&json).expect("deserialize answer key");
        assert_eq!(original, parsed);
    }

    #[test]
    fn entry_yaml_round_trip() {
        let original = sample_entry();
        let yaml = serde_yaml::to_string(&original).expect("serialize entry to YAML");
        let parsed: Entry = serde_yaml::from_str(&yaml).expect("deserialize entry from YAML");
        assert_eq!(original, parsed);
    }

    /// The spec §8.4 "0 invented dates" hard gate enforced at the
    /// type level. A `None` due date must survive JSON round-trip as
    /// `None` — not `Some("")`, not `Some("unknown")`, not anything
    /// else that the scorer might silently treat as "a date was
    /// produced."
    #[test]
    fn due_iso_none_survives_round_trip() {
        let key = AnswerKey {
            dictation_id: "no-date-001".to_string(),
            expected_entry_count: 1,
            entries: vec![ExpectedEntry {
                category: Category::Professional,
                entry_type: EntryType::Note,
                due_iso: None,
                acceptable_topic_tag_sets: vec![vec!["meeting-prep".to_string()]],
            }],
            is_junk_no_entry_expected: false,
        };

        let json = serde_json::to_string(&key).expect("serialize");
        let parsed: AnswerKey = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(parsed.entries.len(), 1);
        assert!(
            parsed.entries[0].due_iso.is_none(),
            "due_iso must round-trip as None, got {:?}",
            parsed.entries[0].due_iso
        );
    }

    /// Spec §7.3: `status` is omitted for non-task entries. We
    /// enforce this at the serializer level via
    /// `skip_serializing_if = "Option::is_none"` on `Entry::status`,
    /// and pin it here so a future refactor can't silently flip the
    /// emitted YAML to include a `status: ~` line.
    #[test]
    fn status_omitted_when_none() {
        let entry = Entry {
            title: "Book to read".to_string(),
            category: Category::Personal,
            entry_type: EntryType::Reference,
            status: None,
            topic_tags: vec!["reading-list".to_string()],
            due_iso: None,
            captured_iso: "2026-05-28T10:15:00Z".to_string(),
            body: "The Mom Test by Rob Fitzpatrick.".to_string(),
        };

        let yaml = serde_yaml::to_string(&entry).expect("serialize to YAML");

        assert!(
            !yaml.contains("status"),
            "YAML must not contain a `status:` key when status is None.\nGot:\n{yaml}"
        );
        // Belt-and-braces: due_iso must also be absent.
        assert!(
            !yaml.contains("due_iso"),
            "YAML must not contain a `due_iso:` key when due_iso is None.\nGot:\n{yaml}"
        );
    }
}
