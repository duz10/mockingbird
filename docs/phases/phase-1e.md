# Phase 1E — Obsidian as source of truth (vault projection + reverse-watcher + KG-Inbox courier)

**Phase entry anchor:** Phase 1D seal commit `3a934db` (ADR 0052 Accepted 2026-06-04).
**Phase exit:** ADR 0053 → Accepted + STATUS sealed-row + this doc's epic bead `mb-<epic>` closed. **No new `phase-*-complete` git tag** — lateral epic per LESSONS PINNED P5.
**Charter ADR:** [ADR 0053](../adr/0053-kg-phase-1e-obsidian-as-source-of-truth.md) — Proposed; flips Accepted at Wave 1E.9 seal.
**Planner / kickoff author:** Dustin (kickoff prompt) + code-puppy-b1aefd (this doc).
**Wave 1E.0 author (this doc):** code-puppy-b1aefd.
**Implementor (downstream waves):** see Wave table below — `migration-author` for 1E.1 / 1E.3; `code-puppy` for 1E.2 / 1E.4 / 1E.5 / 1E.6 / 1E.8 / 1E.9; `ui-author` or `code-puppy` for 1E.7 (mostly content, not interactive UI).
**Estimated iterations:** 10 (one per wave). 1E.0 + 1E.9 are documentation-heavy / judge-heavy; 1E.2 + 1E.5 are the technically hairy waves; 1E.1 / 1E.4 / 1E.7 / 1E.8 are mechanical-implementation-sized; 1E.3 + 1E.6 are medium.

> The binding spec lives in **ADR 0053** (charter), **ADR 0048 §Q1/Q2/Q3** (meta-decisions), **ADR 0049 §6** (v1 architecture commitments), and the original product spec **§7 + §9 + §14 + §15.4 + §15.5** at `C:\Users\dboyd\Downloads\mockingbird-knowledge-graph-spec.md`. This doc operationalizes them wave-by-wave.

---

## Objective

Close the central architecture gap left at the end of Phase 1D:
**KG entries persist to SQLite but not to Markdown.** Spec §14.1 says
"the markdown files *are* the database" — at end-of-1D, that promise
is half-built. Phase 1E flips the source-of-truth axis for the KG
subsystem (per ADR 0048 Q3): files canonical, DB shadow. The user
gets a working Obsidian-rendered board on day one; their edits in
Obsidian flow back into Mockingbird's dashboard via a reverse-watcher.

This is a v1 architectural novelty for Mockingbird — all prior
subsystems (dictation, meetings, activity, mobile-extension history)
treat the DB as canonical and project to vault as one-way history.
The KG inverts that for one reason: the spec's bidirectional promise
(§14.3) requires user edits in Obsidian to flow back. Rather than
re-implement an editable board inside Mockingbird (rejected at ADR
0052 §D2), the vault becomes the editor and we follow.

---

## Source-of-truth pointers

