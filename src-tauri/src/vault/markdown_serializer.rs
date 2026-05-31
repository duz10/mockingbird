//! Phase 1E Wave 1E.2 (`mb-vq8y`, ADR 0053 sections D2 / D3 / D9):
//! deterministic, byte-stable Markdown serializer for KG entries.
//!
//! Pure transformation: [`KgEntry`] -> `(filename, bytes)`. No file
//! I/O, no DB calls, no clock reads, no timezone-database lookups.
//! Wave 1E.3 wires the worker; Wave 1E.5 (reverse-watcher) will need
//! a parser that round-trips the exact bytes this module emits.
//!
//! # Canonical-form contract (the bytes this module produces)
//!
//! - LF line endings everywhere, even on Windows. The reverse-watcher
//!   must NOT re-emit CRLF; round-trip safety depends on it.
//! - Frontmatter is fenced with `---\n` on both sides.
//! - Required frontmatter fields appear in a deterministic order
//!   (`id`, `schema_version`, `capture_kind`, `captured_at`, `title`,
//!   `category`, `type`, `status?`, `due_date?`, `tags`, `entities`,
//!   `source_session_uuid?`). The optional fields (`status`,
//!   `due_date`, `source_session_uuid`) are omitted entirely when
//!   absent; we never emit `field: null`.
//! - Strings are always double-quoted, even when YAML would let us
//!   bare them. Predictable + survives every weird char the user
//!   might dictate.
//! - Empty lists serialize as `tags: []` (flow form) so the reverse
//!   parser can distinguish "no tags" from "field absent".
//! - Non-empty lists serialize as block-style (`-` per line) for
//!   readable diffs.
//! - Timestamps serialize as RFC 3339 with a literal `Z` suffix.
//! - When `status` is present, the body is prefixed with an Obsidian
//!   Tasks checkbox line per Wave 1E.2 D9, separated from the
//!   frontmatter by a single blank line. The body itself (the cleaned
//!   transcript text) follows after another single blank line.
//! - The file ends with exactly one `\n`.
//!
//! # Why a hand-rolled emitter (not serde_yaml)
//!
//! `serde_yaml` has its own ideas about key order (insertion-order,
//! but only with feature gates), about quoting (omits quotes on
//! "safe" strings, which is precisely the unpredictability we want to
//! avoid), and about block vs flow lists (heuristic-driven). We want
//! the bytes to be a pure function of the entry, full stop. So we
//! emit the frontmatter ourselves with a tiny quoted-scalar writer
//! and lean on `serde_yaml` only inside tests to verify the result
//! still parses.
//!
//! # Round-trip safety (forward declaration for 1E.5)
//!
//! Every choice above is deliberate to make the canonical form a
//! fixed point of `serialize(parse(serialize(entry))) == serialize(entry)`:
//!
//! - LF only -> no platform-dependent line-ending churn.
//! - Quoted strings -> the parser doesn't have to guess whether a
//!   bare token is a string or some other YAML scalar type.
//! - Block-style non-empty lists -> the parser produces the same
//!   `Vec<String>` order we emitted; we re-emit in the same order.
//! - Empty list as `[]` (not omitted) -> "no tags" round-trips
//!   identically; we don't have to remember whether the source had
//!   the field present-but-empty or absent.
//! - Omitted optional fields (no `null` sentinels) -> the parser
//!   reports them as `None`; we re-omit them on serialize.

use chrono::{DateTime, NaiveDate, Utc};
use std::fmt::Write as _;

/// Hard cap on the slug portion of the filename.
///
/// 50 chars keeps total filename length well under Windows' MAX_PATH
/// (260) and macOS/Linux per-component limits (255) even with deep
/// vault paths, while still leaving the slug human-recognizable in a
/// file picker. Identity recovery lives in the `__<id8>` suffix, so
/// truncation never costs round-trip-ability.
pub const SLUG_MAX_LEN: usize = 50;

