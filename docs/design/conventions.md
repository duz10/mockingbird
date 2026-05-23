# Mockingbird Design System v1 — Conventions

Short, binding ruleset for the Mockingbird UI. Chartered by bead `mb-n455`.
The audit that motivated this is at
`docs/audits/2026-05-26-design-v1-baseline/REPORT.md`.

## 1. Glass tiers — three, no more

Every glass surface MUST consume one of the canonical semantic tokens.
**No ad-hoc `rgba(...)` backgrounds on glass elements.**

| Token                     | Use case                                                |
|---------------------------|---------------------------------------------------------|
| `--surface-glass-strong`  | Modals, primary cards, session cards on photo background |
| `--surface-glass-soft`    | Secondary panels, list rows, sub-cards                  |
| `--surface-glass-faint`   | Outline-button default state, subtle dividers, ambient chips |

All three pair with `var(--md-sys-on-surface)` text at WCAG AA. The
`[data-photo-bg]` cascade swaps the underlying primitives to dark fills
when a photo background is on; no per-surface override needed.

**Blur cap:** `--glass-blur-cap` = `12px`. Heavier blurs look like
mid-2020s frosted-glass kitsch. Don't.

## 2. Sidebar-content layout — one scroller

| Region              | Rule                                              |
|---------------------|---------------------------------------------------|
| Page root           | `display: grid` two-column, `height: var(--viewport-height)`. Use `.app-shell`. |
| Sidebar column      | Fixed `var(--viewport-height)`, **no** internal `overflow`. Use `.app-shell-sidebar`. |
| Content column      | The ONE scroller. `overflow-y: auto`, `scrollbar-gutter: stable`, `overscroll-behavior: contain`. Use `.app-shell-content`. |
| Sidebar list panes  | **No** internal `overflow`. Items stack; the page scroll handles tall lists. Use `.app-list-pane`. |

Anti-pattern: **two scrollbars visible in one viewport.** If a sidebar
list grew its own scrollbar, you broke this rule.

## 3. Viewport height — `100dvh`, never `100vh`

Always consume `var(--viewport-height)`. It expands to `100dvh` which
handles browser-chrome and mobile safe-area correctly. Phase 9 Mac
support requires it.

## 4. Outline buttons default to faint glass

The `outlined` variant of the canonical `<Button>` ships with
`background: var(--surface-glass-faint)` by default — not transparent.
This keeps the affordance visible on photo backgrounds. Hover bumps to
`--surface-glass-soft`. **Don't author hand-rolled outline buttons in
page CSS modules — use `<Button variant="outlined">`.**

## 5. Focus rings — `:focus-visible` only

Bare `:focus` is forbidden — it lights up the ring on every mouse click,
which is the regression `:focus-visible` was invented to fix. The
canonical ring is `outline: var(--ring-focus)` + `outline-offset:
var(--ring-offset)`, applied inside `:focus-visible`. (The global rule
in `global.css` already covers `button`, `a`, `input`, `[role=button]`,
`[tabindex]` — page modules shouldn't reach for `:focus` at all.)

## 6. Motion — respect `prefers-reduced-motion`

Token-level `--duration-*` already collapses to `0ms` under
`@media (prefers-reduced-motion: reduce)` (see tokens-v2.css). For
**decorative** animation (UnsplashBackground blob drift, hover lifts,
etc.), wrap the keyframes / transitions in
`@media (prefers-reduced-motion: no-preference)` OR add the
`.motion-decorative` class which has a `!important` reduced-motion
opt-out.

## 7. Container queries over media queries inside components

For size-aware components, use CSS `@container` queries against a named
container set by the parent layout — not viewport-level `@media`. Page
layouts know their breakpoints; child components do not.

## 8. List virtualization — `react-virtuoso` only

For lists with potential `>100` items (transcripts, meetings, activity
sessions, blocks), use `react-virtuoso`. **Never** `@tanstack/*` — banned
by hook. Install via `npm install --ignore-scripts react-virtuoso`.

## 9. Native form controls — wrap or replace

Never ship `appearance: auto` `<select>`, `<input type="range">`, or
`<input type="checkbox">` on a user-facing form. Use the project's
custom toggle / select / stepper primitives, or commission one. Native
controls don't match the warm-neutral theme and break visual coherence.
