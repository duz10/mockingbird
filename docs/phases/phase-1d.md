# Phase 1D — Source-gated KG filing + first-class KG screen

**Phase entry anchor:** Phase 1C seal commit `4e93959` (KG retrieval UX + activation toggle + concept modal sealed via ADR 0051 Accepted 2026-05-31).
**Phase exit:** ADR 0052 → Accepted + STATUS sealed-row + this doc's epic bead `mb-<epic>` closed. **No new `phase-*-complete` git tag** — lateral epic per LESSONS PINNED P5.
**Charter ADR:** [ADR 0052](../adr/0052-knowledge-graph-phase-1d-charter.md) — Proposed; flips Accepted at Wave 1D.6 seal.
**Planner / kickoff author:** planning-agent (`planning-agent-f1b5c1`) + Dustin (2026-06-04 alignment review).
**Wave 1D.0 author (this doc):** code-puppy-925ce4.
**Implementor (downstream waves):** see Wave table below — `migration-author` for 1D.1; `ui-author` for 1D.2 / 1D.4 / 1D.5; `ui-author` + code-puppy collab on 1D.3; code-puppy for 1D.6 judges + seal.
**Estimated iterations:** 7 (one per wave). 1D.0 + 1D.6 are documentation-heavy / judge-heavy; 1D.1 / 1D.2 / 1D.4 / 1D.5 are mechanical-implementation-sized; 1D.3 is the highest-blast-radius wave.

> The binding spec lives in **ADR 0052** (charter), **ADR 0049 §6** (v1 architecture commitments), and the original product spec **§15.2–§15.5** at `C:\Users\dboyd\Downloads\mockingbird-knowledge-graph-spec.md`. This doc operationalizes them wave-by-wave.

---

## Objective

Close two surface drifts the Phase 1C seal didn't catch:

1. **Trigger-direction drift.** ADR 0050 §D6 wired the dictation-tail
   hook to enqueue *every* dictation when the toggle is on. Per spec
   §1 + §15.5, it should fire *only* for dictations originating from
   the new KG capture surface. Standard Right Alt PTT dictations and
   the in-app "+ New Dictation" button must never feed the KG.
2. **Missing canonical UI surface.** Per spec §15.3 the KG should be
   a first-class left-sidebar destination with a read-only dashboard
   + audio note capture + text note capture + launch-into-Obsidian
   button. Phase 1C narrowed "new UI" to Settings tab + Dictations
   page filter chips; that was correct for the activation toggle but
   wrong as the v1 home for the feature.

Phase 1D corrects both. Phase 1E (formerly 1E was the beta tag — see
ADR 0052 §"Phase numbering") will then build Obsidian-as-source-of-
truth (vault projection + reverse-watcher per ADR 0048 Q3). Phase 1F
(was 1E) cuts the v1 beta tag.

---

## Source-of-truth pointers

- **Original product spec** — `C:\Users\dboyd\Downloads\mockingbird-knowledge-graph-spec.md`
  - §1 — KG is a separate capture surface, not an automatic
    side-effect of dictation.
  - §7.1 — folder structure (Inbox / Entries / History — Phase 1E,
    not 1D).
  - §10 — deferred-processing UX (glanceable inbox state — Phase 1D
    dashboard reflects this).
  - §15.1 — observe-and-control principle (KG screen is read-only
    for existing data; capture is a write action and is allowed).
  - §15.2 — Settings / admin area (vault path, vocabularies,
    processing/queue behavior, dual-write behavior).
  - §15.3 — the KG screen as a left-sidebar dashboard.
  - §15.4 — launch-into-Obsidian handoff.
  - §15.5 — on-app capture: audio note (dual-write) + text note
    (KG-only).