/// Schema version emitted in every entry's frontmatter.
///
/// Bumped on any non-additive change to the canonical form. Adding a
/// new optional field is additive (existing parsers ignore unknown
/// keys); changing field semantics, renaming a key, or removing a
/// required field is not. Wave 1E.5 will refuse to ingest entries
/// whose `schema_version` exceeds what it understands.
pub const SCHEMA_VERSION: u32 = 1;

// ------------------------------------------------------------
// Entry model
// ------------------------------------------------------------

/// A captured entry as it lives in memory, ready to be written to the
/// vault.
///
/// The fields here are the canonical superset; specific capture paths
/// fill in subsets (e.g. dictation has no `status`; a KG-Inbox task
/// note has one but may have no `due_date`). The serializer treats
/// `None`-valued optional fields as "omit from frontmatter".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KgEntry {
    /// Stable identifier (UUID or ULID string). Survives renames.
    /// The first 8 alphanumeric chars also become the `__<id8>`
    /// filename suffix.
    pub id: String,

    /// UTC capture timestamp. Serialized as RFC 3339.
    pub captured_at: DateTime<Utc>,

    /// Local-timezone capture DATE (not a timestamp). Used ONLY for
    /// the filename prefix. Computed by the caller at capture time
    /// via `chrono::Local::now().date_naive()`; see the module-level
    /// docs for the rationale on storing this separately from
    /// `captured_at`.
    pub captured_at_local_date: NaiveDate,

    /// Origin of the capture (dictation / KG-Inbox audio / in-app
    /// text note). Drives the YAML `capture_kind` field and the
    /// downstream source-gated dictation tail (ADR 0052 D1).
    pub capture_kind: CaptureKind,

    /// Short, inferred title -- already pipeline-cleaned.
    pub title: String,

    /// Layer-1 controlled-vocab tag (personal / professional / objective).
    pub category: Category,

    /// Layer-2 controlled-vocab tag (note / task / idea / ...).
    ///
    /// Named `entry_type` in Rust (not `type`) to dodge the keyword;
    /// serialized as `type:` on the wire.
    pub entry_type: EntryType,

    /// Task status. Drives the Obsidian Tasks checkbox line.
    /// `None` -> no checkbox is emitted.
    pub status: Option<Status>,

    /// Optional due date for task-typed entries.
    pub due_date: Option<DateTime<Utc>>,

    /// Free-form lowercase tags. Empty vec renders as `tags: []`.
    pub tags: Vec<String>,

    /// Mentioned entities (person / org / place / product).
    /// Empty vec renders as `entities: []`.
    pub entities: Vec<String>,

    /// Originating dictation-session UUID, when applicable.
    /// Lets a later viewer cross-link back to the session row.
    pub source_session_uuid: Option<String>,

    /// The entry body -- the cleaned transcript text (or the user-
    /// typed text for `kg-note-text`). Serializer normalizes
    /// trailing whitespace and CRs; embedded LFs are preserved.
    pub body: String,
}

/// Origin of a capture event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureKind {
    /// In-flow dictation that the LLM tagged as worth saving.
    Dictation,
    /// KG-Inbox audio capture (long-press / dedicated capture mode).
    KgNote,
    /// In-app text note (typed, not dictated).
    KgNoteText,
}

impl CaptureKind {
    fn as_wire(self) -> &'static str {
        match self {
            Self::Dictation => "dictation",
            Self::KgNote => "kg-note",
            Self::KgNoteText => "kg-note-text",
        }
    }
}

/// Layer-1 vocabulary category (mirrors `vocabularies.category` in
/// the 1D.5 Settings surface).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    /// Personal / life-admin scope.
    Personal,
    /// Work / professional scope.
    Professional,
    /// Cross-cutting objectives (OKRs, goals, etc.).
    Objective,
}

impl Category {
    fn as_wire(self) -> &'static str {
        match self {
            Self::Personal => "personal",
            Self::Professional => "professional",
            Self::Objective => "objective",
        }
    }
}

