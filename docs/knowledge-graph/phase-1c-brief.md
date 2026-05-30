# Phase 1C Wave Brief — KG retrieval UX + activation toggle

**Bead epic:** `mb-j368`
**Charter:** [ADR 0049](../adr/0049-knowledge-graph-phase-0-5-and-v1-architectural-pivot.md)
§7 (Wave 1C row) + §6 (v1 binding commitments)
+ [ADR 0050](../adr/0050-kg-phase-1b-persistence-and-dictation-hook.md)
(consumed; schema + worker already on disk)
+ [ADR 0051](../adr/0051-kg-phase-1c-retrieval-ux-and-activation.md)
(sub-charter — D1-D8 decisions + UI sealed-surface authorization).
**Work container:** ADR-chartered lateral epic under ADR 0049's
§"Sandbox isolation" exception-window mechanism (per AGENTS.md
"Work sizing"). Sub-charter pattern mirrors ADR 0036 → ADR 0040
and Phase 1B's ADR 0050 → this ADR 0051 chain.
**No new `phase-*-complete` tag** (LESSONS P5).
**Cadence:** six waves.

| Wave | Beads | Deliverable |
|---|---|---|
| 1C.0 (this) | `mb-plz9` | ADR 0051 (Proposed) + this brief + epic bead chain + STATUS in-flight update + per-pass `PassTimings` instrumentation in `pipeline.rs` + worker structured tracing + `kg_latency_bench` bin + empirical baseline doc. Closes `mb-b3jy`. |
| 1C.1 | `mb-ucmx` | Settings KG tab (`SettingsKgTab.tsx`) + `KgGraphEnabled` activation toggle + scoped `kg_settings_get_all` / `kg_settings_set` IPC + boot-vs-poll worker promotion. Closes `mb-s6a8` + `mb-7w5f`. |
| 1C.2 | `mb-9ufg` | Failed-filings UX (list + per-row retry button) + `requeue_failed(queue_id)` store helper + 2 new Tauri commands (list + retry). Closes `mb-j3t1`. |
| 1C.3 | `mb-5ly5` | Dictations page retrieval filter chips (entity, tag, entry-type, status, category, free-text) + per-row entity/tag display + 2 new Tauri commands (list filter candidates, list entries by filter). |
| 1C.4 | `mb-sx6p` | Concept page modal — entity / tag chip click opens a modal of `kg_concept_entries_view` rows for that concept. 1 new Tauri command. |
| 1C.5 | `mb-f4gn` | Three deterministic invariant judges (graph-off-UI, retrieval-correct, failed-filing-retry-idempotent) + ADR 0051 Status → Accepted + STATUS sealed-row added + PRODUCT-STATE KG-section update + epic close. **No new `phase-*-complete` tag.** |

---

## §1. Scope: what lands across 6 waves

Cumulative public-surface delta when 1C ships:

| Layer | Wave | Artifact |
|---|---|---|
| Rust — KG pipeline | 1C.0 | `pipeline::PassTimings` (NEW pub struct) + `PipelineResult.pass_timings` (additive field) |
| Rust — KG worker | 1C.0 | Structured `tracing::info!(target: "kg::worker::latency", …)` per successful filing |
| Rust — KG worker | 1C.1 | `KgGraphEnabled` promoted to per-tick poll (read in drain loop, not just at boot) |
| Rust — KG store | 1C.2 | `kg::store::queue::requeue_failed(queue_id) -> AppResult<()>` |
| Rust — KG judges | 1C.5 | `kg::judges::*` module (3 deterministic invariants) |
| Rust — commands | 1C.1-1C.4 | New `commands/kg.rs` with ~6 commands: `kg_settings_get_all`, `kg_settings_set`, `kg_failed_filings_list`, `kg_failed_filing_retry`, `kg_filter_candidates_list`, `kg_entries_by_filter_list`, `kg_entries_for_entity_list` (concept modal). Registered in `commands/mod.rs` + `capabilities/default.json`. |
| Rust — bench | 1C.0 | `src-tauri/src/kg/latency_bench.rs` + `src-tauri/src/bin/kg_latency_bench.rs` |
| UI — pages | 1C.1 | `ui/src/pages/Settings.tsx` (1 line, add tab registration) + `ui/src/pages/SettingsKgTab.tsx` (NEW) |
| UI — pages | 1C.2 | Extends 1C.1's tab (may extract a `KgFailedFilings.tsx` sub-component if 600-LoC ceiling threatens) |
| UI — pages | 1C.3 | `ui/src/pages/Dictations.tsx` extended + sub-components extracted into `ui/src/pages/components/` |
| UI — pages | 1C.4 | `ui/src/pages/components/KgConceptModal.tsx` (NEW) |
| UI — lib | 1C.1-1C.4 | `ui/src/lib/tauri.ts` + `ui/src/lib/types.ts` extended additively per wave |
| UI — i18n | 1C.1-1C.4 | `ui/src/i18n/en.json` `kg.*` namespace |
| Docs | 1C.0 | This brief + `phase-1c-latency-baseline.md` + `phase-1c-latency-baseline-raw.csv` |
| Docs | 1C.5 | ADR 0051 Status → Accepted, STATUS sealed row, PRODUCT-STATE KG section |