- **Original product spec** — `C:\Users\dboyd\Downloads\mockingbird-knowledge-graph-spec.md`
  - §7.1 — folder structure (`Knowledge Graph/{Inbox,Entries,History}/`).
  - §7.3 — per-entry fields (all inferred, never required of the
    user) → drives the YAML frontmatter shape.
  - §7.4 — Obsidian Tasks format emission.
  - §7.5 — one-memo-many-entries SPLIT (relevant to History/ JSON
    sidecar's `entries_produced` array).
  - §9 — raw transcripts kept untouched as the re-processing
    safety net → drives the History/ archive.
  - §14.1 — "the file IS the database" — the binding architectural
    promise this phase delivers.
  - §14.2 — pre-built board + dashboard notes (Wave 1E.7).
  - §14.3 — bidirectional edit flow (drives reverse-watcher).
  - §15.4 — vault config / launch-into-Obsidian (extended in 1E.3
    for per-entry navigation).
  - §15.5 — silent capture path (KG-Inbox courier sibling).
- **ADR 0046** — vault primitives (vault layout, courier pattern,
  `headless_ingest`, dedup ledger, iOS Shortcut docs). The reusable
  surface Phase 1E sits on top of.
- **ADR 0048 §3** — Q1 vault subtree (CONCRETIZED in D1), Q2
  positional routing (CONCRETIZED in D6 KG-Inbox courier), Q3
  files-as-source-of-truth (FULLY CASHED in D4 + D5).
- **ADR 0049 §6** — pipeline shape, two-field schema, qwen2.5:7b
  pin, opt-in graph guarantee, ~1 min latency budget.
- **ADR 0050** — Phase 1B `kg::worker` (extended in 1E.3 to write
  Markdown after DB insert).
- **ADR 0052** — Phase 1D source-gated dictation tail + capture
  surface (1E.6's KG-Inbox courier feeds the same source-gated path).
- **ADR 0053** — this charter.

---

## Wave 1E.0 — Charter + phase doc + bead epic (this wave)

**Goal:** Author the charter (ADR 0053), this phase doc, the bead
epic + 9 sub-beads, update STATUS.md's in-flight block. NO migration
writes, NO Rust code, NO UI code. Anything tempted into "while we're
at it" creates a follow-up bead instead.

### Deliverables

- `docs/adr/0053-kg-phase-1e-obsidian-as-source-of-truth.md`
  (Status: Proposed).
- `docs/phases/phase-1e.md` (this file).
- Bead epic + 9 sub-beads (see §"Bead epic" below).
- STATUS.md "🟢 Currently active" block updated to reflect Phase 1E
  in-flight at Wave 1E.0 complete / Wave 1E.1 ready.
- Commit referencing the epic + 1E.0 sub-bead.

### Gates (this wave)

This wave is documentation + bead mint only. Gates run anyway per
AGENTS.md end-of-iteration checklist:

- Cargo gate **skipped** (zero Rust changes).
- `npx tsc --noEmit` — must pass (no UI delta; baseline check).
- ADR 0053 cross-references valid (spec §s exist, prior ADRs exist).
- Phase doc internal links resolve.
- `bd ready` returns 1E.1 as next actionable.

### Seal

- All deliverables in tree.
- STATUS.md in-flight block reflects Phase 1E kickoff.
- `bd` epic + sub-beads created.
- Commit message references the epic id + this wave's sub-bead id.
- `bd update <1E.0 id> --status closed` after commit lands.

---

## Wave 1E.1 — Vault subtree bootstrap (idempotent)

**Agent:** `migration-author` or `code-puppy` (pick at dispatch).
**Blocks:** Waves 1E.3, 1E.4, 1E.6, 1E.7.
**Files affected (new):**

- `src-tauri/src/vault/kg_layout.rs` (NEW) — pure-Rust helpers:
  `kg_subtree_paths(vault_path: &Path) -> KgSubtreePaths` and
  `bootstrap_kg_subtree(vault_path: &Path) -> AppResult<BootstrapReport>`.

**Files affected (modified):**

- `src-tauri/src/vault/mod.rs` — re-export.
- `src-tauri/src/commands/kg.rs` — new IPC `kg_subtree_bootstrap()`
  invoked on toggle-on and at app boot when both `KgGraphEnabled` and
  `VaultPath` are set.
- `src-tauri/capabilities/default.json` — allowlist the new command.
- `ui/src/pages/SettingsKgTab.tsx` — wire the toggle-on path to fire
  the bootstrap; render inline error when `VaultPath` is unset.

### Contract (D1 of ADR 0053)

The bootstrap helper is fully idempotent:

| Cell | Pre-state | Post-state | Result |
|---|---|---|---|
| A | dir missing | dir created (`Knowledge Graph/{Inbox,Entries,History}/`) | `Created` |
| B | dir present, empty | unchanged | `AlreadyExists` |
| C | dir present, has user files | unchanged | `AlreadyExists` |
| D | `VaultPath` unset | error | `Err(VaultPathUnset)` |

`std::fs::create_dir_all` handles A + B + C as a no-op-or-create
primitive. The Settings tab's toggle-on handler refuses to fire when
`VaultPath` is unset (the toggle physically can't flip without a path
configured first — same UX shape as ADR 0046's Mobile Sync guard).

### Validation gates (Wave 1E.1)

- Cargo wrapper: fmt + clippy + check + `test --release --no-run`.
- Throwaway-crate test: `bootstrap_kg_subtree` against tempdir
  fixtures covering cells A–D.
- `npx tsc --noEmit` + `npm test` + `npm run build`.

---

## Wave 1E.2 — Deterministic Markdown serializer

**Agent:** `code-puppy`.
**Blocks:** Waves 1E.3, 1E.4, 1E.5, 1E.7, 1E.9.
**Files affected (new):**

- `src-tauri/src/kg/projection.rs` (NEW) — pure-Rust serializer +
  parser. Public API:

  ```rust
  pub fn serialize_entry(entry: &KgEntry) -> String;       // canonical bytes
  pub fn parse_entry(bytes: &str) -> AppResult<KgEntry>;   // round-trip
  pub fn filename_for(entry: &KgEntry) -> String;          // <date>-<slug>__<id8>.md
  ```

- `src-tauri/src/kg/tests/golden/*.md` (NEW) — ~10 fixture files
  covering all `capture_kind` × `category` × `type` permutations,
  with + without `status`, with + without `due_date`, with + without
  entities.

**Files affected (modified):**

- `src-tauri/src/kg/mod.rs` — declare module.
- `src-tauri/src/kg/schema.rs` — expose / extend the `KgEntry` shape
  if needed (additive only).

### Contract (D3 of ADR 0053)

YAML frontmatter shape, field order, quoting rules, list style, line
endings — all per ADR 0053 §D3. Golden-file test enforces byte-stable
round-trip.

`filename_for(entry)` returns `<YYYY-MM-DD>-<slug>__<id8>.md` per D2.
Slug derivation: lowercase, ASCII-only (transliterate via a small
table; reject non-letter/digit/hyphen by collapsing to `-`), max 40
chars, no leading/trailing hyphen.

### Refactor opportunity

The pipeline's normalize pass (`kg::passes::normalize`) and this
serializer share the structured entry shape. If the existing shape
needs extension (e.g. `source_session_uuid` field), it's an additive
extension — no existing field changes. 1E.2 brief picks whether the
new field lives on the existing `KgEntry` struct or a wrapping
`ProjectedEntry { entry: KgEntry, projection_metadata: ... }`.

### Validation gates (Wave 1E.2)

- Cargo wrapper full gate.
- Throwaway-crate test against the golden-file corpus: for each
  fixture, parse → serialize → assert byte-identical.
- Round-trip property test (proptest): random `KgEntry` →
  serialize → parse → assert structurally equal.

---

## Wave 1E.3 — Worker writes Markdown after DB insert (two-phase commit)

**Agent:** `migration-author` + `code-puppy`.
**Blocks:** Waves 1E.4, 1E.5, 1E.6, 1E.9.
**Risk:** Highest blast radius after 1E.5 — touches the existing
`kg::worker` which is sealed Phase 1B/1C code.
**Files affected (new):**

- `src-tauri/src/db/migrations/026_kg_phase_1e_entry_projection.sql`
  (NEW) — adds `kg_entries.file_path TEXT NULL`, `file_hash TEXT
  NULL`, `file_mtime INTEGER NULL` (or new `kg_entry_projections`
  table — 1E.3 brief picks shape).

**Files affected (modified):**

- `src-tauri/src/db/migrations.rs` — register migration 026.
- `src-tauri/src/kg/worker.rs` — extend the `process_queue_row`
  function to call `kg::projection::serialize_entry` after the
  existing DB inserts, write to vault, update entry row.
- `src-tauri/src/kg/store.rs` (or wherever the entries surface
  lives) — additive: `record_projection(entry_id, file_path, hash,
  mtime)`.

### Contract (D4 of ADR 0053)

Per-entry sequence:

1. **DB stage** (existing): worker drains queue row, runs pipeline,
   inserts `kg_entities` + `*_mentions` rows. Queue stays
   `state='processing'`.
2. **File write** (NEW): serialize → write to
   `<vault>/Knowledge Graph/.mockingbird-tmp/<entry-id>.md` →
   `fsync` → atomic rename to
   `<vault>/Knowledge Graph/Entries/<filename>.md`.
3. **DB seal** (NEW): UPDATE entry row with `file_path`, `file_hash`
   (sha256 of file bytes), `file_mtime`.
4. **Queue seal** (existing): UPDATE queue row `state='done'`.

Failure recovery per ADR 0053 §D4 failure-mode table:

- Nightly sweep (new tokio task in worker module): every 5 min, find
  `kg_filing_queue` rows with `state='processing'` AND `started_at >
  5 min ago` → reset to `state='pending'`. Same idempotency contract
  as Phase 1B (ADR 0050 §D5).
- Reverse-watcher orphan-recovery: on first scan at boot,
  cross-reference files in `Entries/` against `kg_entries.file_path`;
  any file not in DB → trigger normalize-pass ingest. Lives in 1E.5,
  not here.

### Per-entry "Open in Obsidian" IPC

Additive to `kg::launcher`: new `launch_obsidian_entry(vault_path,
entry_id_or_filename)` builds `obsidian://open?vault=X&file=Y` and
spawns. Used by the dashboard's per-row affordance (Wave 1E.7 if
time, else deferred to a follow-up bead — does NOT block 1E.9).

### Validation gates (Wave 1E.3)

- Cargo wrapper full gate.
- Migration runner test: fresh DB → migrations 001..026 →
  `schema_version=26` + new columns present.
- Throwaway-crate test: end-to-end one-entry projection — seed DB,
  run worker tick, assert file exists at expected path + DB row has
  `file_path`/`file_hash`/`file_mtime` populated.
- Standing parity gate: `kg_parity --persist` 32/32 (must not
  regress — projection is additive to the existing pipeline).

---

## Wave 1E.4 — History archive

**Agent:** `code-puppy`.
**Blocks:** Wave 1E.9.
**Files affected (new):**

- `src-tauri/src/kg/history.rs` (NEW) — JSON sidecar writer +
  audio-move helper.

**Files affected (modified):**

- `src-tauri/src/kg/worker.rs` — after writing the last entry
  derived from a session (queue row's last sibling-of-session),
  emit the History JSON sidecar + move the audio file out of
  Inbox/ if present.
- `src-tauri/src/kg/projection.rs` — additive: helper to compute
  `entries_produced` array for a given `session_uuid`.

### Contract (D7 of ADR 0053)

Per session (NOT per entry):

```
<vault>/Knowledge Graph/History/<YYYY-MM>/<session-uuid>.json
```

Shape per ADR 0053 §D7. Audio file (when present) moves to
`<vault>/Knowledge Graph/History/<YYYY-MM>/<session-uuid>.m4a` at
the same time. Text notes write the JSON sidecar with
`audio_relpath: null`.

Idempotency: check `path.exists()` before writing; existing means
"already processed; skip." Re-processing flows (post-v1) own
overwrite semantics.

**Trigger:** the last queue row for a given `session_uuid` to
transition `processing` → `done` is the one that fires the History
write. This requires the worker to know which queue rows share a
session — a cheap query against `kg_filing_queue` keyed on
`source_session_uuid`. Text notes are single-row → fires on the
single row's seal.

### Validation gates (Wave 1E.4)

- Cargo wrapper full gate.
- Throwaway-crate test: seed a multi-entry session (3 entries from
  one audio capture) → process all three → assert ONE JSON sidecar
  + ONE audio move.
- Throwaway-crate test: seed a text note → process → assert JSON
  sidecar with `audio_relpath: null`.

---

## Wave 1E.5 — Reverse-watcher (file → DB FTS reconcile)

**Agent:** `code-puppy`.
**Blocks:** Wave 1E.9.
**Risk:** Highest blast radius in the phase. Loop-prevention is the
load-bearing invariant; J1 mechanically enforces.
**Files affected (new):**

- `src-tauri/src/kg/reverse_watcher.rs` (NEW) —
  `notify-debouncer-full` subscriber, debounce loop, hash-match
  short-circuit, normalize-pass dispatch.
- `src-tauri/src/kg/reverse_watcher_runtime.rs` (NEW, if needed) —
  if the runtime wrapper grows beyond the single watcher file. Wave
  brief picks split or single-file.

**Files affected (modified):**

- `src-tauri/src/kg/mod.rs` — declare modules.
- `src-tauri/src/lib.rs` (or wherever app boot lives) — spawn the
  reverse-watcher when `KgGraphEnabled = true` AND `VaultPath` set.
- `src-tauri/src/kg/worker.rs` — minor: write `file_hash` BEFORE
  rename so the watcher's first event for that file is a no-op
  (see D5 ordering guarantee).

### Contract (D5 of ADR 0053)

Per file event:

1. Debounce 2s (matches ADR 0046 inbox-watcher).
2. Read file bytes; compute SHA-256.
3. Look up entry by frontmatter `id` (fallback: filename `__<id8>`
   suffix; second fallback: file_path column match).
4. If `recorded_file_hash == computed_hash` → no-op.
5. Else → normalize-pass on the parsed entry → UPDATE DB FTS index
   + `file_hash` + `file_mtime`.

**Loop-prevention ordering** (load-bearing): the worker MUST record
the projection's hash in DB BEFORE the OS rename that lands the file
in `Entries/`. The watcher's event for that file then arrives AFTER
the DB hash is already in place → step 4 fires no-op → no loop.

Implementation: in `worker::process_queue_row`, the new sequence is:

```rust
let bytes = serialize_entry(&entry);
let hash = sha256(&bytes);
let mtime = SystemTime::now();              // optimistic; corrected post-rename
store::record_projection(entry_id, &file_path, &hash, mtime)?;  // DB seal FIRST
write_atomic(&tmp_path, &final_path, bytes)?;                     // rename
// optional: fix-up mtime to the actual filesystem mtime in a follow-up UPDATE
```

The post-rename mtime fix-up is cosmetic (the watcher doesn't key
on mtime anyway — D5).

### Obsidian Tasks checkbox round-trip

When the user toggles `[ ]` → `[x]` in the body, the watcher's
normalize pass detects the checkbox state changed and:

1. UPDATE DB entry `status` field.
2. Re-serialize the entry (YAML `status` now matches checkbox).
3. Pre-record the new hash → write back.

The write-back's own event fires no-op (hash matches). Net: one
external edit → one watcher fire → one Mockingbird write-back → no
further events. J1 verifies.

### Validation gates (Wave 1E.5)

- Cargo wrapper full gate.
- J1 dry-run (the judge that 1E.9 hardens): write-then-observe-zero-
  fires; external-edit-then-observe-one-fire.
- Throwaway-crate test for the checkbox round-trip: seed an
  unchecked task entry → simulate external check → assert DB
  `status` is `done` + YAML `status: done` on next read.

---

## Wave 1E.6 — KG-Inbox courier (sibling to ADR 0046 inbox)

**Agent:** `code-puppy`.
**Blocks:** Wave 1E.9.
**Files affected (new):**

- `src-tauri/src/vault/kg_inbox_runtime.rs` (NEW) — sibling of
  `inbox/runtime.rs`. Same shape, watches
  `<vault>/Knowledge Graph/Inbox/`, passes `IngestProvenance` with
  the new `IngestSource::MobileInboxKgNote` variant.
- `src-tauri/src/vault/kg_inbox_pickup.rs` (NEW, if extracted) —
  decode → `headless_ingest` → mark `capture_kind='kg-note'` →
  enqueue (existing ADR 0050 dictation-tail hook fires via source-
  gate from ADR 0052 §D1).

**Files affected (modified):**

- `src-tauri/src/dictation/ingest.rs` — additive: new
  `IngestSource::MobileInboxKgNote { courier_path }` enum variant
  + the path that maps it to `capture_kind='kg-note'` on the
  resulting session row.
- `src-tauri/src/lib.rs` — spawn the KG inbox runtime alongside the
  existing inbox runtime when KG is enabled.
- `src-tauri/src/vault/mod.rs` — declare modules + re-export.

### Contract (D6 of ADR 0053)

Positional routing per ADR 0048 Q2. The KG courier ONLY watches
`<vault>/Knowledge Graph/Inbox/`. A file dropped in the standard
`<vault>/inbox/` is processed by the existing ADR 0046 courier as a
plain dictation (unchanged). A file dropped in
`<vault>/Knowledge Graph/Inbox/` is processed by the new courier
and lands with `capture_kind='kg-note'`, which then triggers the
ADR 0052 §D1 source-gated dictation tail → KG filing.

**Reuses (zero duplication):**

- `notify-debouncer-full` library.
- Combined-detector pattern (size + mtime stable for 2s +
  exclusive-open + min 1s age).
- Conflict-file quarantine regex.
- `vault_inbox_ledger` table (shared dedup ledger keyed on content
  SHA-256 — re-delivery to EITHER inbox is a no-op).
- `dictation::ingest::headless_ingest` (ADR 0046 §3).
- Startup catch-up scan + periodic reconciliation pattern.

**Diverges (deliberately):**

- Watched folder.
- `IngestProvenance.source` value (new enum variant).
- The resulting `capture_kind` is `'kg-note'` (vs `'dictation'`).
- Post-ingest disposition: success moves the audio file into
  `History/<YYYY-MM>/<session-uuid>.m4a` (Wave 1E.4 handles this;
  the courier just calls `headless_ingest` and lets the worker do
  the rest), while ADR 0046's standard inbox discards to a local
  temp.

### Validation gates (Wave 1E.6)

- Cargo wrapper full gate.
- Throwaway-crate test: drop a fixture `.m4a` into a tempdir
  KG-Inbox → run one watcher tick → assert one row in
  `kg_filing_queue` with `capture_kind='kg-note'`.
- Cross-check: standing `kg_source_gate_invariant` still 6/6
  cells (the new courier path is the 7th entry-point, must not
  break the invariant).

---

## Wave 1E.7 — Pre-built `.md` seeds on first activation

**Agent:** `ui-author` (template content) or `code-puppy` (projection
glue).
**Blocks:** Wave 1E.9.
**Files affected (new):**

- `src-tauri/src/kg/assets/seeds/dashboard.md` (NEW) — Dataview /
  Bases query block + per-category counts + recent-entries table.
- `src-tauri/src/kg/assets/seeds/kanban-tasks.md` (NEW) — Kanban
  plugin format keyed on `type: task` + `status`.
- `src-tauri/src/kg/assets/seeds/readme.md` (NEW) — Inbox/Entries/
  History walkthrough + plugin-install pointer.
- `src-tauri/src/kg/seeds.rs` (NEW) — pure-Rust helper:
  `project_seeds_if_first_activation(vault_path: &Path) ->
  AppResult<SeedReport>` (`embed_files!` to bake the seeds into the
  binary; copy out on first call).

**Files affected (modified):**

- `src-tauri/src/kg/mod.rs` — declare module + re-export.
- `src-tauri/src/commands/kg.rs` — additive IPC if the toggle-on
  handler doesn't already cover this (likely it does — bootstrap +
  seed fire in the same handler).

### Contract (D8 of ADR 0053)

First-activation detection: `KgGraphEnabled` flips false → true
AND `<vault>/Knowledge Graph/Dashboard.md` does NOT exist → write
seeds. On subsequent toggle-off → toggle-on cycles, seeds already
exist → no-op.

**Seeds are user content the moment they're written.** Mockingbird
never overwrites them — not on re-activation, not on upgrade. If a
user deletes a seed, it stays deleted. The reverse-watcher reconciles
edits to seeds like any other note (but seeds have no frontmatter
`id`, so the watcher's normalize pass sees no entry-row to reconcile
against → treats them as opaque user content and updates only their
`file_hash`/`file_mtime` if those are tracked; otherwise full no-op).

Open question for the 1E.7 brief: do seeds get tracked in `kg_entries`
at all? Default: NO. They aren't entries; they're scaffolding. The
reverse-watcher ignores them by virtue of missing frontmatter `id`.

### Validation gates (Wave 1E.7)

- Cargo wrapper full gate.
- Throwaway-crate test: cell A (no seeds) → activate → 3 seed files
  present. Cell B (seeds exist) → activate → no overwrite. Cell C
  (user deleted Dashboard.md) → re-activate → DELETED seed is NOT
  re-created.
- Manual smoke (Dustin): open the seeded vault in Obsidian with
  Dataview + Kanban installed; assert boards render with the day-
  one entries.

---

## Wave 1E.8 — iOS Shortcut docs for KG-Inbox

**Agent:** `code-puppy`.
**Blocks:** Wave 1E.9 (only soft-blocks; 1E.9 can theoretically run
without 1E.8 if 1E.8 slips, since it's docs-only).
**Files affected (new):**

- `docs/mobile/ios-shortcut-kg.md` (NEW) — mirror of `docs/mobile/
  ios-shortcut.md` (ADR 0046 §8). Same two-action chain (Record
  Audio → Save File); Save destination is
  `<vault>/Knowledge Graph/Inbox/`.

**Files affected (modified):**

- `docs/mobile/ios-shortcut.md` — additive: one-line pointer at the
  top "for the Knowledge Graph variant, see ios-shortcut-kg.md."
- `ui/src/pages/SettingsKgTab.tsx` (optional) — additive link to
  the new docs from the KG settings panel.

### Contract (D10 of ADR 0053)

No importable `.shortcut` file shipped (path-binding issue per
ADR 0046 §8). Docs spell out the chain step-by-step with screenshot
guidance and the naming convention recommendation ("Mockingbird Quick
Capture (Knowledge Graph)" to distinguish from the dictation
Shortcut).

### Validation gates (Wave 1E.8)

- Pure docs wave. No cargo gate. `npx tsc --noEmit` clean (only if
  the optional UI link was added).
- Cross-link integrity: `ios-shortcut.md` ↔ `ios-shortcut-kg.md`
  pointers resolve.

---

## Wave 1E.9 — Phase 1E judges + seal

**Agent:** `code-puppy`.
**Blocks:** epic seal.
**Files affected (new):**

- `src-tauri/src/bin/kg_reverse_watcher_loop_prevention.rs` (NEW) —
  J1 binary. Deterministic; runs in CI.
- `src-tauri/src/bin/kg_file_wins_on_conflict.rs` (NEW) — J2 binary.
- `src-tauri/src/bin/kg_subtree_bootstrap_idempotent.rs` (NEW) — J3
  binary (or inline test in `vault::kg_layout`).
- `src-tauri/src/bin/kg_serializer_golden_roundtrip.rs` (NEW) — J4
  binary (or inline test in `kg::projection`).
- `docs/judges/phase-1e/J1-reverse-watcher-loop-prevention.md` (NEW).
- `docs/judges/phase-1e/J2-file-wins-on-conflict.md` (NEW).
- `docs/judges/phase-1e/J3-subtree-bootstrap-idempotent.md` (NEW).
- `docs/judges/phase-1e/J4-serializer-golden-roundtrip.md` (NEW).

**Files affected (modified):**

- `docs/adr/0053-kg-phase-1e-obsidian-as-source-of-truth.md` —
  Status → Accepted; add "Phase 1E SEALED" close-out section
  mirroring ADR 0052's close-out.
- `STATUS.md` — append Phase 1E to the Sealed table.
- `docs/PRODUCT-STATE.md` — update the KG subsystem entry to
  reflect "file-canonical; DB shadow FTS."

### Contract

Four judges per ADR 0053 §"Acceptance gates":

| Judge | Asserts | Type |
|---|---|---|
| J1 — reverse-watcher loop prevention | own writes fire zero events; external edits fire exactly one | deterministic; tokio integration |
| J2 — file wins on conflict | external file edit → DB reflects file's new content | deterministic; integration |
| J3 — subtree bootstrap idempotent | 4 cells (missing / empty / populated / vault-unset) | deterministic; pure-Rust |
| J4 — serializer golden roundtrip | parse → serialize → byte-identical over ~10 fixtures | deterministic; pure-Rust |

Standing regression gates (must stay green):

- `kg_parity` 32/32 default + `--persist`.
- `kg_graph_off_invariant` 8/8 + controls.
- `kg_source_gate_invariant` 6/6 cells (new KG-Inbox courier is the
  7th entry-point; must not break the invariant).
- `mc-dictation-untouched` still green vs. `phase-mc-start` anchor.

### Seal

- ADR 0053 → Accepted with close-out section mirroring ADR 0052.
- All four judges green.
- Cargo + UI gates green.
- Manual smoke matrix green (Dustin: end-to-end capture from iOS
  Shortcut → watch Obsidian render → edit a task checkbox → watch
  Mockingbird's dashboard reflect the change).
- STATUS.md updated (Sealed table gains a Phase 1E row).
- `bd` epic + all sub-beads closed.
- Commit tagged with a clear `KG Phase 1E SEALED:` message. **No
  new `phase-*-complete` tag.**

---

## Bead epic

Created at Wave 1E.0. All ASCII-only per LESSONS 2026-05-24.

Bead ids are minted in this wave; placeholders below are filled in
by the `bd create` shell calls at the end of this wave.

| Bead | Wave | Priority | Status at create |
|---|---|---|---|
| `mb-<epic>` | epic | P1 | open |
| `mb-<1E.0>` | 1E.0 - charter + phase doc + bead epic | P1 | open → closed at this wave's commit |
| `mb-<1E.1>` | 1E.1 - vault subtree bootstrap | P1 | open |
| `mb-<1E.2>` | 1E.2 - deterministic Markdown serializer | P1 | open |
| `mb-<1E.3>` | 1E.3 - worker writes Markdown (two-phase commit) | P1 | open |
| `mb-<1E.4>` | 1E.4 - history archive | P1 | open |
| `mb-<1E.5>` | 1E.5 - reverse-watcher | P1 | open |
| `mb-<1E.6>` | 1E.6 - KG-Inbox courier | P1 | open |
| `mb-<1E.7>` | 1E.7 - pre-built seeds | P2 | open |
| `mb-<1E.8>` | 1E.8 - iOS Shortcut docs | P2 | open |
| `mb-<1E.9>` | 1E.9 - judges + seal | P1 | open |

Dependency chain (`bd link <a> <b>` means `a` depends on `b`):

- 1E.0 ← 1E.1 (1E.1 depends on 1E.0)
- 1E.1 ← 1E.2 (1E.2 depends on 1E.1 for subtree presence at test time)
- 1E.2 ← 1E.3 (1E.3 depends on serializer)
- 1E.3 ← 1E.4 (history archive uses worker's per-session signal)
- 1E.3 ← 1E.5 (reverse-watcher pre-supposes projection writes)
- 1E.3 ← 1E.6 (KG-Inbox courier feeds entries that the projection
  picks up)
- 1E.1 ← 1E.7 (seeds depend on subtree bootstrap)
- (1E.8 has no code deps; docs-only — order at dispatch convenience.)
- 1E.5 ← 1E.9, 1E.6 ← 1E.9, 1E.4 ← 1E.9, 1E.7 ← 1E.9 (judges/seal
  depend on all implementation waves).

All sub-beads block the epic `mb-<epic>`.

---

## Out of scope (explicit)

### Deferred to Phase 1F (was 1E in pre-1D-rescope numbering)

- The `v1-beta` git tag.
- Full-system smoke matrix on a fresh Win11 box.
- Release wiring (installer / updater verification).
- Marketing-shaped privacy statement / docs polish.

### Deferred post-v1

- Backfill of pre-Phase-1E historical dictations into the KG (per-row
  promote-to-graph affordance on the Dictations page).
- Vocabularies editor in Settings (read-only display shipped in 1D.5).
- Synthesize operation per the Clark article (cross-entry agentic
  summarization).
- Routing-step optimization (per-pass Nemotron-style model selection).
- Archive vs. Ingest mode toggle per spec §10.
- Interactive ingest mode (per-entry user confirmation per spec
  §15.5 / §11).
- History/ retention rollover (current v1: kept untouched forever).
- macOS support (Phase 9).
- `.shortcut` importable file for iOS (path-binding issue; manual
  recreation docs stay the v1 answer per ADR 0046 §8).

---

## Risks

| Risk | Wave | Mitigation |
|---|---|---|
| Reverse-watcher feedback loop. | 1E.5 | Hash-based loop-prevention (ADR 0053 §D5). Pre-record hash BEFORE rename. J1 mechanically enforces. |
| Two-phase commit partial failure. | 1E.3 | Nightly sweep + reverse-watcher orphan recovery. ADR 0053 §D4 failure-mode table. |
| Obsidian Sync byte-identical re-touch trips false user-edit detection. | 1E.5 | Hash match (not mtime). Same call ADR 0046 made for the dedup ledger. |
| Filename slug collision. | 1E.2 | `__<id8>` suffix is collision-resistant (~1-in-trillion at single-day scale). |
| `Knowledge Graph/` (with space) confuses some tooling. | 1E.1 | All paths through `std::path::Path`; URI scheme percent-encodes via existing `launcher.rs` helper. |
| Obsidian Kanban / Dataview not installed → seeds degrade. | 1E.7 | Seeds readable without plugins. README points to install. |
| Pre-existing manual `Knowledge Graph/` subtree (user set it up before activation). | 1E.1 | Bootstrap is fully idempotent. Pre-existing `.md` files picked up on reverse-watcher's first scan. |
| Latency budget regression — adding file writes to the worker. | 1E.3 | File writes are cheap (~ms); the LLM passes dominate the existing p95=59s budget. Re-confirm at 1E.3 smoke. |
| `mb-mxal` (relocate DB to LOCALAPPDATA) lands mid-epic. | (any) | `file_path` stores absolute vault paths; DB location is independent. |
| Stale-prompt risk on 1E.N dispatches. | 1E.1–1E.9 | LESSONS P8: dispatch prompts one-line pointers to this doc; no embedded specs. |

---

## How to resume Phase 1E mid-execution

1. Read STATUS.md "🟢 Currently active" — it'll point at the
   in-flight wave.
2. Read this file (you're here).
3. Read ADR 0053 (charter).
4. For the in-flight wave: `bd show mb-<wave-id>` for the sub-bead
   detail; `bd ready -t task` for next actionable.
5. `git log --oneline --grep "1E\." -20` for the wave commit chain.
6. THEN start work.

The session-start ritual in AGENTS.md still applies — this list is
the Phase-1E-specific layer on top.

---

## Cargo gate (binding per LESSONS P2)

Phase 1E uses the existing accepted fallback gate. No new gate.

- **Pure-Rust modules** → throwaway-crate recipe (LESSONS 2026-05-17).
  Eligible 1E modules: `kg::projection` (no whisper-rs / ort deps),
  `kg::history`, `kg::seeds`, `vault::kg_layout`, `kg::reverse_watcher`
  if extractable (likely; `notify-debouncer-full` is light), and the
  4 judge binaries.
- **Wired modules** → cargo check + clippy `--release -- -D warnings`
  + fmt `--check` + test `--release --no-run` via the Windows
  wrapper. Plus per-wave human-in-loop smoke per the validation gates
  above.

---

## UI gate (binding)

- `npx tsc --noEmit` clean.
- `npm test` (vitest) clean.
- `npm run build` clean.
- `npm run lint` currently broken (`mb-yxh`); ignore until resolved.
- Per-wave Playwright sweep on whatever surfaces moved that wave
  (1E.7 adds the per-entry "Open in Obsidian" affordance if the
  stretch goal hits; 1E.1 may have a Settings smoke for the toggle-on
  bootstrap fire).

---

## Latency budget

Carry-forward from Phase 1C: p95=59s per filing at qwen2.5:7b. Phase
1E ADDS one file write + one DB UPDATE per entry. File writes are
~ms; the budget is dominated by the LLM passes, not the I/O. No
regression expected; re-confirm at 1E.3 smoke.

If the reverse-watcher's normalize pass introduces a perceptible
post-edit lag (user edits in Obsidian, dashboard refreshes after a
few hundred ms), the existing 2s debounce window is the floor. Below
the debounce, async dashboard refresh on `kg_dashboard_snapshot`
re-fetch handles it.

---

## File estimates

| Wave | New files | Modified files | Estimated total LoC |
|---|---:|---:|---:|
| 1E.0 (docs) | 2 (ADR + this doc) + 0 code | 1 (STATUS.md) | ~1100 lines docs |
| 1E.1 | 1 (`vault/kg_layout.rs`) | 4 | ~250 |
| 1E.2 | 1 + ~10 fixtures | 2 | ~500 (incl. fixtures) |
| 1E.3 | 1 migration + maybe 1 `store` helper | 2-3 | ~300 |
| 1E.4 | 1 (`kg/history.rs`) | 2 | ~250 |
| 1E.5 | 1-2 (watcher + maybe runtime) | 3 | ~600 |
| 1E.6 | 1-2 (kg_inbox_runtime + pickup) | 3 | ~500 |
| 1E.7 | 4 (3 seeds + 1 projection helper) | 2 | ~300 (mostly markdown) |
| 1E.8 | 1 (docs) | 1 (cross-link) | ~250 docs |
| 1E.9 | 4 binaries + 4 judge docs | ADR + STATUS + PRODUCT-STATE | ~600 |
| **Total** | **~20 new** | **~20 modified** | **~3700 LoC + ~1400 docs** |

Per-file limit (≤600 LoC) tracked at each wave's seal. Tight
candidates: `kg::reverse_watcher` (split into watcher + runtime if it
crosses), `kg::worker` (already 31.8 KB at end-of-1D; the 1E.3
extension may push it; consider extracting projection-writing into
`kg::worker::projection_step` as a sibling file).
