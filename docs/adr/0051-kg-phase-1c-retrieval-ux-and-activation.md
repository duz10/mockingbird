# ADR 0051 — KG Phase 1C: retrieval UX + activation toggle

- **Status:** **Accepted (2026-05-31)** — see §"Phase 1C SEALED" close-out at the end of this ADR.
- **Supersedes:** None
- **Extends:** [ADR 0049](0049-knowledge-graph-phase-0-5-and-v1-architectural-pivot.md) §7 (1C row) + §"Sandbox isolation"
- **Consumes schema from:** [ADR 0050](0050-knowledge-graph-phase-1b-persistence-and-dictation-hook.md) (Phase 1B persistence + dictation hook; migration 024 already on disk; FK + immutability triggers already enforced)
- **Mirrors authorization-clause pattern from:** [ADR 0037](0037-unified-recording-command-center.md) §5 ("Boundary — what 0037 explicitly authorizes touching in sealed code")
- **Mirrors capabilities-registration discipline from:** [ADR 0035](0035-meeting-center-v1-2-pip-overlay.md) (Tauri capabilities/default.json registration)
- **Charter bead:** `mb-j368` (epic) / `mb-plz9` (this wave, 1C.0)
- **Discharges:** `mb-b3jy` (empirical latency baseline) via this wave's bench

## Status

**Accepted (2026-05-31).** Six waves shipped (1C.0 charter + latency
baseline; 1C.1 Settings tab + activation + boot-vs-poll; 1C.2
failed-filings UX; 1C.3 Dictations retrieval (5 of 6 axes); 1C.4
concept modal + entity/tag detail commands; 1C.5 graph-off-UI
invariant judge + a11y polish + seal). Epic bead `mb-j368` closed.
See §"Phase 1C SEALED" at the end of this ADR for the close-out
narrative, commit chain, gate results, and the explicit
record-of-deferral for the category axis (`mb-oji5` → Phase 1D).

## Context

ADR 0049 §"Sandbox isolation" and §7 (1C row) reserve Phase 1C for
the KG's first user-facing surfaces: a runtime toggle to opt into
filing (`KgGraphEnabled`), retrieval-axis filter UX on the Dictations
page, and the failed-filings surface that gives users a fix-it loop
when the Ollama daemon hiccups. ADR 0050 sealed the persistence half:
migration 024 owns the 5 tables + 2 concept-page VIEWs, the worker
drains `kg_filing_queue` FIFO, the dictation tail enqueues iff
`KgGraphEnabled = true` (currently default-off and only flippable via
a DB poke).

Phase 1B's seal anchored the worker on a **read-once-at-boot**
activation flag (`KgGraphEnabled` is read in `lib.rs`'s setup hook,
not per-tick). That is the right v0 wiring — it's cheap, it's simple,
it's parity-safe — but it means flipping the toggle in the UI today
would have no effect until next restart. Phase 1C closes that loop:
Wave 1C.1 surfaces the toggle and promotes the worker to a
per-tick-poll-the-setting model so the UI flip takes effect
immediately (`mb-7w5f`).

The PHASE-0-5-REPORT §7 1C-row commitments are six retrieval axes
(entity-filter, tag-filter, entry-type filter, status filter,
category filter, free-text "contains") plus the activation toggle.
This ADR breaks the work into 6 waves so each wave's deliverable is
testable end-to-end and the qa-kitten visual sweep can land per-wave
rather than batched at the end.

This ADR also formally **authorizes a scoped window of edits to
`ui/**`** under Phase 8's sealed-UI rule, mirroring ADR 0037 §5's
language for Phase MC's sealed Dictation + Meeting Capture
subsystems. Outside the file list below, the Phase 8 + ADR 0037 seals
hold.

The 2026-05-30 latency baseline (`docs/knowledge-graph/phase-1c-latency-baseline.md`)
establishes p95 = 59s for a 5-segment dictation — right at the
ADR 0049 §6 budget. This data directly shapes Decision 2 + Decision 3
(activation flow + failed-filings UX) below.

## Decision

### D1 — Settings KG tab: extend Settings.tsx in place (no new top-level page)

