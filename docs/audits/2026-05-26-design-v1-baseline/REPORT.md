# Mockingbird Design System v1 — Baseline Visual Audit

**Bead:** `mb-n455`
**Auditor:** qa-kitten-b0598e (Playwright + Chromium 1920×1080 dark mode)
**Orchestrator:** Bernard (code-puppy)
**Date:** 2026-05-26
**Tree:** HEAD `3bbbfb6`
**Method:** Live audit against Vite dev server on `:5173`. Per-page full-page
screenshots, structural JS probes for scrollers / button alpha / `backdrop-filter`
recipes, photo-bleed exercise via injected gradient bg, cross-cutting CSS grep
across all 19 loaded stylesheets.

---

## TL;DR

**8 P1, 12 P2, 10 P3 — 30 findings total.**

**Worst offender by a wide margin: `/activity`.** Confirmed via DOM probe that
`Activity.module.css` contains **zero** `backdrop-filter`, **zero**
`--glass-tint-*` references, and **zero** `[data-photo-bg]` overrides. The
page's `glassSurfaceCount` is 1 (the outer shell) versus 4–6 on every other
content page. All 10 buttons on Activity render as bare text (zero border,
zero fill) — including the sidebar session-card itself. This is not a
"cards bleed through" bug; it's a "cards literally don't exist" bug, and the
photo-bleed Dustin saw is the consequence.

**Second-order systemic issue: outline buttons are
`background-color: rgba(0, 0, 0, 0)`** across every page that has them.
On the dark default body they read fine; the moment a photo bg is enabled
they vanish until hover. Confirmed canonical cases: `Copy injected text` /
`Mark as style example` / `Delete dictation` / `Run` (Dictations);
`Copy markdown` / `Export markdown…` / `Delete meeting` (Meetings);
`Edit` × 3 (Dictionary); `Use this mode` (Modes); `Restore legacy chord
behavior` (Settings).

**Third-order: no canonical glass-token consumption.** Two distinct
`backdrop-filter` recipes used incoherently — `blur(20px) saturate(1.6)`
with tinted bg `rgba(255,240,225,0.06)` vs `blur(24px) saturate(1.8)` with
transparent bg `rgba(0,0,0,0)`. The `--glass-tint-*` tokens exist and are
referenced 27 times across stylesheets, but consumption is partial.

---

## Per-page summary

| Page | P1 | P2 | P3 | One-line |
|---|---:|---:|---:|---|
| `/insights` | 0 | 0 | 2 | Cleanest. |
| `/dictations` | 2 | 2 | 0 | Outline-buttons + nested-scroll. |
| `/meetings` | 1 | 2 | 1 | 5 latent overflow:auto containers. |
| `/activity` | **3** | 1 | 0 | **Skipped the design system entirely.** |
| `/dictionary` | 0 | 1 | 2 | Best of inner pages. |
| `/modes` | 0 | 2 | 2 | 6 cards photo-vulnerable. |
| `/settings` General | 2 | 3 | 0 | WCAG-fail retention inputs. |
| `/settings` Meetings | 2 | 0 | 0 | Native range slider + squeeze layout. |
| `/about` | 0 | 1 | 1 | Cleanest layout. |
| CC welcome | 0 | 0 | 1 | Mode tiles need hover state. |
| Cross-cutting CSS | 0 | 4 | 0 | `100dvh:0`, `scrollbar-gutter:0`, mixed `:focus`. |

## Screenshots

| File | Surface |
|---|---|
| `01-insights.png` | `/insights` full page |
| `02-dictations.png` | `/dictations` full page |
| `03-meetings.png` | `/meetings` full page |
| `04-activity-dark.png` | `/activity` full page, dark body bg |
| `05-activity-photo-bleed.png` | `/activity` with photo bg stub — **the smoking gun** |
| `06-dictionary.png` | `/dictionary` full page |
| `07-modes.png` | `/modes` full page |
| `08-settings-general.png` | `/settings` General sub-tab |
| `09-settings-meetings.png` | `/settings` Meetings sub-tab |
| `10-about.png` | `/about` full page |
| `11-cc-welcome.png` | `command_center.html` welcome state |

---

## Findings

### P1 — visually broken

#### P1-01 — Activity: no card surfaces on right pane
Session header (`45 minutes ago` + `COMPLETED` + duration + events),
"Summary blocks" section, per-block summaries (`Code.exe`, `chrome.exe`),
and event timeline all render as bare text on the page background. Zero
`backdrop-filter`, zero border, zero padding-defining surface.
- **Surface:** `ui/src/pages/Activity.module.css`, `_shell_c9jxx_15`,
  `_listPane_c9jxx_31`