- **ADR 0048 §3** — Q1 vault subtree, Q2 positional routing, Q3
  files-as-source-of-truth (Q3 fully cashed in Phase 1E, but Q1/Q2
  shape Phase 1D's settings surface).
- **ADR 0049 §6** — binding v1 architecture commitments
  (pipeline shape, two-field schema, qwen2.5:7b pin, opt-in graph
  guarantee, ~1 min latency budget).
- **ADR 0050** — Phase 1B persistence + worker + dictation-tail hook
  (D6 superseded here; everything else stays in force).
- **ADR 0051** — Phase 1C retrieval UX + activation toggle
  (§"Out of scope" amended here; chips relocate at Wave 1D.4).
- **ADR 0052** — this charter.
- **PHASE-0-5-REPORT.md §6 + §7** — v1 binding commitments + wave
  build plan.
- **`docs/knowledge-graph/phase-1c-latency-baseline.md`** — p95=59s
  per filing, carries forward.

---

## Wave 1D.0 — Charter + clean-slate verify + phase doc + bead epic (this wave)

**Goal:** Author the charter (ADR 0052), this phase doc, the bead
epic + sub-beads, update STATUS.md's in-flight block, and run the
clean-slate sqlite verification. NO migration writes, NO Rust code,
NO UI code. Anything tempted into "while we're at it" creates a
follow-up bead instead.

### Deliverables

- `docs/adr/0052-knowledge-graph-phase-1d-charter.md` (Status:
  Proposed).
- `docs/phases/phase-1d.md` (this file).
- Bead epic + 7 sub-beads (see §"Bead epic" below).
- STATUS.md "🟢 Currently active" block updated to reflect the
  re-scope.
- Close `mb-oji5` (deferred 1C bead) — consumed by Wave 1D.1.
- Commit referencing the epic + 1D.0 sub-bead.

### Clean-slate sqlite verification — RESULT

Ran at Wave 1D.0 against
`C:\Users\dboyd\AppData\Roaming\com.dustin.mockingbird\mockingbird.db`
(the kickoff prompt's `$env:APPDATA\Mockingbird\mockingbird.db` was
off by one path component — the Tauri bundle identifier is
`com.dustin.mockingbird`, not `Mockingbird`).

| Table | Row count |
|---|---:|
| `kg_entities` | **3** |
| `kg_filing_queue` | **2** (both `state='done'`) |
| `kg_entity_mentions` | **3** |
| `kg_tag_mentions` | **11** |
| `kg_canonical_tags` | 0 (expected — v1.1 inert) |
| `settings.kg_graph_enabled` | **`'true'`** (expected `'false'`) |

**NOT a clean slate.** Two `sessions` entries (`id=128` and `id=129`,
both filed 2026-05-30T17:30–17:33Z) carry KG mentions. These were
clearly 1C development test toggles, not production capture. Tag-slug
spread: `dictation`, `knowledge-graph`, `mockingbird`, `launch-slip`,
`testing`, `deadline`, `communication`, `follow-up`, `obsidian`.
Entities: `mockingbird` (object), `obsidian` (organization),
`knowledge-graph` (project).

**Action:** Wave 1D.1's migration includes a purge step (D6 of ADR
0052) that DELETEs all rows from the four KG mention/queue/entity
tables and resets `settings.kg_graph_enabled = 'false'`, in the same
transaction that adds the new `source` column. Volume is trivial; no
recovery flow is needed. The Wave 1D.6 judge bundle asserts
post-migration cleanliness as a deterministic acceptance gate.

The sqlite probe scripts live in `scratch/kg-cleanslate-{check,detail}.sql`
(this directory is gitignored — the scripts are throwaway).

### Gates (this wave)

This wave is documentation + sqlite SELECT only. Gates run anyway
per AGENTS.md end-of-iteration checklist:

- `powershell -File scripts\cargo-with-cuda.ps1 fmt --check` — must
  pass (no Rust delta this wave; confirms nothing else broke).
- `powershell -File scripts\cargo-with-cuda.ps1 clippy --release -- -D warnings`
  — must pass.
- `npx tsc --noEmit` — must pass (no UI delta this wave).
- `npm test` + `npm run build` — skipped this wave (no UI delta);
  re-runs at every downstream wave.

### Seal

- All deliverables in tree.
- STATUS.md in-flight block reflects re-scope.
- `bd` epic + sub-beads created; `mb-oji5` closed.
- Commit message references the epic id + this wave's sub-bead id.
- Open `bd update <1D.0 id> --status closed` after commit lands.

---

## Wave 1D.1 — Migration: `sessions.source` + category + dictation-tail source-gate

