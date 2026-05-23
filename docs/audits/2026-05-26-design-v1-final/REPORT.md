# Mockingbird Design System v1 — Re-Audit (Final)

**Date:** 2026-05-26 (re-run)
**Auditor:** qa-kitten-4f162e
**Orchestrator:** Bernard (code-puppy)
**Baseline:** [`../2026-05-26-design-v1-baseline/REPORT.md`](../2026-05-26-design-v1-baseline/REPORT.md)
**Viewport:** 1920×1080, dark theme
**Build:** Vite dev server `http://localhost:5173/` (browser fixture mode)
**Pages walked:** Insights, Dictations, Meetings, Activity, Dictionary, Modes,
Settings (General / Meetings / Advanced), About, Command Center (welcome).

---

## TL;DR

**Verdict: SHIP. ✅**

- **8 of 8 P1** baseline findings → **PASS**
- **9 of 12 P2** baseline findings → **PASS**
- **3 P2** reclassified as **FALSE-POSITIVE** after deeper analysis
  (P2-04 mixed `:focus`, P2-09 Modes cards photo-bleed, P2-10 About cards
  photo-bleed)
- **0 new regressions** detected
- **2 known P3 follow-ups** explicitly out-of-scope and filed
  (`mb-5856` SettingsMeetingTab toggle markup, `mb-km6j` segmented-control
  consolidation)

---

> **Note on baseline ID mapping.** Kitten's verdict-table priorities (P1-01..P1-08, P2-01..P2-12)
> are renumbered relative to the original baseline IDs (the audit consolidated
> Activity's three baseline P1s — bare cards / bare buttons / zero token consumption —
> into one root-cause PASS row, since they all collapsed to the dead-token-name fix
> at the same time). The substance maps 1:1: every baseline finding has a verdict.

---

## 1. Verdict table

### P1 — Ship-blockers

| ID | Symptom | Verdict | Justification | Screenshot |
|---|---|---|---|---|
| P1-01 | Outline buttons app-wide have `background: transparent` → invisible on photo bg | **PASS** | All `<Button variant="ghost"\|"danger">` instances now render `rgba(255, 240, 225, 0.06)` glass-faint fill (Copy injected text, Mark as style example, Delete dictation, Run, Copy markdown, Export markdown, Delete meeting, Delete session, Copy as Markdown, PDF (Full), PDF (Work Report), Delete block ×2, Edit, Use this mode). Single-source-of-truth fix in `primitives.module.css` `.btn_ghost` / `.btn_danger`. Probe: 6/6 outline buttons on Activity have `alpha: 0.06, borderWidth: 1`. | `dictations.png`, `meetings.png`, `activity.png` |
| P1-02 | Activity page entirely skipped the design system (bare floating text, no card surfaces) | **PASS** | Smoking-gun fix. Legacy-alias bridge in `tokens-v2.css` resolves dead pre-W6 names (`--surface-1/2/3`, `--text-1/2`, `--border-subtle`, `--accent-1`) to live M3 surfaces. Probe: `_listPane = rgba(255,240,225,0.08)` + `blur(12px) saturate(1.6)`; `_detailPane = rgba(255,240,225,0.12)` + blur; `_card = rgba(255,240,225,0.06)`. All 4 summary-block action buttons + 2 Delete-block + Delete-session now render with visible affordances. | `activity.png` |
| P1-03 | Settings retention inputs fail WCAG (cream text on near-white input) | **PASS** | Dark-pill recipe applied. Probe: all 3 retention inputs have `background: rgb(20, 16, 12)` (lum=16.74) with `color: rgb(245, 234, 224)` — contrast ≫ 4.5:1. Same recipe applied across Modes number inputs and Settings → Meetings inputs. | `settings-general.png` |
| P1-04 | `100vh` used throughout → mobile/dynamic-toolbar viewport breakage | **PASS** | CSS grep across 16 stylesheets: **zero live `100vh` rules**. The 2 remaining matches in `src/design/global.css` are inside explanatory comments. 9 live `100dvh` rules counted. | _(CSS grep evidence)_ |
| P1-05 | Settings → Meetings paragraph-break-gap was a native HTML5 range slider | **PASS** | Themed pill (cream track, coral thumb, value indicator) on Settings → Meetings. Native chrome gone. | `settings-meetings.png` |
| P1-06 | Settings → Meetings dropdowns use native chrome chevron | **PASS** | Custom SVG chevron now on Hotkey modifier, Hotkey main key, Default audio source, Provider dropdowns. Consistent across Settings, Modes, Meetings. | `settings-meetings.png`, `modes.png` |
| P1-07 | Double-scroll bug: outer shell scrolls AND inner list scrolls (ghost scrollbar at app level) | **PASS** | Probe per page: Insights 1 active scroller, Dictations 2 (`_content` shell + scoped sidebar — intended), Meetings 0, Activity 0, Dictionary/Modes/Settings/About 0. Fix landed in `App.module.css` (`.shell` bounded with `grid-template-rows: minmax(0,1fr)`, `.content` gets `scrollbar-gutter: stable` + `overscroll-behavior: contain`) + per-page `.leftPane` (`position: sticky; max-height: calc(100dvh - 140px)`). `scrollbar-gutter` rule count = 5. | `dictations.png`, `meetings.png` |
| P1-08 | `SettingsMeetingTab` toggle text clips into a narrow 80–100px column | **PASS** | `.toggle` CSS now `inline-flex` so the label flows next to the pill. Verified visually: "Pause meeting hotkey / Active", "Strip filler words / On", "Enable optional LLM polish pass / Enabled (Ollama, opt-in per export)" all wrap normally across full row width. Note: visual pill graphic still absent (markup gap, not CSS — see `mb-5856` follow-up). | `settings-meetings.png` |

