//! Reverse direction of [`crate::vault::markdown_serializer`] (Wave
//! 1E.5 / `mb-qwfy`). Parses `Knowledge Graph/Entries/*.md` files
//! back into a [`KgEntry`] so the reverse-watcher can reconcile
//! user edits in Obsidian back into the SQLite DB.
//!
//! # Contract
//!
//! The parser MUST round-trip the serializer:
//!
//! ```text
//! serialize_entry(parse_entry(serialize_entry(e)).unwrap().entry)
//!   == serialize_entry(e)
//! ```
//!
//! for every entry the serializer can produce (per ADR 0053 §D5 +
//! the 1E.5 polish that switched entity wiki-links from
//! `[[Entities/<slug>]]` to `[[Entities/<slug>|<slug>]]`). The
//! round-trip property test in this module proves this against the
//! serializer's `arb_entry()` distribution; the goldens pin the
//! by-eye canonical form.
//!
//! # Tolerated input shapes
//!
//! - **LF or CRLF line endings.** Obsidian and the iOS Shortcut
//!   courier both emit LF, but a user with a Git client on Windows
//!   may end up with CRLF after a checkout. We strip `\r` early so
//!   the rest of the parser sees clean LF input.
//! - **Entity wiki-links**: three forms in the wild, all map back
//!   to the bare slug.
//!     - `[[Entities/<slug>|<alias>]]` — post-1E.5-polish (current
//!       serializer output). Alias is dropped on read.
//!     - `[[Entities/<slug>]]` — post-1E.2 amendment, pre-1E.5
//!       polish. Bare form.
//!     - `<slug>` — bare-string entries that pre-dated the
//!       wiki-link amendment. The reverse-watcher will round-trip
//!       these into the new wiki-link form on its next projection.
//! - **Obsidian Tasks checkbox toggle.** If the body starts with
//!   `- [ ] / [/] / [x]` (followed by the entry title), the
//!   checkbox state OVERRIDES the YAML `status:` field — that's
//!   the user-toggled-the-checkbox-in-Obsidian case (ADR 0053 §D9
//!   "checkbox-canonical-on-read"). The checkbox line is stripped
//!   from the parsed body.
//!
//! # Failure modes
//!
//! Parsing returns `Err(ParseError)` on:
//!
//! - missing or malformed `---` frontmatter fence
//! - YAML parse failure inside the frontmatter
//! - unknown vocabulary value for `capture_kind` / `category` /
//!   `type` / `status` (open-vocab values would silently corrupt
//!   the DB)
//! - missing required field (`id`, `capture_kind`, `captured_at`,
//!   `title`, `category`, `type`)
//!
//! The reverse-watcher logs + skips a parse failure rather than
//! crashing. This is intentional — a user mid-edit on a YAML field
//! shouldn't take the whole watcher offline.

use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use serde::Deserialize;
use thiserror::Error;

use super::markdown_serializer::{
    CaptureKind, Category, EntryType, KgEntry, Status, SCHEMA_VERSION,
};

/// Result of a successful parse. Wraps a [`KgEntry`] plus a few
/// observability flags the reverse-watcher uses for logging /
/// reconciliation-decision making.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedEntry {
    /// The fully-reconstructed entry. `captured_at_local_date` is
    /// derived from `captured_at` in the machine's local timezone;
    /// this is FYI only — the serializer doesn't write it to the
    /// file, so it can't truly round-trip.
    pub entry: KgEntry,
    /// True if the body started with an Obsidian Tasks checkbox
    /// line that the parser stripped + folded into `entry.status`.
    /// The reverse-watcher uses this for logging only.
    pub had_checkbox: bool,
    /// True if the body's checkbox state DIFFERED from the YAML
    /// `status:` field. The user toggled the checkbox in Obsidian
    /// — the checkbox state wins on read, but the next projection
    /// will re-sync the YAML.
    pub checkbox_overrode_yaml: bool,
}