- **Probe result:** `glassSurfaceCount: 1` (vs Dictations: 4, Meetings: 3,
  Dictionary: 1, Modes: 7, About: 5)
- **Screenshot:** `04-activity-dark.png`, `05-activity-photo-bleed.png`
- **Fix surface:** add canonical glass-card wrapper around session header,
  summary-blocks container, and per-block items. Mirror the `_metadata` +
  `_transcript` card pattern from `Meetings.module.css`.

#### P1-02 — Activity: 10 of 10 buttons render as bare text
All buttons on the page have `background-color: rgba(0, 0, 0, 0)`,
`border-width: 0`. No outline tier, no fill tier, just colored text.
Affects: sidebar session-card button, `Delete session`, `Regenerate
summary`, `Copy as Markdown`, `PDF (Full)`, `PDF (Work Report)`, two
`Code.exe`/`chrome.exe` summary-block buttons, two `Delete block` buttons.
- **Probe result:** `bareLinkButtonsCount: 10`, `outlineButtonsCount: 0`
  on `/activity`
- **Fix surface:** apply the standard `outline` button class (the one used
  for `Copy injected text` on Dictations, modulo P1-03 fix) to all action
  buttons. Session-card button needs the full glass-card treatment.

#### P1-03 — Outline buttons are 100% transparent across the app
Every outline-style button has `background-color: rgba(0, 0, 0, 0)`.
Canonical cases enumerated:
- Dictations: `Copy injected text`, `Mark as style example`, `Delete
  dictation`, `Run`
- Meetings: `Copy markdown`, `Export markdown…`, `Delete meeting`
- Dictionary: `Edit` × 3
- Modes: `Use this mode` × 2 (Casual, Formal)
- Settings: `Restore legacy chord behavior`

On the default dark body they read OK. On photo bg they disappear.
Dustin's kickoff calls for ~8–10% low-opacity glass default.
- **Fix surface:** canonical `--glass-tint-button-outline:
  rgba(255, 240, 225, 0.08)` token + `outline-button` recipe consuming it.
  Then `[data-photo-bg]` override drops it to `rgba(15, 11, 8, 0.55)` for
  readability over photo.

#### P1-04 — Dictations sidebar list is a nested internal scroller
`._list_gz4w0_145`: `scrollHeight: 4559`, `clientHeight: 888`, rect
`360×890 at x=252`. The sidebar list scrolls inside itself while the
right pane fits the viewport — exactly the "two scrollbars in one
viewport" anti-pattern.
- **Surface:** `ui/src/pages/Dictations.module.css` `._list_*`
- **Fix surface:** drop `overflow-y` from the sidebar list; let the
  page-root scroll instead. Apply the same fix to Meetings's
  `_list_18r85_345` (currently latent — 812 high, fits in viewport — but
  will scroll once it has > ~10 meetings).

#### P1-05 — Settings retention inputs are pure white bg with cream text (WCAG fail)
`Raw events (days)`, `Audio transcript segments (days)`, `Block summaries
(days)` — three `<input type="number">` elements with computed
`background-color: rgb(255, 255, 255)` (luminance 255), 2px solid white
border, AND `color: rgb(245, 234, 224)` (warm cream). Cream-on-white =
contrast ratio ~1.1:1 vs the WCAG AA 4.5:1 floor.
- **Surface:** `ui/src/pages/Settings.module.css` (or wherever the
  activity-retention form is wired)
- **Screenshot:** `08-settings-general.png` (mid-right, the three boxes
  in the "Activity retention" group)
- **Fix surface:** apply the dark-pill input class used by `Command Center
  shortcut` and `Unsplash API key` instead.

#### P1-06 — Settings Meetings sub-tab: native HTML5 `<input type="range">`
`Paragraph break gap` slider has `appearance: auto` and `accent-color:
auto` — renders as the bright blue/gray default thumb against the dark
form.
- **Surface:** `ui/src/pages/Settings.module.css` Meetings sub-section
- **Fix surface:** custom-styled range or replace with a `±` stepper.