/// Layer-2 vocabulary entry-type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryType {
    /// Plain informational note.
    Note,
    /// Actionable task (gets an Obsidian Tasks checkbox).
    Task,
    /// Inchoate idea / brainstorm fragment.
    Idea,
    /// Open question awaiting answer.
    Question,
    /// Recorded decision / rationale.
    Decision,
}

impl EntryType {
    fn as_wire(self) -> &'static str {
        match self {
            Self::Note => "note",
            Self::Task => "task",
            Self::Idea => "idea",
            Self::Question => "question",
            Self::Decision => "decision",
        }
    }
}

/// Task status (drives the Obsidian Tasks checkbox glyph).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Not started yet.
    Todo,
    /// In progress.
    Doing,
    /// Completed.
    Done,
}

impl Status {
    fn as_wire(self) -> &'static str {
        match self {
            Self::Todo => "todo",
            Self::Doing => "doing",
            Self::Done => "done",
        }
    }

    /// Obsidian Tasks checkbox glyph for this status.
    ///
    /// The `[ ]` / `[x]` pair is core Markdown; `[/]` is an Obsidian
    /// Tasks plugin extension for "in progress". We commit to the
    /// plugin convention here because that's what makes the vault
    /// renderable as a kanban board out of the box (the 1E.7 seed
    /// `Kanban - Tasks.md` will rely on these glyphs).
    fn checkbox_glyph(self) -> &'static str {
        match self {
            Self::Todo => " ",
            Self::Doing => "/",
            Self::Done => "x",
        }
    }
}

// ------------------------------------------------------------
// Public API
// ------------------------------------------------------------

/// Compute the deterministic filename for an entry.
///
/// Shape: `<YYYY-MM-DD>-<slug>__<id8>.md`, matching the contract
/// regex `^\d{4}-\d{2}-\d{2}-[a-z0-9-]+__[a-z0-9]{8}\.md$`.
pub fn filename_for(entry: &KgEntry) -> String {
    let date = entry.captured_at_local_date.format("%Y-%m-%d");
    let slug = slugify_title(&entry.title);
    let id8 = id8_suffix(&entry.id);
    format!("{date}-{slug}__{id8}.md")
}

/// Serialize an entry to canonical Markdown bytes. See the
/// module-level docs for the canonical-form contract.
///
/// Empty-body invariant: when `entry.body` is empty (or whitespace-
/// only after trimming) AND there is no Tasks checkbox section, the
/// output ends with the frontmatter's closing `---\n` and no trailing
/// blank line. That keeps the file's last byte a single `\n` (the
/// fence's terminator) and the byte-level contract -- "exactly one
/// trailing newline" -- unambiguous regardless of body content.
pub fn serialize_entry(entry: &KgEntry) -> String {
    let mut out = String::with_capacity(entry.body.len() + 512);
    write_frontmatter(&mut out, entry);

    // Body normalization happens UP FRONT so we can decide whether
    // to emit a body section at all. CRs are stripped (LF-only is
    // part of the contract); trailing whitespace + newlines are
    // trimmed so the final `\n` we append is the file's only
    // trailing newline.
    let body_normalized = entry.body.replace('\r', "");
    let body = body_normalized.trim_end_matches(['\n', ' ', '\t']);

    let has_checkbox = entry.status.is_some();
    let has_body = !body.is_empty();

    if has_checkbox {
        out.push('\n'); // blank line between frontmatter and checkbox
        write_tasks_checkbox(&mut out, entry);
    }
    if has_body {
        out.push('\n'); // blank line before body section
        out.push_str(body);
        out.push('\n');
    }
    out
}

// ------------------------------------------------------------
// Slug + id8 helpers
// ------------------------------------------------------------

