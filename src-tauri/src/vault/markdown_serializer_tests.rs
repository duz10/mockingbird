//! Tests for `markdown_serializer`.
//!
//! Lives in a sibling file (loaded via `#[cfg(test)] #[path]` from
//! the impl) so the impl module stays under the 600-line file cap
//! without having to fragment cohesive test surface (helpers, unit
//! tests, property tests, and golden tests all share fixture
//! constructors — splitting them across multiple files would
//! duplicate the constructors, violating DRY for no real win).

use super::*;
use chrono::TimeZone;

// ────────────────────────────────────────────────────────────
// Fixture helpers
// ────────────────────────────────────────────────────────────

fn ts(y: i32, m: u32, d: u32, hh: u32, mm: u32, ss: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(y, m, d, hh, mm, ss).unwrap()
}

fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

fn minimal_entry() -> KgEntry {
    KgEntry {
        id: "01HMVWAB7C8X9Y0Z1234567890".to_string(),
        captured_at: ts(2026, 6, 15, 14, 32, 1),
        captured_at_local_date: date(2026, 6, 15),
        capture_kind: CaptureKind::Dictation,
        title: "Switch suppliers for brake pads".to_string(),
        category: Category::Professional,
        entry_type: EntryType::Note,
        status: None,
        due_date: None,
        tags: vec![],
        entities: vec![],
        source_session_uuid: None,
        body: "Maple recommended switching from AutoZone to Home Depot for brake pads.\n"
            .to_string(),
    }
}

/// Full-shape entry exercising every optional frontmatter field +
/// the Obsidian Tasks checkbox body line. Renamed conceptually from
/// "task entry" to "status-bearing note" with the ADR 0054 §G vocab
/// realignment: `type` is now the knowledge shape (`note`) and the
/// checkbox lives on its own gate (`status: Some(_)`), so a Note
/// with a Todo status is the new pattern for opt-in task workflows.
fn full_task_entry() -> KgEntry {
    KgEntry {
        id: "01HMTASK00abcdef1234567890".to_string(),
        captured_at: ts(2026, 6, 15, 14, 32, 1),
        captured_at_local_date: date(2026, 6, 15),
        capture_kind: CaptureKind::KgNote,
        title: "Switch suppliers for brake pads".to_string(),
        category: Category::Professional,
        entry_type: EntryType::Note,
        status: Some(Status::Todo),
        due_date: Some(ts(2026, 6, 20, 0, 0, 0)),
        tags: vec!["brake-pads".to_string(), "suppliers".to_string()],
        entities: vec!["Home Depot".to_string(), "AutoZone".to_string()],
        source_session_uuid: Some("550e8400-e29b-41d4-a716-446655440000".to_string()),
        body: "Maple recommended switching to Home Depot.".to_string(),
    }
}

fn doing_task_entry() -> KgEntry {
    let mut e = full_task_entry();
    e.id = "01HMDOING00abcdef1234567890".to_string();
    e.status = Some(Status::Doing);
    e.title = "Refactor injection strategy override table".to_string();
    e.due_date = None;
    e.tags = vec!["refactor".to_string()];
    e.entities = vec![];
    e.body = "Move the per-app override map out of the orchestrator constructor.".to_string();
    e
}

fn done_task_entry() -> KgEntry {
    let mut e = full_task_entry();
    e.id = "01HMDONE00abcdef1234567890".to_string();
    e.status = Some(Status::Done);
    e.title = "Buy milk".to_string();
    e.due_date = Some(ts(2026, 6, 10, 0, 0, 0));
    e.tags = vec!["errand".to_string()];
    e.entities = vec![];
    e.source_session_uuid = None;
    e.capture_kind = CaptureKind::KgNoteText;
    e.body = "Picked it up on Saturday.".to_string();
    e
}

/// Amendment `mb-08za` fixture: covers the wiki-link entity
/// emission shape explicitly. Includes a slug-collision pair
/// (`"Mockingbird"` + `"mockingbird"` both → `mockingbird`) so the
/// golden + the dedupe property are pinned in one place.
fn wiki_linked_entities_entry() -> KgEntry {
    KgEntry {
        id: "01HMWIKI00abcdef1234567890".to_string(),
        captured_at: ts(2026, 6, 18, 9, 0, 0),
        captured_at_local_date: date(2026, 6, 18),
        capture_kind: CaptureKind::KgNote,
        title: "Team sync about Mockingbird and project status".to_string(),
        category: Category::Professional,
        entry_type: EntryType::Note,
        status: None,
        due_date: None,
        tags: vec!["mockingbird".to_string(), "team-sync".to_string()],
        // Mix: slug-collision pair (Mockingbird + mockingbird) +
        // two distinct entities. Dedupe + wiki-link emission are
        // both exercised by the golden below.
        entities: vec![
            "Mockingbird".to_string(),
            "mockingbird".to_string(),
            "Maple".to_string(),
            "Dustin".to_string(),
        ],
        source_session_uuid: None,
        body: "We discussed Phase 1E and the Obsidian sync work.".to_string(),
    }
}