**Agent:** `migration-author`.
**Blocks:** Waves 1D.2–1D.6.
**Files affected (new):**
- `src-tauri/src/db/migrations/025_kg_phase_1d_source_gate.sql`

**Files affected (modified, additive):**
- `src-tauri/src/db/migrations.rs` (registers 025; bumps trigger /
  table count in tests if applicable).
- `src-tauri/src/dictation.rs` (or `dictation/runtime.rs` —
  whichever holds `persist_complete`) — wrap the existing
  `kg::enqueue_for_filing(...)` call site in
  `if session.source == DictationSource::KgNote { ... }`.
- `src-tauri/src/dictation/types.rs` (or model) — new
  `DictationSource` enum: `Ptt`, `InApp`, `KgNote`. Serializes to
  the new `sessions.source` column.
- `src-tauri/src/dictation/runtime.rs` — `start_dictation(...)`
  accepts an optional `source` param defaulting to `Ptt`. The Right
  Alt PTT path passes `Ptt`; the `+ New Dictation` button passes
  `InApp` (per ADR 0045); the new KG audio-note button (Wave 1D.3)
  passes `KgNote`.
- `src-tauri/src/commands/dictation.rs` — IPC accepts an optional
  `source` string; defaults to `'in_app'` (the existing
  `start_mode` from migration 017 carries the semantic, but
  `source` is the more general field — see Schema design notes
  below).
- `src-tauri/src/settings/model.rs` — no change required (the
  `KgGraphEnabled` setting reset is a SQL-level UPDATE inside the
  migration transaction).

### Migration 025 — schema shape (canonical DDL authored by 1D.1)

Adds (additive only):
- `sessions.source TEXT NOT NULL DEFAULT 'ptt'` — values `'ptt' | 'in_app' | 'kg-note'`.
- `sessions.category TEXT NULL` — consumes `mb-oji5`; classify pass writes Personal / Professional / Objective at filing time. No index in 1D.1 (cardinality 3; bitmap-style index is overkill).
- `CREATE INDEX idx_sessions_source ON sessions(source, started_at DESC)`.

Purges (drift correction; ADR 0052 §D6) — in the same transaction:
- `DELETE FROM kg_entity_mentions; DELETE FROM kg_tag_mentions; DELETE FROM kg_filing_queue; DELETE FROM kg_entities;`
- `UPDATE settings SET value='false' WHERE key='kg_graph_enabled';`

Followed by the standard `UPDATE schema_meta SET value='25' WHERE key='schema_version'`.

### Schema design notes for 1D.1 author

- **`start_mode` vs `source`.** Migration 017 already added `sessions.start_mode` (`'ptt'` / `'in_app'`) for the FSM. Default bias: keep `start_mode` as-is (activation mechanism) and add a separate `source` column (disposition metadata) — `start_mode='in_app'` could be either the Dictations button (history-only) or the KG screen button (history + KG). Final pick by the 1D.1 brief.
- **Purge transactionality.** Single `BEGIN TRANSACTION; ... COMMIT;` envelope; partial migration impossible.
- **Idempotency.** Standard migration-runner schema-version check covers re-runs.

### Dictation-tail call-site change

One `if` block around the existing `kg::enqueue_for_filing(...)` call: fire iff `session_row.source == DictationSource::KgNote`. The `KgGraphEnabled` check stays inside `enqueue_for_filing` as defense in depth. Phase MC's `mc-dictation-untouched` judge re-runs in 1D.6 against the `phase-mc-start` baseline to confirm this is the only new delta on `dictation/*.rs` since the seal.

### Validation gates (Wave 1D.1)

- Cargo wrapper: fmt + clippy + check + `test --release --no-run` —
  all green.
- Migration runner test (`src-tauri/tests/db_migrations.rs` if
  present, else inline) — fresh DB → migrations 001..025 →
  `schema_version=25` + new columns present + KG tables empty.
- `bd close mb-oji5` referenced in the commit message — this
  migration is its consumer.
- The dictation tail's existing parity tests stay green
  (`kg_parity --persist 32/32`).

---

## Wave 1D.2 — KG screen scaffold + read-only dashboard

**Agent:** `ui-author`.
**Blocks:** Waves 1D.3, 1D.4, 1D.5, 1D.6.
**Files affected (new):**
- `ui/src/routes/knowledge-graph/index.tsx` — entry point /
  route component.