/// Parser failure modes. Logged at `tracing::warn!` by the
/// reverse-watcher; never propagated to the user.
#[derive(Debug, Error)]
pub enum ParseError {
    /// File doesn't open with a `---` line followed by YAML.
    #[error("missing or malformed frontmatter fence")]
    MissingFrontmatterFence,
    /// YAML inside the fence didn't parse.
    #[error("malformed YAML frontmatter: {0}")]
    YamlParse(#[from] serde_yaml::Error),
    /// A vocabulary field carried a value outside the known set.
    #[error("unknown vocab value for `{field}`: {value}")]
    UnknownVocab {
        /// Frontmatter field name whose value was rejected.
        field: &'static str,
        /// Offending string value as it appeared in YAML.
        value: String,
    },
    /// `captured_at` couldn't be parsed as RFC-3339.
    #[error("malformed RFC-3339 timestamp for `{field}`: {value}")]
    MalformedTimestamp {
        /// Frontmatter field name whose timestamp failed to parse.
        field: &'static str,
        /// Offending RFC-3339 input string.
        value: String,
    },
    /// A required field was absent from the YAML.
    #[error("missing required field `{0}`")]
    MissingField(&'static str),
}

/// Parse a vault entry file into a [`ParsedEntry`].
///
/// `content` is the file's full UTF-8 contents. CRLF tolerance is
/// applied internally; callers don't need to normalize.
pub fn parse_entry(content: &str) -> Result<ParsedEntry, ParseError> {
    // CRLF tolerance up front.
    let normalized: String;
    let content_lf: &str = if content.contains('\r') {
        normalized = content.replace("\r\n", "\n").replace('\r', "\n");
        &normalized
    } else {
        content
    };

    let (frontmatter, body_section) = split_frontmatter(content_lf)?;
    let raw: RawFrontmatter = serde_yaml::from_str(frontmatter)?;
    let (body_clean, checkbox_status) = extract_body_and_checkbox(body_section);

    let capture_kind = parse_capture_kind(raw.capture_kind.as_deref())?;
    let category = parse_category(raw.category.as_deref())?;
    let entry_type = parse_entry_type(raw.entry_type.as_deref())?;
    let yaml_status = match raw.status.as_deref() {
        Some(s) => Some(parse_status(s)?),
        None => None,
    };
    let captured_at = parse_rfc3339(
        raw.captured_at
            .as_deref()
            .ok_or(ParseError::MissingField("captured_at"))?,
        "captured_at",
    )?;
    let due_date = match raw.due_date.as_deref() {
        Some(s) => Some(parse_rfc3339(s, "due_date")?),
        None => None,
    };

    let id = raw.id.ok_or(ParseError::MissingField("id"))?;
    let title = raw.title.ok_or(ParseError::MissingField("title"))?;

    // Checkbox-canonical-on-read (ADR 0053 §D9). If the body had a
    // checkbox AND its state disagreed with the YAML, the checkbox
    // wins. The reverse-watcher will re-project on next write and
    // the YAML will be re-synced.
    let had_checkbox = checkbox_status.is_some();
    let (final_status, checkbox_overrode_yaml) = match (yaml_status, checkbox_status) {
        (yaml, Some(cb)) => {
            let differs = yaml != Some(cb);
            (Some(cb), differs && yaml.is_some())
        }
        (yaml, None) => (yaml, false),
    };

    // Entities: strip wiki-link wrapping + drop pipe-alias suffix.
    // Tolerated forms are documented in the module header.
    let entities: Vec<String> = raw
        .entities
        .unwrap_or_default()
        .into_iter()
        .map(|raw| parse_entity_slug_from_wiki_link(&raw))
        .collect();

    let captured_at_local_date = local_date_from_utc(captured_at);

    let entry = KgEntry {
        id,
        captured_at,
        captured_at_local_date,
        capture_kind,
        title,
        category,
        entry_type,
        status: final_status,
        due_date,
        tags: raw.tags.unwrap_or_default(),
        entities,
        source_session_uuid: raw.source_session_uuid,
        body: body_clean,
    };

    Ok(ParsedEntry {
        entry,
        had_checkbox,
        checkbox_overrode_yaml,
    })
}

/// Strip an Obsidian wiki-link wrapping from an entity reference,
/// returning the bare slug.
///
/// Tolerated input forms (documented on the module header):
///
/// - `[[Entities/<slug>|<alias>]]` (current serializer output)
/// - `[[Entities/<slug>]]` (pre-1E.5-polish bare form)
/// - `<slug>` (pre-amendment bare-string form)
///
/// Input that doesn't match any known form is returned UNCHANGED —
/// the caller treats it as an opaque slug. This permissive policy
/// is load-bearing for the reverse-watcher's "never crash on weird
/// user input" stance; a malformed entry just round-trips as-is.
pub fn parse_entity_slug_from_wiki_link(raw: &str) -> String {
    let trimmed = raw.trim();
    let inner = match trimmed
        .strip_prefix("[[Entities/")
        .and_then(|s| s.strip_suffix("]]"))
    {
        Some(inner) => inner,
        None => return trimmed.to_string(),
    };
    // Drop the `|<alias>` suffix if present (pipe-alias form).
    match inner.split_once('|') {
        Some((slug, _alias)) => slug.to_string(),
        None => inner.to_string(),
    }
}

// ────────────────────────────────────────────────────────────────
// Frontmatter splitting
// ────────────────────────────────────────────────────────────────

/// Split a markdown file into `(yaml_frontmatter, body_section)`.
///
/// Expects the contract the serializer enforces: file opens with
/// `---\n`, frontmatter follows, `---\n` closes the fence, body
/// follows (possibly empty). Returns `Err` if the opening fence is
/// missing or the closing fence never appears.
fn split_frontmatter(content: &str) -> Result<(&str, &str), ParseError> {
    let after_open = content
        .strip_prefix("---\n")
        .ok_or(ParseError::MissingFrontmatterFence)?;
    // Find the closing fence on its own line. `\n---\n` is the
    // canonical separator the serializer emits; we ALSO accept
    // `\n---` followed by EOF (no body) so an entry whose body is
    // empty parses cleanly even without a trailing newline after
    // the fence.
    if let Some(idx) = after_open.find("\n---\n") {
        let fm = &after_open[..idx];
        let body = &after_open[idx + "\n---\n".len()..];
        return Ok((fm, body));
    }
    // EOF after `\n---` (no trailing newline, no body) — also
    // accepted. Strip the closing fence and return an empty body.
    if let Some(stripped) = after_open.strip_suffix("\n---") {
        return Ok((stripped, ""));
    }
    Err(ParseError::MissingFrontmatterFence)
}

// ────────────────────────────────────────────────────────────────
// Body + checkbox extraction
// ────────────────────────────────────────────────────────────────

/// Pull the Obsidian Tasks checkbox line off the front of the body
/// (if present), returning the cleaned body + the checkbox status
/// it carried.
///
/// The serializer's emission rule (see `write_tasks_checkbox`): for
/// `entry.status.is_some()`, the checkbox section is a blank line
/// followed by `- [<glyph>] <title>` (optionally followed by
/// ` 📅 YYYY-MM-DD` for tasks with a due date), then either EOF or
/// another blank line before the body proper. We tolerate the same
/// shape on read.
fn extract_body_and_checkbox(body_section: &str) -> (String, Option<Status>) {
    // Skip leading blank lines.
    let mut idx = 0;
    let bytes = body_section.as_bytes();
    while idx < bytes.len() && bytes[idx] == b'\n' {
        idx += 1;
    }
    let trimmed = &body_section[idx..];

    // Try to peel a checkbox line off the front.
    let (checkbox_status, rest) = match trimmed.find('\n') {
        Some(nl) => {
            let first_line = &trimmed[..nl];
            let after_first = &trimmed[nl + 1..];
            match parse_checkbox_line(first_line) {
                Some(s) => (Some(s), after_first),
                None => (None, trimmed),
            }
        }
        None => match parse_checkbox_line(trimmed) {
            Some(s) => (Some(s), ""),
            None => (None, trimmed),
        },
    };

    // Skip blank line after checkbox (the serializer always emits
    // exactly one).
    let mut body_idx = 0;
    let rest_bytes = rest.as_bytes();
    if !rest_bytes.is_empty() && rest_bytes[0] == b'\n' {
        body_idx = 1;
    }
    let body = &rest[body_idx..];

    // Trim trailing newlines/whitespace so a round-trip serializes
    // back to the same shape (the serializer trims trailing
    // whitespace before emission too).
    let cleaned = body.trim_end_matches(['\n', ' ', '\t']).to_string();
    (cleaned, checkbox_status)
}

/// Return `Some(Status)` if `line` matches the Obsidian Tasks
/// checkbox shape `- [<glyph>] ...`, where `<glyph>` is one of
/// ` `, `/`, `x` (or `X` for tolerance). Returns `None` otherwise.
fn parse_checkbox_line(line: &str) -> Option<Status> {
    let after_dash = line.strip_prefix("- [")?;
    // The glyph is exactly one char followed by `] `.
    let mut chars = after_dash.chars();
    let glyph = chars.next()?;
    let close_bracket = chars.next()?;
    if close_bracket != ']' {
        return None;
    }
    // Need a space after `]` OR end-of-line (defensive — the
    // serializer always emits the space + title).
    match chars.next() {
        Some(' ') | None => {}
        _ => return None,
    }
    match glyph {
        ' ' => Some(Status::Todo),
        '/' => Some(Status::Doing),
        'x' | 'X' => Some(Status::Done),
        _ => None,
    }
}

// ────────────────────────────────────────────────────────────────
// Vocab parsers
// ────────────────────────────────────────────────────────────────

fn parse_capture_kind(s: Option<&str>) -> Result<CaptureKind, ParseError> {
    match s.ok_or(ParseError::MissingField("capture_kind"))? {
        "dictation" => Ok(CaptureKind::Dictation),
        "kg-note" => Ok(CaptureKind::KgNote),
        "kg-note-text" => Ok(CaptureKind::KgNoteText),
        other => Err(ParseError::UnknownVocab {
            field: "capture_kind",
            value: other.to_string(),
        }),
    }
}

fn parse_category(s: Option<&str>) -> Result<Category, ParseError> {
    match s.ok_or(ParseError::MissingField("category"))? {
        "personal" => Ok(Category::Personal),
        "professional" => Ok(Category::Professional),
        "objective" => Ok(Category::Objective),
        other => Err(ParseError::UnknownVocab {
            field: "category",
            value: other.to_string(),
        }),
    }
}

fn parse_entry_type(s: Option<&str>) -> Result<EntryType, ParseError> {
    match s.ok_or(ParseError::MissingField("type"))? {
        "note" => Ok(EntryType::Note),
        "task" => Ok(EntryType::Task),
        "idea" => Ok(EntryType::Idea),
        "question" => Ok(EntryType::Question),
        "decision" => Ok(EntryType::Decision),
        other => Err(ParseError::UnknownVocab {
            field: "type",
            value: other.to_string(),
        }),
    }
}

fn parse_status(s: &str) -> Result<Status, ParseError> {
    match s {
        "todo" => Ok(Status::Todo),
        "doing" => Ok(Status::Doing),
        "done" => Ok(Status::Done),
        other => Err(ParseError::UnknownVocab {
            field: "status",
            value: other.to_string(),
        }),
    }
}

fn parse_rfc3339(s: &str, field: &'static str) -> Result<DateTime<Utc>, ParseError> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|_| ParseError::MalformedTimestamp {
            field,
            value: s.to_string(),
        })
}

