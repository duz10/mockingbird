# Judge: pdf-renders-correct-block-count (Phase 10)

**Target:** `src-tauri/src/activity/pdf_export.rs::render_session_pdf`,
`PdfMode::Full`, `PdfMode::WorkReport`, ADR 0044.

**Question:** For a fixture activity session containing N = 3 Blocks
with distinct labels + abstracts, does `render_session_pdf(...,
PdfMode::Full, ...)` produce a PDF whose extracted text contains all
3 Block labels in their `started_at` order, and does
`PdfMode::WorkReport` produce a PDF whose extracted text contains all
3 abstracts (and *only* the abstracts, no raw event payload)?

**Rationale:** ADR 0044 fixes the two PDF modes:

- **Full** — session title + duration header, then each Block's
  label, abstract, and a `Raw event list ▶` collapsible (rendered
  expanded in the PDF since PDFs don't have JS for collapsing).
- **Work report** — session title + duration header, then each
  Block's label + abstract only. No raw events. Intended for
  pasting into a status update.

A PDF that *looks* right at a glance but actually drops one of the
three Blocks (because of a page-break edge case, an off-by-one
loop bound, or a stale clone of the Block list) is a silent data
loss bug. The judge round-trips a known-input PDF through the
`pdf-extract` test-only crate and asserts the structural
invariants by counting label / abstract occurrences in the extracted
text. We don't try to assert layout fidelity (that's a visual job,
deferred to a future qa-kitten / Playwright invocation) — just
content presence + ordering.

**New test-only dependency:** `pdf-extract = "0.7"` lives in
`src-tauri/Cargo.toml` under `[dev-dependencies]`. NOT in the
main `[dependencies]` block — the runtime binary has no business
parsing PDFs. The crate is pure-Rust, no native deps, no IOC list
hits (single-author crate from `pdf-rs` maintainer, MIT/Apache-2.0).
This addition is the only Cargo manifest delta authored in Wave 6.A.

**Pass criteria — ALL of:**

1. **Existing `pdf_export.rs` unit tests pass:**

   ```powershell
   powershell -File scripts\cargo-with-cuda.ps1 test --release --lib `
     -- activity::pdf_export::tests
   ```

   Expected: all existing tests for `PdfMode::parse`, header
   composition, wrap_text_to_width, etc. green. Per LESSONS P2 fall
   back to throwaway-crate if live exec blocks (the module's deps
   are `printpdf`, `rusqlite`, `serde` — pure Rust, no native).

2. **`pdf-extract` is registered ONLY as a dev-dep:**

   ```powershell
   Select-String -Path src-tauri\Cargo.toml -Pattern 'pdf-extract'
   ```

   Expected: exactly one match, in the `[dev-dependencies]` block
   (line position should be after `criterion = { workspace = true }`).
   If a match appears in `[dependencies]`, FAIL — that would ship
   the parser into the runtime binary needlessly (and import a tree
   of LZW / Flate decoders that bloat the exe).

3. **Round-trip test: `Full` mode renders 3 blocks (new test):**

   *(New test to author in 6.B as
   `activity::pdf_export::tests::full_mode_renders_three_block_labels`.)*

   - Fixture builder: in-memory DB, 1 `activity_sessions` row with
     a known title + duration; 3 `activity_blocks` rows with labels
     `"Wave 6 judges authoring"`, `"Cargo gate dry-run"`,
     `"STATUS update"`, each with a non-empty
     `generated_abstract`. Each Block has a `source_event_ids`
     pointing at one fixture event each.
   - Call `render_session_pdf(&conn, session_id, PdfMode::Full,
     &tempdir)`. Get back a `PathBuf`.
   - `let text = pdf_extract::extract_text(&pdf_path)?;`
   - Assert all three label substrings appear in `text`.
   - Assert they appear in `started_at` order (first occurrence of
     "Wave 6 judges authoring" < first occurrence of "Cargo gate
     dry-run" < first occurrence of "STATUS update").
   - Assert all three abstract strings also appear.

4. **Round-trip test: `WorkReport` mode renders 3 abstracts + omits
   raw events (new test):**

   *(New test to author in 6.B as
   `activity::pdf_export::tests::work_report_mode_renders_three_abstracts_no_events`.)*

   - Same fixture as criterion 3.
   - Add one `activity_events` row with a distinctive `app_name =
     "do-not-include-in-work-report.exe"`.
   - Call `render_session_pdf(&conn, session_id,
     PdfMode::WorkReport, &tempdir)`.
   - `let text = pdf_extract::extract_text(&pdf_path)?;`
   - Assert all three abstract substrings appear.
   - Assert the string `"do-not-include-in-work-report.exe"` does
     NOT appear in `text`.
   - Assert the literal `"Raw event list"` (or equivalent
     section-header text from `Full` mode) does NOT appear in
     `text`.

5. **Mode parse round-trip (existing test, keep green):**

   `activity::pdf_export::tests` already asserts
   `PdfMode::parse("full") == PdfMode::Full`,
   `PdfMode::parse("work_report") == PdfMode::WorkReport`, and
   `parse("nonsense")` errors. Re-confirm those stay green after
   the new tests land.

**On failure:**

- **Block the `phase-10-complete` tag.**
- If criterion 3 surfaces fewer than 3 labels: a Block was dropped.
  Common culprits: (a) the page-break logic in `render_session_pdf`
  early-returns without flushing the last Block's lines; (b) the
  Block-iteration loop closes one too early; (c) the
  `wrap_text_to_width` returned an empty vec for a long string and
  the renderer skipped it. The shrunk-down test makes (a)/(b)/(c)
  all surface as "expected 3 labels, found 2".
- If criterion 3 surfaces labels out of order: the SELECT in the
  fixture-builder query is missing `ORDER BY started_at`.
- If criterion 4 surfaces leaked event text: the WorkReport branch
  is calling into the Full-mode renderer with an unfiltered Block
  payload. Re-check the `match mode` dispatch in
  `compose_pdf_pages` / `render_session_pdf`.

**Last run (Wave 6.A dry-run):** _TBD — see Wave 6.A dispatch report.
Wave 6.A authored the judge + added the `pdf-extract` dev-dep, but
did NOT author the new fixture-builder + round-trip tests (those
land in 6.B). Expected dry-run result: green on criteria 1, 2, 5;
red on criteria 3 + 4 with root cause "fixture mismatch" (tests
don't exist yet)._