- `ui/src/routes/knowledge-graph/Dashboard.tsx` — read-only
  dashboard band.
- `ui/src/routes/knowledge-graph/Dashboard.module.css`.
- `ui/src/routes/knowledge-graph/types.ts` — typed DTOs mirroring
  Rust.
- `src-tauri/src/commands/kg.rs` — additive
  `kg_dashboard_snapshot()` command returning the dashboard
  payload in one round-trip.
- `src-tauri/src/kg/dashboard.rs` (NEW) — pure-Rust dashboard
  data assembly from existing store helpers. Throwaway-crate
  testable.
- `src-tauri/capabilities/default.json` — allowlist the new
  command.

**Files affected (modified, additive):**
- `ui/src/Sidebar.tsx` — one new nav entry for `/knowledge-graph`
  (hidden when `KgGraphEnabled=false` per the graph-off-UI invariant
  J3).
- `ui/src/main.tsx` (or router config) — register the route.
- `ui/src/lib/tauri.ts` — typed IPC binding for the new command.
- `ui/src/i18n/en.json` — `kg.dashboard.*` copy strings.

### Dashboard contract (D2 of ADR 0052)

The dashboard renders four sub-bands; with toggle off and direct-URL
access, all four render an "KG filing is off" empty state.

1. **Counts.** Entities total + per-type counts; filed entries total +
   per-state counts (`pending`, `processing`, `done`, `failed`).
2. **Recent activity.** Last 10 filed entries with timestamps + the
   per-entry entity / tag chip strip (re-uses the Phase 1C chip
   primitive — Wave 1D.4 owns the relocation).
3. **Queue state line.** `"N queued · M processing · K failed"`. The
   failed count is click-through to the existing failed-filings UI
   (relocates from Settings tab → here in Wave 1D.4).
4. **Flagged for review.** Currently this is the same set as
   `state='failed'` (no other flagging exists in v1). Phase 1F may
   add "uncertain entries" — TBD.