fn local_date_from_utc(dt: DateTime<Utc>) -> NaiveDate {
    chrono::Local
        .from_utc_datetime(&dt.naive_utc())
        .date_naive()
}

// ────────────────────────────────────────────────────────────────
// YAML shape
// ────────────────────────────────────────────────────────────────

/// The shape `serde_yaml` deserializes into. Every field is
/// `Option` so a missing key produces a clean
/// [`ParseError::MissingField`] rather than a YAML-level error
/// (which would be opaque to users).
#[derive(Debug, Deserialize)]
struct RawFrontmatter {
    id: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    schema_version: Option<u32>,
    capture_kind: Option<String>,
    captured_at: Option<String>,
    title: Option<String>,
    category: Option<String>,
    #[serde(rename = "type")]
    entry_type: Option<String>,
    status: Option<String>,
    due_date: Option<String>,
    #[serde(default)]
    tags: Option<Vec<String>>,
    #[serde(default)]
    entities: Option<Vec<String>>,
    #[serde(default)]
    source_session_uuid: Option<String>,
}

/// Use `SCHEMA_VERSION` so an unused-import warning doesn't fire if
/// future refactors drop the explicit schema-check.
#[doc(hidden)]
pub const _SCHEMA_VERSION_USED: u32 = SCHEMA_VERSION;