/// ADR 0054 §G knowledge-shape coverage: a `decision`-typed entry,
/// pinned as a byte-stable golden so the new vocab gets the same
/// round-trip protection the legacy `Note`/`Task`/`Idea` shapes had.
/// One golden is enough -- the per-shape parse smoke lives in
/// `markdown_parser::tests::canonical_knowledge_shapes_round_trip`.
fn knowledge_shape_diversity_entry() -> KgEntry {
    KgEntry {
        id: "01HMSHAPE00abcdef1234567890".to_string(),
        captured_at: ts(2026, 6, 15, 14, 32, 1),
        captured_at_local_date: date(2026, 6, 15),
        capture_kind: CaptureKind::KgNote,
        title: "Decided to standardize on Tailwind v4 across all Mockingbird UI surfaces"
            .to_string(),
        category: Category::Professional,
        entry_type: EntryType::Decision,
        status: None,
        due_date: None,
        tags: vec![
            "tailwind".to_string(),
            "ui-architecture".to_string(),
            "phase-1e".to_string(),
        ],
        entities: vec!["Mockingbird".to_string(), "Tailwind".to_string()],
        source_session_uuid: Some("550e8400-e29b-41d4-a716-446655440000".to_string()),
        body: "Standardizing on Tailwind v4 across every Mockingbird UI surface. \
               Rationale: the v3-vs-v4 split was costing two separate token files \
               and an ESLint-config fork, with no functional payoff. v4's \
               design-token CSS variables compose cleanly with the existing \
               tokens.css. Migration cost: small (lint-only churn on a handful \
               of files); downstream simplification: large.\n"
            .to_string(),
    }
}

fn special_chars_entry() -> KgEntry {
    KgEntry {
        id: "01HMSPECIAL00ab1234567890af".to_string(),
        captured_at: ts(2026, 1, 2, 3, 4, 5),
        captured_at_local_date: date(2026, 1, 2),
        capture_kind: CaptureKind::Dictation,
        // Embeds: backslash, double-quote, newline, tab, unicode.
        title: "Re: \"urgent\"\tnotes & \\paths\nmulti-line — café".to_string(),
        category: Category::Personal,
        // `Idea` was dropped in ADR 0054 §G; the closest knowledge
        // shape for a brainstorm fragment is Observation -- an
        // inchoate noticing-of-a-pattern that the chat-LLM Lint pass
        // can later crystallize into a Concept page.
        entry_type: EntryType::Observation,
        status: None,
        due_date: None,
        tags: vec!["weird-\"tag\"".to_string()],
        entities: vec!["O'Reilly\\".to_string()],
        source_session_uuid: None,
        body: "Body with \"quotes\" and a \\ backslash.".to_string(),
    }
}

// ────────────────────────────────────────────────────────────
// Filename tests
// ────────────────────────────────────────────────────────────

#[test]
fn filename_happy_path() {
    let e = minimal_entry();
    assert_eq!(
        filename_for(&e),
        "2026-06-15-switch-suppliers-for-brake-pads__01hmvwab.md"
    );
}

#[test]
fn filename_empty_title_becomes_untitled() {
    let mut e = minimal_entry();
    e.title = "".to_string();
    assert_eq!(filename_for(&e), "2026-06-15-untitled__01hmvwab.md");
}

#[test]
fn filename_all_symbols_title_becomes_untitled() {
    let mut e = minimal_entry();
    e.title = "??? !!! ###".to_string();
    // After hyphen-mapping + collapse + trim: empty → "untitled".
    assert_eq!(filename_for(&e), "2026-06-15-untitled__01hmvwab.md");
}

#[test]
fn filename_non_ascii_title_drops_to_hyphens() {
    let mut e = minimal_entry();
    // Café, em-dash, CJK, emoji — none are ASCII-alphanum.
    // 'C','a','f' survive; 'é' is NOT ASCII-alphanum so it drops
    // to '-'. Then ' ','—',' ','寿','司',' ','🍣',' ' all drop to
    // '-' and collapse. "plan" survives. Net slug: "caf-plan".
    // (The 'é'-drop is the intentional "no transliteration"
    // choice; identity recovery lives in the `__<id8>` suffix.)
    e.title = "Café — 寿司 🍣 plan".to_string();
    assert_eq!(filename_for(&e), "2026-06-15-caf-plan__01hmvwab.md");
}

