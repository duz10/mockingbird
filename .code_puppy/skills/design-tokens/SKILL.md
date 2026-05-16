---
name: design-tokens
description: UI design tokens (colors, type scale, spacing, radii, motion) for Mockingbird's HUD and history viewer. Activate this skill whenever you are touching anything under `ui/src/design/`, building a new component, or reviewing a PR that introduces hard-coded values.
---

# Mockingbird design tokens

## Files of record

- `ui/src/design/tokens.css` — CSS custom properties (the canonical source)
- `ui/src/design/tokens.ts` — typed re-export for component code
- `ui/tailwind.config.ts` — maps tokens onto Tailwind v4 utility classes

## Hard rules

1. **No raw hex/RGB values in component code.** Every color reference
   goes through a token. If you need a new color, *add a token*
   rather than inlining.

2. **No magic numbers for spacing.** The spacing scale is
   `0, 2, 4, 8, 12, 16, 24, 32, 48` (in px), tokenized as
   `--space-0 ... --space-9`. Don't introduce `13px`.

3. **Type scale is fixed.** `xs / sm / base / lg / xl / 2xl / 3xl`.
   New sizes require a token discussion (and probably aren't needed).

4. **Motion is opinionated.** Three durations (`fast=120ms`,
   `base=240ms`, `slow=480ms`) and two easings (`out-quad`, `out-cubic`).
   Custom timings need a justification.

5. **HUD and main window share tokens.** They differ in *use*, not in
   palette. The HUD's accent color is the same token as the main
   window's accent.

## Palette philosophy

- Two themes: light, dark. Auto-follows OS theme.
- Both themes use the same semantic tokens (`--color-fg`, `--color-bg`,
  `--color-accent`, `--color-success`, `--color-warn`, `--color-error`,
  `--color-mute`) — the *values* differ, the *names* don't.
- The HUD's recording-active state uses `--color-accent` for the ring,
  not red. Red is reserved for errors.

## Component conventions

- Components live under `ui/src/components/<Name>/` with `index.tsx`,
  `<Name>.module.css` (if needed), and a `<Name>.stories.tsx` (Phase 5+).
- Tailwind classes are preferred; module CSS only when Tailwind would
  produce a soup of utility classes longer than ~5.
- No `style={{...}}` for layout — Tailwind only.
- ARIA labels on every interactive element. The HUD is keyboard-driven
  first (hotkey-triggered).

## Cross-references

- PLAN Section 9 — UI architecture
- PLAN Section 9.1 — token table
- skill: quality (TypeScript / React rules)
- ADR 0008 (Tailwind v4 + tokens.css over CSS-in-JS) — write if not present
