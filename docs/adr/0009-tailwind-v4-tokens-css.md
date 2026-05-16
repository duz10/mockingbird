# ADR-0009: Tailwind v4 + `tokens.css` over CSS-in-JS

- **Status:** Accepted
- **Date:** 2026-05-15
- **Deciders:** Dustin, code-puppy-adeb7b

## Context

PLAN §9.1 specifies a CSS-custom-properties-based design token system
at `ui/src/design/tokens.css`. The styling layer needs to (a) consume
those tokens uniformly, (b) keep bundle size small, (c) avoid runtime
style computation, and (d) play nicely with the small surface area
of HUD + history viewer UIs.

## Decision

Use **Tailwind v4** as the utility layer, with token values exposed
through `@theme` (Tailwind v4's native CSS-variable theming) sourced
from `ui/src/design/tokens.css`. Module CSS allowed for narrow cases
where the Tailwind class soup would exceed ~5 utility classes for
the same element.

**CSS-in-JS libraries (styled-components, emotion, stitches, etc.)
are NOT used.** No `style={{ ... }}` for layout.

## Consequences

- **Positive:** zero runtime style cost, design tokens travel
  through CSS custom properties (themable, browser-native),
  Tailwind v4's `@theme` block is the right shape for our scale.
- **Negative:** dynamic style values (e.g., user-chosen accent color
  picker) require an inline `style={{ "--color-accent": userColor }}`
  override — explicitly allowed as a carve-out for *token override*
  only, not for layout.
- **Neutral:** the `design-tokens` skill is the binding reference
  for which token to use where.

## Alternatives considered

- **CSS Modules only (no Tailwind):** more boilerplate per component
  for spacing/layout we'd otherwise express in utility classes.
- **vanilla-extract:** type-safe and zero-runtime, but adds a build
  dependency we don't need at our scale.
- **styled-components / emotion:** runtime style computation +
  hydration concerns; we ship a desktop app, not a SPA.

## Cross-references

- PLAN §9.1 (tokens table)
- `.code_puppy/skills/design-tokens/SKILL.md`
- (Phase 1+) `ui/src/design/tokens.css`, `ui/tailwind.config.ts`