#[test]
fn filename_slug_capped_at_50_chars() {
    let mut e = minimal_entry();
    e.title = "a".repeat(120);
    let name = filename_for(&e);
    // Extract the slug part between the date and the "__".
    let slug = name
        .strip_prefix("2026-06-15-")
        .and_then(|s| s.split("__").next())
        .unwrap();
    assert_eq!(slug.len(), 50, "slug must be capped at 50 chars: {slug}");
    assert!(slug.chars().all(|c| c == 'a'));
}

#[test]
fn filename_slug_truncation_strips_trailing_hyphen() {
    let mut e = minimal_entry();
    // Construct: 49 'a' chars + a '!' that maps to '-'. Cap at 50
    // would keep "aaa...a-"; the trailing '-' must be re-trimmed.
    e.title = format!("{}!", "a".repeat(49));
    let name = filename_for(&e);
    let slug = name
        .strip_prefix("2026-06-15-")
        .and_then(|s| s.split("__").next())
        .unwrap();
    assert!(
        !slug.ends_with('-'),
        "post-truncation slug must not end with '-': {slug}"
    );
    assert_eq!(slug.len(), 49);
}

#[test]
fn filename_id8_strips_hyphens_in_uuids() {
    let mut e = minimal_entry();
    e.id = "550e8400-e29b-41d4-a716-446655440000".to_string();
    let name = filename_for(&e);
    // First 8 alphanumeric chars: 550e8400.
    assert!(
        name.contains("__550e8400.md"),
        "id8 must strip hyphens: got {name}"
    );
}

#[test]
fn filename_id8_pads_short_ids() {
    let mut e = minimal_entry();
    e.id = "abc".to_string();
    let name = filename_for(&e);
    assert!(
        name.ends_with("__abc00000.md"),
        "short ids must zero-pad to 8 chars: {name}"
    );
}

#[test]
fn filename_matches_regex() {
    // The contract regex per the kickoff brief.
    let e = minimal_entry();
    let name = filename_for(&e);
    let re = regex::Regex::new(r"^\d{4}-\d{2}-\d{2}-[a-z0-9-]+__[a-z0-9]{8}\.md$").unwrap();
    assert!(
        re.is_match(&name),
        "filename must match contract regex: {name}"
    );
}

// ────────────────────────────────────────────────────────────
// Frontmatter tests
// ────────────────────────────────────────────────────────────

#[test]
fn frontmatter_field_order_pinned() {
    let e = full_task_entry();
    let out = serialize_entry(&e);
    // Find each field's offset; assert strictly-increasing order.
    let order = [
        "id:",
        "schema_version:",
        "capture_kind:",
        "captured_at:",
        "title:",
        "category:",
        "type:",
        "status:",
        "due_date:",
        "tags:",
        "entities:",
        "source_session_uuid:",
    ];
    let mut prev_pos = 0usize;
    for key in order {
        let pos = out
            .find(key)
            .unwrap_or_else(|| panic!("missing key {key} in:\n{out}"));
        assert!(
            pos > prev_pos,
            "field {key} appears out of order (at {pos}, prev {prev_pos}):\n{out}"
        );
        prev_pos = pos;
    }
}

#[test]
fn frontmatter_conditional_fields_omitted_when_none() {
    let e = minimal_entry();
    let out = serialize_entry(&e);
    assert!(!out.contains("status:"), "status must be omitted: {out}");
    assert!(
        !out.contains("due_date:"),
        "due_date must be omitted: {out}"
    );
    assert!(
        !out.contains("source_session_uuid:"),
        "source_session_uuid must be omitted: {out}"
    );
}

#[test]
fn frontmatter_empty_lists_render_inline() {
    let e = minimal_entry();
    let out = serialize_entry(&e);
    assert!(
        out.contains("tags: []\n"),
        "empty tags must render as `tags: []`: {out}"
    );
    assert!(
        out.contains("entities: []\n"),
        "empty entities must render as `entities: []`: {out}"
    );
}

#[test]
fn frontmatter_lists_use_block_style() {
    let e = full_task_entry();
    let out = serialize_entry(&e);
    assert!(
        out.contains("tags:\n  - \"brake-pads\"\n  - \"suppliers\"\n"),
        "tags must use block style with quoted items: {out}"
    );
}

// ──────────────────────────────────────────────
// Entity wiki-link emission tests (amendment mb-08za)
// ──────────────────────────────────────────────

/// Every entity in a non-empty list emits as a quoted wiki-link
/// matching `"[[Entities/<slug>|<slug>]]"` (Obsidian pipe-alias
/// form — 1E.5 polish). Pinned at the golden site too, but called
/// out as a standalone assertion so a future refactor that breaks
/// the shape surfaces here with a clear message.
#[test]
fn entities_emit_as_wiki_links() {
    let e = full_task_entry();
    let out = serialize_entry(&e);
    assert!(
        out.contains("entities:\n  - \"[[Entities/home-depot|home-depot]]\"\n  - \"[[Entities/autozone|autozone]]\"\n"),
        "entities must emit as quoted pipe-aliased wiki-link block list: {out}"
    );
}

