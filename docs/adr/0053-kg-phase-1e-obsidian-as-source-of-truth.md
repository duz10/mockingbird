# ADR 0053 — KG Phase 1E: Obsidian as source of truth (vault projection + reverse-watcher + KG-Inbox courier)

- **Status:** **Proposed (2026-06-04)** — flips to **Accepted** at Wave 1E.9 seal.
- **Date:** 2026-06-04
- **Deciders:** Dustin, code-puppy (`code-puppy-b1aefd`)
- **Charter for:** ADR-lateral epic; no `phase-*-complete` tag (per LESSONS PINNED P5).
- **Charter bead:** `mb-<epic>` (minted at this wave) / `mb-<1E.0>` (this wave).
- **Concretizes:** [ADR 0048](0048-knowledge-graph-phase-0-validation.md) §Q3 ("files are the source of truth"). Q1 (vault subtree) + Q2 (positional routing) carry through unchanged.
- **Inherits:** [ADR 0049](0049-knowledge-graph-phase-0-5-and-v1-architectural-pivot.md) §6 binding architecture (5-pass pipeline, two-field schema, qwen2.5:7b pin, opt-in graph guarantee).
- **Builds on:** [ADR 0052](0052-knowledge-graph-phase-1d-charter.md) (KG screen + source-gated filing); [ADR 0046](0046-mobile-extension-via-vault.md) (vault primitives + dictation-inbox courier).
- **Source-of-truth pointers:** original product spec `C:\Users\dboyd\Downloads\mockingbird-knowledge-graph-spec.md` §7 (folder structure), §7.4 (Obsidian Tasks format), §9 (raw transcripts safety net), §14 (file IS the database), §15.4 (vault config), §15.5 (silent capture path).

---

## Status

**Proposed (2026-06-04).** Phase 1E's nine implementation waves are
chartered here. Wave 1E.0 (this wave) lands the charter + phase doc +
bead epic. Waves 1E.1–1E.8 implement; Wave 1E.9 seals the epic and
flips this ADR to Accepted. No new `phase-*-complete` tag (lateral
epic; LESSONS PINNED P5). Phase 1E's user-visible surface: every KG
entry persists as a human-readable `.md` file under
`<vault>/Knowledge Graph/Entries/`; user edits in Obsidian flow back
into Mockingbird via a reverse-watcher; mobile captures land via a
sibling iOS Shortcut into `<vault>/Knowledge Graph/Inbox/`; first KG
activation seeds a Kanban board + dashboard notes so the user gets a
working board on day one.

## Context

