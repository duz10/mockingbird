# ADR 0052 — KG Phase 1D: source-gated filing + first-class KG screen

- **Status:** **Accepted (2026-06-04)** — see "Phase 1D SEALED" close-out at the end of this ADR.
- **Date:** 2026-06-04
- **Deciders:** Dustin, planning-agent (`planning-agent-f1b5c1`), code-puppy (`code-puppy-925ce4`)
- **Charter for:** ADR-lateral epic; no `phase-*-complete` tag (per LESSONS PINNED P5)
- **Charter bead:** `mb-x7f9` (epic) / `mb-qhll` (Wave 1D.0)
- **Supersedes:** [ADR 0050](0050-kg-phase-1b-persistence-and-dictation-hook.md) §D6 dictation-tail hook gating semantics — the hook is now source-gated. ADR 0050 otherwise stays in force (worker, schema, IPC, idempotency contract).
- **Amends:** [ADR 0051](0051-kg-phase-1c-retrieval-ux-and-activation.md) §"Out of scope" — the retrieval surface (filter chips + concept modal) relocates from the Dictations page to the new KG screen at Wave 1D.4; deferred `mb-oji5` (category persistence) is consumed by Wave 1D.1.
- **Inherits:** [ADR 0049](0049-knowledge-graph-phase-0-5-and-v1-architectural-pivot.md) §6 binding architecture commitments + §"Sandbox isolation" — this charter opens a fresh graduation window for `src-tauri/src/kg/**`, the new migration, and the new `ui/src/routes/knowledge-graph/**`.
- **Mirrors authorization-clause pattern from:** [ADR 0037](0037-unified-recording-command-center.md) §5 + [ADR 0051](0051-kg-phase-1c-retrieval-ux-and-activation.md) §"UI sealed-surface authorization".
- **Source-of-truth pointers:** original product spec `C:\Users\dboyd\Downloads\mockingbird-knowledge-graph-spec.md` §1, §7.1, §10, §15.2, §15.3, §15.4, §15.5; PHASE-0-5-REPORT.md §6 + §7.

---

## Status

**Accepted (2026-06-04).** Six waves shipped. Wave 1D.0 chartered
(this ADR + clean-slate verification + phase doc + bead epic).
Waves 1D.1–1D.5 implemented. Wave 1D.6 (this seal) lands the three
acceptance-gate judges and flips this ADR to Accepted. No new
`phase-*-complete` tag (lateral epic; LESSONS PINNED P5). Phase 1D's
user-visible surface ships as the new sidebar entry "Knowledge Graph"
with a 5-band dashboard, a capture surface (audio note + text note),
a retrieval surface (filter chips + concept modal, relocated from
Dictations), and a Settings panel expansion (vault path + Obsidian
launch). Default-off binding preserved (migration 025 seed) and
strengthened by the new source-gated filing: standard dictations
NEVER enqueue, regardless of toggle state.

## Context

Phase 1C (ADR 0051) sealed 2026-05-31 with five of the six promised
retrieval axes online plus a concept modal, gated by the activation
toggle and a Playwright-graded `kg-graph-off-ui-untouched` invariant.
The standing Phase 1D framing inherited from that seal was "one-shot
backfill of pre-Phase-1 entries into the graph." On 2026-06-04 Dustin
and planning-agent did a source-of-truth alignment review against the
original product spec (the markdown file Phase 0 was chartered from)
and the Clark article that informed Phase 0.5. **The review identified
two surface drifts in the as-shipped Phase 1B + 1C architecture that
the backfill framing did not address.**

### Drift 1 — Trigger direction is backwards

