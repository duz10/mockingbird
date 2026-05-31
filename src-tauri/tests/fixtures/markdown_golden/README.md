# Markdown serializer golden fixtures (Wave 1E.2 / `mb-vq8y`)

These five files pin the byte-stable canonical form emitted by
`crate::vault::markdown_serializer::serialize_entry`. They are
included into the compiled tests via `include_str!`; the
`tests` module in `src-tauri/src/vault/markdown_serializer.rs`
constructs the corresponding `KgEntry` fixtures programmatically
and asserts equality.

## Files

- `minimal.md` — required fields only; no checkbox section.
- `full_task.md` — every conditional field present; `status: todo`.
- `doing_task.md` — `status: doing` (Obsidian core `[/]` glyph),
  no due date, no entities.
- `done_task.md` — `status: done` (`[x]` glyph), text-note
  capture kind, no source session.
- `special_chars.md` — escaping stress (quotes, backslashes,
  newlines, tabs, em-dashes, unicode in title/tags/entities).

## Line endings

LF only. Part of the contract (ADR 0053 §D3). The tests assert no
`\r` bytes appear in the serializer output, and the goldens must
agree.

## Updating

If you deliberately changed the canonical form (e.g. a schema
version bump), regenerate the fixtures:

```powershell
# Inside the throwaway test-crate harness (LESSONS P2 recipe):
$env:MOCKINGBIRD_UPDATE_GOLDENS = "1"
cargo test --release golden_
# The tests panic with "fixtures updated; re-run" — recompile + rerun
# without the env var and they will pass.
Remove-Item Env:\MOCKINGBIRD_UPDATE_GOLDENS
cargo test --release golden_
```

The update path writes via `std::fs::write`, which on Windows does
NOT translate `\n` to `\r\n` — the regenerated files stay LF-only.

## Why these and not more

Five fixtures cover the kickoff brief's documented matrix:
required-only, full conditional set, each Tasks-checkbox glyph
(`[ ]`/`[/]`/`[x]`), and the escaping stress case. The property
tests (`prop_*`) cover the rest of the input space via random
fuzzing; the goldens pin the by-eye-reviewable canonical bytes
for human inspection during code review.