Upcoming due dates per ADR 0052 §D2 land as an empty-state
placeholder in 1D.2 (the underlying data isn't queryable until Phase
1E's vault projection). Wave 1D.2 ships the slot; Phase 1E populates.

### Validation gates (Wave 1D.2)

- Cargo wrapper: fmt + clippy + check + `test --release --no-run`.
- `npx tsc --noEmit` + `npm test` + `npm run build`.
- New Playwright spec `ui/tests/kg-dashboard.spec.ts` walks the
  dashboard route with fixture KG data; asserts the four bands
  render expected counts.

---

## Wave 1D.3 — KG capture surface: audio note + text note

**Agent:** `ui-author` + code-puppy (collaboration).
**Risk:** Highest blast radius — text-note path may force a thin
cleanup-module / pipeline refactor (see §"Risks" below).
**Blocks:** Wave 1D.4 (which assumes the capture surface exists),
1D.6.
**Files affected (new):**
- `ui/src/routes/knowledge-graph/Capture.tsx` — the audio + text
  band component.
- `ui/src/routes/knowledge-graph/Capture.module.css`.
- `src-tauri/src/kg/ingest_text.rs` — text-note entry point. Bypasses
  the audio pipeline (no transcription stage). Runs the existing
  4-pass-plus-extract_entities pipeline starting at `segment`. Writes
  to `kg_filing_queue` directly with a synthetic `entry_id`
  (shape TBD in the brief — likely a new row in a small auxiliary
  table `kg_text_notes` that the queue's `entry_id` references via a
  discriminator column on the queue row).
- `src-tauri/src/commands/kg.rs` — additive `kg_ingest_text_note(text)`
  + `kg_ingest_audio_note_start()` + `kg_ingest_audio_note_stop()`.
- `src-tauri/capabilities/default.json` — allowlist.

**Files affected (modified):**
- `src-tauri/src/dictation.rs` (or runtime) — the KG audio-note
  flow drives the same dictation pipeline with `source=KgNote`.
  No new audio-capture code; reuse Right Alt's path with an
  alternate start-trigger.
- `ui/src/lib/tauri.ts` + `ui/src/lib/types.ts` + `ui/src/i18n/en.json`.

### Capture contract (D3 of ADR 0052)

- **Audio note button** on the KG screen. Press → start a dictation
  with `source=KgNote`. Optionally fullscreen overlay (or inline
  recording UI — Wave 1D.3 brief picks). On stop, the dictation
  persists to `sessions` / `transcripts` (history; user can still
  find it on the Dictations page) AND the dictation-tail hook fires
  (source-gate passes; toggle check inside `enqueue_for_filing`
  passes). Dashboard's "recent activity" band picks up the new row.
- **Text note input** on the KG screen. A `<textarea>` + "File to
  KG" submit button. On submit → `kg_ingest_text_note(text)` IPC →
  pipeline runs synchronously up to `kg_filing_queue` enqueue (the
  async worker processes from there). **Does NOT write to
  `sessions` or `transcripts`** — text notes are KG-only.

### Refactor seam (LIKELY)

Text-note ingest needs to enter the pipeline at the post-transcription stage. Default to extracting a shared `kg::pipeline::run_from_text(text) -> Result<...>` that text notes call directly and audio notes call after transcription. DRY-honoring; also de-risks Phase 1E's vault-ingest entry point which will need the same shape. Fallback (synthesize a fake `Transcript` struct) is hacky and rejected unless extraction proves disproportionately invasive.

### Validation gates (Wave 1D.3)

- Cargo wrapper full gate.
- New unit test (throwaway-crate): `kg::pipeline::run_from_text`
  produces identical structured output for the same input text
  whether called via audio path or text path.
- Playwright `ui/tests/kg-capture.spec.ts` — fires both capture
  paths against a mock IPC + asserts the expected queue inserts.
- Manual smoke (Dustin): fire an audio note + a text note; verify
  audio note appears in Dictations history + KG dashboard; verify
  text note appears in KG dashboard ONLY.

---

## Wave 1D.4 — Relocate filter chips + concept modal to KG screen

**Agent:** `ui-author`.
**Blocks:** Wave 1D.6.
**Files affected (new):**
- `ui/src/routes/knowledge-graph/Retrieval.tsx` — the retrieval
  band (filter chips + per-row chip strip + filing-state pills).
  Reuses the sub-components extracted from `Dictations.tsx` at
  Wave 1C.3 — no UI logic change, just relocation.

**Files affected (modified, mostly subtractive):**
- `ui/src/pages/Dictations.tsx` — remove the KG filter chip row +
  per-row entity/tag chip strip + filing-state pills + concept-modal
  open handlers. Page returns to its pre-1C shape (history list +
  search). Imports drop.
- `ui/src/pages/components/Dictations*.tsx` — relocated to
  `ui/src/routes/knowledge-graph/components/` (move-and-import-update
  PR — no behavior change).
- `ui/src/pages/SettingsKgTab.tsx` — the failed-filings panel
  relocates to the KG dashboard's queue band; the Settings KG tab
  retains the activation toggle only (plus Wave 1D.5's additions).
- `ui/tests/kg-graph-off-invariant.spec.ts` — extend to walk the
  new KG screen route (toggle off + on) in addition to the existing
  Settings + Dictations walks.
- `ui/tests/kg-dictations-retrieval.spec.ts` — update to point at
  the new route OR move the spec file under the new screen's spec
  tree.

### Why the chips relocate (D5 of ADR 0052)

The chips were Phase 1C's best available home given D1's "no new
top-level page" decision. With the KG screen now first-class, the
chips belong there: they're retrieval UI for a KG-specific surface,
not a general dictation-history primitive. The Dictations page's
search box stays for full-text history search; the KG-specific
retrieval lives where the KG-specific capture lives.

### Validation gates (Wave 1D.4)

- Cargo wrapper full gate (no Rust changes expected this wave).
- `npx tsc --noEmit` + `npm test` + `npm run build`.
- Playwright sweep: `kg-graph-off-invariant`, `kg-dashboard`,
  `kg-retrieval`, `kg-concept-modal`, `kg-capture` all green
  against the new route shape.
- The tightened `kg-graph-off-invariant` (J3 in ADR 0052 §"Acceptance
  gates") now passes against the new route.

---

## Wave 1D.5 — Settings expansion + launch-into-Obsidian

**Agent:** `ui-author`.
**Blocks:** Wave 1D.6.
**Files affected (new):** none (additive to existing).

**Files affected (modified):**
- `ui/src/pages/SettingsKgTab.tsx` — add three rows:
  - Vault path (read-only display + edit button; default value
    pulled from Mobile Sync setting if present, else "(unset)").
  - Vocabularies (Layer 1 categories + Layer 2 types) — read-only
    text/list view per spec §15.2; no editor in 1D.
  - Launch-into-Obsidian button (mirror of the KG screen's).
- `ui/src/routes/knowledge-graph/Actions.tsx` (NEW small
  component) — owns the launch-into-Obsidian button on the KG
  screen.
- `src-tauri/src/commands/kg.rs` — additive `kg_launch_obsidian()`
  + `kg_vocabularies_get()` IPCs. Capabilities allowlist.
- `src-tauri/src/kg/launcher.rs` (NEW) — owns the Obsidian process
  launch. Windows-only impl; macOS stub (`todo!()` behind
  `#[cfg(target_os = "macos")]` per Principle 5).

### Launch-into-Obsidian semantics

Open the configured vault path in Obsidian via the `obsidian://`
URI scheme (`obsidian://open?vault=<vault-name>`). If Obsidian isn't
installed, the launch fails silently with a toast suggesting
installation. The launcher does NOT shell out to the bare
filesystem path — the URI scheme is the canonical handoff per spec
§15.4.

### Validation gates (Wave 1D.5)

- Cargo wrapper full gate.
- `npx tsc --noEmit` + `npm test` + `npm run build`.
- Manual smoke (Dustin): vault path set; click button; Obsidian
  opens. Vocabularies display matches the pipeline's actual taxonomy
  constants.

---

## Wave 1D.6 — Phase 1D judges + seal

**Agent:** code-puppy.
**Files affected (new):**
- `docs/judges/phase-1d/kg-source-gate-invariant.md` (J1).
- `docs/judges/phase-1d/kg-dictation-untouched.md` (J2; extends Phase
  MC's `mc-dictation-untouched`).
- `docs/judges/phase-1d/kg-graph-off-ui-tightened.md` (J3; extends
  1C.5's).
- Optional: `src-tauri/src/kg/judges/source_gate.rs` (deterministic
  Rust-side judge if the LLM-graded path isn't necessary; Wave 1D.6
  author picks).

**Files affected (modified):**
- `docs/adr/0052-knowledge-graph-phase-1d-charter.md` — Status:
  Proposed → Accepted. Append a "Phase 1D SEALED" close-out section
  mirroring the ADR 0051 / ADR 0050 patterns (commit chain, gate
  results, beads closed, key in-flight findings).
- `STATUS.md` — move Phase 1D from "Currently active" to "Sealed"
  (lateral-epic row).
- `docs/PRODUCT-STATE.md` — update the KG subsystem section to
  reflect source-gating, the new screen, and the relocated retrieval.
- `docs/LESSONS.md` — append any non-obvious findings from the 1D
  arc; promote to PINNED only if load-bearing.

### Judges (verbatim spec is in ADR 0052 §"Acceptance gates")

- **J1 — `kg-source-gate-invariant` (principal).** Fixture sessions
  with three sources × two toggle states → assert exactly the
  expected `kg_filing_queue` row count. Throwaway-crate test OR
  Playwright spec.
- **J2 — `kg-dictation-untouched`.** Diff `dictation/*.rs` against
  `phase-mc-start` anchor; the only authorized delta is the 1D.1
  source conditional + the `DictationSource` type wiring. Any other
  change fails.
- **J3 — `kg-graph-off-ui-tightened`.** Playwright walks the KG
  screen route (toggle off + on) + Dictations page + Settings/KG
  tab; records every `kg_*` IPC via `__KG_IPC_SPY__`; with toggle
  off the recorded set is exactly `{kg_settings_get_all}`; with
  toggle on the positive-control IPCs light up on the dashboard mount.
  Sidebar entry hidden when toggle off.

### Deterministic post-migration assertion

A sqlite SELECT in the seal commit verifies that after migration 025
applies, all four `kg_*` mention/queue/entity tables are empty and
`settings.kg_graph_enabled = 'false'`. The Wave 1D.0 clean-slate
verification provides the baseline; 1D.6 confirms the migration
landed the purge correctly.

### Validation gates (Wave 1D.6)

- Cargo wrapper full gate (fmt + clippy + check + `test --release
  --no-run`).
- `npx tsc --noEmit` + `npm test` + `npm run build`.
- All three judges green.
- Manual smoke (Dustin) sign-off on the live Win11 box:
  1. Flip toggle on → KG screen visible in sidebar → dashboard
     renders.
  2. Record an audio note → appears in both Dictations history AND
     KG dashboard.
  3. Submit a text note → appears in KG dashboard ONLY.
  4. Press Right Alt → record a normal PTT dictation → appears in
     Dictations history; **does NOT appear in KG dashboard** (the
     source-gate invariant in user-visible form).
  5. Flip toggle off → KG screen disappears from sidebar; previously
     captured KG data is preserved (no destructive deletion).

### Seal criteria

- ADR 0052 → Accepted with close-out section.
- All three judges green.
- Cargo + UI gates green.
- Manual smoke matrix green.
- STATUS.md updated (Sealed table gains a row).
- `bd` epic + all sub-beads closed.
- Commit tagged with a clear `KG Phase 1D SEALED:` message. **No
  new `phase-*-complete` tag.**

---

## Bead epic

Created at Wave 1D.0. All ASCII-only per LESSONS 2026-05-24.

- `mb-x7f9` — KG Phase 1D - epic - source-gated filing + KG screen (P1)
- `mb-qhll` — 1D.0 - charter + clean-slate verify + phase doc + bead epic (P1) [closed at this wave's commit]
- `mb-pxzk` — 1D.1 - migration: source + category columns; gate dictation-tail hook (P1)
- `mb-j00j` — 1D.2 - KG screen scaffold + read-only dashboard (P1)
- `mb-0gt6` — 1D.3 - KG capture surface: audio note + text note (P1)
- `mb-6hm2` — 1D.4 - relocate filter chips + concept modal to KG screen (P1)
- `mb-navi` — 1D.5 - settings expansion + launch-into-Obsidian (P2)
- `mb-q2p1` — 1D.6 - Phase 1D judges + seal (P1)

Dependency chain (`bd link <a> <b>` means `a` depends on `b`): `mb-qhll ← mb-pxzk ← mb-j00j ← mb-0gt6 ← mb-6hm2 ← mb-navi ← mb-q2p1`. All sub-beads block the epic `mb-x7f9`. `mb-oji5` is **closed by Wave 1D.0** with a resolution pointing to `mb-pxzk` (the migration is the consumer).

---

## Out of scope (explicit)

### Deferred to Phase 1E (Obsidian-as-source-of-truth)

- Markdown projection of KG entries to `<vault>/knowledge-graph/`
  per spec §7.1 + §14.1.
- Reverse-watcher (vault .md changes → mockingbird.db updates) per
  ADR 0048 Q3.
- KG-Inbox courier (mobile-shortcut-delivered captures auto-flow
  through the same KG pipeline).
- History archive folder semantics per spec §7.1.
- Obsidian Tasks format emission per spec §7.4.
- Pre-built Kanban / dashboard / board notes per spec §14.2.

The Phase 1E charter (future ADR 0053) inherits ADR 0048 Q1/Q2/Q3
decisions and ADR 0046 vault-handling primitives.

### Deferred to Phase 1F (was Phase 1E pre-re-scope)

- The `v1-beta` git tag.
- Full-system smoke matrix on a fresh Win11 box.
- Release wiring (installer / updater verification).
- Marketing-shaped privacy statement / docs polish.

### Deferred post-v1

- **Synthesize operation** per the Clark article (cross-entry
  agentic summarization).
- Routing-step optimization (per-pass Nemotron-style model
  selection).
- Entity backfill of historical dictations into the KG (the
  originally-moot "Phase 1D backfill" scope; if it ships, it's a
  per-row promote-to-graph affordance on the Dictations page, not
  a bulk operation).
- Vocabularies editor (post-v1 — KG settings shows them read-only
  in 1D.5).
- Archive vs Ingest mode toggle per spec §10.
- Interactive ingest mode (per-entry user confirmation per spec
  §15.5 / §11).

---

## Risks

| Risk | Wave | Mitigation |
|---|---|---|
| Text-note path forces a cleanup-module / pipeline refactor wider than expected. | 1D.3 | Default to the `kg::pipeline::run_from_text` shared-entry-point shape (also de-risks Phase 1E's vault-ingest entry point). Wave 1D.3 brief picks the seam before any UI code lands. |
| Reverse-watcher infinite-loop concern (Phase 1E flag, not 1D risk). | (1E) | Wave 1D.3's text-note ingest leaves a clean seam for "this row originated outside the vault" provenance so Phase 1E's reverse-watcher can short-circuit re-projection. Documented in 1D.3's brief. |
| Stale-prompt risk on Wave 1D.N dispatches. | 1D.1–1D.6 | The phase doc on disk is the canonical reference; per LESSONS P8 dispatch prompts are one-line pointers (`implement Wave 1D.N per docs/phases/phase-1d.md`) — no embedded specs. |
| Latency budget re-confirmation at 1D.3 smoke. | 1D.3 | Carry-forward from 1C: p95=59s per filing on this hardware. Text notes skip transcription but pay the four-pass + extract_entities cost; expect ~40–50s end-to-end. If text-note UX latency becomes a complaint, the answer is an async "filing in progress" surface on the dashboard — not cutting passes. |
| `mb-oji5` category-axis dropped on the floor. | 1D.1 | Explicitly consumed by Wave 1D.1 migration; the `bd close mb-oji5` step is in Wave 1D.0's commit. |
| Dictations page regression after the chip relocation. | 1D.4 | Subtractive-only; the `Dictations.tsx` post-1D.4 shape returns to the pre-1C.3 state with the search box only. Playwright spec `kg-dictations-retrieval.spec.ts` is rewritten against the new home before the relocation lands. |
| Live-OS regression on the dictation tail (Phase MC's `dictation-untouched` invariant). | 1D.1 + 1D.6 | The 1D.1 patch is one `if` block at the call site; the 1D.6 judge re-runs `mc-dictation-untouched` against the `phase-mc-start` anchor and fails on any other delta. |

---

## How to resume Phase 1D mid-execution

1. Read STATUS.md "🟢 Currently active" — it'll point at the
   in-flight wave.
2. Read this file (you're here).
3. Read ADR 0052 (charter).
4. For the in-flight wave: `bd show mb-<wave-id>` for the sub-bead
   detail; `bd ready -t task` filtered for blocked-by-this-wave.
5. `git log --oneline --grep "1D\." -20` for the wave commit chain
   to date.
6. THEN start work.

The session-start ritual in AGENTS.md still applies (read PINNED
LESSONS, etc.) — this list is the Phase-1D-specific layer on top.

---

## Cargo gate (binding per LESSONS P2)

Phase 1D uses the existing accepted fallback gate. No new gate.

- **Pure-Rust modules** → throwaway-crate recipe (LESSONS 2026-05-17).
  Eligible 1D modules: `kg::ingest_text` (no whisper-rs / ort
  deps if the seam is clean), `kg::dashboard`, `kg::pipeline::run_from_text`
  if extracted.
- **Wired modules** → cargo check + clippy `--release -- -D warnings`
  + fmt `--check` + test `--release --no-run` via the Windows
  wrapper. Plus per-wave human-in-loop smoke per the validation
  gates above.

---

## UI gate (binding)

- `npx tsc --noEmit` clean.
- `npm test` (vitest) clean.
- `npm run build` clean.
- `npm run lint` currently broken (`mb-yxh`); ignore until that's
  resolved.
- Per-wave Playwright sweep on whatever surfaces moved that wave.

---

## Latency budget (carry-forward from Phase 1C)

Phase 1C empirically locked **p95=59s per filing** at qwen2.5:7b on
this hardware (`docs/knowledge-graph/phase-1c-latency-baseline.md`).
The 5-pass pipeline's `extract` + `extract_entities` together account
for ~82% of cost. Phase 1D does NOT change pipeline shape, so the
budget carries forward unchanged. Re-measurement at 1D.3 if the
text-note ingest path shifts the cost shape; otherwise the 1C
baseline stays canonical.