/// Slugify a title for use as a filename component.
///
/// Rules (test-pinned):
///
/// - Lowercase ASCII alphanumeric chars pass through; everything
///   else (including all non-ASCII -- no transliteration) becomes
///   `-`. Identity recovery lives in the `__<id8>` suffix, so we
///   don't owe the user a perfect human-readable slug.
/// - Consecutive `-` collapse to a single `-`.
/// - Leading + trailing `-` are trimmed.
/// - Cap at [`SLUG_MAX_LEN`] chars; if truncation leaves a trailing
///   `-`, trim that too.
/// - An empty result (e.g. all-symbols title, empty title) becomes
///   the literal string `"untitled"`.
pub(crate) fn slugify_title(title: &str) -> String {
    let mut out = String::with_capacity(title.len().min(SLUG_MAX_LEN));
    let mut prev_was_hyphen = false;
    for c in title.chars() {
        let mapped = if c.is_ascii_alphanumeric() {
            c.to_ascii_lowercase()
        } else {
            '-'
        };
        if mapped == '-' {
            if prev_was_hyphen {
                continue;
            }
            prev_was_hyphen = true;
        } else {
            prev_was_hyphen = false;
        }
        out.push(mapped);
    }
    // Trim leading/trailing hyphens.
    let trimmed = out.trim_matches('-');
    // Cap length.
    let capped: String = if trimmed.len() > SLUG_MAX_LEN {
        trimmed.chars().take(SLUG_MAX_LEN).collect()
    } else {
        trimmed.to_string()
    };
    // A truncation can leave a trailing `-`; trim again.
    let final_slug = capped.trim_end_matches('-').to_string();
    if final_slug.is_empty() {
        "untitled".to_string()
    } else {
        final_slug
    }
}

/// Compute the 8-char identity suffix for the filename.
///
/// Takes the first 8 ASCII-alphanumeric chars of `id` (skipping
/// hyphens and other separators so a hyphenated UUID still yields a
/// dense 8-char tail), lowercases them, and zero-pads if the id has
/// fewer than 8 such chars.
fn id8_suffix(id: &str) -> String {
    let mut out = String::with_capacity(8);
    for c in id.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            if out.len() == 8 {
                break;
            }
        }
    }
    while out.len() < 8 {
        out.push('0');
    }
    out
}

// ------------------------------------------------------------
// Frontmatter writer
// ------------------------------------------------------------

fn write_frontmatter(out: &mut String, entry: &KgEntry) {
    out.push_str("---\n");

    // Required fields, in canonical order.
    write_kv_quoted(out, "id", &entry.id);
    let _ = writeln!(out, "schema_version: {SCHEMA_VERSION}");
    write_kv_quoted(out, "capture_kind", entry.capture_kind.as_wire());
    write_kv_quoted(out, "captured_at", &format_rfc3339_z(entry.captured_at));
    write_kv_quoted(out, "title", &entry.title);
    write_kv_quoted(out, "category", entry.category.as_wire());
    write_kv_quoted(out, "type", entry.entry_type.as_wire());

    // Conditional fields, omitted when None.
    if let Some(status) = entry.status {
        write_kv_quoted(out, "status", status.as_wire());
    }
    if let Some(due) = entry.due_date {
        write_kv_quoted(out, "due_date", &format_rfc3339_z(due));
    }

    write_string_list(out, "tags", &entry.tags);
    write_string_list(out, "entities", &entry.entities);

    if let Some(uuid) = &entry.source_session_uuid {
        write_kv_quoted(out, "source_session_uuid", uuid);
    }

    out.push_str("---\n");
}

fn write_kv_quoted(out: &mut String, key: &str, value: &str) {
    out.push_str(key);
    out.push_str(": ");
    push_quoted_scalar(out, value);
    out.push('\n');
}

fn write_string_list(out: &mut String, key: &str, items: &[String]) {
    if items.is_empty() {
        out.push_str(key);
        out.push_str(": []\n");
        return;
    }
    out.push_str(key);
    out.push_str(":\n");
    for item in items {
        out.push_str("  - ");
        push_quoted_scalar(out, item);
        out.push('\n');
    }
}