/// Slug collisions (distinct input strings that slugify identically)
/// collapse to a single wiki-link, first-occurrence wins for order.
#[test]
fn entities_deduped_by_slug_at_serialize_time() {
    let mut e = minimal_entry();
    e.entities = vec![
        "Mockingbird".to_string(),
        "mockingbird".to_string(),
        "MockingBird".to_string(),
        "Maple".to_string(),
    ];
    let out = serialize_entry(&e);
    let entities_block = out
        .split("entities:\n")
        .nth(1)
        .and_then(|s| s.split("\n---\n").next())
        .expect("entities block present");
    let mockingbird_lines = entities_block
        .matches("[[Entities/mockingbird|mockingbird]]")
        .count();
    assert_eq!(
        mockingbird_lines, 1,
        "slug-colliding entities must dedupe to one line: {entities_block}"
    );
    assert!(
        entities_block.contains("[[Entities/maple|maple]]"),
        "distinct entities must survive dedupe: {entities_block}"
    );
}

/// Empty entity list still emits as flow-form `entities: []` (same
/// discipline as `tags: []`) so the reverse-watcher can distinguish
/// "no entities" from "field absent".
#[test]
fn empty_entities_render_inline_after_amendment() {
    let e = minimal_entry();
    let out = serialize_entry(&e);
    assert!(
        out.contains("entities: []\n"),
        "empty entities must still render as flow-form: {out}"
    );
}

/// Wiki-links pass through `push_quoted_scalar` so the YAML parser
/// still sees a single double-quoted scalar with no escape
/// surprises. Pinned as a sanity check; the property test below
/// is the broad guarantee.
#[test]
fn entity_wiki_links_are_valid_yaml_scalars() {
    let e = wiki_linked_entities_entry();
    let out = serialize_entry(&e);
    let fm = extract_frontmatter(&out);
    let parsed: serde_yaml::Value = serde_yaml::from_str(fm).expect("valid YAML");
    let entities_val = parsed
        .as_mapping()
        .and_then(|m| m.get(serde_yaml::Value::String("entities".into())))
        .expect("entities key present");
    let entities_seq = entities_val.as_sequence().expect("entities is a sequence");
    assert_eq!(
        entities_seq.len(),
        3,
        "slug collisions dedupe Mockingbird+mockingbird → 1 entry"
    );
    for v in entities_seq {
        let s = v.as_str().expect("entity is a string");
        assert!(
            s.starts_with("[[Entities/") && s.ends_with("]]"),
            "entity must be a wiki-link: {s}"
        );
    }
}

#[test]
fn frontmatter_strings_always_quoted() {
    let e = full_task_entry();
    let out = serialize_entry(&e);
    // Sample: title should be wrapped in double-quotes.
    assert!(
        out.contains("title: \"Switch suppliers for brake pads\"\n"),
        "title must be double-quoted: {out}"
    );
    // schema_version is an integer — NOT quoted.
    assert!(
        out.contains("schema_version: 1\n"),
        "schema_version must be unquoted: {out}"
    );
}

#[test]
fn frontmatter_escapes_special_chars_in_strings() {
    let e = special_chars_entry();
    let out = serialize_entry(&e);
    // Title contains: " \t \\ \n
    let title_line = out
        .lines()
        .find(|line| line.starts_with("title:"))
        .expect("title line present");
    assert!(
        title_line.contains("\\\""),
        "double-quote must be escaped in title: {title_line}"
    );
    assert!(
        title_line.contains("\\\\"),
        "backslash must be escaped in title: {title_line}"
    );
    assert!(
        title_line.contains("\\n"),
        "newline must be escaped in title: {title_line}"
    );
    assert!(
        title_line.contains("\\t"),
        "tab must be escaped in title: {title_line}"
    );
    // Title must occupy exactly one source line.
    let occurrences = out.matches("\ntitle:").count();
    assert_eq!(
        occurrences, 1,
        "title must be on exactly one line after escaping; raw newlines must NOT split it"
    );
}

#[test]
fn frontmatter_uses_lf_line_endings_on_all_platforms() {
    let e = full_task_entry();
    let out = serialize_entry(&e);
    assert!(
        !out.contains('\r'),
        "output must not contain CR bytes (LF only): {out:?}"
    );
}

#[test]
fn frontmatter_uses_rfc3339_utc_with_z_suffix() {
    let e = full_task_entry();
    let out = serialize_entry(&e);
    assert!(
        out.contains("captured_at: \"2026-06-15T14:32:01Z\""),
        "captured_at must be RFC3339-Z: {out}"
    );
    assert!(
        out.contains("due_date: \"2026-06-20T00:00:00Z\""),
        "due_date must be RFC3339-Z: {out}"
    );
}

