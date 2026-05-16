---
name: quality
description: Coding standards, test discipline, and the green-bar definition-of-done for Mockingbird. Activate this skill whenever you are about to write/refactor code, design a test plan, or close out an iteration (end-of-iteration quality gate).
---

# Mockingbird quality bar

## Definition of done (per iteration)

Every iteration ends with all of:

- `cargo fmt --check` clean
- `cargo clippy -- -D warnings` clean
- `cargo test --quiet` green
- `npm run lint` clean (if JS files changed)
- `npm test` green (if JS files changed)
- `STATUS.md` updated (task progress, cost line, blocked-on, last judge)
- Closed beads for completed work; new beads for discovered work
- LESSONS.md appended for any non-obvious finding
- Commit messages descriptive (no `wip`, no `.`, no `fix stuff`)

Hook `stop-quality-gate` enforces the cargo trio mechanically at session
exit. Hook `post-commit-status-check` warns if STATUS.md was skipped.

## Code standards (binding)

### Rust

- Edition 2021, MSRV = 1.77
- One module = one concern. If a `mod.rs` grows past 600 lines, split.
- `Result<T, E>` everywhere. `unwrap()` only in `#[test]` or in `main`.
- `thiserror` for error types; never `Box<dyn Error>` in library code.
- `tracing` for logs; never `println!` in non-binary crates.
- Public API: doc-comment every item.
- `#[must_use]` on builder returns and important Results.

### TypeScript

- `strict: true`. No `any` without an inline `// SAFETY:` comment.
- React 19 idioms only — no class components, no legacy lifecycle hooks.
- ESLint config is law; do not loosen rules in a feature branch.
- Tailwind v4 only — no inline styles for layout, no CSS-in-JS.
- Design tokens from `ui/src/design/tokens.css` (skill: design-tokens).
- `@tanstack/*` is banned (skill: supply-chain).

### Tests

- Mirror source layout: `src/foo.rs` ↔ `tests/foo.rs` or `mod tests` block.
- One assertion per test where possible; clearly named.
- `rstest` for parameterized cases; `proptest` for invariants.
- Mock at trait boundaries (`mockall::automock`).
- E2E + visual tests via Playwright; qa-kitten authors / reviews them.
- Snapshot tests are allowed only for stable structural output
  (e.g. AST), not for user-visible text that may change wording.

## When something feels off

- If a function is hard to test → refactor until it isn't.
- If a test starts flaking → the test is wrong OR the production code
  has a race. Either is a bug. Never `#[ignore]` to ship.
- If clippy warns "this is fine for now" → it isn't; suppress with a
  named lint and a comment explaining the carve-out.

## Cross-references

- PLAN Section 11 — workflow + judges
- PLAN Section 12 — "do not skip" list
- `.code_puppy/judges-template.json` — judge prompts that score iterations