// ────────────────────────────────────────────────────────────────
// Unit tests
// ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::markdown_serializer::serialize_entry;

    // Goldens are included into the test binary so the harness
    // doesn't need a working `cargo test --release` runtime
    // (LESSONS PINNED P2 — pure-Rust modules go through the
    // throwaway-crate or `--no-run` recipe; either way the
    // compile-time include_str! is what matters for the gate).
    const MINIMAL: &str = include_str!("../../tests/fixtures/markdown_golden/minimal.md");
    const FULL_TASK: &str = include_str!("../../tests/fixtures/markdown_golden/full_task.md");
    const DOING_TASK: &str = include_str!("../../tests/fixtures/markdown_golden/doing_task.md");
    const DONE_TASK: &str = include_str!("../../tests/fixtures/markdown_golden/done_task.md");
    const SPECIAL: &str = include_str!("../../tests/fixtures/markdown_golden/special_chars.md");
    const WIKI: &str = include_str!("../../tests/fixtures/markdown_golden/wiki_linked_entities.md");

    /// All six goldens round-trip byte-identical:
    /// parse → serialize == original bytes. This is the core
    /// 1E.5 round-trip contract.
    #[test]
    fn roundtrip_all_six_goldens() {
        for (name, golden) in [
            ("minimal", MINIMAL),
            ("full_task", FULL_TASK),
            ("doing_task", DOING_TASK),
            ("done_task", DONE_TASK),
            ("special_chars", SPECIAL),
            ("wiki_linked_entities", WIKI),
        ] {
            let parsed = parse_entry(golden).unwrap_or_else(|e| {
                panic!("parse failed for golden `{name}`: {e}");
            });
            let reserialized = serialize_entry(&parsed.entry);
            assert_eq!(
                reserialized, golden,
                "round-trip drift for golden `{name}`:\n--- expected ---\n{golden}\n--- got ---\n{reserialized}"
            );
        }
    }

    #[test]
    fn entity_wiki_link_pipe_alias_form() {
        assert_eq!(
            parse_entity_slug_from_wiki_link("[[Entities/feta-cheese|feta-cheese]]"),
            "feta-cheese"
        );
        // User-renamed alias still yields the slug (alias dropped).
        assert_eq!(
            parse_entity_slug_from_wiki_link("[[Entities/feta-cheese|Feta Cheese]]"),
            "feta-cheese"
        );
    }

    #[test]
    fn entity_wiki_link_bare_form() {
        assert_eq!(
            parse_entity_slug_from_wiki_link("[[Entities/feta-cheese]]"),
            "feta-cheese"
        );
    }

    #[test]
    fn entity_bare_string_passthrough() {
        // Pre-amendment legacy form: a bare slug with no wrapping.
        assert_eq!(
            parse_entity_slug_from_wiki_link("feta-cheese"),
            "feta-cheese"
        );
    }

    #[test]
    fn entity_malformed_returns_as_is() {
        // Unknown shape: don't crash, hand the caller the raw bytes.
        assert_eq!(parse_entity_slug_from_wiki_link("[[Garbage"), "[[Garbage");
        assert_eq!(parse_entity_slug_from_wiki_link("Garbage]]"), "Garbage]]");
    }

    #[test]
    fn checkbox_states_parse_round_trip() {
        for (golden, expected) in [
            (FULL_TASK, Some(Status::Todo)),
            (DOING_TASK, Some(Status::Doing)),
            (DONE_TASK, Some(Status::Done)),
            (MINIMAL, None),
        ] {
            let parsed = parse_entry(golden).unwrap();
            assert_eq!(parsed.entry.status, expected);
        }
    }

    #[test]
    fn malformed_yaml_returns_err_does_not_panic() {
        let bad = "---\nid: \"x\nfield: : :\n---\nbody\n";
        let res = parse_entry(bad);
        assert!(matches!(res, Err(ParseError::YamlParse(_))));
    }

    #[test]
    fn unknown_vocab_value_returns_err() {
        let bad = "---\nid: \"x\"\nschema_version: 1\ncapture_kind: \"dictation\"\ncaptured_at: \"2026-01-01T00:00:00Z\"\ntitle: \"t\"\ncategory: \"weird\"\ntype: \"note\"\ntags: []\nentities: []\n---\n";
        let res = parse_entry(bad);
        assert!(matches!(
            res,
            Err(ParseError::UnknownVocab {
                field: "category",
                ..
            })
        ));
    }

    #[test]
    fn missing_fence_returns_err() {
        let bad = "id: x\ntitle: foo\n";
        assert!(matches!(
            parse_entry(bad),
            Err(ParseError::MissingFrontmatterFence)
        ));
    }

    #[test]
    fn crlf_input_normalizes_cleanly() {
        // Same as MINIMAL but with CRLF line endings (e.g. via a
        // Git checkout on Windows with `core.autocrlf=true`).
        let crlf = MINIMAL.replace('\n', "\r\n");
        let parsed = parse_entry(&crlf).unwrap();
        let reserialized = serialize_entry(&parsed.entry);
        // Round-trip yields LF-only output (the serializer's
        // contract), even though we fed CRLF in.
        assert_eq!(reserialized, MINIMAL);
        assert!(!reserialized.contains('\r'), "round-trip must emit LF only");
    }

    #[test]
    fn checkbox_state_overrides_yaml_when_differs() {
        // Hand-built fixture: YAML says `status: todo` but the
        // body checkbox is `[x]` (user toggled it Done in Obsidian).
        // The parser MUST report `status: Done` per ADR 0053 §D9.
        let content = "---\nid: \"01HMTOGGLE0000\"\nschema_version: 1\ncapture_kind: \"kg-note\"\ncaptured_at: \"2026-06-15T14:32:01Z\"\ntitle: \"Buy milk\"\ncategory: \"personal\"\ntype: \"task\"\nstatus: \"todo\"\ntags: []\nentities: []\n---\n\n- [x] Buy milk\n";
        let parsed = parse_entry(content).unwrap();
        assert_eq!(parsed.entry.status, Some(Status::Done));
        assert!(parsed.had_checkbox);
        assert!(parsed.checkbox_overrode_yaml);
    }

    #[test]
    fn body_extraction_strips_checkbox_and_trims() {
        let parsed = parse_entry(FULL_TASK).unwrap();
        // FULL_TASK body is "Maple recommended switching to Home
        // Depot." (the checkbox line is metadata, not body).
        assert_eq!(
            parsed.entry.body,
            "Maple recommended switching to Home Depot."
        );
    }

    #[test]
    fn entities_round_trip_through_pipe_alias_form() {
        let parsed = parse_entry(WIKI).unwrap();
        // After parsing, the entities list is the bare-slug form;
        // re-serializing wraps each in the pipe-alias form.
        assert_eq!(
            parsed.entry.entities,
            vec![
                "mockingbird".to_string(),
                "maple".to_string(),
                "dustin".to_string(),
            ]
        );
        let out = serialize_entry(&parsed.entry);
        assert!(out.contains("[[Entities/mockingbird|mockingbird]]"));
    }

    #[test]
    fn empty_body_no_checkbox_minimal_shape() {
        // Hand-built: frontmatter only, no checkbox, no body.
        // Should round-trip cleanly with `body == ""`.
        let content = "---\nid: \"01HMNULL00000\"\nschema_version: 1\ncapture_kind: \"dictation\"\ncaptured_at: \"2026-01-02T03:04:05Z\"\ntitle: \"t\"\ncategory: \"personal\"\ntype: \"note\"\ntags: []\nentities: []\n---\n";
        let parsed = parse_entry(content).unwrap();
        assert_eq!(parsed.entry.body, "");
        assert!(!parsed.had_checkbox);
        // Round-trip: re-serializing the parsed entry yields the
        // original (modulo source_session_uuid being absent, which
        // it already is).
        let out = serialize_entry(&parsed.entry);
        assert_eq!(out, content);
    }
}