#[test]
fn frontmatter_uses_type_not_entry_type_on_wire() {
    // Wire-name discipline: the field is `type:` in YAML even
    // though the struct field is `entry_type` (avoiding the Rust
    // keyword collision).
    let e = full_task_entry();
    let out = serialize_entry(&e);
    assert!(
        out.contains("\ntype: \"task\"\n"),
        "must emit `type:`, not `entry_type:`: {out}"
    );
    assert!(
        !out.contains("entry_type:"),
        "must NOT emit `entry_type:`: {out}"
    );
}

// ────────────────────────────────────────────────────────────
// Body tests
// ────────────────────────────────────────────────────────────

#[test]
fn body_no_checkbox_when_status_absent() {
    let e = minimal_entry();
    let out = serialize_entry(&e);
    // The frontmatter ends at "---\n"; the next non-empty line
    // should be the body, not a checkbox.
    let after_fm = out.split("---\n").nth(2).expect("post-frontmatter body");
    assert!(
        !after_fm.trim_start().starts_with("- ["),
        "non-task entry must not start with a checkbox: {after_fm}"
    );
}

#[test]
fn body_emits_checkbox_for_todo() {
    let mut e = full_task_entry();
    e.due_date = None;
    let out = serialize_entry(&e);
    assert!(
        out.contains("- [ ] Switch suppliers for brake pads\n"),
        "todo must render as `- [ ] {{title}}`: {out}"
    );
}

#[test]
fn body_emits_checkbox_for_doing() {
    let e = doing_task_entry();
    let out = serialize_entry(&e);
    assert!(
        out.contains("- [/] Refactor injection strategy override table\n"),
        "doing must render as `- [/] {{title}}`: {out}"
    );
}

#[test]
fn body_emits_checkbox_for_done() {
    let e = done_task_entry();
    let out = serialize_entry(&e);
    assert!(
        out.contains("- [x] Buy milk \u{1F4C5} 2026-06-10\n"),
        "done must render as `- [x] {{title}} 📅 {{date}}`: {out}"
    );
}

#[test]
fn body_checkbox_includes_due_date_in_yyyy_mm_dd() {
    let e = full_task_entry();
    let out = serialize_entry(&e);
    assert!(
        out.contains("\u{1F4C5} 2026-06-20"),
        "checkbox must include `📅 YYYY-MM-DD`: {out}"
    );
    // Must NOT include the full timestamp on the checkbox line.
    let checkbox_line = out
        .lines()
        .find(|line| line.starts_with("- ["))
        .expect("checkbox present");
    assert!(
        !checkbox_line.contains('T'),
        "checkbox line must not contain timestamp 'T' separator: {checkbox_line}"
    );
}

#[test]
fn body_ends_with_exactly_one_trailing_newline() {
    let e = full_task_entry();
    let out = serialize_entry(&e);
    assert!(out.ends_with('\n'), "output must end with newline");
    assert!(
        !out.ends_with("\n\n"),
        "output must end with exactly ONE newline: {out:?}"
    );
}

#[test]
fn body_with_trailing_whitespace_normalizes() {
    let mut e = minimal_entry();
    e.body = "trimmed body\n\n\n   \t\n".to_string();
    let out = serialize_entry(&e);
    assert!(
        out.ends_with("trimmed body\n"),
        "body must normalize: {out}"
    );
}

/// Regression for `mb-wzui`: a multi-bullet body must round-trip
/// every bullet line verbatim. The bug surfaced upstream (worker
/// was passing `entries[0].body` = segment[0] of the segmenter)
/// but the serializer is the last hop before disk, so pinning the
/// invariant here too means any future refactor that re-introduces
/// body mangling (e.g. "smart" markdown rewriting) trips the test.
#[test]
fn body_preserves_markdown_bullet_list_verbatim() {
    let mut e = minimal_entry();
    e.body = "Need to make a quick grocery list. I need to get:\n\
              - feta cheese\n\
              - eggs\n\
              - milk"
        .to_string();
    let out = serialize_entry(&e);
    assert!(out.contains("- feta cheese\n"), "missing feta: {out}");
    assert!(out.contains("- eggs\n"), "missing eggs: {out}");
    assert!(
        out.contains("- milk"),
        "missing milk (final bullet, no trailing \\n required): {out}"
    );
    // And the preamble survives too.
    assert!(
        out.contains("Need to make a quick grocery list. I need to get:\n"),
        "missing preamble: {out}"
    );
}

// ────────────────────────────────────────────────────────────
// Frontmatter YAML parses cleanly (sanity check for the
// future parser at 1E.5)
// ────────────────────────────────────────────────────────────