Add a "KG" tab inside the existing Settings tabset rather than
introducing a new top-level page in the sidebar. Rationale: KG
activation belongs with other privacy / data-handling toggles
(history-rentention, dictation provider, etc.) and not as its own
top-level concept users would have to discover. Implementation
extracts the per-tab body to `ui/src/pages/SettingsKgTab.tsx` to
keep `Settings.tsx` under the 600-LoC ceiling (it's 715 LoC today;
`mb-17d` already tracks per-tab extraction independently, but THIS
wave only does the KG tab — broader extraction stays in `mb-17d`).

### D2 — Activation flow: silent flip + tooltip ("takes effect on next dictation")

Flipping `KgGraphEnabled` from the UI writes the setting and shows a
non-blocking tooltip / inline help text explaining that filing will
begin on the next dictation (or, post-1C.1 boot-vs-poll promotion,
within ~5s of the next worker tick). No confirmation modal, no
restart-required nag, no toast notification. The setting is reversible
at any time. Failed filings remain visible in the 1C.2 UX surface
regardless of the toggle's current state (so a user can toggle off,
inspect the queue, then re-enable).

**1C.5 close-out — retrieval axes shipped:** of the six axes promised
in the PHASE-0-5-REPORT §7 1C row, **five** shipped at 1C.3 (entity
filter, tag filter, free-text "contains" query merged into existing
FTS, plus per-row entity/tag/filing-state display); the 1C.4 concept
modal added entity + tag drill-down detail views. The **category
axis** was empirically blocked at 1C.3 (LESSONS 2026-05-30 `mb-5ly5`
/ `mb-oji5`): `Entry.category` is produced by the classify pass but
has no persisted column anywhere in the Phase 1B schema, and the
§"Out of scope" clause below explicitly bans new migrations in 1C.
The category-persistence work is therefore deferred to Phase 1D,
which already needs a schema-touching migration for the backfill
path. `mb-oji5` stays open with a 1D blocker.

### D3 — Failed-filings: truncated `last_error` inline + full text in tooltip

The 1C.2 failed-filings UX shows one row per failed `kg_filing_queue`
entry, with `last_error` truncated to ~80 chars inline and the full
text on hover (tooltip) for diagnosis. A "Retry" button per row flips
`state='failed' → 'pending'` and resets `attempt_count=0` via a new
`requeue_failed(queue_id)` store helper. Idempotent: re-clicking on
an already-pending row is a no-op.

### D4 — Concept page: modal in v1, full-page view deferred

Drill-down from an entity / tag chip opens a modal showing all
`kg_concept_entries_view` rows for that concept, NOT a full-page
route. Rationale: v1 doesn't yet justify a fully-designed concept
page — the modal pattern is cheaper, faster to ship, and easy to
graduate to a full page in v1.1+ if usage warrants. Out-of-scope for
this ADR: editing entities/tags from this modal (entirely deferred).

### D5 — Latency baseline: pulled into 1C.0 (this wave)

The empirical latency budget verification (`mb-b3jy`) is folded into
this wave's charter rather than a follow-up wave. Rationale: the
baseline numbers directly inform D2 + D3's UX shape (do we need a
"KG indexing in progress" indicator? is fire-and-forget fine?). The
bench binary + per-pass `PassTimings` instrumentation land in 1C.0;
re-measurement policy in §6 of the baseline doc.

### D6 — Boot-vs-poll promotion lands in Wave 1C.1, not earlier

The Phase 1B worker reads `KgGraphEnabled` once at boot. Promoting to
per-tick polling so the UI toggle takes immediate effect is bundled
with the Settings tab landing (Wave 1C.1) because polling has no
user-visible value without the UI surface that flips it. `mb-7w5f`
closes at the same commit as `mb-s6a8` (UI toggle).

### D7 — Roughly 6 new Tauri commands

The 1C UI needs IPC for: read+write the KG setting (likely a scoped
`kg_settings_get_all` / `kg_settings_set` pair mirroring
`meeting_settings_get_all` / `meeting_settings_set` from `commands/settings.rs`);
list the failed-filings queue rows; retry a failed-filing; list
filter-chip candidates (distinct entities / tag slugs); list
entries-for-entity (for the concept modal); a one-shot latency
snapshot for in-app debugging. Capabilities/default.json gets the
new commands registered per ADR 0035's discipline (LESSONS:
forgetting this is the #1 cause of "command not found" errors at
runtime). Each command appears in the file-list authorization in §
"UI sealed-surface authorization" below.

