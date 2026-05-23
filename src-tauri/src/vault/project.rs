//! Pure record-to-markdown projection per ADR 0046 §5.
//!
//! [`project`] is a **pure function**: given a [`ProjectionInput`]
//! it returns a [`ProjectionOutput`] with no I/O, no clocks, no DB
//! access, and no global state. The Phase C reconciliation engine
//! ([`super::export_job`]) is what calls this then writes the bytes
//! to disk.
//!
//! ## Why hand-rolled YAML instead of `serde_yaml`
//!
//! The `projection-is-deterministic` invariant judge (ADR §17)
//! requires that projecting the same record twice produces the
//! **byte-identical** output -- same key order, same quoting, same
//! whitespace, every time. `serde_yaml` is mostly deterministic but
//! has had bugs around scalar quoting heuristics across versions,
//! and the front-matter schema here is a fixed seven-key shape that
//! barely justifies a YAML library. Hand-rolling gives us total
//! control over the byte layout and removes one dep + several
//! seconds of compile time. The cost is ~50 lines of escape
//! handling, all unit-tested in this file.
//!
//! ## Determinism rules (judge-binding)
//!
//! 1. Front-matter keys appear in a fixed order: `id`, `type`,
//!    `created`, `duration_sec`, `title`, `tags`, `source`,
//!    `mockingbird_export_version`.
//! 2. Optional keys (`duration_sec`, `title`, `tags` when empty)
//!    are **omitted** when absent -- *not* rendered as `null` or
//!    `[]`. Omission keeps the front-matter quiet for minimal
//!    records.
//! 3. Tags are sorted alphabetically before serialization.
//! 4. No `exported_at` / `last_synced` / machine-fingerprint /
//!    any other field that would change between runs of the same
//!    input. That kind of metadata lives in `.mockingbird/
//!    manifest.json`, not in the per-file front-matter.
//! 5. Body has exactly one trailing newline. The full bytes
//!    written to disk are `front_matter ++ "\n" ++ body ++ "\n"`
//!    with the body's own trailing whitespace preserved.
//! 6. `content_sha256` is computed over the EXACT bytes returned
//!    in `ProjectionOutput::content` -- which is the EXACT bytes
//!    the export engine writes to disk.

use sha2::{Digest, Sha256};

use crate::error::{AppError, AppResult};
use crate::vault::manifest::{RecordType, MOCKINGBIRD_EXPORT_VERSION};

/// All the canonical-DB-derived inputs the projector needs. Borrow
/// style: the export job already has the underlying strings on its
/// stack from the SQL query; this avoids one round of allocation
/// per record.
#[derive(Debug, Clone)]
pub struct ProjectionInput<'a> {
    /// `sessions.uuid` or `meeting_sessions.uuid` -- the stable
    /// record identifier. The first 8 hex chars are used as the
    /// filename suffix.
    pub uuid: &'a str,

    /// Which canonical table this row came from.
    pub record_type: RecordType,

    /// RFC-3339 timestamp of when the record was captured (PTT
    /// release time, meeting start, etc.). Pulled from
    /// `sessions.started_at` / `meeting_sessions.started_at`.
    ///
    /// Used for two things: the `created:` front-matter key
    /// (verbatim) and the `YYYY-MM-DD-HHMM` filename prefix
    /// (parsed + reformatted via chrono).
    pub created_iso: &'a str,

    /// Optional duration in milliseconds. Rendered as
    /// `duration_sec: 23.4` (one decimal place) when present.
    pub duration_ms: Option<i64>,

    /// Optional human-readable title. For meetings this is the
    /// LLM-derived `meeting_sessions.title`; for dictations it's
    /// reserved for a future first-line-summary feature and is
    /// typically `None` in Iter 2.
    pub title: Option<&'a str>,

    /// User-visible tags. Sorted alphabetically before emit.
    /// Empty slice -> the `tags` key is omitted entirely.
    pub tags: &'a [&'a str],

    /// `sessions.source` (or the analogous meeting-side string).
    /// One of `"desktop" | "desktop-import" | "mobile-inbox" |
    /// "meeting"`. Free-form string at this layer so we don't
    /// couple to the SessionSource enum just for serialization.
    pub source: &'a str,

    /// Pre-rendered transcript body. For dictations this is the
    /// `final` transcript stage (falling back to `cleaned`); for
    /// meetings it's the merged two-channel markdown produced by
    /// `meetings::merge`. The export job builds this before
    /// calling project().
    pub body: &'a str,
}