/// The hand-rolled emitter still needs to produce valid YAML —
/// the reverse-watcher (1E.5) will use a real YAML parser. This
/// test pulls the frontmatter out of each fixture and round-trips
/// it through `serde_yaml::from_str::<serde_yaml::Value>` to
/// catch any quoting/escaping bugs early.
#[test]
fn frontmatter_is_valid_yaml() {
    for entry in [
        minimal_entry(),
        full_task_entry(),
        doing_task_entry(),
        done_task_entry(),
        special_chars_entry(),
        wiki_linked_entities_entry(),
    ] {
        let out = serialize_entry(&entry);
        let fm = extract_frontmatter(&out);
        let parsed: serde_yaml::Value = serde_yaml::from_str(fm)
            .unwrap_or_else(|e| panic!("invalid YAML for entry {}:\n{fm}\n\nerror: {e}", entry.id));
        // Sanity: required fields are present and types are sane.
        let map = parsed.as_mapping().expect("frontmatter is a mapping");
        assert!(map.contains_key("id"));
        assert!(map.contains_key("schema_version"));
        assert!(map.contains_key("capture_kind"));
        assert!(map.contains_key("captured_at"));
        assert!(map.contains_key("title"));
        assert!(map.contains_key("tags"));
        assert!(map.contains_key("entities"));
    }
}

fn extract_frontmatter(serialized: &str) -> &str {
    let after_open = serialized
        .strip_prefix("---\n")
        .expect("frontmatter open fence");
    let close_idx = after_open.find("\n---\n").expect("frontmatter close fence");
    &after_open[..close_idx]
}

// ────────────────────────────────────────────────────────────
// Property tests (proptest)
// ────────────────────────────────────────────────────────────

use proptest::prelude::*;

