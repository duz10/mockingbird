# ADR 0023 — Design Language v1 (Mockingbird earth-tone Liquid Glass)

**Status:** DRAFT — awaiting Dustin sign-off · 2026-05-17
**Phase:** TBD (likely a dedicated mini-phase between Phase 4 and Phase 5; see Open Questions)
**Supersedes:** the implicit "blue-grey cool-dark + Inter" style established ad-hoc across Phases 1-4 and codified loosely in `ui/src/design/tokens.css`.

## Context

The UI today is a competent but generic dark-mode dashboard:

- Tokens in `ui/src/design/tokens.css` use cool blue-grey OKLCH (`oklch(0.16 0.01 240)` family), Inter + JetBrains Mono, a four-step radius scale, and basic linear shadows.
- Components are bespoke (`components/primitives.tsx`, `Sidebar.tsx`) without an explicit type/elevation/motion system above them.
- Pages (History, Modes, Settings, Insights, Dictionary, About, recording overlay) each carry their own CSS module; consistency is by convention, not by token enforcement.

Dustin commissioned a comprehensive design language — delivered as a
single 115 KB HTML reference now at `docs/design/design-language-v1.html`
— that defines:

- **Brand:** new mark (3 ellipses + play triangle), gradient/mono/cream lockups, squircle app icon with radial terracotta gradient.
- **Color:** warm earth dark palette. Terracotta primary (`#D89060`), sand secondary (`#B5A77E`), muted blue-grey tertiary (`#88B5C5`), with full M3 role mapping (`--md-sys-primary`, `--md-sys-on-primary-container`, …). Reference tonal palettes at 10/20/30/…/99 for each family.
- **Typography:** Fraunces variable serif (display + headline, `opsz`/`SOFT` axes), DM Sans (title + body), IBM Plex Mono (label + code/numbers). Full M3 type scale (`display-large` → `label-small`).
- **Shape:** 7-step scale (4/8/12/16/24/32/full) with role guidance — "match radius to function, not size."
- **Spacing:** 4dp grid, 12 steps (4 → 96).
- **Elevation + materials:** M3 elevation tokens *plus* a signature **Liquid Glass** material — `backdrop-filter: blur() saturate()` over a warm ambient background gradient, four tints (ultra-thin, thin, regular, thick).
- **Motion:** M3 easing curves (emphasized / standard / accel / decel) + 12-step duration scale (50 ms → 600 ms).
- **Logo motion:** four named states for the mark itself — `splash` (entry), `exit`, `active` (oscillating loop while recording), `idle` (collapsed circles).
- **Iconography:** outlined, 1.5 px stroke, currentColor.
- **Components:** filled / tonal / outlined / text / **glass** / icon buttons; FAB; text + search-glass inputs; switch; chip; segmented; list item; dialog; toast.
- **App surfaces:** **listening pill** as the signature floating chrome (replaces today's recording overlay), transcript panel, settings panel.
- **Voice & tone:** four principles + paired do/don't examples. Microcopy doctrine.

This is a holistic system, not a paint job. Every token name, every
component primitive, every interaction surface changes. The current
`tokens.css` and `primitives.tsx` won't survive in their current form.

## Decision

**Adopt the design language as `data-design="v2"` and migrate the entire
UI in waves**, with the old system preserved behind the absence of that
flag during the cutover. Specifically:

1. **`docs/design/design-language-v1.html` is the canonical reference.** Any pixel-level disagreement between code and the HTML is a bug in the code, not the doc. The doc evolves via PRs the same way ADRs do.
2. **New token file `ui/src/design/tokens-v2.css`** carries the M3-role + warm-palette + Liquid-Glass tokens. The existing `tokens.css` stays untouched until the cutover wave so both styles can coexist during migration.
3. **Activation via root attribute:** `<html data-design="v2">` opts the whole document tree into v2. Set via a Zustand-backed setting that defaults to `v1` until cutover.
4. **Self-host all three font families** (Fraunces, DM Sans, IBM Plex Mono) under `ui/public/fonts/` and serve via `@font-face`. **No Google Fonts CDN** — the standing project rule forbids telemetry and CDN-hosted fonts ping Google on every cold page load (see ADR for the no-telemetry stance).
5. **Brand mark becomes a typed React component** (`components/brand/MockingbirdMark.tsx`) that accepts a `state` prop (`idle | active | splash | exit | static`) and renders the SVG with the matching CSS class. Used everywhere the mark appears.
6. **Liquid Glass is a utility-class system** (`.glass`, `.glass-thin`, `.glass-thick`, `.glass-ultra-thin`) plus the body-level ambient warm-blob background. Components compose these instead of redefining their own translucent surfaces.
7. **The recording overlay is rebuilt as the Listening Pill** — same code module (`ui/src/recording/`), new visual contract. The audio + IPC plumbing is unchanged; only the React tree + CSS swap.
8. **Phase-out is hard** — once cutover lands, `tokens.css` + `primitives.module.css` + every legacy `*.module.css` page CSS is deleted in the same commit. No long-lived dual code paths.

## Wave plan

Six waves, sequenced so the app stays runnable + dictation-functional
after every commit. Each wave is its own bead (children of an epic).

| Wave | Scope | Beads (titles) |
|---|---|---|
| **W1 Foundations** | tokens-v2.css, self-host fonts, type scale classes, spacing/shape/radius/elevation/motion tokens, `data-design="v2"` switch, reduced-motion + forced-colors guards | `dlw1-tokens`, `dlw1-fonts`, `dlw1-flag` |
| **W2 Materials & motion** | Glass utility classes, ambient warm-blob body bg, M3 elevation tokens for solid surfaces, motion easing/duration tokens, **MockingbirdMark** React component with the 4 animation states, reduced-motion fallback | `dlw2-glass`, `dlw2-mark`, `dlw2-motion` |
| **W3 Component primitives** | Button family (filled/tonal/outlined/text/glass/icon/FAB), Input (text + search-glass), Switch, Chip, Segmented, ListItem, Dialog, Toast — all in `ui/src/design/components/` with stable props | `dlw3-buttons`, `dlw3-form-controls`, `dlw3-surfaces`, `dlw3-iconography` (outlined 1.5 stroke pass) |
| **W4 Page migrations** | Sidebar, Settings, History, Modes, Dictionary, Insights, About — one page per commit behind the v2 flag | `dlw4-sidebar`, `dlw4-settings`, `dlw4-history`, `dlw4-modes`, `dlw4-dictionary`, `dlw4-insights`, `dlw4-about` |
| **W5 Recording surface** | Recording overlay → Listening Pill rebuild. Transcript panel. Splash + exit logo animation on app start/quit. | `dlw5-listening-pill`, `dlw5-transcript`, `dlw5-splash` |
| **W6 Voice/tone + cutover** | Audit `i18n/en.json` against the 4 voice principles, rewrite errors per principle #3, numbers→mono throughout. Flip default from v1 → v2. Delete `tokens.css` + legacy primitives + legacy CSS modules. Seal commit + LESSONS retro. | `dlw6-microcopy`, `dlw6-cutover`, `dlw6-seal` |

Estimate: ~3-5 days of focused work for W1+W2, then ~1 day per W4 page, then ~2 days for W5, then ~1 day for W6. Calendar-wise: 2-3 weeks of evening sessions.

## Consequences

**Positive:**

- A **single source of truth** for visual + interaction language. New
  pages stop being design exercises.
- **M3 token names** match a published spec — onboarding any future
  contributor (or a vendored component library) is straightforward.
- The Liquid Glass material gives Mockingbird a recognizable surface
  — competitors look generic against it.
- The Mockingbird mark with its 4 animation states gives the app
  built-in personality without requiring custom illustration per
  state.
- Self-hosted fonts mean **zero third-party network traffic on cold
  start** — matches the project's no-telemetry stance.

**Negative + mitigations:**

- **Three font families ≈ 600-900 KB** of WOFF2 bundled into the
  app. The current install ships ~80 KB of font weight (system
  Inter fallback). Mitigation: subset Fraunces + DM Sans to Latin
  basic + Latin extended only; ship only weights actually used
  (300/400/500/600 for sans, 400/500 for serif, 400/500 for mono).
- **`backdrop-filter` performance** on the recording overlay matters
  — that surface is on-screen during every dictation. Mitigation:
  measure FPS on the RTX-2060 box during W5; if regressions show,
  fall back to a flat-tinted `--glass-tint-thick` solid for the
  pill specifically. The doc's "decisive finish" motion principle
  supports it.
- **Cool→warm palette change is jarring mid-migration.** Mitigation:
  the `data-design="v2"` flag is page-scoped via CSS cascade — only
  pages migrated to v2 markup get the warm look; unmigrated pages
  stay cool. No half-and-half pages.
- **Light theme is not defined** in the design HTML. Mitigation:
  defer the light theme to post-cutover. The current `tokens.css`
  light overrides only ever applied automatically via
  `prefers-color-scheme`; if any user explicitly toggled light,
  W6 surfaces a "Light theme coming soon" toast on attempted
  toggle.
- **The Listening Pill replaces, not refines, the recording overlay.**
  Mitigation: keep the IPC contract identical (the React tree
  changes, the Tauri command surface does not). All Phase 3 hotkey
  + clipboard + injection invariants stay in force.
- **`tokens.css` deletion in W6 will break any out-of-tree CSS** that
  references the old `--surf-*` / `--mode-*` names. Mitigation: a
  grep gate in W6 that fails the commit if any `--surf-` or pre-v2
  `--mode-` reference survives outside known archive paths.

## Open questions (for Dustin before promoting from DRAFT)

1. **Phase placement.** Is this its own mini-phase (e.g. "Phase 4.5 — Design Language v1") with a `phase-4.5-complete` tag at W6 seal? Or does it become Phase 5 outright, with the original Phase 5 scope (whatever that was) deferred? Or do we just ship under "postship Wave N" headers like the current cleanup work?
2. **Light theme:** drop entirely, keep `prefers-color-scheme` auto-switch with a hand-derived light palette, or defer with a "coming soon" toast?
3. **Listening Pill UX scope:** is it strictly a visual rebuild of the existing recording overlay (same trigger, same lifecycle), or do we also lean into the design doc's "tap to start" / "tap to pause" / "double-tap to drop a marker" interactions? The latter is a real UX scope expansion that would need its own ADR.
4. **Fonts subsetting + bundling tool:** do we want me to wire `glyphhanger` or `fonttools-subset` into the build, or just commit pre-subset WOFF2s by hand once?
5. **Cutover risk tolerance:** is the W6 big-bang acceptable, or do you want the v2 flag exposed in Settings → General for the first week post-cutover so you can flip back if something feels off?

## Reversibility

The wave plan is reversible up to W6. Through W5, the legacy `tokens.css` + every legacy CSS module is intact; flipping `data-design` back to absent restores the current look. After W6 lands the deletion commit, rollback means a `git revert` of that one commit.

## Implementation notes (sketch)

- W1 ships ONLY tokens + the flag. No visual change to any page (because no page references the new tokens yet). This is the safest possible first wave and lets us prove the font loading + flag plumbing in isolation.
- W2 ships utilities + the mark. Visual change: only the brand
  asset wherever it's already rendered (sidebar header, About
  page). Validates Liquid Glass on a real surface (the sidebar).
- W3 ships components but no page uses them yet. Add a `/design-system` developer-only route that renders every component in every state — the in-repo equivalent of the source HTML, but interactive. Acts as W3's smoketest target.
- W4 migrations are one PR per page. Each migration deletes the page's old CSS module in the same commit so legacy + v2 styles can't coexist within one page.
- W5 rebuilds the recording overlay. The IPC payload is unchanged; only the React tree + CSS change.
- W6 is the one-commit cutover + cleanup + seal.

## References

- `docs/design/design-language-v1.html` — canonical visual spec
- `ui/src/design/tokens.css` — what gets replaced
- `ui/src/components/primitives.tsx` — what gets replaced
- ADR 0008 (append-only DB migrations) — applies if any new
  settings rows are needed for theme toggle
- ADR 0011 (Windows toolchain) — no new toolchain deps from this
  change; fonts are content
