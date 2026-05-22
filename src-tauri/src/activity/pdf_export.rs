//! Activity-session PDF export.
//!
//! Phase 10 Wave 5. ADR 0044 (printpdf crate; layout v1; YAGNI scope).
//!
//! The layout is locked v1 — A4, Helvetica, two modes (full +
//! work-report). The IPC command [`crate::commands::activity::
//! activity_export_pdf`] is a thin shim that loads the session detail,
//! calls [`render_session_pdf`], and writes the bytes to the user-
//! chosen path via the dialog plugin.
//!
//! ## Modes
//!
//! - [`PdfMode::Full`] — session title + duration header, then each
//!   Block with time-range, primary-app line, and abstract paragraph.
//! - [`PdfMode::WorkReport`] — session title + duration header, then
//!   only Block time-range + abstract. No primary-app / idle bullets.
//!
//! ## Layout constants (ADR 0044 §Layout — binding)
//!
//! - Page: A4 (210 × 297 mm).
//! - Margins: 18 mm sides, 20 mm top, 18 mm bottom.
//! - Header: Helvetica-Bold 18 pt for title; Helvetica 10 pt grey for
//!   date range.
//! - Block header: Helvetica-Bold 12 pt.
//! - Body text: Helvetica 11 pt.
//! - Footer: Helvetica 9 pt grey.
//!
//! ## Wrapping
//!
//! Pure function [`wrap_text_to_width`] implements greedy word-wrap.
//! Falls back to character-wrap when a single word exceeds the column
//! width (URLs, hashes). Width estimation is a coarse 0.5 × `font_pt`
//! per char for Helvetica — accurate enough for layout without
//! pulling a metrics-table dep.

#![allow(missing_docs)]

use std::io::{BufWriter, Cursor};

use printpdf::{BuiltinFont, IndirectFontRef, Mm, PdfDocument, PdfDocumentReference};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::activity::{
    assembler::format_duration, blocks_persist::list_blocks, persist::get_session_detail,
};
use crate::error::{AppError, AppResult};

// ---------------------------------------------------------------------------
// Layout constants (ADR 0044 §Layout). All units in mm except font_pt.
// ---------------------------------------------------------------------------

const PAGE_W_MM: f32 = 210.0;
const PAGE_H_MM: f32 = 297.0;
const MARGIN_TOP_MM: f32 = 20.0;
const MARGIN_BOTTOM_MM: f32 = 18.0;
const MARGIN_SIDE_MM: f32 = 18.0;
const COLUMN_W_MM: f32 = PAGE_W_MM - 2.0 * MARGIN_SIDE_MM;

const TITLE_FONT_PT: f32 = 18.0;
const DATE_FONT_PT: f32 = 10.0;
const BLOCK_HEADER_FONT_PT: f32 = 12.0;
const BODY_FONT_PT: f32 = 11.0;
const FOOTER_FONT_PT: f32 = 9.0;

/// Approximate Helvetica glyph width in mm at a given pt size.
/// Coarse (0.5 × pt / mm-per-pt) but stable; sufficient for layout
/// since we don't justify text.
const HELVETICA_GLYPH_FACTOR: f32 = 0.5;
const PT_PER_MM: f32 = 2.834_645_7; // 1 mm = 2.83465 pt

fn glyph_width_mm(font_pt: f32) -> f32 {
    (font_pt * HELVETICA_GLYPH_FACTOR) / PT_PER_MM
}

fn line_height_mm(font_pt: f32) -> f32 {
    // 1.3× leading on the pt-derived row height.
    (font_pt * 1.3) / PT_PER_MM
}

/// Which layout to render.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PdfMode {
    /// Full layout — block header includes primary app + idle %.
    Full,
    /// Work-report layout — block header is just the time range; the
    /// primary-app / idle breakdown is omitted.
    WorkReport,
}