---

## §2. Scope: what does NOT land

Explicit deferrals so future sessions don't accidentally absorb them
into 1C. These restate ADR 0051 §"Out of scope":

- **Phase 1D — backfill** of pre-Phase-1 dictations into the graph
  (explicit batch UX). Its own ADR.
- **Phase 1E** — v1 beta tag + UX polish + power-user surfaces
  (graph export, schema-aware advanced filters).
- **Cross-entity co-occurrence view** — "show me dictations
  mentioning both X and Y". Deferrable to 1D or later if usage
  patterns justify.
- **Full-page concept view** — D4 explicitly defers to v1.1+ if
  data justifies. Modal is the v1 contract.
- **Editing entities / tags from the UI** — entirely deferred to
  v1.1+. The graph is read-only from the UI in 1C.
- **Metrics table** — the latency baseline is log-only in 1C.0.
  If 1C.2's failed-filings UX surfaces a need for a metrics surface,
  that's a `bd create` + follow-up wave, not in 1C scope.
- **LLM-graded judges** — 1C invariants are all deterministic.
- **New migrations** — 1C consumes ADR 0050's migration 024 only.
  Confirmed 2026-05-30: `kg_filing_queue` already has
  `attempt_count INTEGER NOT NULL DEFAULT 0` and `last_error TEXT`
  columns, so `requeue_failed` doesn't need a schema change.
- **Meeting-capture filing.** MC remains sealed at `phase-mc-complete`.
  KG indexing of meeting transcripts is Phase 1D+ territory (the
  data model would need different segment semantics).

If a 1C diff reaches into any of the above, that's a scope leak —
escalate via STATUS and a bead, do not push through.

---

## §3. Binding decisions (D1–D8 codified)

Anchored here so future-session triage can quickly disambiguate the
wave's commitments without re-reading ADR 0051.

- **D1** Settings KG tab: extend `Settings.tsx` in place; body lives
  in a NEW `SettingsKgTab.tsx` to respect the 600-LoC ceiling.
- **D2** Activation flow: silent flip + tooltip explaining "takes
  effect next dictation" (or, post-1C.1, within ~5s next worker tick).
  No confirmation modal.
- **D3** Failed-filings: per-row truncated `last_error` (~80 chars)
  with full text on hover; per-row "Retry" button calls
  `requeue_failed`.
- **D4** Concept page: **modal in v1.** Full-page deferred to v1.1+.
- **D5** Latency baseline: pulled into 1C.0 (this wave). Already
  measured 2026-05-30 — see `phase-1c-latency-baseline.md`.
- **D6** Boot-vs-poll worker promotion: lands in Wave 1C.1 alongside
  the UI surface that motivates it.
- **D7** ~6 new Tauri commands acceptable; capabilities/default.json
  registration is per-wave discipline (ADR 0035 precedent;
  forgetting it is the #1 cause of "command not found" runtime
  failures).
- **D8** This ADR opens a scoped `ui/**` authorization window per
  ADR 0051 §"UI sealed-surface authorization". Outside that list,
  the Phase 8 + ADR 0037 seals hold.

---

## §4. Acceptance gate procedure (Wave 1C.5)

All three judges must be **green** before flipping ADR 0051 to
Accepted. All deterministic (no LLM grading); each judge is a
`#[cfg(test)]` test or a `[[bin]]` invariant probe (TBD by 1C.5
author).

### J1 — `kg-graph-off-ui-untouched` (principal)