### P2 — Polish / consistency

| ID | Symptom | Verdict | Justification | Screenshot |
|---|---|---|---|---|
| P2-01 | Glass tier semantics not codified | **PASS** | New canonical tokens in `tokens-v2.css`: `--surface-glass-strong` (0.82 alpha under photo-bg), `--surface-glass-soft` (0.72), `--surface-glass-faint` (0.60). Mapped to existing `--glass-tint-thick/regular/thin` primitives so the `[data-photo-bg]` cascade still fires. 31 `--glass-tint*` references counted across CSS. | _(token-defs evidence)_ |
| P2-02 | Sidebar list-pane scroll competes with shell scroll | **PASS** | Covered by P1-07. `.leftPane` is `position: sticky; align-self: start` with scoped inner-list scroll only. | `dictations.png` |
| P2-03 | Blur recipes inconsistent (16–24px varied per surface) | **PASS** | All blur recipes normalized to `blur(var(--glass-blur-cap))` = 12px. New `--glass-blur-cap` token. | _(probe evidence)_ |
| P2-04 | Mixed `:focus` / `:focus-visible` usage | **FALSE-POSITIVE** | All 8 bare `:focus` selectors target `<input>` / `<select>` elements where bare `:focus` is the **correct** affordance — mouse-clicking into a field should show focus state. Global rule already applies `:focus-visible` to `button, a, [role=button], [tabindex]`. 14 `:focus-visible` rules counted. | _(CSS grep evidence)_ |
| P2-05 | Page-level scroll uses inconsistent `100vh` references | **PASS** | Covered by P1-04. | _(CSS grep evidence)_ |
| P2-06 | Settings retention "Save" / "Sweep now" actions look like body text | **PASS** | Inline link-style buttons with hover affordance — consistent with link tokens. | `settings-general.png` |
| P2-07 | Mode-card "Active" pill vs "Use this mode" outline button inconsistent state visual | **PASS** | Active card: coral-border + coral-filled "Active" pill. Inactive: glass-faint "Use this mode" outline. Visual hierarchy clean. | `modes.png` |
| P2-08 | Dictation list-card chip colors not aligned with mode palette | **PASS** | Mode chips: CASUAL (sage green), NORMAL (warm coral), FORMAL (cream/neutral), ABORTED (red). | `dictations.png` |
| P2-09 | Modes 6 mode cards photo-vulnerable | **FALSE-POSITIVE** | Mode cards use `background: linear-gradient(var(--glass-tint-regular) 0%, var(--glass-tint-thin) 100%)` — that's `background-image` (gradient), NOT `background-color`. The `[data-photo-bg]` cascade overrides the underlying tint primitives so the gradient automatically darkens. Photo-bleed exercise confirms cards visibly darken to near-black under bright photo bg; body text fully legible. Kitten's earlier `getComputedStyle().backgroundColor` probe couldn't see gradients. | `modes-photobg.png` |
| P2-10 | About 4 cards photo-vulnerable | **FALSE-POSITIVE** | Same root cause as P2-09. About uses canonical `<Card>` primitive with the same gradient-of-tint-tokens recipe. Photo-bleed exercise confirms cards darken; body text legible. | `about-photobg.png` |
| P2-11 | Tab affordances styled as transparent buttons | **PASS (by design)** | These are `background: transparent; border: 0` — the **correct** tab affordance (underline + active-color state, no fill). Both Insights and Meetings tab strips have active-tab underline visible. | `insights.png`, `meetings.png` |
| P2-12 | Console errors / warnings on page navigation | **PASS** | 0 errors, 0 warnings across all 8 main routes during the page walk. | _(probe evidence)_ |