Phase 1D (ADR 0052) sealed 2026-06-04 with a first-class KG sidebar
destination, a source-gated dictation-tail hook, and audio/text
capture entry points. KG entries persist to SQLite tables
(`kg_entities`, `kg_entity_mentions`, etc. per ADR 0050). **They do
not yet exist on disk as Markdown files** — so the central design
promise of the original spec (§14.1: "the markdown files *are* the
database") is, at the end of Phase 1D, half-built. The DB is the
source of truth; Obsidian sees nothing.

Phase 1E closes that gap. Per ADR 0048 Q3 and spec §14, the steady-
state architecture is the inverse of the existing
dictation/meeting/activity subsystems: **files are canonical, DB is
a shadow FTS index for the KG dashboard's queries.** This is a v1
architectural novelty — Mockingbird's other subsystems treat DB as
truth and project to vault as one-way history (ADR 0046 §1 zone
contracts: "DB is truth; Markdown is projection; ... no code path
in Mockingbird ever reads `<vault>/history/*.md` as input"). The KG
explicitly inverts that for one reason: the spec's bidirectional
promise (§14.3) requires that user edits in Obsidian — checking a
task box, dragging a Kanban card, renaming an entry, changing tags
— flow back through. Either we re-implement an editable board inside
Mockingbird (rejected at ADR 0052 §D2: Option A "observe and
control, do not edit"), or the vault is the editor and we follow.

The cost is two new subsystems (a markdown serializer + a reverse-
watcher) and a careful conflict story; the payoff is that the user
gets a Monday-style board experience without us building a board
engine.

### What Phase 1E does NOT do

- Backfill historical dictations into Markdown (post-v1 — ADR 0052
  out-of-scope carry-forward).
- Implement the Vocabularies editor (post-v1).
- Cut the v1 beta tag (Phase 1F).
- Re-litigate ADR 0048 Q1/Q2/Q3 (those are the meta-decisions this
  ADR concretizes).

---

## Decision

### D1 — Vault subtree shape (concretizes ADR 0048 Q1)

The KG subtree lives at `<vault>/Knowledge Graph/{Inbox,Entries,History}/`
exactly as spec §7.1 specifies. The folder name carries a literal
space (matching the spec's spelling); this is intentional and
user-visible in Obsidian's file explorer.

**Subtree creation trigger:** `KgGraphEnabled` toggle flips on AND the
`VaultPath` setting is non-empty. The check fires at toggle-on time
and at app boot (if both conditions hold and the subtree is missing).
Idempotent: `std::fs::create_dir_all` no-ops on existing dirs. No
migration step is needed if a user has manually created the subtree
ahead of time — the bootstrap discovers it and proceeds.

**Failure mode:** if `VaultPath` is unset when the toggle flips on,
Settings UI shows an inline error ("Set the vault path before
activating the knowledge graph") and the toggle physically refuses
to flip — same UX shape as ADR 0046's "Mobile Sync needs a vault
path" guard.

### D2 — Markdown filename format

Filename: `<YYYY-MM-DD>-<short-title-slug>__<id8>.md`.

- `<YYYY-MM-DD>` — capture date (UTC), drives chronological sort in
  Obsidian's file explorer. Stable: the prefix never updates after
  initial write, even if the user renames the file.
- `<short-title-slug>` — title slug, ≤40 chars, lowercase, hyphenated.
  Cosmetic only; rename-safe (see below).
- `__<id8>` — 8-character prefix of the entry's ULID, durable anchor
  for reverse-watcher identity recovery. Two-underscore separator
  matches ADR 0046's history-projection precedent
  (`2026-05-27-1408__a4f7c2d3.md`).

**Why this shape:**

- **Pure `<id>.md`** rejected: unreadable in Obsidian's file
  explorer; loses the at-a-glance scan affordance that's a core
  vault UX.
- **Pure `<title-slug>.md`** rejected: title collisions ("Buy milk"
  twice on the same day is plausible); rename-on-edit churn.
- **Pure `<YYYY-MM-DD>-<slug>.md`** rejected: same collision risk,
  no durable identity anchor for reverse-watcher loop-prevention.
- **`__<id8>` suffix is load-bearing.** Obsidian auto-updates
  `[[wikilink]]` references when the user renames a file (with the
  default "Update links on rename" setting). The reverse-watcher
  also uses the suffix as a fallback identity probe when the YAML
  frontmatter `id` field has been stripped or corrupted by a careless
  edit.

Rename behavior: user rename in Obsidian → reverse-watcher updates
`file_path` in DB. We do NOT auto-rename our own files when an
entry's title changes via the dashboard (we don't have a dashboard-
side title edit in v1 anyway, per ADR 0052 §15.1 read-only principle).

### D3 — YAML frontmatter shape

Required fields (every entry):

```yaml
---
id: 01HMVWAB7C8X9Y0Z1234567890   # ULID; durable identity
schema_version: 1                # bump on shape changes
capture_kind: kg-note             # dictation | kg-note | kg-note-text
captured_at: 2026-06-15T14:32:01Z
title: "Switch suppliers for brake pads"
category: professional             # personal | professional | objective
type: task                         # task | research | idea | note | reference
tags:                              # Layer 3 open-vocab, normalized
  - brake-pads
  - suppliers
entities:                          # ADR 0049 typed-reference taxonomy
  - {name: "Home Depot", type: organization}
  - {name: "brake pads", type: object}
---
```

Conditionally-emitted fields:

- `status: todo | doing | done` — only when `type == task`.
- `due_date: YYYY-MM-DD` — only when the capture explicitly mentions
  timing. Never invented (Principle: ADR 0048 §8.4 hard-gate).
- `source_session_uuid: <uuid>` — pointer to the originating
  `sessions` row when `capture_kind != kg-note-text` (text notes have
  no session). Lets the reverse-watcher and History/ projection
  cross-link.

**Canonical-form rules** (binding on the serializer — golden-file
tested at Wave 1E.2):

- UTF-8 throughout; LF line endings (NEVER CRLF — Obsidian-on-iOS
  has historically been wobbly on CRLF in YAML).
- Field order as listed above (id, schema_version, capture_kind,
  captured_at, title, category, type, [status], [due_date], tags,
  entities, [source_session_uuid]).
- String values double-quoted iff they contain a YAML-special char
  (`:`, `#`, `-` at line start, etc.); otherwise unquoted.
- `tags` and `entities` are always block-style lists (never inline
  `[a, b]`), even when single-element, for stable diffs.
- Empty `tags: []` and empty `entities: []` ARE emitted (not
  omitted) — the round-tripper distinguishes "no tags inferred" from
  "tags field never existed."

**Versioning strategy:** `schema_version: 1` is the Phase 1E baseline.
Shape changes bump the integer; reverse-watcher reads any version
≤ current and migrates in memory on read. Round-trip writes always
emit the current version. The writer + reverse-watcher MUST agree on
the canonical form per version (the golden-file test enforces this).

### D4 — Two-phase commit semantics

**Steady-state guarantee:** file is canonical (per ADR 0048 Q3). The
DB row is a shadow FTS index, refreshed by both the worker (on
filing) and the reverse-watcher (on user edit).

**Capture-time sequence (one-shot, transactional in spirit):**

1. **DB stage** (existing Phase 1B/1C/1D flow): worker drains
   `kg_filing_queue` row → runs pipeline → INSERTs `kg_entities` +
   `kg_entity_mentions` + `kg_tag_mentions` rows. `kg_filing_queue`
   row stays `state='processing'`.
2. **File write** (NEW in Phase 1E): serialize the entry to Markdown
   (per D3) → write to a temp file in
   `<vault>/Knowledge Graph/.mockingbird-tmp/<uuid>.md` → fsync →
   atomic rename to
   `<vault>/Knowledge Graph/Entries/<filename>.md`.
3. **DB seal** (NEW): UPDATE the relevant entry-anchor row with
   `file_path = <abs path>`, `file_mtime = <mtime>`,
   `file_hash = <sha256>` (the projection's hash, NOT mtime — see D5).
4. **Queue seal:** UPDATE `kg_filing_queue` SET `state='done'`.

**Why DB-first, file-second:**

- The async worker (ADR 0050) already lives at the DB-write seam;
  extending it to write the file is one new function call, not a
  rearchitecture.
- File-first would race with the reverse-watcher: a newly-projected
  file looks indistinguishable from a user edit until the DB row
  catches up. DB-first lets us record the projection hash BEFORE the
  watcher sees the file (the watcher's loop-prevention check then
  reliably short-circuits — see D5).
- ADR 0048 Q3 carves out steady-state ("file wins on conflict"), not
  capture-time. At capture the user has never seen the file yet, so
  there is no conflict to resolve.

**Failure modes:**

| Step fails | Observable state | Recovery |
|---|---|---|
| 2 (file write) | DB rows exist; no file. `kg_filing_queue` row stuck `processing`. | Nightly sweep: requeue every `state='processing'` row older than 5 min. Idempotent (steps 2/3/4 re-run). |
| 3 (DB seal) | File exists; DB row has NULL `file_path`. | Reverse-watcher discovers the orphan, computes its hash, matches against the "missing seal" pattern → fills in `file_path`/`file_hash`/`file_mtime`. |
| 4 (queue seal) | File + DB synced; queue still `processing`. | Idempotent retry of step 4 only (no file rewrite). |

The new "entry-anchor row" is a new table `kg_entries` added by
migration 026 — a row per filed entry holding the projection
metadata. **Design pick at the 1E.2 brief**, not at the charter: the
shape may end up being a column-set on `kg_entities` instead. The
ADR commits to the *contract* (file_path + file_hash + file_mtime
recorded somewhere queryable per entry), not the table layout.

### D5 — Reverse-watcher conflict resolution (concretizes ADR 0048 Q3)

**File wins, always.** No mtime comparison, no "DB updated_at vs file
mtime" race resolution. The user's edit in Obsidian is canonical;
Mockingbird's DB is a refreshable index.

**Loop-prevention via hash, NOT mtime.**

- After every projection write (D4 step 2), we record `file_hash =
  sha256(file_bytes)` in the DB.
- The reverse-watcher's debounced handler reads the file and computes
  the same SHA-256.
- **If the hash matches the recorded `file_hash` → no-op.** This was
  our own write echoing back through the OS file-event queue.
- If the hash differs → user edit. Re-parse the frontmatter + body,
  re-run the pipeline's normalize pass (NOT the LLM passes — the
  user has already structured the entry), update the DB FTS index +
  `file_hash` + `file_mtime`.

**Why hash-based, not mtime-based:**

- Obsidian Sync can rewrite a file with byte-identical contents
  during conflict resolution (mtime updates, contents unchanged).
  An mtime-keyed loop-prevention check would falsely flag this as a
  user edit.
- Obsidian itself can update file mtime when reformatting frontmatter
  (e.g. the Tasks plugin re-rendering a checkbox line) without
  semantically changing the entry.
- SHA-256 is cheap (~50 µs per kB on modern hardware); the watcher
  debounce window (2s, per ADR 0046 inbox-watcher precedent) easily
  absorbs the cost.

**Debounce window:** 2s, matching ADR 0046's combined-detector
quiet-window. Phase 1E uses `notify-debouncer-full` (same library)
for the same reasons (raw `notify` collapses bursts unreliably).

**Concurrent edits:** Mockingbird never writes to an entry file that
the user has open in Obsidian, because Mockingbird only writes at
capture time (D4) — by which time the file does not yet exist. After
the file lands, all subsequent writes come from the user. The race
window is theoretical.

**Special case — Obsidian Tasks checkbox toggle:** when the user
checks `- [ ] foo` → `- [x] foo` in the body, the reverse-watcher
detects this as a body change (hash differs). The normalize pass
extracts the new checkbox state and updates `status` in DB + (on the
NEXT write — i.e. when the watcher itself writes back) syncs the
YAML `status` field too. To avoid a loop (we wrote it → watcher fires
→ we write again → watcher fires…), the watcher's writes follow
the same loop-prevention contract: it pre-records the new hash
BEFORE writing, so its own echo is a no-op. Spec'd in detail at 1E.5.

**Loop-prevention judge** (Wave 1E.9): J1 — programmatically write
an entry, observe the watcher fire zero times. Then edit the file
externally (test fixture), observe the watcher fire exactly once.
Determinism-by-construction.

### D6 — KG-Inbox courier (sibling to ADR 0046)

Per ADR 0048 Q2 positional routing, a second iOS Shortcut delivers
audio to `<vault>/Knowledge Graph/Inbox/` (separate from the existing
`<vault>/inbox/` for standard dictation).

**Implementation:** **separate watcher**, NOT shared with the
existing `inbox::runtime::InboxRuntime`. Reasoning:

- ADR 0046's inbox runtime is sealed Phase MC-era code. Reaching into
  it to add a second watched path would re-open the boundary for
  more than this epic justifies.
- The KG courier needs a different ingest path: the resulting
  session must land with `capture_kind = 'kg-note'` so the source-
  gated dictation-tail hook (ADR 0052 §D1) actually fires. The
  existing inbox courier writes `capture_kind = 'dictation'` and
  cannot be parameterized without touching the sealed code.
- The two couriers share infrastructure at the **library** level:
  `notify-debouncer-full`, the combined-detector pattern, the
  conflict-file quarantine, the dedup ledger, and
  `dictation::ingest::headless_ingest`. The new module
  `src-tauri/src/vault/kg_inbox_runtime.rs` is a parallel of
  `src-tauri/src/inbox/runtime.rs` — same shape, different
  watched folder and different `IngestProvenance`.

**Race condition — file dropped in wrong inbox:** each watcher only
watches its own folder. A file in `<vault>/inbox/` is unambiguously a
dictation; a file in `<vault>/Knowledge Graph/Inbox/` is
unambiguously a KG-note. No cross-routing logic. If a user manually
misfiles, they manually move; we do not attempt to second-guess
their intent.

**Dedup ledger:** the existing `vault_inbox_ledger` table is keyed
on content SHA-256; the KG courier shares it (no new ledger). A
re-delivered file (same bytes) into either inbox is a no-op.

### D7 — History archive

Per spec §7.1, `History/` is the "re-processing safety net" — the
raw transcript + processed audio kept untouched. Phase 1E ships this
as a per-session JSON sidecar.

**Trigger:** per-entry-finalize (NOT a daily rollup). When the
worker completes step 4 (queue seal) on the LAST entry derived from
a session (spec §7.5: "one memo, many entries"), it writes:

```
<vault>/Knowledge Graph/History/<YYYY-MM>/<session-uuid>.json
```

with shape:

```json
{
  "schema_version": 1,
  "session_uuid": "...",
  "captured_at": "2026-06-15T14:32:01Z",
  "capture_kind": "kg-note",
  "raw_transcript": "...",
  "cleaned_transcript": "...",
  "audio_relpath": "../History/2026-06/<session-uuid>.m4a",
  "entries_produced": ["01HM...", "01HM...", ...]
}
```

**Audio file fate:** when ingested from
`<vault>/Knowledge Graph/Inbox/`, the audio file moves out of Inbox
into `History/<YYYY-MM>/<session-uuid>.m4a` after the JSON sidecar
lands. This satisfies spec §7.1 ("processed audio is moved out [of
Inbox], kept untouched"). The Inbox stays near-empty per spec
("empty inbox is the signal that everything is processed"). Text
notes (`capture_kind = 'kg-note-text'`) write the JSON sidecar but
have no audio to move.

**Retention:** none in v1. "Kept untouched" per spec §7.1; nightly
sweeps are out-of-scope. If History/ growth becomes a complaint
post-v1, address with a settings-driven rollover then.

**Searchability:** Obsidian indexes JSON files (with the right
plugins). Plain-text `rg`/`grep` works. No FTS index in DB for
History records — they're the safety net, not a primary surface.

**Idempotency:** the worker checks for existing `<session-uuid>.json`
before writing; existing means "already processed; skip." A
re-processing flow (post-v1) overwrites.

### D8 — Pre-built `.md` seed (Wave 1E.7)

Spec §14.2: "the user must not be handed a blank vault and homework."
On first activation (`KgGraphEnabled` flips true AND the vault is
empty of KG content), Mockingbird projects a small set of seed `.md`
files into `<vault>/Knowledge Graph/`:

- `Knowledge Graph/Dashboard.md` — query-driven view of open tasks,
  recent entries, and per-category counts using Obsidian Dataview
  syntax (with a fallback section using Bases or plain markdown links
  for users without Dataview installed; Wave 1E.7 brief picks).
- `Knowledge Graph/Kanban - Tasks.md` — Obsidian Kanban plugin
  format keyed on `type: task` + `status: todo/doing/done`.
- `Knowledge Graph/README.md` — quick-start docs: explaining the
  Inbox/Entries/History trinity, the dual-write behavior, where to
  configure the activation toggle.

**Seed vs. user content:** the moment Mockingbird writes a seed file,
it counts as user content. The reverse-watcher reconciles edits to
these files like any other note (D5). Mockingbird **never overwrites
seeds** — not on re-activation, not on upgrade. If the user deletes a
seed, it stays deleted. This avoids the "stomp on user customization"
footgun.

**Plugin dependency:** Dataview and Kanban are popular but not
universal Obsidian plugins. The seeds work degraded-but-readable
without them (Dataview blocks render as their raw query text; Kanban
boards render as their underlying nested-list markdown). The README
seed includes a one-liner pointer to "install Dataview + Kanban for
the full board experience." Hard binding on those plugins is out of
scope — we ship the convention, the user opts in.

### D9 — Obsidian Tasks emission

Per spec §7.4, `type: task` entries emit a native Obsidian Tasks
plugin checkbox line at the top of the body:

```markdown
- [ ] Switch suppliers for brake pads 📅 2026-06-20 🏷️ brake-pads/suppliers
```

Format: standard Obsidian Tasks plugin emoji syntax (📅 due, ⏫/🔼/🔽
priority, 🏷️ tags, ✅ done date when checked).

**Dual-source-of-truth concern (resolved):** the YAML frontmatter
`status` field and the checkbox state are TWO representations of the
same boolean. On round-trip:

- Writer always emits the checkbox in agreement with
  frontmatter `status`. Canonical source on write is YAML
  (frontmatter is the structured contract).
- Reader (reverse-watcher normalize pass) treats the checkbox as
  canonical when it differs from YAML, then on the next write the
  YAML is updated to match. This handles the common user flow:
  user checks a box in Obsidian → watcher reads checkbox → DB +
  YAML sync up.
- Spec §14.3 explicitly enables this: "completing/checking a task...
  sync back from any surface."

No separate `Tasks.md` index file (rejected — duplicates the entry
data; Obsidian's Tasks plugin queries across the vault and renders
its own index views).

### D10 — iOS Shortcut docs (Wave 1E.8)

Mirror of ADR 0046 §8. New file: `docs/mobile/ios-shortcut-kg.md`
with the same two-action chain (Record Audio → Save File) but with
the Save destination set to `<vault>/Knowledge Graph/Inbox/`.

**No `.shortcut` import file shipped.** Apple's `.shortcut` format
embeds the user's specific iCloud / Files paths, so it's not
portable across vault locations. ADR 0046 §8 made the same call for
the dictation Shortcut; we keep parity.

The docs explicitly call out: this is a SECOND Shortcut, distinct
from the dictation Shortcut. Naming convention suggestion:
"Mockingbird Quick Capture (Dictation)" + "Mockingbird Quick Capture
(Knowledge Graph)." User decides per-capture which Shortcut to fire
(Action Button binding picks one; Home Screen icons can host both).

---

## Wave plan (nine waves)

| Wave | Scope | Agent | Sub-bead |
|---|---|---|---|
| 1E.0 | This charter + phase doc + bead epic. NO migration, no Rust, no UI. | code-puppy | (this wave) |
| 1E.1 | Vault subtree bootstrap (idempotent `Knowledge Graph/{Inbox,Entries,History}/` creation on toggle-on / boot; settings-side guard for `VaultPath` unset). | migration-author or code-puppy | TBD |
| 1E.2 | Deterministic Markdown serializer (entry → `.md` with YAML frontmatter per D3, byte-stable, golden-file tested). Pure-Rust module; throwaway-crate testable. | code-puppy | TBD |
| 1E.3 | Worker writes Markdown after DB insert per D4 two-phase commit. Migration 026 adds `kg_entries.file_path` / `file_hash` / `file_mtime` columns (or new table; 1E.2 brief picks the shape). Nightly sweep for stuck-processing rows. | migration-author + code-puppy | TBD |
| 1E.4 | History archive: per-session JSON sidecar in `History/<YYYY-MM>/<uuid>.json` + audio file move-out-of-Inbox. | code-puppy | TBD |
| 1E.5 | Reverse-watcher: file-event → debounce → hash-match short-circuit → normalize pass → DB FTS update. Obsidian Tasks checkbox special case. | code-puppy | TBD |
| 1E.6 | KG-Inbox courier (`vault::kg_inbox_runtime`): sibling of `inbox::runtime`. Positional routing per Q2; shared dedup ledger. | code-puppy | TBD |
| 1E.7 | Pre-built seeds (Dashboard.md + Kanban - Tasks.md + README.md) projected on first activation. Never overwritten. | ui-author or code-puppy | TBD |
| 1E.8 | iOS Shortcut docs at `docs/mobile/ios-shortcut-kg.md`. Mirror of ADR 0046 §8 for the KG-Inbox destination. | code-puppy | TBD |
| 1E.9 | Phase 1E judges + seal: J1 (reverse-watcher loop-prevention), J2 (file-wins-on-conflict), J3 (subtree-bootstrap idempotency), J4 (serializer golden-file round-trip). Flip this ADR to **Accepted**. | code-puppy | TBD |

Sub-bead IDs are minted at Wave 1E.0 and recorded in
`docs/phases/phase-1e.md` §"Bead epic".

---

## Sandbox isolation (graduation window)

Per ADR 0049 §"Sandbox isolation" + ADR 0052 §"Sandbox isolation"
precedents, this ADR opens a scoped edit window on the following
surfaces. Outside the list, the seal holds.

| Surface | Authorized change | Closes at |
|---|---|---|
| `src-tauri/src/db/migrations/026_kg_phase_1e_entry_projection.sql` (NEW) | Add `kg_entries.file_path` + `file_hash` + `file_mtime` (or new table per 1E.3 brief). | 1E.9 seal. |
| `src-tauri/src/db/migrations.rs` | Register migration 026. | 1E.9 seal. |
| `src-tauri/src/kg/projection.rs` (NEW) | Markdown serializer + deserializer; pure-Rust. | 1E.9 seal. |
| `src-tauri/src/kg/worker.rs` | Extend the existing async worker to call the projection writer per D4. | 1E.9 seal. |
| `src-tauri/src/kg/reverse_watcher.rs` (NEW) | `notify-debouncer-full` subscriber; debounce + hash-match + normalize. | 1E.9 seal. |
| `src-tauri/src/vault/kg_inbox_runtime.rs` (NEW) | KG-Inbox courier; parallel of `inbox/runtime.rs`. | 1E.9 seal. |
| `src-tauri/src/vault/layout.rs` | Additive: KG subtree paths + bootstrap helper. | 1E.9 seal. |
| `src-tauri/src/dictation/ingest.rs` | Additive: new `IngestSource::MobileInboxKgNote` variant. One enum case. | 1E.9 seal. |
| `src-tauri/src/commands/kg.rs` | Additive IPC: `kg_subtree_bootstrap`, `kg_open_entry` (`obsidian://open?vault=X&file=Y`). | 1E.9 seal. |
| `src-tauri/src/kg/launcher.rs` | Additive: per-entry `launch_obsidian_entry` helper alongside the existing vault-level launcher. | 1E.9 seal. |
| `src-tauri/capabilities/default.json` | Allowlist new commands (ADR 0035 discipline). | 1E.9 seal. |
| `src-tauri/src/kg/assets/seeds/*.md` (NEW) | Seed templates for Dashboard, Kanban, README. | 1E.9 seal. |
| `docs/mobile/ios-shortcut-kg.md` (NEW) | Wave 1E.8 deliverable. | 1E.9 seal. |
| `ui/src/routes/knowledge-graph/Dashboard.tsx` | Additive: per-entry "Open in Obsidian" affordance using the new `kg_open_entry` IPC. | 1E.9 seal. |
| `ui/src/lib/tauri.ts` + `ui/src/lib/types.ts` + `ui/src/i18n/en.json` | Additive bindings + types + copy. | 1E.9 seal. |

### Explicitly NOT authorized

- New telemetry (Principle 4).
- Edits to `transcripts` table or its triggers (Principle 1; raw-data
  immutability).
- Edits to migrations 001-003 (post-`phase-1-complete` hook).
- Edits to migrations 011 / 012-016 (sealed phases).
- Edits to `meetings/*` (Phase MC sealed).
- Edits to `activity/*` (Phase 10 sealed).
- Edits to `inbox/*` (ADR 0046 sealed — the KG courier is a SIBLING
  module, not a modification of the existing one).
- Backfill of pre-Phase-1E entries into Markdown (post-v1; ADR 0052
  out-of-scope carry-forward).
- Vocabularies editor (post-v1).
- Synthesize operation per Clark article (post-v1).
- v1 beta tag (Phase 1F).

---

## Acceptance gates (Wave 1E.9 judge bundle)

Four judges. All deterministic; this epic's invariants reduce to
hash/file-state checks that don't need LLM grading.

### J1 — `kg-reverse-watcher-loop-prevention`

Programmatically project an entry via the worker → observe the
file-event queue → assert the watcher's handler fires zero times for
that write (hash-match short-circuit). Then externally edit the file
(write fresh bytes) → assert the watcher fires exactly once. Pure-
Rust integration test in a throwaway-crate harness.

### J2 — `kg-file-wins-on-conflict`

Seed an entry in DB + project to file → externally edit the file
body → observe the reverse-watcher reconciliation → assert DB FTS
reflects the file's new content (NOT the old DB content). Catches
any accidental DB-wins regression in the watcher's normalize pass.

### J3 — `kg-subtree-bootstrap-idempotent`

Run the subtree bootstrap with: (a) missing dir → assert creates;
(b) existing empty dir → assert no-op + no error; (c) existing dir
with user files → assert no-op + no overwrite; (d) `VaultPath`
unset → assert guard refuses to fire. Four-cell deterministic test.

### J4 — `kg-serializer-golden-roundtrip`

Golden-file test: for N (~10) fixture entries spanning the full
frontmatter shape (all `capture_kind`s, all `category`s, all `type`s,
with + without `status`, with + without `due_date`, with + without
entities), serialize → write bytes → parse → re-serialize → assert
byte-identical. Catches any drift in the canonical-form rules (D3).

### Standing regression gates

- `kg_parity` default 32/32 + `kg_parity --persist` 32/32 (Phase 1B
  invariant; must stay green).
- `kg_graph_off_invariant` 8/8 + controls (Phase 1C invariant).
- `kg_source_gate_invariant` 6/6 cells (Phase 1D invariant; the KG
  courier MUST land entries with `capture_kind = 'kg-note'`).
- `mc-dictation-untouched` still green vs. `phase-mc-start` anchor.

---

## Risks

| Risk | Wave | Mitigation |
|---|---|---|
| Reverse-watcher feedback loop (we write → watcher fires → we re-process → we write again…). | 1E.5 | Hash-based loop-prevention (D5) — pre-record hash BEFORE write. J1 mechanically enforces. |
| Two-phase commit partial failure leaves DB + file out of sync. | 1E.3 | Nightly sweep requeues `state='processing'` rows >5 min old. Reverse-watcher discovers orphan files and fills in missing seal data. |
| Obsidian Sync byte-identical re-touch trips false user-edit detection. | 1E.5 | Hash match (not mtime) short-circuits this. Same call ADR 0046 made for the dedup ledger (§6). |
| Filename slug collision between two entries dictated on the same day with very similar titles. | 1E.2 | `__<id8>` suffix is the collision-resistant anchor (8 chars of ULID = 40 bits ≈ 1-in-trillion collision). Fully resolved. |
| Spec §7.1 says `Knowledge Graph/` with a space; some shell tooling stumbles on spaced paths. | 1E.1 | All path handling goes through `std::path::Path` / `PathBuf` (Rust handles spaces transparently). Obsidian's URI scheme percent-encodes via the existing `launcher.rs` `encode_vault_name` helper. |
| Obsidian Kanban / Dataview plugins not installed → seeds degrade. | 1E.7 | Seeds are degraded-but-readable without plugins. README seed includes a one-liner pointer to install. |
| Dustin's vault is already populated with KG notes (manual setup before activation). | 1E.1 | Bootstrap is fully idempotent (D1); pre-existing subtree is discovered and used as-is. Pre-existing `.md` files are picked up by the reverse-watcher on first scan (treated as user content from the start). |
| Stale-prompt risk on Wave 1E.N dispatches. | 1E.1–1E.9 | LESSONS P8: dispatch prompts are one-line pointers (`implement Wave 1E.N per docs/phases/phase-1e.md`); the phase doc is canonical reference. |
| `mb-mxal` (relocate DB to LOCALAPPDATA) could land mid-epic and change the file_path mapping. | (any) | If it lands mid-epic, the file_path column stores absolute paths so DB location changes don't break entries. Vault path is independent of DB path. |

---

## Beads

Wave 1E.0 mints the epic + nine sub-beads, ASCII-only per LESSONS
2026-05-24. Linkage: `mb-<epic>` blocked by all nine waves;
serial chain `1E.0 → 1E.1 → 1E.2 → 1E.3 → 1E.5 → 1E.9` (the load-
bearing critical path); branches: 1E.4 depends on 1E.3, 1E.6
depends on 1E.3, 1E.7 depends on 1E.1, 1E.8 has no code
dependencies (docs-only). All beads recorded in the phase doc.

## Supersession

This ADR does NOT supersede ADR 0048 (it concretizes Q3). It builds
on ADR 0052 (the source-gated dictation tail is unchanged; this ADR
adds the missing projection layer). It is parallel to ADR 0046
(separate-but-sibling courier — same vault, different inbox, shared
ingest primitives).

## References

- Spec `C:\Users\dboyd\Downloads\mockingbird-knowledge-graph-spec.md`
  §7 (folder structure), §7.4 (Obsidian Tasks), §9 (raw safety
  net), §14 (file-IS-the-database), §15.4 (vault config),
  §15.5 (silent capture).
- [ADR 0046](0046-mobile-extension-via-vault.md) — vault primitives;
  `inbox::runtime` + `dictation::ingest::headless_ingest` are the
  reusable surface this ADR sits on top of.
- [ADR 0048](0048-knowledge-graph-phase-0-validation.md) — Q1
  (subtree), Q2 (positional routing), Q3 (files-are-truth) are the
  meta-decisions this ADR concretizes.
- [ADR 0049](0049-knowledge-graph-phase-0-5-and-v1-architectural-pivot.md)
  — pipeline + schema commitments.
- [ADR 0050](0050-kg-phase-1b-persistence-and-dictation-hook.md) —
  the async worker (`kg::worker`) that gains the projection writer
  in Wave 1E.3.
- [ADR 0052](0052-knowledge-graph-phase-1d-charter.md) — the source-
  gated dictation-tail hook + KG screen this ADR builds on.
- LESSONS PINNED P5 — lateral epics seal via Accepted ADR; no
  `phase-*-complete` tag.
- LESSONS PINNED P8 — `session_id` discipline for downstream wave
  dispatches.

## Phase numbering after this ADR

- Phase 1E (this ADR) — Obsidian as source of truth (vault projection
  + reverse-watcher + KG-Inbox courier).
- Phase 1F (was 1E in the pre-1D-rescope numbering, then renumbered
  at ADR 0052; this ADR confirms the renumbering) — v1 beta tag,
  full-system smoke matrix, release wiring.

## Out of scope (deferred to Phase 1F or post-v1)

**Deferred to Phase 1F:**
- The `v1-beta` git tag.
- Full-system smoke matrix on a fresh Win11 box.
- Release wiring (installer / updater verification).
- Marketing-shaped privacy statement / docs polish.

**Deferred post-v1:**
- Backfill of pre-Phase-1E historical dictations into the KG (per-row
  promote-to-graph affordance on the Dictations page).
- Vocabularies editor in Settings (read-only display ships in 1D.5).
- Synthesize operation per the Clark article (cross-entry agentic
  summarization).
- Routing-step optimization (per-pass Nemotron-style model selection).
- Archive vs. Ingest mode toggle per spec §10.
- Interactive ingest mode (per-entry user confirmation per spec
  §15.5 / §11).
- History/ retention rollover (current v1: kept untouched forever).
- macOS support (Phase 9).

## Amendments

Dated entries below are AUTHORITATIVE over the original §D sections
where they conflict. Original §D text is preserved for historical
record; new readers should treat "D1 (as amended)" / "D3 (as
amended)" + the new §D11 / §D12 below as the live contract.

### 2026-06-06 — Charter amendment: entity pages, project pages, wiki-link entities

**Bead:** `mb-08za`. **Trigger:** Wave 1E.4 shipped end-to-end and
Dustin opened the resulting vault in Obsidian. Entities were
emitted as bare strings in frontmatter, which Obsidian renders as
untyped text — not as graph nodes. Without per-entity pages, the
"all entries mentioning Maple" view that's the whole point of a KG
doesn't exist. The four amendments below close the gap before
1E.5 ships (the reverse-watcher would otherwise lock the
bare-string shape in).

#### Amendment to §D1 — Vault subtree shape

The KG subtree expands from `{Inbox, Entries, History}` to
**`{Inbox, Entries, History, Entities, Projects}`** (five folders).
The two new folders host the auto-generated stub pages introduced
by §D11 / §D12 below.

Idempotent creation continues to use the same `create_dir_all`
pattern (`vault::kg_layout::bootstrap_kg_subtree`); the
`BootstrapReport::AlreadyExists` discriminator now requires all
FIVE directories to be present-as-dirs. Bootstrap fires on the
same triggers as before (toggle-on; opportunistic on first
capture).

#### Amendment to §D3 — YAML frontmatter shape

The `entities:` field emits as **Obsidian wiki-links** to the
corresponding entity page under `Entities/`, NOT as bare strings.
Example:

```yaml
entities:
  - "[[Entities/feta-cheese]]"
  - "[[Entities/eggs]]"
  - "[[Entities/milk]]"
```

Wiki-link emission rules:

1. **Slug derivation** reuses the §D2 filename-slug helper
   (`vault::markdown_serializer::slugify_title`) — lowercase ASCII
   kebab-case, length-capped at `SLUG_MAX_LEN` (50 chars), empty
   collapses to the literal `"untitled"`. The slug rule is the
   SAME across filename + entity-link emission so there is one
   source of truth for "how Mockingbird turns a free-form string
   into an ASCII identifier".

2. **Slug-collision merging** happens at serialize time. Distinct
   input strings that slugify identically (`["Mockingbird",
   "mockingbird", "MockingBird"]` all → `mockingbird`) collapse to
   a single wiki-link in the emitted list. v1 explicitly does NOT
   ship a disambiguation/merge UX; collapsing to the same entity
   is the default behaviour and the user can split via manual edit
   if needed. Tracked post-v1 as `mb-...` (filed alongside this
   amendment).

3. **YAML quoting** keeps the existing double-quote-everything
   discipline — the wiki-link itself contains `[`, `]`, `/`, and a
   hyphen, all of which YAML 1.2 would accept bare, but the
   reverse-watcher's parser is happier when every string scalar
   has the same shape.

4. **Round-trip contract for 1E.5:** the reverse-watcher MUST
   accept BOTH shapes (legacy bare-string entries written before
   this amendment AND the new wiki-link form). Parsing
   `"[[Entities/<slug>]]"` back to a bare slug for DB write is the
   reverse direction; the canonical form on the way OUT is always
   wiki-link. See `vault::markdown_serializer` module docs for the
   round-trip safety statement.

#### New §D11 — Entity pages (auto-generated, write-once, user-owns-thereafter)

On every successful filing, after the entry's `.md` file has been
committed to `Entries/` and sealed, the worker iterates the entry's
unique entity slugs and **idempotently** creates a stub at
`Knowledge Graph/Entities/<slug>.md` for any slug whose stub does
not yet exist.

Stub frontmatter:

```yaml
---
id: feta-cheese
type: entity
schema_version: 1
created_at: 2026-06-06T10:00:00Z
aliases: []
---
```

Stub body:

```markdown
# feta-cheese

```dataview
TABLE category, type, status, captured_at
FROM "Knowledge Graph/Entries"
WHERE contains(entities, "[[Entities/feta-cheese]]")
SORT captured_at DESC
```
```

**Write-once, user-owns-thereafter contract:**

- The stub is written ONCE, on first mention. The detection
  mechanism is purely existence-based: `Entities/<slug>.md` exists
  on disk ⇒ skip stub generation entirely. No content hash; no
  schema upgrades that mutate user-edited stubs.
- After creation, Mockingbird NEVER overwrites the file. The user
  may freely rewrite the body (add notes, add their own queries,
  change the title) — Mockingbird will not touch it.
- Failure to write the stub is **non-fatal** to the entry's
  filing. Same retry-budget decoupling as §D4 step 4: log via
  `tracing::warn!`, continue, let `reconcile_vault` (or a future
  extension thereof) pick up missing stubs later.
- Atomic write via temp-sibling + rename, same discipline as
  §D4 step 4 (`vault::writer::commit_entry_to_vault`).
- Canonical-form bytes (LF only, deterministic frontmatter field
  order, single trailing newline) for byte-identity diffs and
  Phase 1E.5 round-trip discipline.

#### New §D12 — Project pages (same pattern as §D11, scoped to Project-typed entities)

For entities the pipeline's `extract_entities` pass classifies as
`EntityType::Project` (one of the five-bucket closed enum from
`kg::passes::extract_entities`), the worker ALSO generates a stub
at `Knowledge Graph/Projects/<slug>.md` on first mention. Same
write-once user-owns-thereafter semantics as §D11.

Stub frontmatter:

```yaml
---
id: <slug>
type: project
schema_version: 1
created_at: 2026-06-06T10:00:00Z
status: active
---
```

Stub body:

```markdown
# <slug>

```dataview
TABLE category, type, status, captured_at
FROM "Knowledge Graph/Entries"
WHERE contains(entities, "[[Entities/<slug>]]")
SORT captured_at DESC
```
```

Notes:

- The Dataview body filters on `entities`, not a separate
  `projects` field — the frontmatter has one entity list and the
  user can navigate from either Entity page or Project page to the
  same set of entries.
- A single slug can produce BOTH an Entity page and a Project page
  when classified as Project (Project entities are still emitted
  as `entities:` wiki-links in the entry frontmatter; the
  duplication is intentional — Dataview queries on the Project
  page filter by the same wiki-link).
- `status: active` is a STATIC initial value; the user is
  expected to edit it (`active` / `paused` / `done` etc.) — same
  user-owns-thereafter contract; Mockingbird never rewrites it.

#### Wave plan impact

All five amendment-bundled changes (subtree expansion, serializer
retrofit, two stub generators, worker integration) ship in ONE
wave (`mb-08za`), interstitial between 1E.4 and 1E.5. The wave
seals with the original Phase 1E acceptance gates re-passed
(`kg_parity` 32/32, `kg_source_gate_invariant` 6/6,
`kg_graph_off_invariant` 8/8) plus the new entity-page golden +
property tests on the wiki-link emission shape.

#### Backfill

Entries written before this amendment retain bare-string entities
on disk; new entries get wiki-link entities. Dataview queries
should defensively handle both shapes (e.g. `contains(entities,
"feta-cheese") OR contains(entities, "[[Entities/feta-cheese]]")`).
Automatic backfill is deferred to Phase 1F; the manual
alternatives (re-capture, hand-edit) are documented in LESSONS.

### 2026-06-06 — Superseded in part by ADR 0054 (Karpathy/Clark adoption)

**Bead:** `mb-rik9` (Phase 1E Alignment Wave). **Trigger:** Dustin
recognized that Obsidian is knowledge-graph-first (not
task-management-first) and surfaced the Karpathy "LLM Wiki" gist +
Alvin Clark "Building a Personal Knowledge Engine with LLMs and
Obsidian" (April 2026) as the architectural north star. ADR 0054
(Proposed) charters the adoption of the Personal Knowledge Engine
substrate pattern over this ADR's foundation; it does NOT supersede
wholesale.

**Sections of THIS ADR that ADR 0054 supersedes:**

- **§D3 (YAML frontmatter shape — type vocabulary) — partial.** The
  `task` / `event` / `research` / `reference` / `note` set is
  replaced by the nine knowledge shapes (`source` / `note` /
  `concept` / `entity` / `project` / `question` / `decision` /
  `reference` / `observation`). The frontmatter *shape* (field
  order, conditional fields, LF discipline, escape rules) remains
  in force unchanged. See ADR 0054 §G.
- **§D8 (Pre-built `.md` seeds — Wave 1E.7) — partial.** The
  `Kanban-Tasks.md` seed is dropped; the new seeds are `SCHEMA.md`
  (write-once user-owned), `INDEX.md` (auto-maintained catalog),
  `LOG.md` (append-only operations log), and the `Tags/` subtree.
  The `Dashboard.md` seed in its original task-flavored shape is
  superseded by `INDEX.md`. The `README.md` seed shape carries
  forward with updated framing. See ADR 0054 §J (Wave 1E.7
  rescope) + §C/§D/§E/§F.
- **§D9 (Obsidian Tasks emission) — de-emphasized.** Tasks-plugin
  checkbox emission moves from default-on to opt-in via a user
  preference setting. The serializer retains the capability; the
  default flips off. See ADR 0054 §L.

**Sections that carry forward unchanged:** D1 (vault subtree shape;
as amended above to five folders), D2 (filename format), D3
*shape* (only the vocabulary changed; field order + escape rules
intact), D4 (two-phase commit), D5 (reverse-watcher), D6 (KG-Inbox
courier), D7 (history archive), D10 (iOS Shortcut docs), D11
(entity pages), D12 (project pages). The reverse-watcher remains
the load-bearing reconcile path; the two-phase commit remains the
DB-first-then-file-then-DB-seal ordering; the file-wins conflict
resolution remains in force.

**Action:** Phase 1E continues toward seal via the rescoped 1E.7 +
1E.9 (per ADR 0054 §J). 1E.6 + 1E.8 are unchanged. ADR 0053 stays
Proposed until Phase 1E seals; at seal both ADR 0053 and ADR 0054
flip to Accepted together.