/// Push `s` into `out` as a YAML double-quoted scalar.
///
/// Handles all escape requirements YAML 1.2 5.7 imposes inside
/// double-quoted strings. There are two reasons a char might need
/// escaping:
///
/// 1. It has a special meaning that would break the quoted-scalar
///    parse (`\`, `"`, embedded LF/CR/NEL/LS/PS act as line breaks).
/// 2. It's not in YAML's `c-printable` range (the libyaml reader
///    rejects raw bytes for these BEFORE the parser even sees them,
///    with a "control characters are not allowed" error). The
///    `c-printable` range is exactly `#x9 | #xA | #xD | [#x20-#x7E]
///    | #x85 | [#xA0-#xD7FF] | [#xE000-#xFFFD] | [#x10000-#x10FFFF]`,
///    so e.g. U+FFFE/U+FFFF, U+007F (DEL), U+0080..=U+0084, and
///    U+0086..=U+009F all need escaping. Pinned by
///    `prop_frontmatter_is_valid_yaml`.
fn push_quoted_scalar(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            // NEL / LINE SEPARATOR / PARAGRAPH SEPARATOR are in
            // c-printable but act as line breaks inside a quoted
            // scalar -- escape them so the scalar stays one logical
            // line.
            '\u{0085}' => out.push_str("\\N"),
            '\u{2028}' => out.push_str("\\L"),
            '\u{2029}' => out.push_str("\\P"),
            // Anything outside the YAML c-printable set: pick the
            // shortest escape that fits.
            c if !is_yaml_printable(c) => {
                let n = c as u32;
                if n <= 0xFF {
                    let _ = write!(out, "\\x{n:02x}");
                } else if n <= 0xFFFF {
                    let _ = write!(out, "\\u{n:04x}");
                } else {
                    let _ = write!(out, "\\U{n:08x}");
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Returns true iff `c` is in YAML 1.2's `c-printable` character set
/// (the chars that libyaml's reader accepts without escaping).
fn is_yaml_printable(c: char) -> bool {
    let n = c as u32;
    n == 0x09
        || n == 0x0A
        || n == 0x0D
        || (0x20..=0x7E).contains(&n)
        || n == 0x85
        || (0xA0..=0xD7FF).contains(&n)
        || (0xE000..=0xFFFD).contains(&n)
        || (0x10000..=0x10FFFF).contains(&n)
}

fn format_rfc3339_z(ts: DateTime<Utc>) -> String {
    // chrono's default RFC3339 formatter emits `+00:00` for UTC; we
    // pin to the `Z` form for visual brevity and because Obsidian's
    // Dataview parser is happier with `Z` in some configurations.
    ts.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

// ------------------------------------------------------------
// Obsidian Tasks checkbox line
// ------------------------------------------------------------

fn write_tasks_checkbox(out: &mut String, entry: &KgEntry) {
    let status = entry
        .status
        .expect("write_tasks_checkbox called only when status is Some");
    out.push_str("- [");
    out.push_str(status.checkbox_glyph());
    out.push_str("] ");
    // Title goes raw on the checkbox line (Obsidian Tasks parses it
    // as Markdown). We DON'T quote-escape here -- this isn't a YAML
    // scalar -- but we DO collapse any embedded newlines to a single
    // space so the checkbox stays a one-liner.
    for c in entry.title.chars() {
        if c == '\n' || c == '\r' {
            out.push(' ');
        } else {
            out.push(c);
        }
    }
    if let Some(due) = entry.due_date {
        // Obsidian Tasks date glyph + `YYYY-MM-DD`. NOT the full
        // RFC3339 timestamp -- the plugin parses date-only.
        let _ = write!(out, " \u{1F4C5} {}", due.format("%Y-%m-%d"));
    }
    out.push('\n');
}

#[cfg(test)]
#[path = "markdown_serializer_tests.rs"]
mod tests;