/// Projection result. The reconciliation engine writes
/// `content` to disk at `filename` if and only if its
/// `content_sha256` differs from the manifest's recorded hash for
/// this UUID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionOutput {
    /// Filename relative to `<vault>/history/`. Shape:
    /// `YYYY-MM-DD-HHMM__<uuid8>.md`.
    pub filename: String,

    /// Full file bytes (front-matter + blank line + body +
    /// trailing newline).
    pub content: String,

    /// Lowercase hex SHA-256 over `content.as_bytes()`. Use this
    /// to populate `ManifestRecord::content_sha256`.
    pub content_sha256: String,
}

/// Pure projection: input -> (filename, content, sha). No I/O.
pub fn project(input: ProjectionInput<'_>) -> AppResult<ProjectionOutput> {
    let filename = derive_filename(input.created_iso, input.uuid)?;
    let content = render_content(&input);
    let content_sha256 = sha256_hex(content.as_bytes());
    Ok(ProjectionOutput {
        filename,
        content,
        content_sha256,
    })
}

/// `YYYY-MM-DD-HHMM__<uuid8>.md` per ADR §5.
fn derive_filename(created_iso: &str, uuid: &str) -> AppResult<String> {
    let dt = chrono::DateTime::parse_from_rfc3339(created_iso).map_err(|e| {
        AppError::Vault(format!(
            "project: invalid created_iso '{created_iso}' -- {e}"
        ))
    })?;
    let stem = dt.format("%Y-%m-%d-%H%M").to_string();
    let uuid8 = uuid8_prefix(uuid);
    Ok(format!("{stem}__{uuid8}.md"))
}

/// Take the first 8 hex chars of the UUID, lower-casing and
/// stripping the canonical dashes so the suffix is dense and
/// filename-safe. A UUID shorter than 8 hex chars (shouldn't
/// happen for v4 UUIDs, but we don't want to panic) is padded
/// with zeros on the right.
fn uuid8_prefix(uuid: &str) -> String {
    let hex: String = uuid
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .map(|c| c.to_ascii_lowercase())
        .take(8)
        .collect();
    if hex.len() == 8 {
        hex
    } else {
        format!("{hex:0<8}")
    }
}

/// Build the full file content. Front-matter + blank line + body
/// + ensured trailing newline.
fn render_content(input: &ProjectionInput<'_>) -> String {
    let mut out = String::with_capacity(input.body.len() + 256);
    out.push_str("---\n");
    write_front_matter(&mut out, input);
    out.push_str("---\n\n");
    out.push_str(input.body);
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Write front-matter keys in fixed order, omitting optional keys
/// that are absent. Each line is `key: value\n`.
fn write_front_matter(out: &mut String, input: &ProjectionInput<'_>) {
    // 1. id
    out.push_str("id: ");
    write_yaml_scalar(out, input.uuid);
    out.push('\n');

    // 2. type
    out.push_str("type: ");
    out.push_str(match input.record_type {
        RecordType::Dictation => "dictation",
        RecordType::Meeting => "meeting",
    });
    out.push('\n');

    // 3. created
    out.push_str("created: ");
    write_yaml_scalar(out, input.created_iso);
    out.push('\n');

    // 4. duration_sec (optional)
    if let Some(ms) = input.duration_ms {
        let secs = (ms as f64) / 1000.0;
        out.push_str(&format!("duration_sec: {secs:.1}\n"));
    }

    // 5. title (optional)
    if let Some(t) = input.title {
        out.push_str("title: ");
        write_yaml_scalar(out, t);
        out.push('\n');
    }

    // 6. tags (optional; sorted)
    if !input.tags.is_empty() {
        let mut sorted: Vec<&str> = input.tags.to_vec();
        sorted.sort_unstable();
        out.push_str("tags: [");
        for (i, t) in sorted.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            write_yaml_scalar(out, t);
        }
        out.push_str("]\n");
    }

    // 7. source
    out.push_str("source: ");
    write_yaml_scalar(out, input.source);
    out.push('\n');

    // 8. mockingbird_export_version
    out.push_str(&format!(
        "mockingbird_export_version: {}\n",
        MOCKINGBIRD_EXPORT_VERSION
    ));
}