### P3 — not individually re-verified

Per the re-audit brief, P3s were not individually walked. Two known P3
carry-overs filed as separate beads:

- **`mb-5856`** — SettingsMeetingTab toggles missing canonical `.toggleTrack`
  markup. Text flow is fixed; visual pill flip is not.
- **`mb-km6j`** — 3 segmented-control patterns not consolidated.

Settings → Advanced "Learning loop" toggle (visible coral pill, flipped right)
proves the canonical toggle markup works elsewhere — the gap is scoped to
`SettingsMeetingTab` only.

---

## 2. New regressions

**None detected.**

Probe confirms across all 8 main routes:

- 0 console errors / 0 console warnings
- 0 live `100vh` rules
- All canonical glass-tint cascade points resolve under `[data-photo-bg]`
- All probed inputs have dark fill (luminance ≪ 200)
- All probed outline buttons have `alpha ≥ 0.06` with visible border
- `scrollbar-gutter: stable` present on the single page-level scroller

Visual diff vs baseline shows **only improvements** — no surface regressed,
no chip lost color, no text became illegible.

---

## 3. Overall verdict

# **SHIP** ✅

All 8 P1 ship-blockers from baseline are resolved. 9/12 P2 findings resolved;
the remaining 3 are documented false-positives where the baseline reading
missed a non-`background-color` recipe (P2-09, P2-10) or correct accessibility
behavior (P2-04). No hotfix required.

---

## 4. Probe data appendix (selected highlights)

### Activity (the smoking-gun page)

```
panes:
  _listPane:   rgba(255,240,225,0.08) + blur(12px) saturate(1.6)
  _detailPane: rgba(255,240,225,0.12) + blur(12px) saturate(1.6)
  _panel:      rgba(255,240,225,0.08) + blur(12px) saturate(1.6)
  _card:       rgba(255,240,225,0.06)
outlineBtns (6):
  Delete session / Copy as Markdown / PDF (Full) /
  PDF (Work Report) / Delete block ×2
  all → rgba(255,240,225,0.06), borderWidth: 1
errors: 0, warns: 0
```

### Settings retention inputs (the WCAG-fail page)

```
inputs: bg rgb(20,16,12) lum=16.74, color rgb(245,234,224) contrast ≫ 4.5:1
```

### Photo-bg cascade probe

```
--glass-tint-regular:    rgba(15, 11, 8, 0.72)   ✓ near-black 72%
--glass-tint-thick:      rgba(15, 11, 8, 0.82)
--glass-tint-thin:       rgba(15, 11, 8, 0.60)
--surface-glass-strong:  rgba(15, 11, 8, 0.82)
--surface-glass-faint:   rgba(15, 11, 8, 0.60)
```

### Cross-cutting CSS grep (16 stylesheets)

```
100vh:           2  (both in explanatory comments)
100dvh:          9  (live)
:focus:          9  (8 on inputs/selects, 1 in comment)
:focus-visible: 14  (live, buttons/links)
scrollbar-gutter: 5
backdrop-filter: 37
--glass-tint:   31
data-photo-bg:  12
```

---

## 5. Workflow note

The existing `mockingbird_design_v1_baseline_audit.md` qa-kitten workflow was
followed verbatim and worked end-to-end. No workflow updates needed for
future audits.

The Command Center modePicker / sessionCard states remain the known
unreachable-from-browser gap (require `page.addInitScript()` to set
`__MOCKINGBIRD_FIXTURES__` pre-mount; not exposed by `cp_browser_*` tools).
Worth a future ADR if we want full-CC visual coverage in browser-mode
fixtures.
