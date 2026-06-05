//! Pure-Rust UIA payload assembly + size cap.
//!
//! This module is deliberately platform-agnostic — it knows nothing
//! about COM, HWND, or Windows headers. The Windows side
//! (`activity::uia::windows_com`) constructs a [`ProbeResult`] from
//! IUIAutomation calls; this module turns that DTO into the JSON
//! string we persist in `activity_events.snapshot_json`.
//!
//! ## Why split?
//!
//! - **Testability.** The size-cap + truncation logic is the part most
//!   likely to bite us in production (3000-element page DOMs are not
//!   theoretical). It needs ≥20 unit tests, and unit-testing pure
//!   JSON math through a `cargo test --release --no-run` gate is
//!   miserable (LESSONS P2). Pure module → throwaway-crate live tests.
//! - **Cross-platform discipline (Principle 5).** Phase 9 macOS will
//!   feed AX API readings into the *same* [`ProbeResult`] shape and
//!   reuse this module verbatim.
//!
//! ## Schema version
//!
//! `snapshot_json` carries a `"schema"` discriminator. Wave 1B's
//! payload was implicit-v1 (`{"app":"...","title":"..."}`); Wave 2's
//! richer payload is `"v2"`. The detail UI uses the discriminator
//! to switch render paths. Older sessions keep rendering with the
//! flat fields the schema-less reader infers.
//!
//! ## Size cap
//!
//! The cap is 32 KB per event (`MAX_PAYLOAD_BYTES`). Visible-text
//! fragments are truncated first (longest tail dropped); if the
//! resulting payload still exceeds the cap the focused-field value
//! is truncated; if still over, `control_summary` survives alone.
//! `app`, `title`, `monitor`, `password_field_active`, and `uia_status`
//! are NEVER truncated — they're tiny and load-bearing.

#![allow(missing_docs)]

use serde::{Deserialize, Serialize};

/// Per-event payload size cap. Empirical floor: a deeply-nested
/// page DOM walk surfaces a few hundred text fragments, well inside
/// 32 KB once truncation hits. Wave 2 brief documents the rationale.
pub const MAX_PAYLOAD_BYTES: usize = 32 * 1024;

/// Soft cap on the count of visible-text fragments. Below this we
/// keep all fragments; above this we drop until under the byte cap.
pub const MAX_TEXT_FRAGMENTS: usize = 256;

/// Per-fragment character cap (post-NFC). 512 chars is enough for
/// any sane single label / span / paragraph snippet. Longer fragments
/// are truncated with a single `…` sentinel.
pub const MAX_FRAGMENT_CHARS: usize = 512;