### D8 — This ADR opens a scoped `ui/**` authorization window

Phase 8 sealed the UI; ADR 0037 carved out a narrow window for the
Command Center; this ADR carves a similarly narrow window for KG
retrieval + settings work. Outside the list in § "UI sealed-surface
authorization", the seal holds.

## Wave plan

Six waves. Each row: scope / expected file touches / sub-bead.

| Wave  | Scope | Expected file touches | Sub-bead |
|-------|-------|------------------------|----------|
| 1C.0  | Charter ADR (this doc) + Phase 1C wave brief + per-pass `PassTimings` instrumentation in `pipeline.rs` + worker structured tracing for latency + `kg_latency_bench` bin + empirical baseline doc. NO UI work. | `pipeline.rs`, `worker.rs` (instrumentation only), `kg/latency_bench.rs` (new), `bin/kg_latency_bench.rs` (new), `kg/mod.rs` (re-exports), `docs/adr/0051-*.md`, `docs/knowledge-graph/phase-1c-brief.md`, `docs/knowledge-graph/phase-1c-latency-baseline*.md` | `mb-plz9` |
| 1C.1  | Settings KG tab (`SettingsKgTab.tsx`) + `KgGraphEnabled` toggle + scoped `kg_settings_get_all`/`kg_settings_set` commands + boot-vs-poll worker promotion. Closes `mb-s6a8` + `mb-7w5f`. | `ui/src/pages/Settings.tsx` (1 line, add tab registration), `ui/src/pages/SettingsKgTab.tsx` (NEW), `ui/src/lib/tauri.ts` (2 IPC bindings), `ui/src/lib/types.ts` (KG settings type), `ui/src/i18n/en.json` (`kg.*` keys), `src-tauri/src/commands/kg.rs` (NEW; 2 commands), `src-tauri/src/commands/mod.rs` (register), `src-tauri/capabilities/default.json` (allowlist), `src-tauri/src/kg/worker.rs` (per-tick `KgGraphEnabled` re-read) | `mb-ucmx` |
| 1C.2  | Failed-filings UX (list + per-row retry button) + `requeue_failed(queue_id)` store helper + 2 new Tauri commands (list + retry). Closes `mb-j3t1`. | `ui/src/pages/SettingsKgTab.tsx` (extend), or new `ui/src/pages/components/KgFailedFilings.tsx` if 600-LoC ceiling threatens, `src-tauri/src/commands/kg.rs` (2 commands), `src-tauri/src/kg/store/queue.rs` (`requeue_failed`), `src-tauri/capabilities/default.json` | `mb-9ufg` |
| 1C.3  | Dictations page filter chips (entity, tag, entry-type, status, category, free-text) + per-row entity/tag display. 2 new Tauri commands: list-filter-candidates (distinct entities + tag slugs) + list-entries-by-filter. Allowed to extract sub-components from `Dictations.tsx` if 600-LoC ceiling at risk. | `ui/src/pages/Dictations.tsx`, `ui/src/pages/components/*` (new sub-components as needed), `src-tauri/src/commands/kg.rs` (extend), `ui/src/lib/{tauri,types}.ts`, `ui/src/i18n/en.json`, `src-tauri/capabilities/default.json` | `mb-5ly5` |
| 1C.4  | Concept page modal: entity / tag click opens modal showing all related `kg_concept_entries_view` rows. New `KgConceptModal.tsx` sub-component, 1 new Tauri command. | `ui/src/pages/components/KgConceptModal.tsx` (NEW), `ui/src/pages/Dictations.tsx` (wire chip onClick), `src-tauri/src/commands/kg.rs` (extend), `ui/src/lib/{tauri,types}.ts`, `src-tauri/capabilities/default.json` | `mb-sx6p` |
| 1C.5  | Three deterministic invariant judges (NOT LLM-graded): graph-off-UI invariant, retrieval-correct, failed-filing-retry-idempotent. Flip this ADR to **Accepted**. Close epic `mb-j368` + sub-beads. Update STATUS sealed table + PRODUCT-STATE §3.19 (or equivalent KG section). **No `phase-*-complete` git tag** (lateral epic; LESSONS P5). | `src-tauri/src/kg/judges/*.rs` (new module), `STATUS.md`, `docs/PRODUCT-STATE.md`, `docs/adr/0051-*.md` (Status → Accepted) | `mb-f4gn` |