fn arb_entry() -> impl Strategy<Value = KgEntry> {
    let title = prop::collection::vec(prop::char::any(), 0..100)
        .prop_map(|chars| chars.into_iter().collect::<String>());
    let id = prop::string::string_regex("[A-Za-z0-9-]{4,40}").unwrap();
    let body = prop::collection::vec(prop::char::any(), 0..200)
        .prop_map(|chars| chars.into_iter().collect::<String>());
    let tags = prop::collection::vec("[a-z][a-z0-9-]{0,20}", 0..5)
        .prop_map(|v| v.into_iter().map(String::from).collect::<Vec<_>>());
    let entities = prop::collection::vec("[A-Za-z][A-Za-z0-9 -]{0,30}", 0..5)
        .prop_map(|v| v.into_iter().map(String::from).collect::<Vec<_>>());
    let day = 1u32..28;
    let month = 1u32..13;
    let year = 2000i32..2099;
    (
        id,
        title,
        body,
        tags,
        entities,
        year,
        month,
        day,
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(
            |(id, title, body, tags, entities, y, m, d, has_status, has_due, has_uuid)| KgEntry {
                id,
                captured_at: ts(y, m, d, 12, 0, 0),
                captured_at_local_date: date(y, m, d),
                capture_kind: CaptureKind::Dictation,
                title,
                category: Category::Personal,
                // Status emission is now decoupled from `type:` (ADR
                // 0054 §L). Pick Note unconditionally; let `has_status`
                // drive the optional Status field independently.
                entry_type: EntryType::Note,
                status: if has_status { Some(Status::Todo) } else { None },
                due_date: if has_due {
                    Some(ts(y, m, d, 23, 59, 59))
                } else {
                    None
                },
                tags,
                entities,
                source_session_uuid: if has_uuid {
                    Some("550e8400-e29b-41d4-a716-446655440000".to_string())
                } else {
                    None
                },
                body,
            },
        )
}

proptest! {
    #[test]
    fn prop_filename_matches_contract_regex(entry in arb_entry()) {
        let name = filename_for(&entry);
        let re = regex::Regex::new(r"^\d{4}-\d{2}-\d{2}-[a-z0-9-]+__[a-z0-9]{8}\.md$").unwrap();
        prop_assert!(re.is_match(&name), "filename violates regex: {name}");
    }

    #[test]
    fn prop_frontmatter_is_valid_yaml(entry in arb_entry()) {
        let out = serialize_entry(&entry);
        let fm = extract_frontmatter(&out);
        let parsed: Result<serde_yaml::Value, _> = serde_yaml::from_str(fm);
        prop_assert!(parsed.is_ok(), "invalid YAML: {fm}\n\nerror: {parsed:?}");
    }

    #[test]
    fn prop_output_uses_lf_only(entry in arb_entry()) {
        let out = serialize_entry(&entry);
        prop_assert!(!out.contains('\r'), "CR found in output");
    }

    #[test]
    fn prop_output_ends_with_single_newline(entry in arb_entry()) {
        let out = serialize_entry(&entry);
        prop_assert!(out.ends_with('\n'));
        prop_assert!(!out.ends_with("\n\n"));
    }

    #[test]
    fn prop_slug_is_lowercase_alphanum_or_hyphen(title in ".{0,80}") {
        let slug = slugify_title(&title);
        prop_assert!(!slug.is_empty(), "slug must never be empty");
        prop_assert!(
            slug.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
            "slug must be lowercase-alphanum-or-hyphen: {slug}"
        );
        prop_assert!(!slug.starts_with('-'), "slug must not start with '-': {slug}");
        prop_assert!(!slug.ends_with('-'), "slug must not end with '-': {slug}");
        prop_assert!(slug.len() <= SLUG_MAX_LEN, "slug must be <= {SLUG_MAX_LEN}: {slug}");
    }

    /// Amendment `mb-08za` + 1E.5 polish: every emitted entity
    /// scalar matches the `[[Entities/<slug>|<slug>]]` pipe-alias
    /// shape with the same slug alphabet as the filename slug, and
    /// the alias text equals the slug. The regex is the formal
    /// contract the reverse-watcher's parser keys on.
    #[test]
    fn prop_entities_emit_as_wiki_links(entry in arb_entry()) {
        let out = serialize_entry(&entry);
        let fm = extract_frontmatter(&out);
        let parsed: serde_yaml::Value = serde_yaml::from_str(fm)
            .map_err(|e| TestCaseError::fail(format!("YAML parse failed: {e}")))?;
        let entities = parsed
            .as_mapping()
            .and_then(|m| m.get(serde_yaml::Value::String("entities".into())))
            .ok_or_else(|| TestCaseError::fail("entities key missing".to_string()))?;
        // Empty entities renders as flow-form `[]` which still
        // parses as a sequence — the for-loop below is a no-op in
        // that case, which is the desired behaviour.
        let seq = entities
            .as_sequence()
            .ok_or_else(|| TestCaseError::fail("entities must be a sequence".to_string()))?;
        // Pipe-alias form: `[[Entities/<slug>|<alias>]]`. The slug
        // and alias are both ASCII kebab-case; we additionally
        // assert (via captures, below) that slug == alias for our
        // serializer's output. A reader that encounters a different
        // alias (e.g. user-edited Obsidian rename) is still valid
        // input for the parser — the contract is one-way (writer
        // emits identity-alias; parser tolerates any alias).
        let re = regex::Regex::new(
            r"^\[\[Entities/([a-z0-9-]+)\|([a-z0-9-]+)\]\]$",
        )
        .unwrap();
        for v in seq {
            let s = v.as_str().ok_or_else(|| {
                TestCaseError::fail(format!("entity item must be a string: {v:?}"))
            })?;
            let caps = re.captures(s).ok_or_else(|| {
                TestCaseError::fail(format!(
                    "entity wiki-link violates regex `{}`: {s}",
                    re.as_str()
                ))
            })?;
            prop_assert_eq!(
                &caps[1],
                &caps[2],
                "serializer emits identity-alias (slug == alias): {}",
                s
            );
        }
    }

    /// Slug-collision dedupe is total: the emitted entity count is
    /// at most the number of unique slugs derived from the input
    /// (and the slug alphabet is closed-form ASCII kebab-case).
    #[test]
    fn prop_entities_dedupe_by_slug(entry in arb_entry()) {
        let unique_slugs: std::collections::BTreeSet<String> = entry
            .entities
            .iter()
            .map(|s| slugify_title(s))
            .collect();
        let out = serialize_entry(&entry);
        let fm = extract_frontmatter(&out);
        let parsed: serde_yaml::Value = serde_yaml::from_str(fm)
            .map_err(|e| TestCaseError::fail(format!("YAML parse failed: {e}")))?;
        let seq_len = parsed
            .as_mapping()
            .and_then(|m| m.get(serde_yaml::Value::String("entities".into())))
            .and_then(|v| v.as_sequence())
            .map(|s| s.len())
            .unwrap_or(0);
        prop_assert_eq!(
            seq_len,
            unique_slugs.len(),
            "emitted entity count must equal unique-slug count"
        );
    }
}

// ────────────────────────────────────────────────────────────
// Golden-file tests
//
// The five fixtures cover the documented matrix from the kickoff
// brief: minimal (only required fields), full (all conditional
// fields), task-with-checkbox variants for each status glyph, and
// a special-chars stress fixture for the escaping rules. Files
// live under `src-tauri/tests/fixtures/markdown_golden/` for
// human-reviewable diffs; the test below `include_str!`s each so
// a stale fixture surfaces as a clean assertion failure at the
// diff site, not a "file not found" at run time.
//
// To intentionally update a golden after a deliberate canonical-
// form change: set `MOCKINGBIRD_UPDATE_GOLDENS=1` in the env and
// re-run; the test will rewrite the on-disk fixtures and fail
// with a one-line "fixtures updated, re-run" message so the human
// notices and reviews the diff.
// ────────────────────────────────────────────────────────────

fn golden_for(name: &str) -> &'static str {
    match name {
        "minimal" => include_str!("../../tests/fixtures/markdown_golden/minimal.md"),
        "full_task" => include_str!("../../tests/fixtures/markdown_golden/full_task.md"),
        "doing_task" => include_str!("../../tests/fixtures/markdown_golden/doing_task.md"),
        "done_task" => include_str!("../../tests/fixtures/markdown_golden/done_task.md"),
        "special_chars" => include_str!("../../tests/fixtures/markdown_golden/special_chars.md"),
        "wiki_linked_entities" => {
            include_str!("../../tests/fixtures/markdown_golden/wiki_linked_entities.md")
        }
        "knowledge_shape_diversity" => {
            include_str!("../../tests/fixtures/markdown_golden/knowledge_shape_diversity.md")
        }
        other => panic!("unknown golden fixture: {other}"),
    }
}