With `KgGraphEnabled = false`:
- The KG Settings tab still renders (it's how you turn it on), but
  every other KG UI element is hidden: no filter chips on
  Dictations, no per-row entity/tag chip display, no concept modals
  reachable from any UI surface.
- No KG IPC commands are invoked from the UI (asserted via UI test
  harness intercepting IPC by command-name).

Implementation: extends
`src-tauri/src/kg/graph_off_invariant.rs` to additionally drive a
qa-kitten Playwright sweep that asserts UI absence, OR a sibling
probe binary — 1C.5 author's call.

### J2 — `kg-retrieval-correct`

Seeding a fixture set of `(entry, entity, tag)` triples and applying
a filter for entity X returns exactly the entries the
`kg_concept_entries_view` would return for that entity. Deterministic;
no Ollama in the loop.

Implementation: tempfile SQLite, direct INSERT to seed, hit the IPC
list-entries-by-filter command (1C.3 deliverable), assert set-equality.

### J3 — `kg-failed-filing-retry-idempotent`

Clicking the Retry button on a failed `kg_filing_queue` row flips
`state='failed' → 'pending'` and resets `attempt_count=0`. Re-clicking
on a now-pending row is a no-op.

Implementation: tempfile SQLite, INSERT a `state='failed'` row, call
`requeue_failed(id)` twice, assert (state, attempt_count) sequence:
`(failed, N) → (pending, 0) → (pending, 0)`.

---

## §5. Seal criteria (Wave 1C.5)

All of the following before closing `mb-j368`:

- All three Wave 1C.5 judges green per §4.
- **Cargo gate** itemized (uses Windows wrapper):
  - `cargo-with-cuda.ps1 check`
  - `cargo-with-cuda.ps1 fmt --check`
  - `cargo-with-cuda.ps1 clippy --release -- -D warnings`
  - `cargo-with-cuda.ps1 test --release --no-run` (P2 fallback per
    LESSONS PINNED; live `cargo test --release` still broken)
- **Parity probe** still 32/32 in default and `--persist` modes
  (re-run after every wave that touches `pipeline.rs` or `store/`).
- **UI gate**: `npx tsc --noEmit`, `npm test`, `npm run build`.
  `npm run lint` remains broken per `mb-yxh`; not a 1C blocker.
- **qa-kitten visual sweep** per UI wave (1C.1, 1C.2, 1C.3, 1C.4)
  documented inline with the wave's commit message.
- ADR 0051 flipped to **Accepted** with date.
- STATUS sealed-row added to the "Sealed" section (per the
  ADR-chartered lateral-epic seal pattern).
- PRODUCT-STATE KG section updated (or §3.19 if that's the row).
- Epic bead `mb-j368` closed; all sub-beads closed; standing beads
  closed where this epic discharges them: `mb-s6a8`, `mb-7w5f`,
  `mb-j3t1`, `mb-b3jy` (the last already closed by 1C.0).
- **NO new ADR** beyond 0051.
- **NO new `phase-*-complete` git tag.**

---

## §6. Public-surface delta (cumulative)

### New Tauri commands (1C.1-1C.4)

In `src-tauri/src/commands/kg.rs` (NEW file). Final names may differ
slightly; authoring wave decides:

- `kg_settings_get_all() -> KgSettingsSnapshot` — scoped read of the
  KG-namespace allowlisted settings (mirrors `meeting_settings_get_all`).
- `kg_settings_set(key: SettingKey, value: SettingValue)` — scoped
  write, allowlist-validated.
- `kg_failed_filings_list() -> Vec<KgFailedFilingRow>` — paginated
  list of `state='failed'` rows.
- `kg_failed_filing_retry(queue_id: i64)` — wrapper around the
  `requeue_failed` store helper.
- `kg_filter_candidates_list() -> KgFilterCandidates` — distinct
  entities + tag slugs for the Dictations page filter chips.
- `kg_entries_by_filter_list(filter: KgFilter) -> Vec<KgEntryRow>` —
  filtered entries query for the Dictations page result rows.
- `kg_entries_for_entity_list(entity_name, entity_type) -> Vec<KgEntryRow>`
  — concept modal's row source.

All commands appear in `src-tauri/capabilities/default.json` per
ADR 0035.

### New UI components (1C.1-1C.4)

- `ui/src/pages/SettingsKgTab.tsx` (1C.1)
- `ui/src/pages/components/KgFailedFilings.tsx` (1C.2, optional if
  `SettingsKgTab.tsx` stays under 600 LoC)
- `ui/src/pages/components/KgFilterChips.tsx` (1C.3, likely extracted)
- `ui/src/pages/components/KgEntryRow.tsx` (1C.3, likely extracted)
- `ui/src/pages/components/KgConceptModal.tsx` (1C.4)

### New i18n keys

All under `kg.*` namespace in `ui/src/i18n/en.json`. Approximate
shape (subject to ux-author refinement):

```
kg.settings.tab.title
kg.settings.activation.label
kg.settings.activation.tooltip
kg.settings.failed_filings.title
kg.settings.failed_filings.retry
kg.settings.failed_filings.empty
kg.dictations.filter.entity.placeholder
kg.dictations.filter.tag.placeholder
kg.dictations.filter.type.placeholder
kg.dictations.filter.status.placeholder
kg.dictations.filter.category.placeholder
kg.dictations.filter.search.placeholder
kg.concept.modal.title
kg.concept.modal.entry_count
```

---

## §7. Risk register

Brought forward from the planning-agent's pre-charter plan; each
risk gets a mitigation strategy.

| ID | Risk | Mitigation |
|---|---|---|
| R1 | **Sealed Phase 8 UI** — ADR 0051's authorization clause is the first KG carve-out into a sealed UI surface; agents may panic and bounce. | Mitigation: ADR 0051 §"UI sealed-surface authorization" mirrors ADR 0037 §5's wording exactly. Future-session triage will recognize the pattern. The explicit "anything not on this list stays sealed" line stops over-reach. |
| R2 | **`Settings.tsx` is 22.6 KB** today (715 lines, already over the 600-LoC guideline). | Mitigation: 1C.1 extracts the KG tab body into `SettingsKgTab.tsx` (separate file); the touch to `Settings.tsx` is a one-line tab registration. `mb-17d` continues to track full per-tab extraction independently — 1C does NOT undertake that broader refactor. |
| R3 | **`Dictations.tsx` is 28.3 KB.** Filter chips + per-row entity/tag display will push it well over 600 LoC. | Mitigation: 1C.3 is allowed by ADR 0051's authorization to extract sub-components into `ui/src/pages/components/`. Sub-component extraction is a no-behavior-change refactor; qa-kitten visual sweep is the regression gate. |
| R4 | **qa-kitten fixture seeding** — KG retrieval visual tests need seeded fixtures the Playwright harness doesn't currently know how to set up. | Mitigation: 1C.3 wave brief (when authored) must include a seed-fixtures bullet. May need a one-shot Tauri command for "seed test fixtures" gated behind a debug-only build feature; decide at 1C.3 kickoff. |
| R5 | **ADR 0049 mission cohesion** — the KG's mission ("the user finds their stuff again") is easy to lose sight of when shipping 6 IPC commands + 5 UI components. | Mitigation: every wave's commit message references ADR 0049 §6 directly. 1C.5's J2 judge tests retrieval correctness end-to-end — the mission-cohesion gate is mechanical at seal. |
| R6 | **Concept page undesign** — D4 commits to modal-only but the modal itself isn't designed in the ADR. | Mitigation: 1C.4 wave brief will include the modal spec at kickoff time. Spec is narrow (rows from `kg_concept_entries_view`); no ux-author dispatch should be needed. |
| R7 | **Per-tick poll cost** — D6's promotion adds a SQLite read per worker tick. | Mitigation: `Settings.get::<bool>(SettingKey::KgGraphEnabled)` is a single indexed lookup. The current tick interval is `IDLE_SLEEP = 5s`; an extra ~1 ms SQLite read per 5s is negligible. Will re-measure at 1C.5 if there's any user-visible CPU hit. |
| R8 | **i18n string growth** — ~14 new `kg.*` keys land cumulatively. | Mitigation: structured under a single namespace; one consolidated edit per wave. Translation surface explicitly v1-en-only per Phase 8's i18n posture. |

---

## §8. Standing-bead linkage

Beads created or already-open that this epic discharges:

| Bead | Title | Wave that discharges | Status at 1C.0 kickoff |
|---|---|---|---|
| `mb-s6a8` | KG Phase 1C: surface KgGraphEnabled toggle in Settings UI | 1C.1 (`mb-ucmx`) | open, blocked-by `mb-ucmx` |
| `mb-7w5f` | KG Phase 1C: promote KgGraphEnabled worker boot-vs-poll | 1C.1 (`mb-ucmx`) | open, blocked-by `mb-ucmx` |
| `mb-j3t1` | KG Phase 1C: failed-filings UI surface + manual retry button | 1C.2 (`mb-9ufg`) | open, blocked-by `mb-9ufg` |
| `mb-b3jy` | KG: empirical latency budget measurement on real hardware | 1C.0 (`mb-plz9`) | **closed by this wave** |

Discovered mid-1C must `bd create` immediately per AGENTS.md
"Default rule: bead-first".

---

## §9. References

- ADR 0051 (this wave's charter) — D1-D8 decisions + UI authorization.
- ADR 0050 (Phase 1B sealed; schema + worker consumed by 1C).
- ADR 0049 §6 + §7 (1C row) — binding latency budget + wave scope.
- ADR 0037 §5 — authorization-clause pattern precedent.
- ADR 0035 — capabilities/default.json registration discipline.
- `docs/knowledge-graph/PHASE-0-5-REPORT.md` §7 — 1C row commitments.
- `docs/knowledge-graph/phase-1b-brief.md` — shape model for this brief.
- `docs/knowledge-graph/phase-1c-latency-baseline.md` — empirical
  numbers feeding D2 + D3.
- LESSONS P5 — lateral epics seal via ADR + STATUS, NOT new tag.
- LESSONS P9 — IAP split discipline (if a 1C IPC command needs to
  exceed one read/write).