impl PdfMode {
    pub fn parse(s: &str) -> AppResult<Self> {
        match s {
            "full" => Ok(Self::Full),
            "work_report" | "workReport" => Ok(Self::WorkReport),
            other => Err(AppError::Other(format!("unknown pdf mode: {other:?}"))),
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Build the PDF for one session and return the encoded bytes.
///
/// `now_unix_seconds_for_filename` is unused at render time; callers
/// use it to suggest a filename. Kept out of this function to keep
/// [`render_session_pdf`] pure relative to its inputs.
pub fn render_session_pdf(
    conn: &mut Connection,
    session_id: &str,
    mode: PdfMode,
) -> AppResult<Vec<u8>> {
    let detail = get_session_detail(conn, session_id)?
        .ok_or_else(|| AppError::Other(format!("no such activity session: {session_id}")))?;
    let blocks = list_blocks(conn, session_id)?;

    let title = format!("Activity Session {}", short_id(session_id));
    let date_line = format_date_range(detail.session.started_at, detail.session.ended_at);
    let total_duration_ms = detail
        .session
        .ended_at
        .unwrap_or(detail.session.started_at)
        .saturating_sub(detail.session.started_at)
        .max(0);
    let total_human = format_duration(total_duration_ms);

    let mut doc_ctx = DocBuilder::start(&title)?;

    // Header.
    doc_ctx.write_title(&title);
    doc_ctx.write_subtle(&format!("{date_line}  ·  {total_human} total"));
    doc_ctx.advance(line_height_mm(BODY_FONT_PT));

    if blocks.is_empty() {
        doc_ctx.write_body("(no blocks generated for this session yet)");
    } else {
        for b in &blocks {
            doc_ctx.write_block_header(&format_block_header(b, mode));
            let abstract_text = b
                .generated_abstract
                .clone()
                .unwrap_or_else(|| "(no summary)".to_string());
            doc_ctx.write_body_wrapped(&abstract_text);
            doc_ctx.advance(line_height_mm(BODY_FONT_PT) * 0.6);
        }
    }

    doc_ctx.finish()
}

// ---------------------------------------------------------------------------
// Pure helpers
// ---------------------------------------------------------------------------

/// Greedy word-wrap. Returns a list of lines whose rendered width
/// (per [`glyph_width_mm`]) does not exceed `max_width_mm`. Falls
/// back to character-wrap for individual words longer than the
/// column. Pure — no allocs beyond the returned Vec.
pub fn wrap_text_to_width(text: &str, font_pt: f32, max_width_mm: f32) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }
    let glyph = glyph_width_mm(font_pt);
    let max_chars = if glyph > 0.0 {
        ((max_width_mm / glyph).floor() as usize).max(1)
    } else {
        1
    };

    let mut out: Vec<String> = Vec::new();
    for paragraph in text.split('\n') {
        if paragraph.is_empty() {
            out.push(String::new());
            continue;
        }
        let mut current = String::new();
        for word in paragraph.split_whitespace() {
            if word.chars().count() > max_chars {
                // Flush whatever we have, then char-wrap the giant word.
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                }
                let chars: Vec<char> = word.chars().collect();
                for chunk in chars.chunks(max_chars) {
                    out.push(chunk.iter().collect());
                }
                continue;
            }
            let candidate_len = if current.is_empty() {
                word.chars().count()
            } else {
                current.chars().count() + 1 + word.chars().count()
            };
            if candidate_len <= max_chars {
                if !current.is_empty() {
                    current.push(' ');
                }
                current.push_str(word);
            } else {
                out.push(std::mem::take(&mut current));
                current.push_str(word);
            }
        }
        if !current.is_empty() {
            out.push(current);
        }
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

/// Format the block-header line per the active mode.
fn format_block_header(
    b: &crate::activity::blocks_persist::ActivityBlockRow,
    mode: PdfMode,
) -> String {
    let started = ms_to_clock(b.started_at);
    let ended = ms_to_clock(b.ended_at);
    match mode {
        PdfMode::Full => {
            let label = b.label.as_deref().unwrap_or(&b.primary_title);
            format!("{started} – {ended}  ·  {}  ·  {label}", b.primary_app)
        }
        PdfMode::WorkReport => {
            let label = b.label.as_deref().unwrap_or(&b.primary_title);
            format!("{started} – {ended}  ·  {label}")
        }
    }
}

/// Six-char prefix of a session id for human reference.
fn short_id(id: &str) -> String {
    id.chars().take(8).collect()
}

/// Render an epoch-ms timestamp as a `HH:MM` clock string in local-
/// adjacent UTC. We deliberately don't pull `chrono` here — the
/// existing assembler uses the same trick, and the user always sees
/// the surrounding date in the header line.
fn ms_to_clock(ms: i64) -> String {
    let secs = (ms / 1000).rem_euclid(86_400);
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    format!("{h:02}:{m:02}")
}

/// Render `YYYY-MM-DD` for the date-range header. Same justification
/// for not pulling chrono — we want the file content to look stable
/// across machines.
fn format_date_range(started_at: i64, ended_at: Option<i64>) -> String {
    let start = ms_to_date(started_at);
    match ended_at {
        Some(e) if ms_to_date(e) != start => format!("{start} → {}", ms_to_date(e)),
        _ => start,
    }
}

fn ms_to_date(ms: i64) -> String {
    // Algorithm: days since unix epoch → civil date. Public-domain
    // formula by Howard Hinnant.
    let days = ms.div_euclid(86_400_000);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

// ---------------------------------------------------------------------------
// Document builder — encapsulates printpdf's page/layer/cursor state.
// ---------------------------------------------------------------------------

struct DocBuilder {
    doc: PdfDocumentReference,
    /// Stack of `(page_index, layer_index)`. Last entry is the
    /// currently-active layer.
    pages: Vec<(printpdf::PdfPageIndex, printpdf::PdfLayerIndex)>,
    /// y-cursor in mm from the TOP of the page. We track top-down for
    /// convenience; printpdf expects mm from the BOTTOM, which we
    /// convert at write time.
    cursor_y_mm: f32,
    font_regular: IndirectFontRef,
    font_bold: IndirectFontRef,
}

impl DocBuilder {
    fn start(title: &str) -> AppResult<Self> {
        let (doc, page1, layer1) = PdfDocument::new(title, Mm(PAGE_W_MM), Mm(PAGE_H_MM), "Layer 1");
        let font_regular = doc
            .add_builtin_font(BuiltinFont::Helvetica)
            .map_err(|e| AppError::Other(format!("pdf add font: {e}")))?;
        let font_bold = doc
            .add_builtin_font(BuiltinFont::HelveticaBold)
            .map_err(|e| AppError::Other(format!("pdf add font: {e}")))?;
        Ok(Self {
            doc,
            pages: vec![(page1, layer1)],
            cursor_y_mm: MARGIN_TOP_MM,
            font_regular,
            font_bold,
        })
    }

    fn write_title(&mut self, text: &str) {
        let lines = wrap_text_to_width(text, TITLE_FONT_PT, COLUMN_W_MM);
        for line in lines {
            self.ensure_room(line_height_mm(TITLE_FONT_PT));
            self.draw_line(&line, TITLE_FONT_PT, /*bold=*/ true);
            self.cursor_y_mm += line_height_mm(TITLE_FONT_PT);
        }
    }

    fn write_subtle(&mut self, text: &str) {
        let lines = wrap_text_to_width(text, DATE_FONT_PT, COLUMN_W_MM);
        for line in lines {
            self.ensure_room(line_height_mm(DATE_FONT_PT));
            self.draw_line(&line, DATE_FONT_PT, /*bold=*/ false);
            self.cursor_y_mm += line_height_mm(DATE_FONT_PT);
        }
    }

    fn write_block_header(&mut self, text: &str) {
        let lines = wrap_text_to_width(text, BLOCK_HEADER_FONT_PT, COLUMN_W_MM);
        for line in lines {
            self.ensure_room(line_height_mm(BLOCK_HEADER_FONT_PT));
            self.draw_line(&line, BLOCK_HEADER_FONT_PT, /*bold=*/ true);
            self.cursor_y_mm += line_height_mm(BLOCK_HEADER_FONT_PT);
        }
    }

    fn write_body(&mut self, text: &str) {
        self.write_body_wrapped(text);
    }

    fn write_body_wrapped(&mut self, text: &str) {
        let lines = wrap_text_to_width(text, BODY_FONT_PT, COLUMN_W_MM);
        for line in lines {
            self.ensure_room(line_height_mm(BODY_FONT_PT));
            self.draw_line(&line, BODY_FONT_PT, /*bold=*/ false);
            self.cursor_y_mm += line_height_mm(BODY_FONT_PT);
        }
    }

    fn advance(&mut self, mm: f32) {
        self.cursor_y_mm += mm;
    }

    /// Add a fresh page when there's no room for `needed_height_mm`
    /// before the bottom margin.
    fn ensure_room(&mut self, needed_height_mm: f32) {
        if self.cursor_y_mm + needed_height_mm > PAGE_H_MM - MARGIN_BOTTOM_MM {
            let (p, l) = self.doc.add_page(
                Mm(PAGE_W_MM),
                Mm(PAGE_H_MM),
                format!("Layer {}", self.pages.len() + 1),
            );
            self.pages.push((p, l));
            self.cursor_y_mm = MARGIN_TOP_MM;
        }
    }

    fn draw_line(&self, text: &str, font_pt: f32, bold: bool) {
        let (page_idx, layer_idx) = *self.pages.last().expect("at least one page");
        let layer = self.doc.get_page(page_idx).get_layer(layer_idx);
        let font = if bold {
            &self.font_bold
        } else {
            &self.font_regular
        };
        // printpdf y-axis: 0 = bottom. We track from top.
        let y_from_bottom = PAGE_H_MM - self.cursor_y_mm - mm_from_pt(font_pt);
        layer.use_text(text, font_pt, Mm(MARGIN_SIDE_MM), Mm(y_from_bottom), font);
    }

    fn finish(self) -> AppResult<Vec<u8>> {
        // Footer pass: stamp "Mockingbird · page N of M" on every page.
        let total_pages = self.pages.len();
        for (i, (page_idx, layer_idx)) in self.pages.iter().enumerate() {
            let layer = self.doc.get_page(*page_idx).get_layer(*layer_idx);
            let footer = format!("Mockingbird  ·  page {} of {}", i + 1, total_pages);
            let y_from_bottom = MARGIN_BOTTOM_MM * 0.5;
            layer.use_text(
                footer,
                FOOTER_FONT_PT,
                Mm(MARGIN_SIDE_MM),
                Mm(y_from_bottom),
                &self.font_regular,
            );
        }

        let mut out: Vec<u8> = Vec::new();
        {
            let cursor = Cursor::new(&mut out);
            let mut writer = BufWriter::new(cursor);
            self.doc
                .save(&mut writer)
                .map_err(|e| AppError::Other(format!("pdf save: {e}")))?;
        }
        Ok(out)
    }
}

fn mm_from_pt(pt: f32) -> f32 {
    pt / PT_PER_MM
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_short_text_is_single_line() {
        let lines = wrap_text_to_width("hello world", BODY_FONT_PT, COLUMN_W_MM);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], "hello world");
    }

    #[test]
    fn wrap_breaks_on_word_boundary() {
        let long =
            "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi";
        // Force a narrow column to provoke wrapping.
        let lines = wrap_text_to_width(long, BODY_FONT_PT, 40.0);
        assert!(lines.len() >= 2);
        // No line exceeds the budget (rough check via char count).
        let max_chars = ((40.0 / glyph_width_mm(BODY_FONT_PT)).floor() as usize).max(1);
        for line in &lines {
            assert!(
                line.chars().count() <= max_chars,
                "line {line:?} ({} chars) exceeds budget {max_chars}",
                line.chars().count()
            );
        }
    }

    #[test]
    fn wrap_char_falls_back_for_giant_word() {
        // A 200-char word in a narrow column must char-wrap.
        let giant = "x".repeat(200);
        let lines = wrap_text_to_width(&giant, BODY_FONT_PT, 30.0);
        assert!(lines.len() > 1);
        let total: usize = lines.iter().map(|s| s.chars().count()).sum();
        assert_eq!(total, 200);
    }

    #[test]
    fn wrap_preserves_explicit_paragraph_breaks() {
        let lines = wrap_text_to_width("first\n\nthird", BODY_FONT_PT, COLUMN_W_MM);
        assert_eq!(
            lines,
            vec!["first".to_string(), String::new(), "third".to_string()]
        );
    }

    #[test]
    fn ms_to_clock_rolls_at_24h() {
        assert_eq!(ms_to_clock(0), "00:00");
        // 9h 5m past midnight UTC.
        assert_eq!(ms_to_clock((9 * 3600 + 5 * 60) * 1000), "09:05");
        // Next day rollover at 24h * 3600 * 1000.
        assert_eq!(ms_to_clock(86_400_000), "00:00");
    }

    #[test]
    fn ms_to_date_handles_unix_epoch_and_known_dates() {
        assert_eq!(ms_to_date(0), "1970-01-01");
        // 2024-01-01T00:00:00Z = 1_704_067_200_000 ms.
        assert_eq!(ms_to_date(1_704_067_200_000), "2024-01-01");
    }

    #[test]
    fn mode_round_trips_via_str() {
        assert_eq!(PdfMode::parse("full").unwrap(), PdfMode::Full);
        assert_eq!(PdfMode::parse("work_report").unwrap(), PdfMode::WorkReport);
        assert!(PdfMode::parse("nonsense").is_err());
    }

    // ----------------------------------------------------------
    // Phase 10 Wave 6.B — pdf-renders-correct-block-count judge fixtures.
    // ADR 0044 (printpdf v1 layout; two modes).
    // ----------------------------------------------------------

    /// Build a fixture session with three labelled blocks (distinct
    /// labels + abstracts, ordered by `started_at`). Returns the
    /// session id + the label strings in render order.
    fn seed_three_block_session(
        db: &mut crate::db::Database,
    ) -> (String, [&'static str; 3], [&'static str; 3]) {
        use crate::activity::blocks_persist::{insert_block, rename_block};
        use crate::activity::persist::insert_session;

        let sid = insert_session(&db.conn, 1_000).unwrap();
        // End the session so the duration header doesn't read 0ms.
        db.conn
            .execute(
                "UPDATE activity_sessions SET ended_at = ?1, status = 'completed' WHERE id = ?2",
                rusqlite::params![10_000i64, &sid],
            )
            .unwrap();

        let labels = ["Wave6JudgesAuthoring", "CargoGateDryRun", "StatusUpdate"];
        let abstracts = [
            "Wrote the six invariant judges and the dry-run rig.",
            "Ran cargo clippy plus fmt plus the no-run gate to prove the link surface.",
            "Pushed STATUS plus closed the Wave six beads in beads database.",
        ];
        for i in 0..3 {
            let started = 1_000 + (i as i64) * 1_000;
            let bid = insert_block(
                &db.conn,
                &sid,
                started,
                started + 500,
                "a.exe",
                "t",
                Some(abstracts[i]),
                "[]",
                "abstract_v1-deadbeef",
                started,
            )
            .unwrap();
            rename_block(&db.conn, &bid, Some(labels[i]), started).unwrap();
        }
        (sid, labels, abstracts)
    }

    #[test]
    fn full_mode_renders_three_block_labels() {
        use crate::db::Database;
        let mut db = Database::open_in_memory().expect("open in-memory db");
        let (sid, labels, abstracts) = seed_three_block_session(&mut db);

        let bytes = render_session_pdf(&mut db.conn, &sid, PdfMode::Full)
            .expect("render_session_pdf Full mode");
        assert!(!bytes.is_empty(), "PDF bytes should not be empty");

        let text = pdf_extract::extract_text_from_mem(&bytes)
            .expect("pdf_extract should round-trip our PDF");

        // All three labels appear, in started_at order (idx 0 < 1 < 2).
        let positions: Vec<usize> = labels
            .iter()
            .map(|l| {
                text.find(l)
                    .unwrap_or_else(|| panic!("label {l:?} missing from PDF text:\n{text}"))
            })
            .collect();
        assert!(
            positions[0] < positions[1] && positions[1] < positions[2],
            "labels must appear in started_at order, got positions {positions:?}"
        );

        // All three abstract strings appear too.
        for a in abstracts {
            assert!(
                text.contains(a),
                "abstract {a:?} missing from PDF text:\n{text}"
            );
        }
    }

    #[test]
    fn work_report_mode_renders_three_abstracts_no_events() {
        use crate::activity::persist::insert_event;
        use crate::db::Database;

        let mut db = Database::open_in_memory().expect("open in-memory db");
        let (sid, _labels, abstracts) = seed_three_block_session(&mut db);

        // One distinctive event row that work-report mode must NOT render.
        insert_event(
            &db.conn,
            &sid,
            1_500,
            "app_switch",
            Some("do-not-include-in-work-report.exe"),
            Some("leak-marker"),
            None,
        )
        .unwrap();

        let bytes = render_session_pdf(&mut db.conn, &sid, PdfMode::WorkReport)
            .expect("render_session_pdf WorkReport mode");
        let text = pdf_extract::extract_text_from_mem(&bytes)
            .expect("pdf_extract should round-trip our PDF");

        for a in abstracts {
            assert!(
                text.contains(a),
                "abstract {a:?} missing from work-report PDF text:\n{text}"
            );
        }
        assert!(
            !text.contains("do-not-include-in-work-report.exe"),
            "work-report mode leaked raw event app_name into the PDF:\n{text}"
        );
        assert!(
            !text.contains("Raw event list"),
            "work-report mode rendered a raw-event section header:\n{text}"
        );
    }
}