fn assert_golden(name: &str, entry: &KgEntry) {
    let actual = serialize_entry(entry);
    if std::env::var("MOCKINGBIRD_UPDATE_GOLDENS").is_ok() {
        // Resolve via `file!()` rather than `CARGO_MANIFEST_DIR`
        // so the throwaway-test-crate harness (LESSONS P2) writes
        // back to the REAL `src-tauri/tests/...` fixtures, not
        // the throwaway crate's own (empty) tests dir.
        // `file!()` returns the source-file path as the compiler
        // saw it — with `#[path]` attrs pointing at absolute
        // paths, this is the real on-disk location.
        let here = std::path::PathBuf::from(file!());
        let path = here
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
            .expect("file!() yields a deep-enough path")
            .join("tests")
            .join("fixtures")
            .join("markdown_golden")
            .join(format!("{name}.md"));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &actual).unwrap();
        panic!(
            "MOCKINGBIRD_UPDATE_GOLDENS=1 -- wrote {}; re-run without the env var",
            path.display()
        );
    }
    let expected = golden_for(name);
    if actual != expected {
        panic!(
            "golden mismatch for `{name}`.\n\
             --- expected ---\n{expected}\n\
             --- actual ---\n{actual}\n\
             --- byte diff ---\nexpected.len()={}, actual.len()={}",
            expected.len(),
            actual.len()
        );
    }
}

#[test]
fn golden_minimal() {
    assert_golden("minimal", &minimal_entry());
}

#[test]
fn golden_full_task() {
    assert_golden("full_task", &full_task_entry());
}

#[test]
fn golden_doing_task() {
    assert_golden("doing_task", &doing_task_entry());
}

#[test]
fn golden_done_task() {
    assert_golden("done_task", &done_task_entry());
}

#[test]
fn golden_special_chars() {
    assert_golden("special_chars", &special_chars_entry());
}

/// Amendment `mb-08za`: pins the wiki-link entity emission shape
/// (including slug-collision dedupe) as a byte-stable golden.
#[test]
fn golden_wiki_linked_entities() {
    assert_golden("wiki_linked_entities", &wiki_linked_entities_entry());
}

/// ADR 0054 §G: pins the `decision` knowledge-shape as a byte-stable
/// golden so the new vocab has a guarded round-trip example, not just
/// hand-built test strings. Same fixture is also referenced by the
/// parser's `roundtrip_all_seven_goldens` test (one source of truth
/// on disk; consumed from both sides).
#[test]
fn golden_knowledge_shape_diversity() {
    assert_golden(
        "knowledge_shape_diversity",
        &knowledge_shape_diversity_entry(),
    );
}

/// Filenames for every golden fixture must also match the
/// contract regex — pinned alongside the body goldens so any
/// future filename-shape refactor surfaces at the same site.
#[test]
fn golden_filenames_match_contract() {
    let cases: &[(&str, &dyn Fn() -> KgEntry)] = &[
        ("minimal", &(minimal_entry as fn() -> KgEntry)),
        ("full_task", &(full_task_entry as fn() -> KgEntry)),
        ("doing_task", &(doing_task_entry as fn() -> KgEntry)),
        ("done_task", &(done_task_entry as fn() -> KgEntry)),
        ("special_chars", &(special_chars_entry as fn() -> KgEntry)),
        (
            "wiki_linked_entities",
            &(wiki_linked_entities_entry as fn() -> KgEntry),
        ),
        (
            "knowledge_shape_diversity",
            &(knowledge_shape_diversity_entry as fn() -> KgEntry),
        ),
    ];
    let re = regex::Regex::new(r"^\d{4}-\d{2}-\d{2}-[a-z0-9-]+__[a-z0-9]{8}\.md$").unwrap();
    for (label, ctor) in cases {
        let entry = ctor();
        let name = filename_for(&entry);
        assert!(
            re.is_match(&name),
            "filename for `{label}` violates regex: {name}"
        );
    }
}
