# ADR-0050: Knowledge Graph Phase 1B — SQLite persistence + async filing queue + dictation-tail hook (default-off)

- **Status:** **Proposed** (flips to **Accepted** at Chunk 5 seal, per §"Acceptance criteria" below)
- **Date:** 2026-06-02 (Proposed)
- **Deciders:** Dustin (project lead), Bernard / code-puppy (chartering)
- **Charter for:** ADR-chartered lateral epic — Knowledge Graph Phase 1B
  (epic bead `mb-bjni`). Five chunks: `mb-go9l` (this charter),
  `mb-geds` (migration + store + SettingKey), `mb-eke8` (worker thread),
  `mb-ryq4` (dictation tail surgical edit), `mb-k17a` (extended parity
  probe + graph-off judge + seal).
- **Extends:** [ADR 0049](0049-knowledge-graph-phase-0-5-and-v1-architectural-pivot.md)
  §"Sandbox isolation" — opens a scoped exception window for
  `src-tauri/**` + `migrations/**` for the duration of epic `mb-bjni`.
  Does NOT supersede or replace ADR 0049; the v1 architecture
  commitments (§6 of that ADR) and amendments A1/A2/A3 carry forward
  unchanged.
- **References (pattern precedent):**
  - [ADR 0037](0037-unified-recording-command-center.md) — sealed-surface
    authorization language. §5 ("Boundary — what 0037 explicitly authorizes
    touching in sealed code") is the shape this ADR's §5 mirrors.
  - [ADR 0036](0036-activity-capture-sibling-subsystem.md) +
    [ADR 0040](0040-activity-summarization-pipeline.md) — sub-charter
    ADR under a parent epic pattern. ADR 0050 is to ADR 0049 what
    ADR 0040 is to ADR 0036.
  - [ADR 0010](0010-raw-transcript-immutability.md) — Principle 1
    enforcement model that 1B's audit triggers follow.
  - `src-tauri/src/activity/persist.rs` + `blocks_persist.rs` +
    `segments_persist.rs` — the "DB layer lives with the subsystem"
    precedent for D2.
  - `src-tauri/src/activity/runtime.rs` worker dispatch — the
    persisted-queue + std-thread worker pattern for D3.
- **Wave brief:** [`docs/knowledge-graph/phase-1b-brief.md`](../knowledge-graph/phase-1b-brief.md)
- **Phase 0.5 evidence anchor:**
  [`docs/knowledge-graph/PHASE-0-5-REPORT.md`](../knowledge-graph/PHASE-0-5-REPORT.md)
  §6 (v1 binding commitments) + §7 (Phase 1A-1E wave plan).

---

## Context

ADR 0049 sealed the Phase 0.5 architectural pivot and graduated the
schema-driven KG pipeline to `src-tauri/src/kg/` under Phase 1A
(commit `bfc9dcd`, epic `mb-2mc9` closed). The pipeline now exists
as a callable library — but it has **zero consumers** and **no
persistence**. The dictation orchestrator does not call it; the DB
has no tables for entities, tags, mentions, or filing-queue rows;
the UI surface does not exist yet.

PHASE-0-5-REPORT.md §7 lines up Phase 1B as the persistence wave:
SQLite extensions for entity / tag / edge tables, concept pages as
computed SQL views, files-as-source-of-truth preserved (Q1/Q2/Q3
from ADR 0048 §3 inherited unchanged). This ADR codifies the
specific schema design + the sealed-surface authorization for
the one new dictation-tail call site that bridges the dictation
pipeline to the (default-off) graph-filing queue.

The ADR 0049 binding mission-cohesion guarantee is what makes this
ADR small in surface but high-stakes in invariant discipline:
**existing dictation users see ZERO regression with the graph off.**
The Phase MC `mc-dictation-untouched` judge pattern translates here
to `kg-graph-off-untouched` — and that invariant is the principal
gate at Chunk 5 seal.

This ADR is the smallest authorization sufficient to make Phase 1B
land cleanly: it spells out the schema, opens the sealed dictation
surface for ONE surgical call-site insertion, and pre-commits the
gate Chunk 5 will run.

---

## Decision

We charter **Phase 1B as a five-chunk lateral epic under ADR 0049's
§"Sandbox isolation" exception-window mechanism.** The eight binding
parameters below are pre-approved by Dustin at kickoff and recorded
here for future-session anchoring (per AGENTS.md "Permanently
sealed" + LESSONS PINNED P4 stale-prompt discipline).

### D1 — Provenance: per-segment

`kg_entity_mentions` and `kg_tag_mentions` carry
`(entry_id, segment_idx, ...)` UNIQUE keys. Per-dictation rollups
are derived via `GROUP BY entry_id` in concept-page views;
per-segment storage is the primitive.

**Rationale.** The production `extract_entities` pass already emits
per-segment shape (the pipeline fans `extract_entities` across
segments and returns aggregated `entities: Vec<Entity>` at the
dictation level — see `src-tauri/src/kg/pipeline.rs`). Storing at
the per-segment grain costs ~5x row count vs per-dictation, but
preserves an irreversible primitive: aggregate rows can be
materialized from per-segment rows but not vice-versa. AGENTS.md
Principle 2 ("Provenance is total") makes this the conservative
default. Phase 1A's parity README §3 explicitly anchored this
decision for 1B.

**Considered alternatives.** Per-dictation rollup tables (cheaper,
simpler joins). Rejected: discards information the pipeline already
produces. If we discover the cost is real (R5 latency budget), the
mitigation is a denormalized rollup VIEW or materialized table —
NOT throwing away the per-segment primitive. P5 standing-bead
material, not gating 1B seal.

### D2 — DB layer location: `src-tauri/src/kg/store/`

The 1B store layer lives inside the `kg::` subsystem, **not** under
`src-tauri/src/db/`. New paths:

```
src-tauri/src/kg/store/
├── mod.rs         — re-exports + enqueue_for_filing entry point
├── entities.rs    — CRUD against kg_entities
├── tags.rs        — CRUD against kg_canonical_tags
├── mentions.rs    — CRUD against kg_entity_mentions + kg_tag_mentions
└── queue.rs       — CRUD + state-machine helpers against kg_filing_queue
```

**Rationale.** Mirrors the activity-capture precedent:
`src-tauri/src/activity/persist.rs` (sessions/events),
`activity/blocks_persist.rs` (blocks), `activity/segments_persist.rs`
(transcript segments). The "subsystem owns its persistence module"
pattern keeps `src-tauri/src/db/` for cross-subsystem primitives
(`migrations.rs`, `sessions.rs`, `transcripts.rs`, `audit.rs`,
`prompt_loader.rs`) — i.e. surfaces the rest of the app reads
directly. The KG store is consumed only by `kg::` itself + (at the
Chunk 4 boundary) the dictation tail call site.

**Considered alternatives.** `src-tauri/src/db/kg_entities.rs` etc.
Rejected: puts the KG schema-half far from the KG library-half,
making future refactors split-edit. Cohesion beats one shared dir.

### D3 — Filing queue: persisted SQLite `kg_filing_queue` table + dedicated worker thread

The queue is **on disk** (table `kg_filing_queue`, see §"DB schema"
below). A dedicated `std::thread` worker (`kg::worker`) drains it
FIFO. The worker is kicked off at app boot **only if** `KgGraphEnabled = true`
(D4). Crash recovery: on every worker startup, a sweep flips any
`state = 'processing'` rows back to `'pending'` so an interrupted
filing resumes cleanly on next run.

**Rationale.** Mirrors `src-tauri/src/activity/runtime.rs`'s
std::thread + channel + Arc<Mutex<Connection>> dispatch model.
AGENTS.md "Why no tokio / async" applies (activity runtime header):
adding tokio for one worker thread balloons the runtime footprint.
On-disk queue means: (a) survives crash (per ADR 0049 §6 binding
"~1 min intake latency budget" with crash-tolerance), (b) inspectable
post-hoc, (c) gives the future failed-filings UI surface (Phase 1C
deferred) a real table to render.

**Considered alternatives.** In-memory channel + tokio task. Rejected
per AGENTS.md "no tokio" + loss of crash recovery + no inspectable
post-hoc state. Synchronous in-line filing on the dictation tail.
Rejected: would block dictation post-paste behind LLM passes, which
violates the ~1 min intake latency budget AND would tangle the
"graph-off ⇒ untouched" invariant. The on-disk queue is the cheapest
correct option.

### D4 — Settings gate: `SettingKey::KgGraphEnabled`, default `false`

New typed setting key, default `false`. Migration 024 seeds the
`settings` row at install (idempotent INSERT OR IGNORE). The worker
thread does NOT start at boot when the gate is off — the spawn code
is a single boot-time conditional check (saves resources for users
who never opt in). The dictation-tail hook (D6) is also a no-op
when the gate is off (`if !KgGraphEnabled { return; }` at the very
top of `kg::enqueue_for_filing`).

The runtime toggle UX (Settings → Knowledge Graph) is **Phase 1C**.
Phase 1B's gate is a developer/power-user knob: editing the row in
`settings` table directly OR setting via the existing typed-settings
IPC. No user-facing UI surface lands in 1B.

**Rationale.** ADR 0049 §6 binding: "Graph layer is OPT-IN — Default
off. Activated via Settings → Knowledge Graph. Dictation experience
unchanged. Binding mission-cohesion guarantee." Default-off makes
the principal invariant (D8 `kg-graph-off-untouched`) the trivial
case: untoggled by default, untouched by default. The flag is the
boundary between "Phase 1B shipped" and "Phase 1B activated".

**Considered alternatives.** Boot-time env var only (no settings
row). Rejected: settings row is the durable, IPC-addressable
surface 1C will wire. Default-on with a kill switch. Rejected:
violates ADR 0049 §6 binding opt-in commitment.

### D5 — Migration granularity: one `024_kg_phase_1b.sql`

A single SQL migration file covers the entire 1B schema. Sections
are clearly delimited with `-- ===` banner comments mirroring
`012_activity_capture.sql`'s style:

```
024_kg_phase_1b.sql
├── kg_entities                — entity rows
├── kg_canonical_tags          — closed-vocab tag rows (v1.1 inert; populated in v1.1)
├── kg_entity_mentions         — per-segment entity provenance
├── kg_tag_mentions            — per-segment tag provenance
├── kg_filing_queue            — async filing queue with FIFO state machine
├── kg_concept_entities_view   — entries-by-entity computed view
├── kg_concept_tags_view       — entries-by-tag computed view
├── audit triggers             — immutability on mention rows (Principle 1 analog)
└── seed row                   — KgGraphEnabled = false
```

**Rationale.** One migration = one schema_meta version bump = one
atomic transaction wrapping the entire 1B schema. ADR 0008
(prompt-versioning, applied to migrations by convention) + the
migration test harness expect this. Splitting across multiple
migrations would force schema_meta version bumps mid-epic that the
hook engine would (correctly) refuse to walk back if Chunk 5's
parity probe surfaces a redesign need.

**Considered alternatives.** One migration per artifact class
(`024_entities.sql`, `025_tags.sql`, ...). Rejected: violates
"one bd seal = one migration" rhythm the project has used since
phase 0. 011_meeting_capture and 012_activity_capture both bundled
their whole subsystem in one migration; 024 follows suit.

### D6 — Dictation hook insertion point: post-paste, post-success, on the orchestrator's tail

**Exactly ONE** call site is added to the dictation orchestrator:
a single invocation of `kg::enqueue_for_filing(&conn, entry_id, ...)`
placed on the orchestrator's tail **after** clipboard injection
has succeeded **and** the session row is finalized. The hook has
ignore-error semantics — any `Err(_)` from `enqueue_for_filing` is
logged at `tracing::warn!` and discarded; the dictation outcome
is unaffected.

When `KgGraphEnabled = false`, the function returns `Ok(())`
immediately at its top — the call site sees a no-op (~nanosecond
cost). The graph-off path is byte-identical to pre-1B behaviour
on the orchestrator's observable outcomes.

**Rationale.** Sealed-surface authorization mirrors ADR 0037's
boundary table. The dictation orchestrator's tail (post-paste,
post-success) is the right insertion point because:

- The session row is already finalized (`entry_id` available).
- All Phase MC `mc-dictation-untouched` invariants are preserved
  upstream of this point.
- Failure isolation: the only thing happening after the hook fires
  is the orchestrator returning `Ok(_)` — there's nothing left to
  break if the hook misbehaves.

**Considered alternatives.** Hook into the cleanup pipeline. Rejected:
cleanup runs upstream of paste; a misbehaving hook there could
delay paste, breaking the dictation latency budget. Hook into the
post-stop state-machine event emitter. Rejected: same problem;
emitters are on the hot path. Tail-after-success is the only
location where the dictation contract is already fulfilled.

### D7 — Concept pages = SQL VIEWs only

Two SQL VIEWs land in 1B:

- `kg_concept_entities_view` — entries grouped by entity, ordered
  by most-recent mention. Powers a future "concept page" UI (Phase
  1C) where the user navigates to e.g. "Mom" and sees every
  dictation that mentioned her.
- `kg_concept_tags_view` — entries grouped by canonical tag,
  ordered by most-recent mention. Powers the same UX axis for tags.

Cross-entity co-occurrence views (e.g. "Mom × Dad: entries that
mention both") are **deferred to Phase 1C** alongside the retrieval
UX they'd power.

**Rationale.** ADR 0049 §6 binding: "files-as-source-of-truth +
vault subtree + positional routing — inherited unchanged from
Phase 0." Concept pages as VIEWs (computed each query against the
mention tables) preserve this: the entries themselves remain the
source of truth; concept pages are projections, not stored
entities. VIEWs cost nothing at write time; the read cost is paid
when the UI is built (Phase 1C), at which point we can promote to
indexed materializations IF the read latency surfaces as a real
issue. YAGNI applies — ship the cheapest correct shape now.

**Considered alternatives.** Stored concept-page rows updated on
each new mention. Rejected: introduces write amplification + a new
maintenance burden + a source-of-truth ambiguity. Stored only as
VIEWs (no rows): the chosen shape.

### D8 — Acceptance gate: extended `kg_parity` probe `--persist` mode + graph-off invariant judge

Two gates fire at Chunk 5 seal:

1. **Extended `kg_parity --persist` mode.** The existing
   `src-tauri/src/bin/kg_parity.rs` probe gains a `--persist` flag.
   When passed, it round-trips every fixture dictation through the
   new `kg::store::*` layer using an in-memory SQLite connection.
   Assertions: (a) `PipelineResult` equality (the existing 1A
   contract), (b) `kg_entity_mentions` row counts match expected
   per-fixture entity counts, (c) `kg_tag_mentions` row counts
   match expected per-fixture tag counts (zero in 1B — tags-half
   is open-vocab; canonical-tag wiring lands later). All 32
   fixtures must pass.
2. **`kg-graph-off-untouched` invariant judge.** A deterministic
   test (not LLM-graded) runs the dictation orchestrator end-to-end
   with `KgGraphEnabled = false` against a baseline captured at
   Chunk 5 start, asserts byte-identical `PipelineResult` + DB
   write-set (sessions, transcripts rows). Mirrors the Phase MC
   `mc-dictation-untouched` pattern. Principal gate; non-negotiable.

**Rationale.** D8 is structured per LESSONS PINNED P9 (IAP split):
the graph-off invariant is **strict no-regression** (trust-critical;
the binding mission-cohesion commitment); the entity-quality side
of the parity probe is **Pareto-frontier** (quality metric, can
trade off). Both flow through the same Chunk 5 seal procedure.

**Considered alternatives.** Live integration tests under
`cargo test --release`. Rejected per LESSONS PINNED P2 — the
test runner is broken on this box. Binary probe + throwaway-crate
for pure-Rust modules is the sanctioned fallback. Five-judge bundle.
Rejected: only the graph-off invariant has the trust-critical
profile that wants strict IAP; the others (idempotency,
non-regression on failure) are cleanly covered by the deterministic
parity probe + UNIQUE constraints. A single one-off judge for the
one invariant fits per AGENTS.md "Work sizing" judges-when rule
("a single one-off judge for a single bead is fine when the
invariant is narrow").

---

## DB schema (DDL — canonical)

This block is the canonical record of the 1B schema. Migration 024
will replicate it byte-for-byte; if the two ever diverge, **this
ADR wins**, and a new migration is authored to reconcile.

FK target verified: `sessions.id INTEGER PRIMARY KEY` per
`src-tauri/src/db/migrations/001_initial.sql` line 110.

```sql
-- 024_kg_phase_1b.sql
-- KG Phase 1B (ADR 0050). Schema_version 23 → 24.
--
-- Adds the persistence half of the KG subsystem:
--
--   kg_entities                — first-class entity rows
--   kg_canonical_tags          — closed-vocab tag rows (v1.1 inert in 1B)
--   kg_entity_mentions         — per-segment entity provenance (D1)
--   kg_tag_mentions            — per-segment tag provenance (D1)
--   kg_filing_queue            — async filing queue (D3)
--   kg_concept_entities_view   — entries-by-entity computed view (D7)
--   kg_concept_tags_view       — entries-by-tag computed view (D7)
--
-- Plus immutability triggers on the two mention tables (per AGENTS.md
-- Principle 1 analog — mentions are extracted provenance and must not
-- be edited in place; reconciliation flows through DELETE + re-INSERT)
-- and the seed row for KgGraphEnabled = false (D4).
--
-- Audit triggers count delta: +2 (mention tables × 1 BEFORE UPDATE each).
--
-- ADR refs: 0049 (Phase 0.5 + v1 pivot), 0050 (this charter).
-- Principle 1 (raw immutability): unaffected — only the sessions/
-- transcripts side of the principle applies; the mention tables are
-- a derived analog whose immutability is enforced here.
-- Principle 2 (provenance total): per-segment storage is what makes
-- this principle hold for the KG layer.
-- Principle 4 (no telemetry): all KG state is local-only. No code
-- path exfiltrates anything.

PRAGMA foreign_keys = ON;

BEGIN TRANSACTION;

-- =============================================================================
-- kg_entities — one row per canonical entity (Mom, Acme Corp, etc.).
-- =============================================================================
CREATE TABLE kg_entities (
  id            INTEGER PRIMARY KEY,
  name          TEXT NOT NULL,                    -- canonical surface form
  entity_type   TEXT NOT NULL,                    -- 'person'|'organization'|'project'|'location'|'thing'
  aliases_json  TEXT NOT NULL DEFAULT '[]',       -- JSON array of alternate surfaces
  created_at    TEXT NOT NULL,
  updated_at    TEXT NOT NULL,
  UNIQUE(name, entity_type)
);

CREATE INDEX idx_kg_entities_type ON kg_entities(entity_type, name);

-- =============================================================================
-- kg_canonical_tags — closed-vocab semantic tags (v1.1 starting point).
-- =============================================================================
-- In 1B this table is created but unpopulated. The two-field schema
-- amendment A2 makes the `tags:` field open-vocab in v1; closed-vocab
-- wiring activates in v1.1 after corpus re-labeling per LESSONS P11.
-- The table exists in 1B so 1C/1D don't need a migration redirect.
CREATE TABLE kg_canonical_tags (
  id           INTEGER PRIMARY KEY,
  slug         TEXT NOT NULL UNIQUE,              -- e.g. 'car-repair'
  display_name TEXT NOT NULL,                     -- e.g. 'Car Repair'
  category     TEXT,                              -- optional grouping; nullable
  created_at   TEXT NOT NULL
);

-- =============================================================================
-- kg_entity_mentions — per-segment entity provenance (D1).
-- =============================================================================
CREATE TABLE kg_entity_mentions (
  id            INTEGER PRIMARY KEY,
  entry_id      INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  entity_id     INTEGER NOT NULL REFERENCES kg_entities(id) ON DELETE CASCADE,
  segment_idx   INTEGER NOT NULL,                 -- pipeline segment index, 0-based
  surface_form  TEXT NOT NULL,                    -- exact text the model emitted
  created_at    TEXT NOT NULL,
  UNIQUE(entry_id, segment_idx, entity_id)        -- idempotency (per principal invariant kg-filing-idempotent)
);

CREATE INDEX idx_kg_entity_mentions_entry  ON kg_entity_mentions(entry_id);
CREATE INDEX idx_kg_entity_mentions_entity ON kg_entity_mentions(entity_id, entry_id);

-- =============================================================================
-- kg_tag_mentions — per-segment tag provenance (D1).
-- =============================================================================
-- In 1B `tag_slug` is the open-vocab string the model emitted; foreign
-- key to kg_canonical_tags is NULLable so open-vocab tags land cleanly
-- (the canonical-tag join activates in v1.1 once the table is populated).
CREATE TABLE kg_tag_mentions (
  id              INTEGER PRIMARY KEY,
  entry_id        INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  canonical_tag_id INTEGER REFERENCES kg_canonical_tags(id) ON DELETE SET NULL,
  segment_idx     INTEGER NOT NULL,
  tag_slug        TEXT NOT NULL,                  -- open-vocab string (v1 primary key)
  created_at      TEXT NOT NULL,
  UNIQUE(entry_id, segment_idx, tag_slug)
);

CREATE INDEX idx_kg_tag_mentions_entry ON kg_tag_mentions(entry_id);
CREATE INDEX idx_kg_tag_mentions_slug  ON kg_tag_mentions(tag_slug, entry_id);

-- =============================================================================
-- kg_filing_queue — async filing queue (D3).
-- =============================================================================
-- FIFO state machine: pending → processing → done | failed.
-- The worker reaps `done` rows older than 30 days on startup (failure
-- rows are kept forever in 1B; Phase 1C surfaces a failures UI).
CREATE TABLE kg_filing_queue (
  id              INTEGER PRIMARY KEY,
  entry_id        INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  state           TEXT NOT NULL,                  -- 'pending'|'processing'|'done'|'failed'
  enqueued_at     TEXT NOT NULL,
  processing_started_at TEXT,                     -- set when state→processing
  finished_at     TEXT,                           -- set when state→done|failed
  attempt_count   INTEGER NOT NULL DEFAULT 0,
  last_error      TEXT,                           -- diagnostic message on failure
  UNIQUE(entry_id)                                -- one queue row per entry; re-enqueue is a no-op
);

CREATE INDEX idx_kg_filing_queue_state ON kg_filing_queue(state, enqueued_at);

-- =============================================================================
-- Concept-page VIEWs (D7).
-- =============================================================================
-- These are projections of the mention tables. NOT stored — recomputed
-- on each SELECT. Phase 1C will index/materialize ONLY if production
-- latency surfaces an issue.

CREATE VIEW kg_concept_entities_view AS
SELECT
  e.id                       AS entity_id,
  e.name                     AS entity_name,
  e.entity_type              AS entity_type,
  m.entry_id                 AS entry_id,
  MIN(m.segment_idx)         AS first_segment_idx,
  COUNT(*)                   AS mention_count,
  MAX(m.created_at)          AS most_recent_mention_at
FROM kg_entities e
JOIN kg_entity_mentions m ON m.entity_id = e.id
GROUP BY e.id, m.entry_id;

CREATE VIEW kg_concept_tags_view AS
SELECT
  m.tag_slug                 AS tag_slug,
  c.id                       AS canonical_tag_id,
  c.display_name             AS canonical_display_name,
  m.entry_id                 AS entry_id,
  MIN(m.segment_idx)         AS first_segment_idx,
  COUNT(*)                   AS mention_count,
  MAX(m.created_at)          AS most_recent_mention_at
FROM kg_tag_mentions m
LEFT JOIN kg_canonical_tags c ON c.id = m.canonical_tag_id
GROUP BY m.tag_slug, m.entry_id;

-- =============================================================================
-- Immutability triggers on mention tables (Principle 1 analog).
-- =============================================================================
-- Mention rows are extracted provenance: once written, they are not
-- edited in place. Reconciliation (e.g. re-filing an entry after a
-- pipeline change) flows through DELETE + re-INSERT, so the audit
-- trail of "what the model said when" is preserved.

CREATE TRIGGER kg_entity_mentions_no_update
BEFORE UPDATE ON kg_entity_mentions
BEGIN
  SELECT RAISE(ABORT, 'kg_entity_mentions is write-once (Principle 2 / ADR 0050)');
END;

CREATE TRIGGER kg_tag_mentions_no_update
BEFORE UPDATE ON kg_tag_mentions
BEGIN
  SELECT RAISE(ABORT, 'kg_tag_mentions is write-once (Principle 2 / ADR 0050)');
END;

-- =============================================================================
-- Seed: KgGraphEnabled = false (D4).
-- =============================================================================
-- INSERT OR IGNORE so re-running the migration (test harness) is
-- idempotent. The actual default lives in `SettingKey::default_value`
-- in Rust; this row exists so the Settings UI in Phase 1C has
-- something to bind to even before the user toggles the value.
INSERT OR IGNORE INTO settings (key, value)
VALUES ('kg_graph_enabled', 'false');

UPDATE schema_meta SET value = '24' WHERE key = 'schema_version';

COMMIT;
```

---

## Dictation-surface authorization clause

**This section mirrors ADR 0037 §5's shape. Outside this list, the
seal on `src-tauri/src/dictation/**` continues to hold.**

The ONE allowed edit:

| File | Surgical change | Why it's surgical (not a redesign) |
|---|---|---|
| **`src-tauri/src/dictation/runtime.rs`** (or the orchestrator's tail call site; exact file TBD in Chunk 4) | ONE call-site insertion: `let _ = kg::enqueue_for_filing(&conn, entry_id, ...);` on the orchestrator's tail, **after** clipboard injection succeeded **and** the session row is finalized. Ignore-error semantics: `Err(_)` is logged at `tracing::warn!` and discarded. Gated by `KgGraphEnabled` check inside `enqueue_for_filing` (returns Ok(()) at function top when off). | One let-binding. No state-machine change. No new function signature. No control-flow alteration on the dictation outcome path. The 383 dictation tests stay green. |

**NOT authorized by this ADR:**

- Any other edits to `src-tauri/src/dictation/**` (the rest of the
  module stays sealed per ADR 0036/0037 boundary discipline).
- Any edits to `src-tauri/src/meetings/**` — Meeting Capture remains
  sealed at `phase-mc-complete`. KG filing on meeting transcripts is
  out of scope (Phase 1C+ may revisit).
- Any UI surface — Settings → Knowledge Graph, retrieval pages, opt-in
  activation UX are all Phase 1C.
- Any changes to the `transcripts` table — Principle 1 (raw
  transcript immutability) is non-negotiable; the KG mention tables
  are extracted derivatives that reference `sessions(id)`, never the
  raw transcript rows.
- Any changes to `src-tauri/src/activity/**`, `src-tauri/src/command_center/**`,
  `src-tauri/src/cleanup/**`, `src-tauri/src/injection/**`,
  `src-tauri/src/hotkey/**` — outside the boundary.
- Any new IPC commands beyond the existing typed-settings IPC reading
  `KgGraphEnabled`. The runtime toggle UX is Phase 1C.

**If Chunk 2/3/4 discovers it needs a touch outside this list**, that
is a successor-ADR amendment to 0050 — not a unilateral expansion.
The 5-attempt rule applies; surface via STATUS.md "Blocked / human
input needed" + a beads issue tagged `escalation`.

---

## Invariants (Chunk 5 gate captures)

These are the binding invariants Chunk 5's gate procedure must
exercise. They are recorded here so future-session anchoring is
self-contained.

### `kg-graph-off-untouched` (principal invariant)

With `KgGraphEnabled = false`, the dictation orchestrator's
observable outcome is **byte-identical** to the pre-1B baseline:

- `PipelineResult` shape + values unchanged
- DB write-set unchanged (sessions row, transcripts rows, `edit_free_within_5min` instrumentation)
- No rows written to `kg_*` tables
- No worker thread spawned at boot

Captured via a deterministic test, NOT LLM-graded. Mirrors the
Phase MC `mc-dictation-untouched` pattern (`docs/judges/phase-mc/`).
Trust-critical → **strict IAP** discipline per LESSONS P9.

### `kg-filing-idempotent`

Enqueuing the same `entry_id` twice produces ONE filed result, not
duplicates. Enforced at the schema layer by:

- `kg_filing_queue UNIQUE(entry_id)` — re-enqueue collapses to one row
- `kg_entity_mentions UNIQUE(entry_id, segment_idx, entity_id)` — re-file collapses to existing rows
- `kg_tag_mentions UNIQUE(entry_id, segment_idx, tag_slug)` — same

Tested via the extended `kg_parity --persist` mode (Chunk 5): run
each fixture twice through the in-memory store, assert row counts
match single-run expectations.

### `kg-graph-failure-non-regressing`

If `kg::enqueue_for_filing` raises (any error path), the dictation
tail completes normally. Captured by the ignore-error semantics in
the hook (D6) + a unit test on the call site that asserts a panicking
mock `enqueue_for_filing` does not propagate.

---

## Out of scope (Phase 1C+)

Pre-recorded so future sessions don't accidentally pull these into
1B:

- **UI surface** — Settings → Knowledge Graph toggle, retrieval
  pages (six axes per ADR 0049 §7 Wave 1C), opt-in activation UX,
  failed-filings UI surface. All Phase 1C.
- **Backfill of pre-1B entries** — one-shot job classifying +
  tagging + entity-extracting historical dictations. Phase 1D.
- **Cross-entity co-occurrence view** — "entries that mention both
  Mom AND Dad" projection. Phase 1C alongside retrieval UX.
- **Empirical latency-budget measurement** — the ADR 0049 §6
  "~1 min intake latency budget" target. Standing P2 bead (will
  spin up in 1C when there's real traffic to measure). NOT
  gating 1B seal.
- **Reaping policy for `kg_filing_queue` done rows beyond the 30-day
  TTL** — Phase 1C will surface a user-visible failure-counts UI;
  in 1B we just sweep done > 30 days on startup. Failure rows are
  retained forever in 1B.
- **Meeting-capture filing** — KG hook on the Meeting Capture tail
  is explicitly NOT in scope. MC remains sealed. Phase 1C may
  revisit.
- **Worker-thread runtime toggle** — flipping `KgGraphEnabled` at
  runtime should start/stop the worker thread without restart. The
  hooks for this land in 1B (the boot-time conditional spawn) but
  the actual runtime toggle wiring is Phase 1C.

---

## Supersession / relationship to other ADRs

- **Extends** ADR 0049 (does not replace). ADR 0049 §6 v1 binding
  commitments are inherited verbatim; amendments A1/A2/A3 carry
  forward. ADR 0049 §"Sandbox isolation" is extended with the
  Phase 1B exception window (open from this ADR's Proposed date;
  closes at Chunk 5 seal when this ADR moves Accepted).
- **References** ADR 0037 — sealed-surface authorization pattern
  (the boundary table in §5 is the shape this ADR's
  "Dictation-surface authorization clause" mirrors).
- **References** ADR 0036 + ADR 0040 — sub-charter ADR under a
  parent epic pattern. 0050 is to 0049 what 0040 is to 0036.
- **References** ADR 0010 — Principle 1 enforcement model. The
  immutability triggers on `kg_entity_mentions` + `kg_tag_mentions`
  are a Principle 2 (provenance total) analog of the Principle 1
  triggers ADR 0010 codified for `transcripts`.

---

## Acceptance criteria

This ADR moves to **Accepted** when:

1. Migration 024 lands and the schema matches the canonical DDL
   block above byte-for-byte (Chunk 2).
2. `src-tauri/src/kg/store/*` ships per §D2 (Chunk 2).
3. `SettingKey::KgGraphEnabled` is wired with default `false`
   (Chunk 2).
4. The worker thread spawns conditionally at boot per D3/D4
   (Chunk 3).
5. The ONE dictation-tail call site lands within the authorized
   boundary (Chunk 4).
6. Extended `kg_parity --persist` mode green at 32/32 (Chunk 5).
7. `kg-graph-off-untouched` invariant judge green (Chunk 5).
8. Cargo gate green via the wrapper (per LESSONS P2 fallback for
   the `test --release` step).
9. STATUS.md "Currently active" cleared; "Sealed lateral epics"
   block updated with the 1B seal entry.
10. PRODUCT-STATE.md §3.19 updated to reflect persistence + worker
    + dictation hook + the default-off binding.
11. Epic bead `mb-bjni` closed with resolution referencing the seal
    commit + this ADR.

**NO new `phase-*-complete` tag** — lateral epic per LESSONS PINNED
P5. Seals via ADR Accepted + STATUS update + epic bead close.

---

## References

- [`docs/knowledge-graph/phase-1b-brief.md`](../knowledge-graph/phase-1b-brief.md) — wave brief (this iteration)
- [`docs/knowledge-graph/PHASE-0-5-REPORT.md`](../knowledge-graph/PHASE-0-5-REPORT.md) — §6 v1 binding commitments + §7 Wave 1B row
- [`docs/knowledge-graph/parity/README.md`](../knowledge-graph/parity/README.md) §3 — per-segment vs per-dictation entity provenance (D1 anchor)
- ADR 0049 — Phase 0.5 + v1 architectural pivot (parent epic)
- ADR 0037 — Unified Recording Command Center (sealed-surface authorization pattern precedent)
- ADR 0036 + ADR 0040 — sub-charter ADR under parent epic pattern
- ADR 0010 — raw transcript immutability (Principle 1 enforcement model)
- LESSONS PINNED **P4** — session-start ritual (kickoff anchoring discipline)
- LESSONS PINNED **P5** — lateral epics seal via ADR + STATUS, NOT via `phase-*-complete` tags
- LESSONS PINNED **P9** — strict IAP on trust-critical gates, Pareto-frontier on quality metrics (D8 split)
- LESSONS PINNED **P11** — "tags ≠ entities" (D1 + the two mention tables)
- AGENTS.md "Permanently sealed" — kickoff stale-prompt discipline
- AGENTS.md "Work sizing & workflow selection" — container choice rationale (ADR-chartered lateral epic)
- AGENTS.md "Build / run / test environment (Windows)" — cargo wrapper

---

_The `adr-format` judge validates this structure exists in every numbered ADR. Keep section headings stable._