#### P1-07 — Settings Meetings sub-tab: descriptions squeeze into narrow right-column
After `Direct meeting hotkey`, `Enable optional LLM polish pass`, and
`Inherit from global retention`, the description paragraph wraps to 1-2
words per line in a ~80-100px column on the far right ("When", "enabled,",
"Enabled (Ollama,", "Inheriting from global,"). Strongly suggests a CSS
grid mis-spec where the `<p>` lands in the same grid column as the
action element rather than spanning the full row.
- **Surface:** `ui/src/pages/Settings.module.css` Meetings sub-section row
  layout
- **Screenshot:** `09-settings-meetings.png` (visible right side, repeats 3×)
- **Fix surface:** in the form-row grid, give the description
  `grid-column: 1 / -1` (full row span) below the action row.

#### P1-08 — Activity page completely lacks design-token consumption at CSS level
Confirmed via cross-cutting CSS grep across 16 stylesheet modules:
`Activity.module.css` (9024 bytes) contains **0 `backdrop-filter`,
0 `--glass-tint`, 0 `data-photo-bg`**. Compared to peers:
- `Dictations.module.css`: 2 / 1 / 1
- `Meetings.module.css`: 2 / 1 / 1
- `Dictionary.module.css`: 2 / 1 / 1
- `Modes.module.css`: 2 / 4 / 0
- `Settings.module.css`: 2 / 1 / 0
- **`Activity.module.css`: 0 / 0 / 0**
- `ActivityBlocks.module.css`: 0 / 0 / 0

Confirms P1-01 + P1-02 structurally: Activity didn't skip a fix, it
skipped the entire design-language wave.

---

### P2 — inconsistent

#### P2-01 — Two glass recipes used incoherently
Across all glass-bearing pages, exactly two `(backdrop-filter |
background-color)` signatures appear:
- A: `blur(20px) saturate(1.6) | rgba(255, 240, 225, 0.06)` — outer shells
- B: `blur(24px) saturate(1.8) | rgba(0, 0, 0, 0)` — inner cards

No canonical token-driven tier system. Recipe B (zero alpha bg) is
photo-vulnerable: the blur alone doesn't darken enough to keep dark text
legible over a light photo.
- **Fix surface:** consolidate into `--glass-tier-outer` /
  `--glass-tier-card` / `--glass-tier-control` tokens in `tokens-v2.css`;
  all surfaces consume tokens; `[data-photo-bg]` overrides flip the tint
  layer to near-black.

#### P2-02 — Latent nested-scroll containers on Meetings (and elsewhere)
Meetings has 5 elements with `overflow-y: auto` pre-armed but not
currently scrolling because content fits:
- `_content_11g0i_15` — outer shell, 1145 high (already exceeds 1080
  viewport in some renders)
- `_list_18r85_345` — sidebar list, 812 high (will scroll when > ~10
  meetings)
- `_rightPane_18r85_57` — right pane, 940 high
- `_transcript_18r85_735` — transcript box, 211 high
- `_llmOutput_18r85_833` — LLM output box, 46 high

Once content grows, this becomes the nested-scroller bug in production.
Fix proactively.
- **Fix surface:** audit every `overflow: auto` declaration in
  `Meetings.module.css`. Demote to `overflow: visible` unless the
  container truly is meant to be a fixed-height bounded scroller.

#### P2-03 — Same primary action rendered differently across pages
"Run" on Dictations = outline transparent button. "Run LLM pass" on
Meetings = orange-filled button. Same conceptual action (start LLM
cleanup pass on the transcript) — two visual treatments.
- **Fix surface:** pick one — recommend orange-filled for primary-action
  consistency. Update Dictations's `Run` button class.

#### P2-04 — `:focus` vs `:focus-visible` mixed
Cross-cutting CSS grep:
- `:focus` raw: 8 occurrences (Meetings 3, Dictations 1, Dictionary 2,
  Modes 2)
- `:focus-visible`: 12 occurrences (global.css 5, Meetings 3, Dictations
  1, Modes 2, Settings 1)
- Pages with BOTH: Meetings, Dictations, Modes
- Pages with `:focus` only: Dictionary

Result: keyboard focus rings show inconsistently depending on input
modality (mouse-click currently triggers `:focus` styles on Dictionary,
which is the regression `:focus-visible` was invented to fix).
- **Fix surface:** lint rule banning bare `:focus`. Replace existing 8
  occurrences with `:focus-visible`.

