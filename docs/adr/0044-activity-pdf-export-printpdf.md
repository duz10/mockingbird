# ADR 0044 — Activity PDF export via `printpdf`

- **Status:** Accepted
- **Date:** 2026-05-26
- **Phase:** 10 (Wave 5 — Hardening)
- **Author:** code-puppy (Bernard) on behalf of Dustin
- **Supersedes:** none
- **Superseded by:** none

## Context

Wave 5 polish item: per-session PDF export of the activity summary,
mirroring the existing Markdown export (`activity_export_markdown` /
`activity_render_work_report`). Two modes:

- **Full** — session title, totals, then each Block with its abstract +
  primary app + duration.
- **Work-report** — session title, totals, then Block abstracts only
  (no primary-app breakdown).

The kickoff explicitly forbids "any new heavy dep without flagging in
the commit message" and requests justification.

## Decision

Add **`printpdf` ^0.7** as a direct dependency of the `mockingbird`
binary crate. Implement `activity::pdf_export::render_session_pdf`
which returns a `Vec<u8>` of PDF bytes. The new IPC command
`activity_export_pdf` saves to disk via the existing
`tauri-plugin-dialog` save-as picker (already a project dep — see
`Cargo.toml` workspace deps).

## Rationale

| Candidate     | Pros                                                      | Cons                                                                                          |
|---------------|-----------------------------------------------------------|-----------------------------------------------------------------------------------------------|
| **printpdf**  | Pure-Rust. ~50 kloc. MIT. Battle-tested. No native deps.  | API is verbose for tables; we'll wrap a thin helper.                                          |
| `genpdf`     | Higher-level layout                                       | Built on top of `printpdf` anyway; adds another layer for a 1-screen layout we control.       |
| `pulldown-cmark` → `wkhtmltopdf` | Reuses our markdown               | Requires installing `wkhtmltopdf.exe` as an external binary. NO.                              |
| `pdf-writer`  | Smaller surface                                           | Lower-level; we'd reinvent font + layout primitives.                                          |
| Tauri webview print-to-PDF | Reuses the UI                              | Requires opening a webview offscreen + driving a print dialog from Rust; brittle.             |

`printpdf` is the obvious pure-Rust pick. It's added under
`[dependencies]` directly in `src-tauri/Cargo.toml` (no workspace
entry — the workspace is small and this dep is only used by the
binary).

## Layout (locked v1)

- Page size: A4 (210 × 297 mm).
- Top margin: 20 mm; bottom: 18 mm; sides: 18 mm.
- Header: session title (Helvetica Bold 18 pt), session date range
  (Helvetica Regular 10 pt grey).
- Body per Block:
  - Block header line: time-range + primary app (Helvetica Bold 12 pt).
  - Abstract paragraph (Helvetica Regular 11 pt, line-wrapped to
    column width).
- Footer (every page): "Mockingbird • page N of M" right-aligned,
  9 pt grey.
- "Work-report" mode drops the per-Block primary-app line + idle
  fraction; keeps Block header (time range) + abstract.

Wrapping is greedy word-wrap implemented in
`pdf_export::wrap_text_to_width` — a pure function we unit-test
against a width budget. Falls back to character-wrap if a single
word exceeds the column width (URLs, hashes).

## Out of scope for v1

- Embedded fonts beyond the printpdf built-in PostScript ones.
- Markdown rendering inside abstracts (bold/italic/headings/lists).
  The abstract text is rendered as a single Helvetica paragraph; if
  the LLM emits markdown, the user sees the raw `**bold**` — acceptable
  for v1; v1.1 may add minimal markdown handling.
- Images / charts / heatmaps.

## Test plan

- Pure unit tests (throwaway crate) for `wrap_text_to_width`:
  - short text → single line
  - long text → multiple lines respecting width budget
  - single word longer than width → char-wrap fallback
- Smoke test: invoke `render_session_pdf` against a fixture session +
  blocks, assert returned `Vec<u8>` starts with `%PDF-` magic.
- Live verification: human-in-loop opens the PDF in Edge/Acrobat,
  confirms layout.

## Risk

Low. `printpdf` is a well-known crate; pure Rust; no `unsafe` in our
usage; no native deps. Worst case it has a bug — we'd swap to
`genpdf` or vendor a one-off generator.