/// The DTO the Windows COM impl populates. Pure data — `Clone`
/// + `Serialize` for free testing.
// `Eq` is intentionally NOT derived because `MonitorInfo.dpi_scale: Option<f32>`
// rules it out; `PartialEq` is enough for the tests we have.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProbeResult {
    /// Snapshotted at the same tick as the UIA query. Always present
    /// (even if UIA failed) so the timeline has a stable label.
    pub app: String,
    pub title: String,

    /// Per-window monitor attribution. None if the lookup failed
    /// (rare — typically protected windows on a disconnected display).
    pub monitor: Option<MonitorInfo>,

    /// The control with keyboard focus, when discoverable.
    pub focused_field: Option<FocusedField>,

    /// Visible text fragments harvested from the foreground window's
    /// UIA tree. Each fragment is one control's "what the screen reader
    /// would announce" — name, value, or text-pattern content.
    pub visible_text_fragments: Vec<String>,

    /// Cheap aggregate counts. The detail UI uses these to estimate
    /// page complexity without re-walking the (truncated) text array.
    pub control_summary: ControlSummary,

    /// True iff the focused element exposes `IsPassword = true` (UIA
    /// property id `UIA_IsPasswordPropertyId`). When this is true
    /// the COM impl MUST emit an empty `focused_field.value` AND empty
    /// `visible_text_fragments` (Principle 8 / Wave 2 Q5 redaction).
    pub password_field_active: bool,

    /// The reason the snapshot is the way it is. `Ok` when the UIA
    /// walk succeeded fully; `Failed(reason)` for protected processes,
    /// game windows with no UIA tree, COM init failure, etc.
    pub status: ProbeStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MonitorInfo {
    /// E.g. `\\\\.\\DISPLAY1`. From `GetMonitorInfoW.szDevice`.
    pub name: String,
    /// `is_primary` is `true` when `MONITORINFOF_PRIMARY` is set on
    /// the monitor. The system can only ever have one primary monitor.
    pub is_primary: bool,
    pub bounds: Rect,
    /// DPI scale relative to 96 dpi (e.g. `1.5` for 144 dpi).
    /// `None` when GetDpiForWindow is unavailable (unlikely on Win10+).
    pub dpi_scale: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FocusedField {
    /// UIA `Name` property — e.g. `"Username"`, `"Search Google or type a URL"`.
    pub name: String,
    /// UIA `LocalizedControlType` — `"edit"`, `"button"`, `"document"`, etc.
    pub control_type: String,
    /// Best-effort textual value. From either ValuePattern.Value or
    /// TextPattern.DocumentRange.GetText(N). Truncated to
    /// `MAX_FRAGMENT_CHARS`. NEVER populated for password fields.
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ControlSummary {
    pub edit_count: u32,
    pub button_count: u32,
    pub document_count: u32,
    pub link_count: u32,
    pub text_count: u32,
    /// Catch-all for any other control type encountered in the walk.
    pub other_count: u32,
    /// Total elements visited by the walker, including those whose
    /// control type fell into `other_count`. Used to detect "we
    /// truncated the walk" without revealing tree shape.
    pub elements_visited: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case", tag = "kind", content = "reason")]
pub enum ProbeStatus {
    /// Probe ran cleanly; payload is full.
    Ok,
    /// Probe ran but found nothing useful (e.g. fullscreen game with
    /// no UIA tree). Payload is sparse but valid. This is the default
    /// because [`ProbeResult::default`] should not lie about a probe
    /// having succeeded.
    #[default]
    NoPayload,
    /// Probe failed at some point (COM init, ElementFromHandle null,
    /// access denied). Payload may be partial; treat as best-effort.
    Failed(String),
}

/// Convert a [`ProbeResult`] to JSON, applying the truncation cascade
/// to keep the result ≤ [`MAX_PAYLOAD_BYTES`].
///
/// Returns `(json, was_truncated)`. The caller may stash the bool in
/// `control_summary.elements_visited`'s sibling for the UI, but the
/// canonical record is `json` itself.
pub fn to_payload_json(result: &ProbeResult) -> (String, bool) {
    let mut out = result.clone();
    truncate_fragments_per_fragment(&mut out);

    // Serialize once. If under cap, ship it.
    let mut json = serialize(&out);
    if json.len() <= MAX_PAYLOAD_BYTES {
        return (json, false);
    }

    // Soft cap by fragment count: keep the FIRST N. Fragments harvested
    // in tree-walk order — earlier ones are usually higher in the UI
    // (page header, primary nav, focused container), so first-N is a
    // better signal than random-N.
    if out.visible_text_fragments.len() > MAX_TEXT_FRAGMENTS {
        out.visible_text_fragments.truncate(MAX_TEXT_FRAGMENTS);
        json = serialize(&out);
    }

    // Drop tail fragments until under cap (or empty).
    while json.len() > MAX_PAYLOAD_BYTES && !out.visible_text_fragments.is_empty() {
        out.visible_text_fragments.pop();
        json = serialize(&out);
    }

    // If still over, truncate the focused-field value.
    if json.len() > MAX_PAYLOAD_BYTES {
        if let Some(ff) = out.focused_field.as_mut() {
            ff.value = truncate_chars(&ff.value, 64);
        }
        json = serialize(&out);
    }

    // Final stand: drop visible_text_fragments entirely.
    if json.len() > MAX_PAYLOAD_BYTES {
        out.visible_text_fragments.clear();
        json = serialize(&out);
    }

    (json, true)
}

/// Truncate every fragment to [`MAX_FRAGMENT_CHARS`] characters,
/// in place. Char-based (not byte-based) so we don't slice mid-glyph.
fn truncate_fragments_per_fragment(r: &mut ProbeResult) {
    for f in r.visible_text_fragments.iter_mut() {
        if f.chars().count() > MAX_FRAGMENT_CHARS {
            *f = truncate_chars(f, MAX_FRAGMENT_CHARS);
        }
    }
}

/// Truncate `s` to at most `n` chars, appending `'…'` when truncation
/// happened. Chars (not bytes) — safe on multi-byte glyphs.
fn truncate_chars(s: &str, n: usize) -> String {
    let total = s.chars().count();
    if total <= n {
        return s.to_string();
    }
    let mut out: String = s.chars().take(n).collect();
    out.push('…');
    out
}

fn serialize(r: &ProbeResult) -> String {
    // We tag the schema for forward compat. The serde-derived shape
    // is stable across waves; new optional fields can be added with
    // `#[serde(default)]` without breaking older rows.
    #[derive(Serialize)]
    struct Wrapper<'a> {
        schema: &'static str,
        #[serde(flatten)]
        body: &'a ProbeResult,
    }
    // serde_json::to_string can fail only on non-string keys or
    // unrepresentable f32 (NaN). ProbeResult has neither — but we
    // still fall back to a minimal payload so the sampler can never
    // panic on a downstream sample.
    serde_json::to_string(&Wrapper {
        schema: "v2",
        body: r,
    })
    .unwrap_or_else(|_| {
        format!(
            "{{\"schema\":\"v2\",\"app\":{},\"title\":{},\"status\":{{\"kind\":\"failed\",\"reason\":\"serialize_failed\"}}}}",
            json_string(&r.app),
            json_string(&r.title)
        )
    })
}

/// Minimal JSON-string escaper used only by the serialize-failure
/// recovery path above; the happy path goes through serde_json.
fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
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
    out
}

/// Build the minimal "title-only" probe result the sampler emits
/// when the platform has no UIA backend (non-Windows stub or COM
/// init failure). The runtime can still record a useful
/// `context_snapshot` row.
pub fn title_only(app: &str, title: &str, reason: &str) -> ProbeResult {
    ProbeResult {
        app: app.to_string(),
        title: title.to_string(),
        status: ProbeStatus::Failed(reason.to_string()),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> serde_json::Value {
        serde_json::from_str(json).expect("valid JSON")
    }

    #[test]
    fn happy_path_serializes_with_schema_v2() {
        let r = ProbeResult {
            app: "notepad.exe".into(),
            title: "Untitled - Notepad".into(),
            status: ProbeStatus::Ok,
            ..Default::default()
        };
        let (json, truncated) = to_payload_json(&r);
        assert!(!truncated);
        let v = parse(&json);
        assert_eq!(v["schema"], "v2");
        assert_eq!(v["app"], "notepad.exe");
        assert_eq!(v["title"], "Untitled - Notepad");
        assert_eq!(v["status"]["kind"], "ok");
    }

    #[test]
    fn under_cap_no_truncation() {
        let r = ProbeResult {
            app: "a.exe".into(),
            title: "t".into(),
            visible_text_fragments: vec!["hello".into(), "world".into()],
            ..Default::default()
        };
        let (json, truncated) = to_payload_json(&r);
        assert!(!truncated);
        assert!(json.len() < MAX_PAYLOAD_BYTES);
        let v = parse(&json);
        assert_eq!(v["visibleTextFragments"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn over_cap_truncates_fragments_first() {
        // 1000 fragments of 200 chars each = 200 KB raw -> well over cap.
        let big = "x".repeat(200);
        let r = ProbeResult {
            app: "a.exe".into(),
            title: "t".into(),
            visible_text_fragments: vec![big; 1000],
            ..Default::default()
        };
        let (json, truncated) = to_payload_json(&r);
        assert!(truncated);
        assert!(
            json.len() <= MAX_PAYLOAD_BYTES,
            "got {} bytes, cap is {}",
            json.len(),
            MAX_PAYLOAD_BYTES
        );
        let v = parse(&json);
        // App + title must survive verbatim.
        assert_eq!(v["app"], "a.exe");
        assert_eq!(v["title"], "t");
    }

    #[test]
    fn per_fragment_chars_truncated_with_ellipsis() {
        let huge_fragment = "x".repeat(MAX_FRAGMENT_CHARS + 100);
        let r = ProbeResult {
            visible_text_fragments: vec![huge_fragment],
            ..Default::default()
        };
        let (json, _) = to_payload_json(&r);
        let v = parse(&json);
        let frag = v["visibleTextFragments"][0].as_str().unwrap();
        assert!(frag.ends_with('…'));
        assert_eq!(frag.chars().count(), MAX_FRAGMENT_CHARS + 1);
    }

    #[test]
    fn multi_byte_glyphs_are_truncated_safely() {
        // Each emoji is multi-byte in UTF-8. Slicing on bytes would
        // panic; we MUST slice on chars.
        let s: String = "🐦".repeat(MAX_FRAGMENT_CHARS + 10);
        let r = ProbeResult {
            visible_text_fragments: vec![s],
            ..Default::default()
        };
        let (json, _) = to_payload_json(&r);
        let v = parse(&json);
        let frag = v["visibleTextFragments"][0].as_str().unwrap();
        assert!(frag.ends_with('…'));
        // chars().count() — not byte length.
        assert_eq!(frag.chars().count(), MAX_FRAGMENT_CHARS + 1);
    }

    #[test]
    fn focused_field_value_truncated_when_payload_overflows() {
        // Construct a result whose ONLY large field is focused_field.value.
        // Fragments fit, but FF value alone busts the cap.
        let big_value = "v".repeat(MAX_PAYLOAD_BYTES);
        let r = ProbeResult {
            app: "a.exe".into(),
            title: "t".into(),
            focused_field: Some(FocusedField {
                name: "Document".into(),
                control_type: "edit".into(),
                value: big_value,
            }),
            ..Default::default()
        };
        let (json, truncated) = to_payload_json(&r);
        assert!(truncated);
        assert!(json.len() <= MAX_PAYLOAD_BYTES);
        let v = parse(&json);
        // The truncated value ends with ellipsis (focused-field fallback).
        let value = v["focusedField"]["value"].as_str().unwrap();
        assert!(value.ends_with('…'));
    }

    #[test]
    fn password_field_active_serializes_as_bool() {
        let r = ProbeResult {
            app: "browser.exe".into(),
            title: "Sign in".into(),
            password_field_active: true,
            ..Default::default()
        };
        let (json, _) = to_payload_json(&r);
        let v = parse(&json);
        assert_eq!(v["passwordFieldActive"], true);
    }

    #[test]
    fn status_failed_includes_reason() {
        let r = ProbeResult {
            app: "game.exe".into(),
            title: "Steam".into(),
            status: ProbeStatus::Failed("ElementFromHandle returned null".into()),
            ..Default::default()
        };
        let (json, _) = to_payload_json(&r);
        let v = parse(&json);
        assert_eq!(v["status"]["kind"], "failed");
        assert_eq!(v["status"]["reason"], "ElementFromHandle returned null");
    }

    #[test]
    fn status_no_payload_serializes_cleanly() {
        let r = ProbeResult::default();
        let (json, _) = to_payload_json(&r);
        let v = parse(&json);
        assert_eq!(v["status"]["kind"], "no_payload");
    }

    #[test]
    fn monitor_info_round_trips() {
        let r = ProbeResult {
            app: "a.exe".into(),
            title: "t".into(),
            monitor: Some(MonitorInfo {
                name: r"\\.\DISPLAY2".into(),
                is_primary: false,
                bounds: Rect {
                    left: 1920,
                    top: 0,
                    right: 3840,
                    bottom: 1080,
                },
                dpi_scale: Some(1.0),
            }),
            ..Default::default()
        };
        let (json, _) = to_payload_json(&r);
        let v = parse(&json);
        assert_eq!(v["monitor"]["name"], r"\\.\DISPLAY2");
        assert_eq!(v["monitor"]["isPrimary"], false);
        assert_eq!(v["monitor"]["bounds"]["right"], 3840);
        assert_eq!(v["monitor"]["dpiScale"], 1.0);
    }

    #[test]
    fn control_summary_preserves_counts() {
        let r = ProbeResult {
            app: "a.exe".into(),
            title: "t".into(),
            control_summary: ControlSummary {
                edit_count: 3,
                button_count: 17,
                document_count: 1,
                link_count: 42,
                text_count: 88,
                other_count: 5,
                elements_visited: 200,
            },
            ..Default::default()
        };
        let (json, _) = to_payload_json(&r);
        let v = parse(&json);
        assert_eq!(v["controlSummary"]["buttonCount"], 17);
        assert_eq!(v["controlSummary"]["linkCount"], 42);
        assert_eq!(v["controlSummary"]["elementsVisited"], 200);
    }

    #[test]
    fn title_only_helper_marks_failed_status() {
        let r = title_only("a.exe", "t", "no_windows");
        assert_eq!(r.app, "a.exe");
        assert_eq!(r.title, "t");
        match r.status {
            ProbeStatus::Failed(reason) => assert_eq!(reason, "no_windows"),
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn empty_app_and_title_still_serialize() {
        // Protected processes can yield empty app + title. We must not
        // panic or skip the row — the runtime needs a payload to write.
        let r = ProbeResult::default();
        let (json, _) = to_payload_json(&r);
        let v = parse(&json);
        assert_eq!(v["app"], "");
        assert_eq!(v["title"], "");
    }

    #[test]
    fn truncation_drops_fragments_in_tail_first() {
        // First fragment is sentinel; we want it to survive truncation.
        let mut fragments = vec!["SENTINEL".to_string()];
        for i in 0..500 {
            fragments.push(format!("filler-{i}-{}", "x".repeat(100)));
        }
        let r = ProbeResult {
            app: "a.exe".into(),
            title: "t".into(),
            visible_text_fragments: fragments,
            ..Default::default()
        };
        let (json, truncated) = to_payload_json(&r);
        assert!(truncated);
        let v = parse(&json);
        let frags = v["visibleTextFragments"].as_array().unwrap();
        assert_eq!(
            frags.first().and_then(|x| x.as_str()),
            Some("SENTINEL"),
            "first fragment must survive truncation"
        );
    }

    #[test]
    fn json_string_escapes_quotes_and_backslashes() {
        let escaped = json_string(r#"a"b\c"#);
        assert_eq!(escaped, r#""a\"b\\c""#);
    }

    #[test]
    fn json_string_escapes_control_chars() {
        let escaped = json_string("a\x01b");
        assert!(escaped.contains("\\u0001"));
    }

    #[test]
    fn schema_field_appears_first_for_human_readability() {
        // Cosmetic but load-bearing for debugging — we want
        // `head -c 80 snapshot.json` to show the schema discriminator.
        let r = ProbeResult {
            app: "a.exe".into(),
            title: "t".into(),
            ..Default::default()
        };
        let (json, _) = to_payload_json(&r);
        assert!(json.starts_with(r#"{"schema":"v2""#));
    }

    #[test]
    fn extremely_long_app_name_does_not_panic() {
        // App + title are never truncated, but we want the cap-overshoot
        // path to handle them gracefully (the final-stand drops fragments,
        // not app/title — so the JSON may exceed cap if app is malicious;
        // that's acceptable because process basenames are bounded by the
        // OS at 260 chars).
        let r = ProbeResult {
            app: "a".repeat(280),
            title: "t".into(),
            ..Default::default()
        };
        let (json, _) = to_payload_json(&r);
        let v = parse(&json);
        assert_eq!(v["app"].as_str().unwrap().len(), 280);
    }

    #[test]
    fn payload_is_deterministic_for_same_input() {
        // Provenance principle — the same probe result must yield the
        // same JSON byte-for-byte across runs. Otherwise diffing two
        // sessions for regressions is meaningless.
        let r = ProbeResult {
            app: "a.exe".into(),
            title: "t".into(),
            visible_text_fragments: vec!["a".into(), "b".into(), "c".into()],
            ..Default::default()
        };
        let (j1, _) = to_payload_json(&r);
        let (j2, _) = to_payload_json(&r);
        assert_eq!(j1, j2);
    }
}