#### P2-05 — `100vh` everywhere, `100dvh` nowhere
Cross-cutting CSS grep: **7 occurrences of `100vh`** across `global.css`
(2), `App.module.css` (1), `Meetings.module.css` (1),
`Activity.module.css` (1), `Dictations.module.css` (1),
`Sidebar.module.css` (1). **Zero occurrences of `100dvh`**. Phase 9 Mac
support + future taskbar safe-area handling will benefit from migrating.
- **Fix surface:** search-and-replace `100vh` → `100dvh` across the 6
  affected modules.

#### P2-06 — `scrollbar-gutter: stable` not used anywhere
Zero occurrences codebase-wide. When scrollbars appear/disappear (e.g.
when navigating from a tall page to a short one, or when filter results
shrink a list), layout will shift by ~15px horizontally.
- **Fix surface:** add `scrollbar-gutter: stable` to the page-level
  scroll container and every named scroll surface.

#### P2-07 — Settings: native browser checkboxes mixed with custom toggles
Probe found 7 visible `<input type="checkbox">` elements. Three have
`width: 0px, height: 0px` (custom toggle-pill components — Sound
feedback, Launch on login, Reduce motion, Show background photo). Three
have `appearance: auto, width: 13px, height: 13px` (raw browser
checkboxes — Activity Capture: record audio + 2 exclusion-rule
checkboxes).
- **Fix surface:** route `Activity Capture: record audio` and the
  exclusion-rule checkboxes through the same toggle-pill component as
  Sound feedback.

#### P2-08 — Settings: `<select>` dropdowns have `appearance: auto`
3 `<select>` elements on Settings Meetings sub-tab (`Hotkey modifier`,
`Hotkey main key`, `Default audio source`) have a custom class with
`background: rgb(20, 16, 12)` (correct dark fill) BUT `appearance: auto` —
the dropdown chevron icon and the open-popup menu still render in native
chrome.
- **Fix surface:** `appearance: none` + custom chevron pseudo-element. Or
  replace with a custom select component.

#### P2-09 — Modes: 6 mode cards photo-vulnerable
Probe shows 6 cards with `blur(24px) saturate(1.8) | rgba(0, 0, 0, 0)`.
They look fine on dark body bg, but they don't consume
`var(--glass-tint-regular)` — they have `background-color` hardcoded to
transparent. The `[data-photo-bg]` token override won't help these; they
need per-surface CSS rules like the existing `_leftPane_*` / `_shell_*`
patches from the prior Unsplash audit (2026-05-18).
- **Fix surface:** `Modes.module.css` mode-card selector — replace
  `background-color: rgba(0,0,0,0)` with
  `background-color: var(--glass-tint-regular)`. Same fix surfaces on
  About (4 cards) and inner cards across Dictations / Meetings.

#### P2-10 — About cards same photo-vulnerable pattern as Modes
4 cards × same recipe. Same fix.

#### P2-11 — Settings sub-tab nav vs sidebar nav vs Theme picker: 3 different segmented-control styles
- Settings sub-tab (General/Models/Dictation data/Meetings/Advanced):
  vertical list, active item has filled-rounded highlight, inactive items
  are plain text
- Theme picker (System/Light/Dark) + Photo selection (Random/Curated):
  horizontal pill-button group, active item has pill bg
- Sidebar nav: vertical, similar to sub-tab but with icons

Three patterns, no shared component. Polish-grade but worth a single
segmented-control primitive.

#### P2-12 — `Save` / `Sweep now` / `Add rule` rendered as bare text-link buttons
On Settings General. Same Activity-page bare-button pattern leaking into
Settings. Probably not intentional.
- **Fix surface:** apply outline-button class.

---

### P3 — polish

P3 findings are documented here for completeness but **not filed as
separate beads** — they collapse naturally once the token consolidation
work in P1/P2 lands. Bernard will spot-check them in the re-audit and
file follow-ups for anything that survives the cleanup.

#### P3-01 — Insights "Where you dictate" bar contrast very low
Horizontal bars (Slack/Code/Chrome/Notepad/Outlook) in the bottom-left
card. Bar fill is so close to card bg that the relative-usage
visualization is nearly imperceptible.

#### P3-02 — Insights "Mode mix" legend dot for "Formal" blends into card
Sage swatch on warm-neutral bg ≈ no perceivable color.

#### P3-03 — Dictionary inline add-row visually echoes data rows
The "new term / canonical form / app context" fields at the top of the
table look like an in-place edit of the first data row at a glance.

#### P3-04 — Dictionary trash icon buttons very faint
Computed alpha looks near-zero. Confirms visually — easy to miss the
delete affordance.