/// Write `s` as a YAML scalar. Plain (unquoted) when it's
/// safe; double-quoted with escapes otherwise.
///
/// "Safe" = ASCII alphanumeric / dash / underscore / dot / colon
/// / plus, plus a few delimiters that don't trigger YAML
/// flow-scalar ambiguity. Anything else triggers quoting. This is
/// intentionally conservative: extra quoting is fine, missing
/// quoting around (say) a leading `[` would parse as a flow
/// sequence and break Obsidian's front-matter parser.
fn write_yaml_scalar(out: &mut String, s: &str) {
    if is_plain_yaml_safe(s) {
        out.push_str(s);
    } else {
        out.push('"');
        for c in s.chars() {
            match c {
                '\\' => out.push_str("\\\\"),
                '"' => out.push_str("\\\""),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if (c as u32) < 0x20 => {
                    out.push_str(&format!("\\u{:04x}", c as u32));
                }
                c => out.push(c),
            }
        }
        out.push('"');
    }
}

/// Conservative test for "can be emitted as a YAML plain scalar".
/// Errs on the side of quoting -- the goal is byte-deterministic
/// output that round-trips through Obsidian's front-matter
/// parser, not minimal byte count.
fn is_plain_yaml_safe(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    // Reject leading characters that have special YAML meaning.
    let first = s.chars().next().unwrap();
    if matches!(
        first,
        '!' | '&'
            | '*'
            | '['
            | ']'
            | '{'
            | '}'
            | '|'
            | '>'
            | '\''
            | '"'
            | '%'
            | '@'
            | '`'
            | ' '
            | '#'
            | ','
    ) {
        return false;
    }
    // Reject "yes" / "no" / "true" / "false" / "null" / "~" /
    // numeric-looking strings to keep Obsidian's parser from
    // type-coercing them.
    let lower = s.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "yes" | "no" | "true" | "false" | "null" | "~" | "on" | "off"
    ) {
        return false;
    }
    if s.parse::<f64>().is_ok() {
        return false;
    }
    // Reject any character outside the conservative allowlist.
    s.chars().all(|c| {
        c.is_ascii_alphanumeric()
            || matches!(c, '-' | '_' | '.' | ':' | '+' | '/' | ' ')
    })
        // Forbid trailing space (YAML strips it).
        && !s.ends_with(' ')
        // Forbid leading dash (looks like a list item).
        && !s.starts_with('-')
        // Forbid colon-space (looks like a key/value in flow context).
        && !s.contains(": ")
}