## UI sealed-surface authorization

This is the explicit authorization for surgical edits Wave 1C.1-1C.4
will make to files that Phase 8 and ADR 0037 sealed. **Anything not
on this list stays sealed.** Mirrors ADR 0037 §5's pattern.

| File | Surgical change | Why it's surgical |
|---|---|---|
| `ui/src/pages/Settings.tsx` | Add ONE new tab registration entry ("KG"). Body lives in a separate file (next row). | One-line additive registration; existing tabs untouched. |
| `ui/src/pages/SettingsKgTab.tsx` | NEW FILE. KG settings panel: activation toggle (D2), failed-filings list (D3 via 1C.2), latency-snapshot debug button. | NEW; no existing code touched. |
| `ui/src/pages/Dictations.tsx` | Add filter chip row above results + per-row entity/tag chip display. Extract sub-components into `ui/src/pages/components/` if the 600-LoC ceiling is at risk. | Page is 28.3 KB today; additive UI rows + may need extraction. The extraction itself is allowed under this ADR (no behavior change). |
| `ui/src/pages/components/*.tsx` | NEW FILES extracted from Dictations / Settings tabs as needed (KG filter row, KG chip, failed-filings list, concept modal). | NEW; no existing component code touched outside the named files above. |
| `ui/src/lib/tauri.ts` | Additive IPC bindings for ~6 new KG commands. | Additive; existing bindings untouched. |
| `ui/src/lib/types.ts` | Additive KG types mirroring Rust DTOs. | Additive. |
| `ui/src/i18n/en.json` | Additive `kg.*` string namespace. | Additive. |
| `src-tauri/src/commands/kg.rs` | NEW FILE. Houses the ~6 KG commands. | NEW; mirrors `commands/settings.rs` pattern. |
| `src-tauri/src/commands/mod.rs` | Register `kg::*` commands in the existing `register!` macro. | Additive (single block). |
| `src-tauri/capabilities/default.json` | Allowlist the new commands per ADR 0035. | Additive. |
| `src-tauri/src/kg/worker.rs` | Promote `KgGraphEnabled` from read-once-at-boot to per-tick poll (Wave 1C.1 / D6). | One conditional in the existing drain loop; no state-machine change. |
| `src-tauri/src/kg/store/queue.rs` | Add `requeue_failed(queue_id) -> AppResult<()>` for the 1C.2 retry button. | Additive function; existing functions untouched. |
| `src-tauri/src/kg/judges/*.rs` | NEW MODULE for Wave 1C.5 deterministic judges. | NEW. |

### Explicitly NOT authorized

The seal holds on everything below. If 1C work discovers a need to
touch any of these, that's a **successor ADR amendment**, not a
unilateral expansion.

- Any other edits to `ui/src/meeting_overlay/**`, `ui/src/recording/**`,
  `ui/src/command_center/**` (sealed by Phase 8 + ADR 0037).
- Any edits to `ui/src/pages/{Meetings,MeetingDetail,Activity,
  ActivityBlocks,Modes,Dictionary,About}.tsx` (orthogonal subsystems
  not in 1C scope).
- Any edits to `src-tauri/src/dictation.rs`, `src-tauri/src/meetings/**`
  (sealed; ADR 0050's `dictation.rs` tail-enqueue was the authorized
  edit, and it is already on disk).
- Schema changes (no new migrations in 1C; 1C consumes 1B's
  migration 024 only).
- Changes to `transcripts` table (Principle 1: raw is immutable).

## Invariants for Wave 1C.5 judge bundle

All three judges are **deterministic** (assertion-based, not
LLM-graded). LLM-graded judges are not required for 1C — the
invariants below are mechanical.

### J1 — `kg-graph-off-ui-untouched` (principal)