ADR 0050 §D6 wired the dictation-tail hook to enqueue **every**
dictation when `KgGraphEnabled = true`. Per spec §1 ("knowledge graph
is a separate capture surface; dictations are the speech-to-text
primitive") and §15.5 ("on-app capture — audio note and text note,
same pipeline, different entry points"), and per Dustin's 2026-06-04
clarification, the dictation-tail hook should fire **only** for
dictations originating from the KG capture surface. Standard Right
Alt PTT dictations and the in-app "+ New Dictation" button must
never feed the KG, regardless of toggle state. The current wiring
treats dictation as the KG's input — but the spec treats them as
sibling subsystems sharing one pipeline, with the user explicitly
choosing which surface to capture from.

The empirical evidence: the clean-slate check at Wave 1D.0 (this
wave) shows two pre-existing entries in the KG (`sessions.id` 128 +
129, both filed 2026-05-30 during 1C development), which is non-zero
but trivially small because the toggle was rarely flipped on. Had the
toggle been left on across a normal dictation week, the KG would now
contain hundreds of stream-of-consciousness PTT entries that the user
never intended as graph-worthy notes.

### Drift 2 — The KG screen was never built

Per spec §15.3 ("the Knowledge Graph screen — left sidebar,
read-only dashboard, first-class destination alongside the existing
sections"), the canonical "new UI surface" for the v1 KG is a
left-sidebar destination with a read-only dashboard, audio note
capture, text note capture, and a launch-into-Obsidian button. Phase
1C D1 ("extend Settings.tsx in place, no new top-level page")
narrowed the canonical "new UI surface" to filter chips on the
Dictations page. That narrowing was correct for the activation
toggle (Settings is the right home for it) but incorrect as the
overall v1 KG home: there's no place to fire an audio note from, no
place to capture a typed-only note from, no place to see at-a-glance
counts or queue state, and no Obsidian launch surface. The retrieval
chips and concept modal that did ship at 1C.3 / 1C.4 are correctly
shaped UI primitives but live on the wrong page.

### Why the original "Phase 1D = backfill" framing is moot

If the dictation-tail hook should only fire for `source = 'kg-note'`
captures (drift 1), then **the pre-Phase-1 dictations were never
meant to be in the graph in the first place**. Backfilling them
would amplify the drift, not heal it. The correct shape of Phase 1D
is: source-gate the trigger, build the canonical capture surface, and
let the user choose graph-worthy notes going forward. Backfill of
historical dictations into the KG is post-v1 work (post-`v1-beta` tag)
with its own ADR and its own user-facing "promote to graph" affordance
on the Dictations page, if it ships at all.

### Phase numbering after the re-scope

| Phase | Charter | Scope |
|---|---|---|
| **1D** (this ADR) | ADR 0052 | Source-gated filing + first-class KG screen + retrieval relocation |
| **1E** (next, future ADR 0053) | TBD | Obsidian-as-source-of-truth: vault subtree projection (ADR 0048 Q3) + reverse-watcher (.md → mockingbird.db) |
| **1F** (was 1E) | TBD | v1 beta tag; full-system smoke; release wiring |

The renumbering is mechanical and recorded in §References below.

## Decision

### D1 — Source-gate the dictation-tail hook

Add a `source` column to the `sessions` table (or extend an existing
column if one is suitable — Wave 1D.1's brief picks). Values: `'ptt'`
(Right Alt push-to-talk; today's default), `'in_app'` (existing
`+ New Dictation` button; cf. migration 017 `start_mode`), `'kg-note'`
(NEW — KG audio note from the new screen). The dictation-tail call
site (`dictation.rs::persist_complete`'s ignore-error
`kg::enqueue_for_filing(...)`) is gated to fire **only** when
`source = 'kg-note'` AND `KgGraphEnabled = true`. The `KgGraphEnabled`
check stays inside `enqueue_for_filing` for defense in depth; the new
source-gate is a new conditional at the call site.

Text notes (D3 below) bypass the dictation pipeline entirely; they
write directly to `kg_filing_queue` with a synthetic `entry_id` shape
TBD in Wave 1D.3's brief.

### D2 — The KG screen as a first-class sidebar destination

New route `/knowledge-graph` with its own left-sidebar entry above
or below Dictations (final position picked in Wave 1D.2 with
qa-kitten review). The screen is **read-only for existing data**
(per spec §15.1) and contains:

- **Top band — Dashboard.** Counts (entities, mentions, queued,
  done, failed), recent activity (last N filed entries with
  timestamps + entity/tag chips), queue state line ("3 queued · 1
  failed"), upcoming due dates (placeholder until task-with-due
  surfaces; Wave 1D.2 ships an empty-state if no data), flagged-for-
  review (anything `kg_filing_queue.state = 'failed'`, click-through
  to the failures retry UX which migrates here from Settings).
- **Middle band — Capture.** Two affordances side-by-side:
  audio-note record button (full-screen recording overlay or
  same-window record, picker TBD in Wave 1D.3) + text-note input
  field with "File to KG" submit.
- **Right side or below — Retrieval.** The Phase 1C filter chips
  (entity, tag, free-text, filing-state pills) + per-row entity/tag
  chip strip + the concept modal — **all relocated** from the
  Dictations page (Wave 1D.4). The Dictations page returns to its
  pre-1C shape (history list with search, no KG chips).
- **Action band — Launch.** "Open in Obsidian" button per spec
  §15.4 (Wave 1D.5; uses the vault path configured in Settings).

### D3 — KG capture surface: dual-write audio + KG-only text

Per spec §15.5:

- **Audio note** captures via the same audio path as Right Alt
  dictation, **dual-writes** to (a) the regular `sessions` /
  `transcripts` tables with `source = 'kg-note'` AND (b) — via the
  source-gated dictation-tail hook — into `kg_filing_queue`. The
  user can still find the dictation in their history; the KG
  filing is the additive side effect.
- **Text note** is KG-only: typed into the KG screen, runs through
  the post-transcription portion of the pipeline (segment →
  classify → extract → extract_entities → normalize), files into
  `kg_filing_queue`, and **does not** write to `sessions` /
  `transcripts`. No audio ever existed; the dictation history is
  not the right home for it.

Wave 1D.3's brief picks the exact wire shape — likely a new
`kg::ingest_text_note(text)` function that synthesizes a virtual
entry_id for the queue row (or a new nullable column on
`kg_filing_queue` for text-note provenance — schema decision deferred
to brief).

### D4 — Settings expansion (Wave 1D.5)

The existing Settings → KG tab (1C.1) stays; this wave adds:

- **Vault path** field. Picks up the value from any existing
  Mobile Sync setting (ADR 0046) when present; otherwise prompts.
  Read-only display + edit button to keep the destructive change
  intentional.
- **Vocabularies — read-only display.** Per spec §15.2, show the
  Layer 1 categories + Layer 2 types that the pipeline currently
  uses. Read-only in Phase 1D (editor deferred per §"Out of scope").
- **Launch-into-Obsidian button** (mirror of D2's on the KG
  screen — convenience).

### D5 — Retrieval relocation (Wave 1D.4)

The filter chips + concept modal extracted out of `Dictations.tsx`
sub-components (Wave 1C.3 already did the 3-way extraction;
relocation is mostly an import-path + route move). `Dictations.tsx`
loses its KG-specific UI; the Phase 1C `kg-graph-off-ui-untouched`
Playwright spec is updated to walk the new KG screen route (with
toggle on AND off) instead of (or in addition to) the Dictations
page. The invariant tightens: with toggle off, the KG screen route
is hidden from the sidebar entirely (the route still exists for
direct-URL access but the dashboard renders a "KG filing is off —
flip the toggle in Settings to enable" empty state and invokes
zero `kg_*` IPCs).

### D6 — Clean-slate Wave 1D.1 purge step

Wave 1D.0's sqlite probe revealed non-zero pre-existing KG state on
Dustin's box (3 entities, 11 tag mentions, 3 entity mentions, 2
done filings — entries 128 + 129 from 2026-05-30, evidently a 1C
development test). Per the trigger-direction drift, these entries
were never meant to be in the graph. Wave 1D.1's migration
**purges all rows** from `kg_entities`, `kg_entity_mentions`,
`kg_tag_mentions`, `kg_filing_queue` (and resets
`settings.kg_graph_enabled = 'false'`) as part of the same
transaction that adds the `source` column. This is **safe** because
(a) the contract was always opt-in, (b) the volume is trivial, and
(c) the data was test-mode, not production capture. If a future
Dustin's box (or a beta tester's) has a larger pre-existing KG, the
migration is still correct — the rule "if it isn't from a
`source = 'kg-note'` dictation, it was never meant to be in the
graph" is uniform.

The judge bundle in Wave 1D.6 includes a post-migration assertion
(`kg-source-gate-purge-clean`) that no rows from the pre-1D
fixture survive into a post-migration DB.

### D7 — Wave plan (six waves)

| Wave  | Scope | Agent | Sub-bead |
|---|---|---|---|
| 1D.0  | This charter + clean-slate verify + phase doc + bead epic. NO migration, no Rust, no UI. | code-puppy | (this wave) |
| 1D.1  | Migration 025: `sessions.source` column + purge step + `KgGraphEnabled=false` reset + consumes `mb-oji5` category-axis schema work + source-gate the dictation-tail hook call site. | migration-author | TBD |
| 1D.2  | KG screen scaffold + read-only dashboard (counts, recent activity, queue state, due-dates placeholder, flagged-for-review). | ui-author | TBD |
| 1D.3  | KG capture surface: audio note (dual-write) + text note (KG-only). Likely forces a small cleanup-module refactor to share the post-transcription pipeline. | ui-author + code-puppy | TBD |
| 1D.4  | Relocate filter chips + concept modal from Dictations page → KG screen; tighten the graph-off-UI invariant judge. | ui-author | TBD |
| 1D.5  | Settings expansion (vault path config; vocabularies read-only display; launch-into-Obsidian button). | ui-author | TBD |
| 1D.6  | Judges (source-gate invariant, dictation-untouched extension, graph-off-UI tightened) + epic seal. Flip this ADR to **Accepted**. | code-puppy | TBD |

Sub-bead IDs are minted by Wave 1D.0 and recorded in
`docs/phases/phase-1d.md` §"Bead epic".

## Sandbox isolation (graduation window)

Per ADR 0049 §"Sandbox isolation" precedent and ADR 0051 §"UI
sealed-surface authorization", this ADR opens a scoped edit window
on the following surfaces. Outside the list, the seal holds.

| Surface | Authorized change | Closes at |
|---|---|---|
| `src-tauri/src/db/migrations/025_kg_phase_1d_source_gate.sql` (NEW) | Add `sessions.source` column + purge step. | 1D.6 seal. |
| `src-tauri/src/db/migrations.rs` | Register migration 025. | 1D.6 seal. |
| `src-tauri/src/dictation.rs::persist_complete` (or nearest equivalent) | Add `source = 'kg-note'` conditional around the existing `kg::enqueue_for_filing(...)` call. One `if` block. | 1D.6 seal. |
| `src-tauri/src/dictation/runtime.rs` + IPC start path | Accept an optional `source` param on the start signal (defaults to `'ptt'`). | 1D.6 seal. |
| `src-tauri/src/kg/ingest_text.rs` (NEW) | Text-note ingest entry point that bypasses the audio pipeline. | 1D.6 seal. |
| `src-tauri/src/commands/kg.rs` | Additive IPC for the new screen: `kg_dashboard_snapshot`, `kg_ingest_text_note`, `kg_ingest_audio_note_start/stop`. Capabilities/default.json gets new allowlist entries (ADR 0035 discipline). | 1D.6 seal. |
| `ui/src/routes/knowledge-graph/**` (NEW dir) | The new KG screen + dashboard + capture + retrieval. | 1D.6 seal. |
| `ui/src/pages/Dictations.tsx` + `ui/src/pages/components/Dictations*` | Remove the relocated KG filter chips + concept-modal wiring. Page returns to pre-1C shape. Subtractive only — additive UI moves to the KG screen. | 1D.6 seal. |
| `ui/src/pages/Settings.tsx` + `ui/src/pages/SettingsKgTab.tsx` | Additive: vault path field, vocabularies read-only display, launch-into-Obsidian button. | 1D.6 seal. |
| `ui/src/Sidebar.tsx` | One new nav entry for `/knowledge-graph`. | 1D.6 seal. |
| `ui/src/lib/tauri.ts` + `ui/src/lib/types.ts` + `ui/src/i18n/en.json` | Additive bindings + types + copy strings. | 1D.6 seal. |
| `src-tauri/capabilities/default.json` | Allowlist new commands per ADR 0035. | 1D.6 seal. |
| `ui/tests/kg-*.spec.ts` (Playwright) | Update `kg-graph-off-invariant.spec.ts` to walk the new screen; new spec(s) for the source-gate invariant + capture surface. | 1D.6 seal. |

### Explicitly NOT authorized

- New telemetry of any kind (Principle 4).
- Edits to `transcripts` table or its triggers (Principle 1).
- Edits to migrations 001-003 (post-`phase-1-complete` hook enforced).
- Edits to migrations 011 / 012-016 (sealed phases).
- Edits to `meetings/*` (Phase MC sealed).
- Edits to `activity/*` (Phase 10 sealed).
- Vault projection / reverse-watcher / KG-Inbox courier (Phase 1E).
- Obsidian Tasks format emission (Phase 1E).
- Pre-built boards (Phase 1E).
- Vocabularies editor (post-v1).
- Synthesize operation per Clark article (post-v1).
- Archive vs Ingest mode toggle (post-v1).

## Acceptance gates (Wave 1D.6 judge bundle)

Three judges. All deterministic where possible; the source-gate
invariant is LLM-graded only if the deterministic spec proves
infeasible (Wave 1D.6 author picks).

### J1 — `kg-source-gate-invariant` (principal)

Seed a fixture sessions table with three rows: `source='ptt'`,
`source='in_app'`, `source='kg-note'`. With `KgGraphEnabled=true`,
run the dictation-tail enqueue path for each. Assert `kg_filing_queue`
contains exactly one row (the `kg-note` row). With
`KgGraphEnabled=false`, assert zero rows. Pure-Rust throwaway-crate
test against `kg::enqueue_for_filing` semantics OR a Playwright
spec that drives the IPC path — author picks at Wave 1D.6.

### J2 — `kg-dictation-untouched` (extends Phase MC's judge)

The Phase MC `mc-dictation-untouched` judge anchors on the
`phase-mc-start` git tag's `dictation/*.rs` diff. This judge extends
the surface by re-running with the post-1D.1 `source`-conditional
patch as the only authorized delta. Any other diff in `dictation/*`
or `dictation.rs` between `phase-mc-start` and HEAD = fail.

### J3 — `kg-graph-off-ui-tightened` (extends 1C.5's judge)

The 1C.5 `kg-graph-off-ui-untouched` Playwright spec asserted no
`kg_*` IPC was invoked from the Dictations page or Settings (except
`kg_settings_get_all`). This judge re-runs the spec against the new
KG screen route with toggle off AND on:

- Toggle off: sidebar entry hidden; direct-URL access lands on the
  empty state; recorded `kg_*` IPC set is exactly `{kg_settings_get_all}`.
- Toggle on: full screen renders; positive-control flip lights up
  the expected IPCs (`kg_dashboard_snapshot`, `kg_list_failed_filings`,
  etc.).
- Dictations page in either toggle state: zero `kg_*` IPCs (because
  the chips relocated away).

Plus deterministic checks per the seal commit:

- **Clean-slate post-purge:** post-migration sqlite query returns
  zero rows from `kg_entities`, `kg_entity_mentions`,
  `kg_tag_mentions`, `kg_filing_queue`, and `kg_graph_enabled='false'`
  in `settings`.
- **Cargo gate via the Windows wrapper:** fmt + clippy + check + test
  `--release --no-run` all green (LESSONS P2 fallback for the test
  runner).
- **UI gate:** `npx tsc --noEmit`, `npm test`, `npm run build` all
  green (lint still gated by `mb-yxh`).

## Risks

- **Text-note path forces a cleanup-module refactor.** The audio
  pipeline today starts at transcription; the KG pipeline starts at
  segment. Wave 1D.3 needs to expose the segment-onward portion of
  the pipeline without dragging the audio dependency. Likely a thin
  new entry point in `kg/mod.rs` — but the refactor blast radius is
  what makes 1D.3 the highest-risk wave.
- **Reverse-watcher infinite-loop concern (Phase 1E, flagged for
  awareness here).** When Phase 1E ships .md → SQLite reverse
  ingest, naive wiring can ping-pong (SQLite write triggers vault
  projection triggers fs watcher triggers SQLite write). Phase 1D
  does NOT ship the projection or watcher, but Wave 1D.3's text-note
  ingest should be shaped so its eventual vault-projection consumer
  has a clean way to mark "this row was authored from the vault,
  don't re-project". Wave 1D.3's brief documents the seam.
- **Stale-prompt risk on Wave 1D.N dispatches.** The phase doc on
  disk is the canonical reference; per LESSONS P8 dispatch prompts
  should be one-line pointers ("implement Wave 1D.N per
  `docs/phases/phase-1d.md`") rather than embedding the full spec.
- **Latency budget unchanged but worth re-confirming at 1D.3
  smoke.** Phase 1C empirically locked p95=59s per filing at qwen2.5:7b
  on this hardware. Text notes have no transcription stage but
  still pay the four-pass LLM cost; expect ~40-50s end-to-end. If
  text-note feedback latency becomes a UX complaint, an async "filing
  in progress" surface on the dashboard is the right answer — not
  cutting passes.
- **`mb-oji5` category-axis consumption.** The 1C.5 close-out
  promised category persistence would land in 1D's first migration.
  Wave 1D.1's brief picks the column shape (`sessions.category` vs.
  a join table) and wires it through the `SearchFilter` type the UI
  already speaks. Don't drop this on the floor.

## Beads

Wave 1D.0 created the epic + sub-beads (ASCII-only per LESSONS 2026-05-24):

- `mb-x7f9` — epic
- `mb-qhll` — 1D.0 (this wave)
- `mb-pxzk` — 1D.1 (migration + source-gate)
- `mb-j00j` — 1D.2 (KG screen + dashboard)
- `mb-0gt6` — 1D.3 (capture surface)
- `mb-6hm2` — 1D.4 (retrieval relocation)
- `mb-navi` — 1D.5 (settings expansion + Obsidian launch)
- `mb-q2p1` — 1D.6 (judges + seal)

Dependency chain: 1D.0 → 1D.1 → 1D.2 → 1D.3 → 1D.4 → 1D.5 → 1D.6, and each sub-bead blocks the epic `mb-x7f9`. `mb-oji5` (category-axis persistence) was **closed at Wave 1D.0** with a resolution pointing to `mb-pxzk` — that migration is the consumer.

## Supersession

- **Supersedes** ADR 0050 §D6 dictation-tail hook gating (the hook is
  now source-gated; the rest of 0050 stays in force).
- **Amends** ADR 0051 §"Out of scope" — the retrieval surface
  relocates; `mb-oji5` is consumed here.
- **Does not supersede** any other ADR.

## References

- `C:\Users\dboyd\Downloads\mockingbird-knowledge-graph-spec.md` —
  original product spec §1, §7.1, §10, §15.2–§15.5.
- `docs/knowledge-graph/PHASE-0-5-REPORT.md` §6 (v1 binding
  commitments) + §7 (wave-level build plan).
- `docs/adr/0048-knowledge-graph-phase-0-validation.md` §3 Q1/Q2/Q3
  (vault-subtree, positional routing, files-as-source-of-truth).
- `docs/adr/0049-knowledge-graph-phase-0-5-and-v1-architectural-pivot.md`
  §6 binding commitments + §"Sandbox isolation".
- `docs/adr/0050-kg-phase-1b-persistence-and-dictation-hook.md` —
  Phase 1B seal report; D6 superseded here.
- `docs/adr/0051-kg-phase-1c-retrieval-ux-and-activation.md` —
  Phase 1C seal report; §"Out of scope" amended here.
- `docs/phases/phase-1d.md` — full wave-by-wave brief (this wave
  authors it).
- `docs/knowledge-graph/phase-1c-latency-baseline.md` — p95=59s
  latency reference, carries forward to 1D.
- LESSONS PINNED **P5** (lateral epics seal via Accepted ADR + STATUS,
  not by a new tag).
- LESSONS PINNED **P8** (`session_id` discipline — serial Wave N → N+1
  dispatches get fresh invocations, no `session_id` carry).
- LESSONS PINNED **P10**/**P11**/**P12** (load-bearing KG findings).

## Phase numbering after this ADR

| Phase | Charter | Status |
|---|---|---|
| 1D | this ADR (0052) | **Accepted (2026-06-04 via Wave 1D.6 seal)** |
| 1E | future ADR 0053 | Obsidian-as-source-of-truth (vault projection + reverse-watcher; ADR 0048 Q3) — kickoff after 1D seal |
| 1F (was 1E) | future ADR 0054 | v1 beta tag; full-system smoke; release wiring |

---

## Phase 1D SEALED — Wave 1D.6 close-out (2026-06-04)

Flipping this ADR to **Accepted**. Phase 1D landed all six waves on
the original charter shape with no scope drift; the only material
deviation from the as-proposed text was ADR 0052 §D3's original
sketch of a "synthetic entry id" for text notes — Wave 1D.3
(`mb-0gt6`) instead reused the `sessions` + `transcripts` provenance
path with a new `capture_kind = KgNoteText` discriminator, exactly
matching the audio capture path's shape (documented in
`kg/ingest_text.rs`'s module docs).

**Commit chain (Phase 1D arc):**

| Wave | Bead | Commit(s) | Shipped |
|---|---|---|---|
| 1D.0 | `mb-qhll` | `b5c17bf` | Charter + clean-slate verify + phase doc + bead epic |
| 1D.1 | `mb-pxzk` | `f3b9a4a` | Migration 025 + `capture_kind` source-gate (3-gate cascade) |
| 1D.2 | `mb-j00j` | `37feed7` | KG screen scaffold + 5-band dashboard |
| 1D.3 | `mb-0gt6` | `846ecd5` | Capture surface (audio + text notes) |
| 1D.4 | `mb-6hm2` / `mb-f4gn` | `acb8f9a`, `0142ddb` | Chip/modal relocation Dictations → KG |
| 1D.5 | `mb-navi` | `0cd54ff`, `f9cb27c` | Settings expansion + Obsidian launch |
| 1D.6 | `mb-q2p1` | (this commit) | Judges + ADR seal + STATUS update |

**Acceptance-gate evidence (§"Acceptance gates" judge bundle):**

- **J1 — `kg-source-gate-invariant`:** authored as deterministic
  Rust per the charter author-picks option. Lives at
  `src-tauri/src/kg/source_gate_invariant.rs` + binary shim
  `src-tauri/src/bin/kg_source_gate_invariant.rs`. Judge doc at
  `docs/judges/phase-1d/kg-source-gate-invariant.md`. **GREEN:**
  6/6 corpus cells (3 `capture_kind` values × 2 toggle states)
  match expected `kg_filing_queue` row counts (0, 0, 0, 1, 0, 1).
  Per-cell fresh DB so a regression's blame is unambiguous.
  Drives both entry points (`dictation::try_enqueue_for_kg_filing`
  AND `kg::ingest_text::ingest_text_note`) — the
  sibling-of-`kg_graph_off_invariant` posture authorized by
  §"Acceptance gates" J1.

- **J2 — `kg-dictation-untouched`:** authored as a Phase 1D twin
  of Phase MC's `mc-dictation-untouched`, formalizing the runtime
  behavior (Phase MC's judge is a diff judge; this one is a
  behavior judge). Judge doc at
  `docs/judges/phase-1d/kg-dictation-untouched.md`. **GREEN:** the
  J1 probe's cells 1+2 both produce 0 queue rows; the existing
  `kg_graph_off_invariant` probe sweeps both capture kinds × all
  8 `InjectionOutcome` variants under toggle-off; only two call
  sites to `kg::store::enqueue_for_filing` in the entire codebase
  (the audio path's gated helper + the text-note path's
  ingest_text_note). The Phase MC do-not-touch sweep is empty
  over the narrowed file set (i.e. ignoring `dictation*` per the
  ADR 0052 §D1 supersession).

- **J3 — `kg-graph-off-ui-tightened`:** **no new code shipped at
  1D.6** — the consolidated Playwright invariant set was already
  authored across Waves 1D.2 (KG screen walk), 1D.4 (Dictations
  KG-free assertion after the chip/modal relocation), and 1D.5
  (vocabularies allowlist). Judge doc at
  `docs/judges/phase-1d/kg-graph-off-ui-tightened.md` records the
  consolidated invariant set as fully satisfied. **GREEN:**
  `npx playwright test kg-graph-off-invariant` reports `1 passed
  (12.6s)`.

**Cargo + UI gates (Wave 1D.6 full pass):**

- `powershell -File scripts\cargo-with-cuda.ps1 fmt --check` — clean.
- `powershell -File scripts\cargo-with-cuda.ps1 clippy --release -- -D warnings` — clean.
- `powershell -File scripts\cargo-with-cuda.ps1 test --release --no-run` — all test binaries link (including the new `kg_source_gate_invariant`). LESSONS P2 fallback (test exec blocked by `STATUS_ENTRYPOINT_NOT_FOUND`).
- `kg_source_gate_invariant` (NEW) — GREEN, 6/6 cells.
- `kg_graph_off_invariant` (regression) — GREEN, 8 outcomes × both `capture_kind`s + source-gate negative + positive control.
- `kg_parity` default — GREEN, 32/32.
- `kg_parity --persist` — GREEN, 32/32 + immutability triggers fire.
- `npx tsc --noEmit` — clean.
- `npm test` — 12 files / 140 tests passed.
- `npm run build` — clean.
- `npx playwright test kg-graph-off-invariant` — 1/1 passed.

**Bead closures:**

Wave epic `mb-x7f9` and all six wave beads close with this commit:
`mb-qhll` (1D.0) → `mb-pxzk` (1D.1) → `mb-j00j` (1D.2) → `mb-0gt6`
(1D.3) → `mb-6hm2` (1D.4) → `mb-navi` (1D.5) → `mb-q2p1` (1D.6).

**Standing carry-forwards** (not gating seal; carry into Phase 1E
planning):

- `mb-bbl2` — sonner toast retrofit.
- `mb-y6pq` — `--status-bad` token sweep.
- `mb-26aw` — `smoke.spec.ts` ×4 pre-1C Playwright failures.
- `mb-2wbk` — KG row → Dictations deep-link (P3; filed in 1D.4).
- `mb-0ui1` — vocab editor (P3; filed in 1D.5).

**What Phase 1D explicitly did NOT ship** (deferred to Phase 1E
per this ADR §"Out of scope"):

- Markdown projection / `vault/kg-graph/` subtree write-out.
- Reverse-watcher (filesystem → SQLite ingest).
- History archive (3-month rollover, lossless export).
- KG-Inbox courier (vault.kg-inbox → ingest pipeline).
- Pre-built filter boards (named-search persistence).
- iOS Shortcut documentation for the text-note IPC.
- v1 beta tag (Phase 1F territory).

**Phase 1E kickoff posture:** Obsidian-as-source-of-truth is the
next frontier. The text-note path's `kg/ingest_text.rs` module
docstring already flags the reverse-watcher seam ("this row was
authored from the vault, don't re-project") per ADR 0052 §Risks.
A future ADR 0053 will charter Phase 1E.