/// Lowercase hex SHA-256.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    let digest = h.finalize();
    let mut s = String::with_capacity(64);
    for b in digest {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dictation_input<'a>(uuid: &'a str, created: &'a str, body: &'a str) -> ProjectionInput<'a> {
        ProjectionInput {
            uuid,
            record_type: RecordType::Dictation,
            created_iso: created,
            duration_ms: Some(23_400),
            title: None,
            tags: &["dictation", "mockingbird", "normal"],
            source: "desktop",
            body,
        }
    }

    #[test]
    fn filename_has_yyyy_mm_dd_hhmm_uuid8_shape() {
        let fname = derive_filename(
            "2026-05-27T14:08:42Z",
            "a4f7c2d3-9c61-4e8a-9c1f-2b8e0d61f9a2",
        )
        .unwrap();
        assert_eq!(fname, "2026-05-27-1408__a4f7c2d3.md");
    }

    #[test]
    fn filename_uuid8_drops_dashes_and_lowercases() {
        let fname = derive_filename(
            "2026-01-02T03:04:05Z",
            "DEAD-BEEF-9C61-4e8a-9c1f-2b8e0d61f9a2",
        )
        .unwrap();
        // "DEADBEEF" -> "deadbeef" after lowercase.
        assert!(fname.ends_with("__deadbeef.md"), "got {fname}");
    }

    #[test]
    fn filename_rejects_bad_iso() {
        let err =
            derive_filename("not-a-date", "a4f7c2d3-9c61-4e8a-9c1f-2b8e0d61f9a2").unwrap_err();
        match err {
            AppError::Vault(msg) => assert!(msg.contains("invalid created_iso")),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn projection_includes_every_required_front_matter_key() {
        let out = project(dictation_input(
            "a4f7c2d3-9c61-4e8a-9c1f-2b8e0d61f9a2",
            "2026-05-27T14:08:42Z",
            "Quick note about the projection job.",
        ))
        .unwrap();
        for needle in [
            "id: a4f7c2d3-9c61-4e8a-9c1f-2b8e0d61f9a2",
            "type: dictation",
            "created: 2026-05-27T14:08:42Z",
            "duration_sec: 23.4",
            "tags: [dictation, mockingbird, normal]",
            "source: desktop",
            "mockingbird_export_version: 1",
        ] {
            assert!(
                out.content.contains(needle),
                "missing {needle:?} in:\n{}",
                out.content
            );
        }
        assert!(out.content.ends_with('\n'));
        assert!(out.content.contains("Quick note about the projection job."));
    }

    #[test]
    fn front_matter_keys_appear_in_fixed_order() {
        let out = project(dictation_input(
            "a4f7c2d3-9c61-4e8a-9c1f-2b8e0d61f9a2",
            "2026-05-27T14:08:42Z",
            "body",
        ))
        .unwrap();
        let positions: Vec<usize> = [
            "id:",
            "type:",
            "created:",
            "duration_sec:",
            "tags:",
            "source:",
            "mockingbird_export_version:",
        ]
        .iter()
        .map(|k| {
            out.content
                .find(k)
                .unwrap_or_else(|| panic!("missing key {k}"))
        })
        .collect();
        // Strictly increasing.
        for w in positions.windows(2) {
            assert!(w[0] < w[1], "key order violated: {positions:?}");
        }
    }

    #[test]
    fn optional_keys_are_omitted_when_absent() {
        let input = ProjectionInput {
            uuid: "a4f7c2d3-9c61-4e8a-9c1f-2b8e0d61f9a2",
            record_type: RecordType::Dictation,
            created_iso: "2026-05-27T14:08:42Z",
            duration_ms: None,
            title: None,
            tags: &[],
            source: "desktop",
            body: "minimal",
        };
        let out = project(input).unwrap();
        assert!(
            !out.content.contains("duration_sec"),
            "got:\n{}",
            out.content
        );
        assert!(!out.content.contains("title:"), "got:\n{}", out.content);
        assert!(!out.content.contains("tags:"), "got:\n{}", out.content);
    }

    #[test]
    fn tags_are_sorted_alphabetically() {
        let input = ProjectionInput {
            uuid: "a4f7c2d3-9c61-4e8a-9c1f-2b8e0d61f9a2",
            record_type: RecordType::Dictation,
            created_iso: "2026-05-27T14:08:42Z",
            duration_ms: None,
            title: None,
            // Unsorted on input -- projector must sort.
            tags: &["zeta", "alpha", "mike"],
            source: "desktop",
            body: "x",
        };
        let out = project(input).unwrap();
        assert!(
            out.content.contains("tags: [alpha, mike, zeta]"),
            "got:\n{}",
            out.content
        );
    }

    #[test]
    fn projection_is_byte_identical_on_re_run() {
        let a = project(dictation_input(
            "a4f7c2d3-9c61-4e8a-9c1f-2b8e0d61f9a2",
            "2026-05-27T14:08:42Z",
            "the body",
        ))
        .unwrap();
        let b = project(dictation_input(
            "a4f7c2d3-9c61-4e8a-9c1f-2b8e0d61f9a2",
            "2026-05-27T14:08:42Z",
            "the body",
        ))
        .unwrap();
        assert_eq!(a, b, "projection must be deterministic");
        assert_eq!(a.content_sha256, b.content_sha256);
    }

    #[test]
    fn meeting_projection_uses_meeting_type() {
        let input = ProjectionInput {
            uuid: "a4f7c2d3-9c61-4e8a-9c1f-2b8e0d61f9a2",
            record_type: RecordType::Meeting,
            created_iso: "2026-05-27T14:08:42Z",
            duration_ms: Some(1_800_000),
            title: Some("Sync with Bob"),
            tags: &["meeting"],
            source: "meeting",
            body: "**You:** Hi.\n**Other(s):** Hey.\n",
        };
        let out = project(input).unwrap();
        assert!(out.content.contains("type: meeting"));
        assert!(out.content.contains("title: Sync with Bob"));
        assert!(out.content.contains("duration_sec: 1800.0"));
        assert!(out.content.contains("**You:** Hi."));
    }

    #[test]
    fn yaml_scalar_quotes_when_unsafe() {
        // Title with embedded colon-space, leading bracket, etc.
        let input = ProjectionInput {
            uuid: "a4f7c2d3-9c61-4e8a-9c1f-2b8e0d61f9a2",
            record_type: RecordType::Dictation,
            created_iso: "2026-05-27T14:08:42Z",
            duration_ms: None,
            title: Some("Meeting: planning [Q4]"),
            tags: &[],
            source: "desktop",
            body: "x",
        };
        let out = project(input).unwrap();
        assert!(
            out.content.contains("title: \"Meeting: planning [Q4]\""),
            "got:\n{}",
            out.content
        );
    }

    #[test]
    fn yaml_scalar_escapes_quotes_and_backslashes() {
        let mut s = String::new();
        write_yaml_scalar(&mut s, "she said \"hi\\bye\"");
        // The whole thing should be double-quoted with backslash escapes.
        assert_eq!(s, "\"she said \\\"hi\\\\bye\\\"\"");
    }

    #[test]
    fn is_plain_yaml_safe_rejects_booleans_and_numbers() {
        for s in [
            "yes", "no", "true", "FALSE", "null", "~", "on", "off", "3.14", "42", "1e5",
        ] {
            assert!(!is_plain_yaml_safe(s), "{s:?} should NOT be plain-safe");
        }
    }

    #[test]
    fn is_plain_yaml_safe_accepts_ordinary_words() {
        for s in ["desktop", "mobile-inbox", "meeting", "Sync with Bob"] {
            assert!(is_plain_yaml_safe(s), "{s:?} should be plain-safe");
        }
    }

    #[test]
    fn sha256_matches_known_value() {
        // SHA-256("hello") = 2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824
        assert_eq!(
            sha256_hex(b"hello"),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        assert_eq!(sha256_hex(b"").len(), 64);
    }

    #[test]
    fn content_sha256_matches_content_bytes() {
        let out = project(dictation_input(
            "a4f7c2d3-9c61-4e8a-9c1f-2b8e0d61f9a2",
            "2026-05-27T14:08:42Z",
            "body",
        ))
        .unwrap();
        // The reported sha MUST be the sha of the exact content
        // the reconciliation engine will write to disk -- otherwise
        // the manifest's content_sha256 column lies and the
        // skip-write optimization corrupts the vault.
        assert_eq!(out.content_sha256, sha256_hex(out.content.as_bytes()));
    }

    #[test]
    fn body_with_no_trailing_newline_gets_one_appended() {
        let input = ProjectionInput {
            uuid: "a4f7c2d3-9c61-4e8a-9c1f-2b8e0d61f9a2",
            record_type: RecordType::Dictation,
            created_iso: "2026-05-27T14:08:42Z",
            duration_ms: None,
            title: None,
            tags: &[],
            source: "desktop",
            body: "no newline at end",
        };
        let out = project(input).unwrap();
        assert!(out.content.ends_with("no newline at end\n"));
    }

    #[test]
    fn body_with_existing_trailing_newline_is_not_double_ended() {
        let input = ProjectionInput {
            uuid: "a4f7c2d3-9c61-4e8a-9c1f-2b8e0d61f9a2",
            record_type: RecordType::Dictation,
            created_iso: "2026-05-27T14:08:42Z",
            duration_ms: None,
            title: None,
            tags: &[],
            source: "desktop",
            body: "has newline\n",
        };
        let out = project(input).unwrap();
        assert!(out.content.ends_with("has newline\n"));
        assert!(!out.content.ends_with("\n\n\n"));
    }

    /// "Snapshot"-style: pin the full byte stream so an accidental
    /// change to the front-matter order or whitespace fails loudly.
    /// If this test needs updating, *every* exported file in every
    /// user's vault will be re-written on next reconciliation --
    /// so do it deliberately and bump MOCKINGBIRD_EXPORT_VERSION.
    #[test]
    fn golden_snapshot_minimal_dictation() {
        let input = ProjectionInput {
            uuid: "a4f7c2d3-9c61-4e8a-9c1f-2b8e0d61f9a2",
            record_type: RecordType::Dictation,
            created_iso: "2026-05-27T14:08:42Z",
            duration_ms: Some(23_400),
            title: None,
            tags: &["normal"],
            source: "desktop",
            body: "hello world",
        };
        let out = project(input).unwrap();
        let expected = "\
---
id: a4f7c2d3-9c61-4e8a-9c1f-2b8e0d61f9a2
type: dictation
created: 2026-05-27T14:08:42Z
duration_sec: 23.4
tags: [normal]
source: desktop
mockingbird_export_version: 1
---

hello world
";
        assert_eq!(out.content, expected, "front-matter byte layout drifted");
        assert_eq!(out.filename, "2026-05-27-1408__a4f7c2d3.md");
    }
}