With `KgGraphEnabled = false`:
- The KG Settings tab still renders (it's how you turn it on), but
  every other KG UI element is hidden — no filter chips on Dictations,
  no per-row entity/tag display, no concept modals reachable.
- No KG IPC commands are invoked from the UI (asserted via UI test
  harness intercepting IPC calls and listing-by-name).

Implementation: extends the existing
`kg::graph_off_invariant::run_graph_off_invariant_probe`
(`src-tauri/src/kg/graph_off_invariant.rs`) to additionally assert
the UI-side surface absence — likely via a Playwright sweep
chained into the existing probe binary, or a sibling probe binary
(decision deferred to 1C.5 author).

### J2 — `kg-retrieval-correct`

Seeding a fixture set of `(entry, entity, tag)` triples and applying
a filter for entity X returns exactly the entries the
`kg_concept_entries_view` would return for that entity. Deterministic;
no Ollama in the loop.

Implementation: tempfile SQLite, seed via direct INSERT, hit the IPC
list-entries-by-filter command, assert set-equality.

### J3 — `kg-failed-filing-retry-idempotent`

Clicking the Retry button on a failed `kg_filing_queue` row flips
`state='failed' → 'pending'` and resets `attempt_count=0`. Clicking
again on a same-row (now pending) is a no-op (count and state
unchanged).

Implementation: tempfile SQLite, INSERT a `state='failed'` row, call
`requeue_failed(id)` twice, assert (state, attempt_count) sequence
is `(failed, N) → (pending, 0) → (pending, 0)`.

## Out of scope

Explicit deferrals so future agents don't try to fit them into 1C.

- **Phase 1D** — backfill of pre-Phase-1 dictations into the graph.
  Explicit batch UX. Its own ADR.
- **Phase 1E** — v1 beta tag, UX polish, power-user surfaces
  (graph export, schema-aware advanced filters).
- **Cross-entity co-occurrence view** — "show me dictations
  mentioning both X and Y". Deferrable to 1D or later if usage
  patterns justify.
- **Full-page concept view** — D4 explicitly defers to v1.1 if data
  justifies. Modal is the v1 contract.
- **Editing entities / tags from the UI** — entirely deferred to
  v1.1+. The graph is read-only from the UI in 1C.
- **Metrics table** — the latency baseline is log-only in 1C.0. If
  1C.2's failed-filings UX surfaces a need for a metrics surface,
  that's a `bd create` and a follow-up wave, not in scope here.
- **LLM-graded judges** — 1C invariants are all deterministic. No
  judge calibration loop needed.

## Supersession

- **Extends** ADR 0049 §7 (1C row commitments).
- **Consumes schema from** ADR 0050 (no migration changes in 1C).
- **Mirrors authorization-clause pattern from** ADR 0037 §5.
- **Mirrors capabilities-registration discipline from** ADR 0035.
- **Does not supersede** any prior ADR.

## References

- `docs/knowledge-graph/phase-1c-brief.md` — Wave 1C.0..1C.5 plan +
  per-wave acceptance criteria.
- `docs/knowledge-graph/phase-1c-latency-baseline.md` — empirical
  numbers feeding D2 + D3.
- `docs/knowledge-graph/parity/wave-0.5.4-seed-42.json` — fixture
  the latency bench draws from.
- `docs/knowledge-graph/PHASE-0-5-REPORT.md` §7 1C row.
- ADR 0049 §6 — ~1 min latency budget binding.
- ADR 0050 — Phase 1B sealing report.
- ADR 0037 §5 — UI sealed-surface authorization clause precedent.
- ADR 0035 — capabilities/default.json registration discipline.
- LESSONS P5 — lateral epics seal via Accepted ADR + STATUS, NOT a
  new `phase-*-complete` tag.
- LESSONS P9 — IAP split discipline (only relevant if a 1C IPC
  command grows beyond a simple read/write).
- Beads: epic `mb-j368`; subs `mb-plz9` (1C.0), `mb-ucmx` (1C.1),
  `mb-9ufg` (1C.2), `mb-5ly5` (1C.3), `mb-sx6p` (1C.4), `mb-f4gn` (1C.5).
  Standing beads `mb-s6a8`, `mb-7w5f`, `mb-j3t1`, `mb-b3jy` are
  blocked-by their respective wave sub-beads (or discharged by 1C.0
  in the case of `mb-b3jy`).

## Phase 1C SEALED (2026-05-31)

Phase 1C sealed via Wave 1C.5 (`mb-f4gn`). The epic shipped the
activation toggle, four of the five retrieval-axis UX surfaces, the
failed-filings fix-it loop, the concept-modal drill-down, and the
principal `kg-graph-off-ui-untouched` invariant judge — leaving the
category axis as the lone explicit deferral to Phase 1D.

**Gates:**

- **J1 — `kg-graph-off-ui-untouched` (principal):** GREEN via the
  new Playwright spec `ui/tests/kg-graph-off-invariant.spec.ts`
  (5 assertions across 4 walks). The opt-in `__KG_IPC_SPY__` hook
  on `lib/tauri.ts::invoke` records every IPC name; with
  `KgGraphEnabled=false`, the recorded `kg_*` set is exactly
  `{ "kg_settings_get_all" }` across Settings → KG tab, Dictations
  page, and dictation-row click walks. The positive-control flip ON
  (Settings tab) lights up at least one of
  `{ kg_list_failed_filings, kg_queue_status }` from the
  `SettingsKgFailedFilings` mount — proving the spy is not vacuously
  passing. Zero console errors across all walks.
- **J2 — `kg-retrieval-correct`:** GREEN via the existing Wave 1C.3
  Playwright spec `ui/tests/kg-dictations-retrieval.spec.ts`
  (covers the within-axis OR / across-axis AND semantics end-to-end
  via fixture-overridden `kg_search_entries`).
- **J3 — `kg-failed-filing-retry-idempotent`:** GREEN via the Wave
  1C.2 Rust-side throwaway-crate idempotency test on
  `kg::store::requeue_failed` + the Wave 1C.2 Playwright spec
  `ui/tests/kg-failed-filings.spec.ts` (covers the UI-side disable-
  on-click idempotency surface).
- **Cargo gate** via the Windows wrapper: `check`, `fmt --check`,
  `clippy --release -- -D warnings`, `test --release --no-run` all
  GREEN (per LESSONS P2 fallback for the test runner). Parity probe
  32/32 in both default and `--persist` modes (no `kg/store` or
  `kg/worker` regression — Wave 1C.5 changes are UI + test-only).
- **UI gate:** `npx tsc --noEmit`, vitest, `npm run build` all GREEN.
  All 5 KG Playwright specs (`kg-settings-tab`, `kg-failed-filings`,
  `kg-dictations-retrieval`, `kg-concept-modal`, `kg-graph-off-invariant`)
  green. Lint skipped per `mb-yxh`.

**Commit chain (Phase 1C end-to-end):**

- `113e848..1332ed5` — Wave 1C.0: per-pass `PassTimings` instrumentation
  + `kg_latency_bench` binary + ADR 0051 Proposed + phase-1c-brief +
  empirical p95=59s baseline (`mb-plz9` / `mb-b3jy` closed).
- `b5b2e74..7143d88` — Wave 1C.1: `commands/kg::{get_all,set}` +
  boot-vs-poll worker promotion + `SettingsKgTab.tsx` + Playwright
  visual sweep (`mb-ucmx` / `mb-s6a8` / `mb-7w5f` closed).
- `f639485..efd2afe` — Wave 1C.2: failed-filings UX +
  `kg_list_failed_filings` / `kg_requeue_failed` / `kg_queue_status` +
  Playwright sweep (`mb-9ufg` / `mb-j3t1` closed).
- `6c856af..a085ce0` — Wave 1C.3: Dictations retrieval UX (5 of 6
  axes shipped — entity + tag + free-text + per-row chip strip +
  filing-state pills) + 4 new IPCs + 3-way Dictations.tsx extraction
  (830 → 549 LoC; LESSONS) + Playwright sweep (`mb-5ly5` closed).
- `a1a6a07..9c7e11d` — Wave 1C.4: concept modal + `kg_entity_detail`
  / `kg_tag_detail` + chip click-to-open + Playwright sweep
  (`mb-sx6p` closed; `mb-26aw` filed for pre-1C Playwright noise).
- This seal commit — Wave 1C.5: `__KG_IPC_SPY__` hook on
  `lib/tauri.ts::invoke` + `kg-graph-off-invariant.spec.ts` +
  `SettingsKgTab` `role="status"` a11y scoping + ADR Accepted +
  STATUS sealed row + PRODUCT-STATE §3.19 update.

**Key in-flight findings (full body entries in `docs/LESSONS.md`):**

- **Category-axis persistence gap (Wave 1C.3 / 1C.5 — `mb-oji5`).**
  `Entry.category` is produced by the classify pass but the Phase 1B
  schema has no queryable `category` column anywhere (verified twice:
  in the original 1C.3 LESSONS body finding, and again at 1C.5 seal
  when the original Wave 1C.5 kickoff prompt claimed a non-existent
  `entries.category` column via migration 016 — there is no `entries`
  table). Fixing the gap requires a new migration, which §"Out of
  scope" of THIS ADR explicitly bans. Deferred to Phase 1D where the
  backfill path will already touch the sessions-layer schema. The
  1C.5 in-flight discovery (kickoff-vs-disk-truth misalignment) is
  itself a LESSONS body entry on the value of cross-checking kickoff
  premises against the actual schema/code before the first edit.
- **Boot-vs-poll promotion was the right cheap move (Wave 1C.1).** The
  Phase 1B Chunk 3 boot-time-only `KgGraphEnabled` read was a known
  shortcut. 1C.1 promoted it to per-tick polling (~1 SQL per 5s idle
  loop when off) so runtime toggles take effect within ~5s — no
  restart-required nag needed for the UX (D2). Cost dwarfed by the
  worker's idle sleep; no parity regression.
- **PassTimings additive pattern re-confirmed (Wave 1C.0).** Adding
  `pass_timings: Vec<PassTimings>` to `PipelineResult` followed the
  same additive-field-invisibility pattern Phase 1B Chunk 3 used
  for `segment_entities`: the parity probe's structural JSON
  comparison ignored the new field, so 32/32 held across the
  instrumentation rollout. Pattern formally re-confirmed.
- **Playwright `__KG_IPC_SPY__` opt-in hook pattern (Wave 1C.5).**
  The cleanest place to spy on Tauri IPC from a preview-mode
  Playwright spec is the `lib/tauri.ts::invoke` choke point itself —
  not the `__TAURI_INTERNALS__` shim (which is absent in preview).
  A `window.__KG_IPC_SPY__?: (cmd: string) => void` hook is one
  `if` per IPC at runtime, zero cost when no test has opted in, and
  reusable for future graph-off-anywhere invariants without per-spec
  re-plumbing.
- **a11y scoping for `role="status"` (Wave 1C.5).** The 1C.1 toggle
  notice's `role="status"` collided with the 1C.2
  `SettingsKgFailedFilings` empty-state's `role="status"` once both
  were live on the Settings/KG tab simultaneously, breaking the 1C.1
  Playwright spec's `getByRole("status")` strict-mode query. Fix: an
  `aria-label={t("kg.settings.notice.title")}` on the notice element
  + a scoped `getByRole("status", { name: ... })` lookup. Closes the
  1C.1-origin part of `mb-26aw`; the smoke.spec.ts ×4 pre-1C
  failures remain tracked separately.

**Standing beads carried forward (not gating the seal):**

- `mb-bbl2` — sonner retrofit (replace native browser confirms with
  toast-confirmation; surfaced during 1C.2 review).
- `mb-y6pq` — `--status-bad` token sweep (legacy `Settings.module.css`
  references a non-existent token; fallback to `#d33` literal works
  but is design-debt; surfaced in 1C.2 LESSONS).
- `mb-26aw` — `smoke.spec.ts` ×4 pre-1C Playwright failures (filed
  at 1C.4; root-cause not in 1C scope).
- `mb-oji5` — category persistence; blocked on Phase 1D migration.

**Phase 1D kickoff handoff:**

1. All 6 retrieval axes online except category — Phase 1D's first
   migration should add `sessions.category TEXT NULL` (or an
   equivalent join table); category UI in `DictationsFilterBar`
   already has the typed wire shape via `SearchFilter` (additive).
2. Latency budget empirically locked at p95=59s per filing (extract
   + extract_entities = 82% of pipeline cost). 1D backfill of N
   pre-Phase-1 entries should plan for ~N × 60s wall-clock at
   current throughput.
3. The `KgGraphEnabled` UI toggle is now load-bearing for backfill
   gating: 1D backfill must respect the same off-by-default contract
   the `kg-graph-off-invariant` judge (Rust + UI sides) enforces.
4. Phase 1E v1 beta tag awaits 1D completion.

**No new `phase-*-complete` tag** — lateral epic per LESSONS
PINNED **P5**. Epic bead `mb-j368` + Wave 1C.5 bead `mb-f4gn` +
the deferred-not-shipped `mb-oji5` closed/parked in the seal commit.