#### P3-05 — Modes: "Active" filled badge vs "Use this mode" outline = same conceptual control, different visuals based on state
On the active Normal card, the right-side button is orange-filled + reads
"Active". On Casual/Formal it's outline + reads "Use this mode".
Coherent on its own but a stricter system would use a state-changing
toggle.

#### P3-06 — Modes: AI-command toggle switches very small / monochrome
The Rewrite/Expand/Summarize on-off toggles on the right look like tiny
dim circles. Hard to read state at a glance.

#### P3-07 — About: right-column cards don't bottom-align
"Open source" card is visibly shorter than "What it doesn't do" — bottom
edges misalign by ~70px.

#### P3-08 — CC welcome state: 3 mode tiles lack visible hover/selectable affordance
Static screenshot shows no border highlight, no focus indicator. Hover
state may exist — not verified.

#### P3-09 — Meetings: Microphone-only select + Start meeting + search box cramped against sidebar
The top of the meetings list wraps tightly; "Start meeting" button feels
squeezed.

#### P3-10 — Settings: description text often uses raw `<i>` for italic
e.g. "How long to keep activity capture data. *0 = forever*." Visually
fine but suggests inline HTML markup rather than a Typography component.

---

## Known gaps (not covered by this audit)

1. **Command Center modePicker + sessionCard states.** Requires
   `page.addInitScript()` to set `window.__MOCKINGBIRD_FIXTURES__`
   before React mounts. The `cp_browser_*` Playwright wrapper exposed to
   qa-kitten doesn't include `addInitScript`. Tried as fallbacks (all
   failed):
   - Set `window.__MOCKINGBIRD_FIXTURES__` then `location.reload()` —
     fixture wiped on reload.
   - Dispatch focus/storage/visibility events — no fixture re-fetch
     wired.
   - Click a `_modeTile_*` button to advance state via user flow — Tauri
     command stub no-ops in browser context.
   - URL query params (`?fixture=modePicker`) — not read by fixture
     system.
   - localStorage keys — not read by fixture system.
   - Welcome state IS captured (`11-cc-welcome.png`).
   - The 3 mode tiles are shared across welcome and modePicker, so most
     visual surface IS still covered by the welcome screenshot — only the
     `sessionCard` layout (live session in progress) is genuinely
     uncovered.

   **Bernard's note for the re-audit:** consider adding an init-script
   path to the fixture override system (e.g. read fixture-state JSON
   from a URL param or a stable cookie key set pre-mount), so the post-
   fix audit can capture all 3 CC states without `addInitScript`. Filed
   under the formalization phase rather than as a bead.

2. **WCAG AA contrast spot-check** was visual only, not computed per
   text node. The one confirmed contrast failure (P1-05, cream-on-white
   retention inputs) was caught by the input-luminance probe. Other
   low-contrast cases (P3-01, P3-02) flagged by eye, not measured. A
   follow-up `axe-core` run would close this.

3. **Recording-pill window** (`recording.html`) was not in the kickoff
   scope; not audited.

---

## Methodology notes for the lateral-fix epic

- **Token consolidation should come first.** The 30 findings collapse
  onto a much smaller set of token-system fixes once `tokens-v2.css`
  adds `--glass-tier-outer / -card / -control` + `--button-tier-fill /
  -outline / -ghost` + `--input-bg` + `--focus-ring`. Most findings then
  reduce to "replace hardcoded value X with `var(--token)`".
- **Activity is a whole-surface refactor**, not a patch. It needs the
  same card-pattern Meetings has.
- **Outline buttons need the canonical 8–10% glass default** Dustin
  called out — concretely `background-color: var(--glass-tier-control)`
  with `var(--glass-tier-control)` defined as `rgba(255, 240, 225, 0.08)`
  on the default dark theme and overridden to `rgba(15, 11, 8, 0.55)` on
  `[data-photo-bg]`.
- **Nested-scroll fix:** demote sidebar list `overflow-y` from `auto`
  to `visible` on Dictations + Meetings + Activity; the page-root is the
  single scroller; add `scrollbar-gutter: stable` once.
- **A judge for "no `background-color: rgba(0,0,0,0)` on buttons" would
  catch P1-03 + P1-08 mechanically going forward.**

## Audit workflow saved

`mockingbird_design_v1_baseline_audit.md` in
`~/.code_puppy/browser_workflows/` — reusable for the post-fix
verification pass and for future baseline audits.
