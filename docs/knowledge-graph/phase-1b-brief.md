# Phase 1B Wave Brief — SQLite persistence + async filing queue + dictation-tail hook (default-off)

**Bead epic:** `mb-bjni`
**Charter:** [ADR 0049](../adr/0049-knowledge-graph-phase-0-5-and-v1-architectural-pivot.md)
§7 (Wave 1B row) + §6 (v1 binding commitments)
+ [ADR 0050](../adr/0050-kg-phase-1b-persistence-and-dictation-hook.md)
(sub-charter — schema + dictation-surface authorization).
**Work container:** ADR-chartered lateral epic under ADR 0049's
§"Sandbox isolation" exception-window mechanism (per AGENTS.md
"Work sizing"). Sub-charter pattern mirrors ADR 0036 → ADR 0040.
**No new `phase-*-complete` tag** (LESSONS P5).
**Cadence:** five dispatches (chunks).

| Chunk | Beads | Deliverable |
|---|---|---|
| 1 (this) | `mb-go9l` | ADR 0050 + this brief + epic bead chain + STATUS in-flight update. |
| 2 | `mb-geds` | Migration `024_kg_phase_1b.sql` + `src-tauri/src/kg/store/{mod,entities,tags,mentions,queue}.rs` + `SettingKey::KgGraphEnabled`. |
| 3 | `mb-eke8` | `src-tauri/src/kg/worker.rs` (FIFO drainer, std::thread, crash-recovery sweep) + boot wiring (spawn iff `KgGraphEnabled = true`). |
| 4 | `mb-ryq4` | ONE call-site insertion in the dictation orchestrator tail per ADR 0050 §"Dictation-surface authorization clause". |
| 5 | `mb-k17a` | Extended `kg_parity --persist` mode + `kg-graph-off-untouched` invariant judge + ADR 0050 Status → Accepted + STATUS seal + PRODUCT-STATE §3.19 update + epic close. |

---

## §1. Scope: what lands

The 1B deliverable is the **persistence + worker + ONE dictation-tail
call site**. End state: `kg::` graduates from "callable library with
zero consumers" (Phase 1A end state) to "library with persistence,
async worker, and exactly one production caller, all gated default-off
behind `KgGraphEnabled`."

| Artifact | Notes |
|---|---|
| `src-tauri/src/db/migrations/024_kg_phase_1b.sql` | Migration with 4 tables + 2 views + 2 audit triggers + seed row + `schema_meta` bump 23→24. DDL is canonical in ADR 0050 §"DB schema". |
| `src-tauri/src/kg/store/mod.rs` | Re-exports + `pub fn enqueue_for_filing(conn, entry_id, ...) -> AppResult<()>` (the dictation-tail entry point). |
| `src-tauri/src/kg/store/entities.rs` | CRUD against `kg_entities`. Includes `find_or_create(name, type)` for the upsert path the filing worker drives. |
| `src-tauri/src/kg/store/tags.rs` | CRUD against `kg_canonical_tags`. In 1B the table is created but unpopulated; module compiles + has unit tests, no production callers beyond the worker's optional join. |
| `src-tauri/src/kg/store/mentions.rs` | INSERT helpers for `kg_entity_mentions` + `kg_tag_mentions` (per-segment provenance per D1). |
| `src-tauri/src/kg/store/queue.rs` | State-machine helpers for `kg_filing_queue` (`enqueue`, `claim_next`, `mark_done`, `mark_failed`, `reset_processing_on_startup`). |
| `src-tauri/src/kg/worker.rs` | std::thread FIFO drainer. Crash-recovery sweep (`processing` → `pending`) at startup. Calls `kg::run_pipeline` against the configured `OllamaDispatcher`, persists outcome via the store layer. 30-day TTL sweep on done rows at startup. |
| `src-tauri/src/kg/pipeline.rs` (extended, **Chunk 3 amendment**) | `extract_entities` wired as the **5th pipeline pass** between `extract` and assemble (ADR 0049 §6 pipeline order). New additive `PipelineResult.segment_entities: Vec<SegmentEntities>` carries per-segment entity outputs through to the worker's `apply_filed_outcome` consumer. Failure on the entity pass is isolated — the segment's `Entry` still ships with `entities: Vec::new()`. Phase 1A's parity probe stays 32/32 GREEN by construction (the probe's `pipeline_result_to_value` manually emits only the original three keys; the new field is invisible). **This wiring was implicit in §1's table but absent in Phase 1A's `run_pipeline` — Chunk 3 closed the gap; ADR 0049 §6 binding is now enforced in production.** |
| `src-tauri/src/settings/model.rs` (additive) | `SettingKey::KgGraphEnabled`, default `false`. `default_value` + `as_str` + `try_parse` rows added. |
| `src-tauri/src/dictation/` (one file, one line) | The single call-site insertion authorized by ADR 0050 §"Dictation-surface authorization clause". Chunk 4 picks the exact file (probably `runtime.rs`) based on where the orchestrator tail lives at that point. |
| `src-tauri/src/bin/kg_parity.rs` (extended) | New `--persist` CLI flag. Round-trips fixtures through `kg::store::*` against an in-memory SQLite connection. |
| `tests/kg_graph_off_untouched.rs` (or equivalent location) | Deterministic invariant test: dictation orchestrator end-to-end with `KgGraphEnabled = false` is byte-identical to a Chunk-5-captured baseline. |

---

## §2. Scope: what does NOT land

These are explicit Phase 1C+ deferrals, recorded here so future
sessions don't accidentally absorb them into 1B:

- **UI surface** — Settings → Knowledge Graph toggle, retrieval pages
  (6 axes per ADR 0049 §7 Wave 1C), opt-in activation UX, failed-filings
  UI. All Phase 1C.
- **Backfill** of pre-1B entries. Phase 1D.
- **Cross-entity co-occurrence view** (e.g. "entries mentioning both
  Mom AND Dad"). Phase 1C.
- **Empirical latency-budget measurement** — the ADR 0049 §6
  "~1 min intake latency budget" target. Standing P2 bead, NOT
  gating 1B seal.
- **Worker-thread runtime toggle** (live start/stop on `KgGraphEnabled`
  changes without app restart). Phase 1C wires the IPC; 1B only
  handles boot-time conditional spawn.
- **Meeting-capture filing.** MC remains sealed at `phase-mc-complete`.
  Phase 1C may revisit.

---

## §3. Out of scope (Phase 1C+)

Restated here in brief-shape mirror to `phase-1a-brief.md` §3 for
future-session anchoring. **If a Phase 1B diff reaches into `ui/**`,
that's a Phase 1C-scope leak — escalate via STATUS and a bead,
do not push through.**

The Phase 1A seal's "no consumers wired yet — that's Phase 1C"
caveat partially closes in Phase 1B: the ONE dictation-tail call
site is the first consumer. **Everything else** stays Phase 1C or
later.

---

## §4. Binding decisions (D1–D8, confirmed by Dustin at kickoff)

Same shape as 1A's brief §4. Full rationale + considered alternatives
live in ADR 0050 §"Decision".

| ID | Decision | Rationale (one line) |
|---|---|---|
| **D1** | Provenance is **per-segment**. `kg_entity_mentions` + `kg_tag_mentions` carry `(entry_id, segment_idx, ...)` UNIQUE. Per-dictation rollups via GROUP BY in views. | Pipeline already emits per-segment shape. AGENTS.md Principle 2 makes per-segment the conservative default. Aggregate can be materialized from per-segment; not vice versa. |
| **D2** | DB layer = `src-tauri/src/kg/store/` (subsystem-internal). | Mirrors `activity/persist.rs` precedent. Cohesion beats one-shared-`db/`-dir. |
| **D3** | Filing queue = persisted SQLite `kg_filing_queue` table + dedicated `std::thread` worker, FIFO. Crash-recovery sweep `processing` → `pending` at startup. | Activity-runtime pattern. AGENTS.md "no tokio". On-disk = crash-tolerant + inspectable + future-failures-UI surface. |
| **D4** | Settings gate = `SettingKey::KgGraphEnabled`, default `false`. Migration 024 seeds the row. Worker doesn't spawn at boot when off (resource saving). | ADR 0049 §6 binding opt-in commitment. Default-off makes D8's `kg-graph-off-untouched` invariant the trivial case. |
| **D5** | Migration granularity = **one** file `024_kg_phase_1b.sql`. Section-banner comments mirror `012_activity_capture.sql` style. | One seal = one migration = one schema_meta bump = atomic. 011/012 set the precedent. |
| **D6** | Dictation hook insertion = post-paste, post-success, on the orchestrator's tail. ONE call site. Ignore-error semantics. Gated by `KgGraphEnabled` check inside `enqueue_for_filing` (Ok(()) at top when off). | The only location where the dictation contract is already fulfilled. No state-machine touch. |
| **D7** | Concept pages = SQL VIEWs only in 1B. Two views: `kg_concept_entities_view` + `kg_concept_tags_view`. Cross-entity co-occurrence deferred to 1C. | Preserves files-as-source-of-truth (ADR 0049 §6 binding). YAGNI on materialization until Phase 1C surfaces read-latency need. |
| **D8** | Acceptance gate = extended `kg_parity --persist` mode (round-trip through `kg::store::*` over in-memory SQLite, all 32 fixtures) + `kg-graph-off-untouched` invariant judge (deterministic test vs Chunk-5-captured baseline). | LESSONS P9 split: strict IAP on the trust-critical invariant; Pareto-frontier on quality metrics inside parity. |

---

## §5. Acceptance gate procedure (Chunk 5)

### Step 1 — Capture pre-1B baseline (start of Chunk 5)

Before any Chunk-5 code changes, capture a deterministic run of the
dictation orchestrator against a fixed input + a fixed mock Whisper
+ mock LLM. Record the resulting `PipelineResult` JSON + the
sessions/transcripts row diff. This becomes the "Chunk-5-captured
baseline" referenced by D8.

### Step 2 — Extended `kg_parity --persist` mode

`powershell -File scripts\cargo-with-cuda.ps1 run --release --bin kg_parity -- --persist`

For each of the 32 Wave 0.5.4 seed-42 fixture dictations:

1. Open an in-memory SQLite connection (`rusqlite::Connection::open_in_memory`).
2. Run migrations 001..024 to materialize the schema.
3. Insert a synthetic `sessions` row for the fixture's `dictation_id` (the FK target).
4. Call `kg::run_pipeline` against the fixture-scripted MockOllama (same dispatcher 1A's probe uses).
5. Call `kg::store::enqueue_for_filing` + drive the worker's filing logic synchronously (test-only `drain_now` helper on the worker).
6. Assert:
   - `PipelineResult` equality (existing 1A contract; regression check).
   - `kg_entity_mentions` row count = sum of per-segment entity counts for this fixture.
   - `kg_tag_mentions` row count = sum of per-segment tag counts for this fixture.
   - `kg_filing_queue` final state = `done` for this entry.
7. Run steps 4–6 a second time (idempotency check for `kg-filing-idempotent`). Row counts must not double.

Exit non-zero on any divergence; print a unified diff so the failure
mode is human-debuggable. All 32 fixtures must pass.

### Step 3 — `kg-graph-off-untouched` invariant test

Deterministic test (NOT LLM-graded). Run the dictation orchestrator
end-to-end with `KgGraphEnabled = false` against the same fixed input
+ mocks used in Step 1. Assert the resulting `PipelineResult` JSON +
sessions/transcripts row diff are **byte-identical** to the Chunk-5
baseline. The test also asserts `kg_*` tables have zero rows post-run
+ no worker thread spawned at boot.

Strict-IAP discipline: any byte-level divergence = HALT, root-cause,
do not paper over.

### Step 4 — Cargo gate (per LESSONS P2 fallback)

`powershell -File scripts\cargo-with-cuda.ps1 fmt --check`
`powershell -File scripts\cargo-with-cuda.ps1 clippy --release -- -D warnings`
`powershell -File scripts\cargo-with-cuda.ps1 test --release --no-run` (link-only proof)

For pure-Rust modules with no whisper-rs/ort/CUDA deps (`kg::store::*`
likely qualify), use the throwaway-crate recipe (LESSONS 2026-05-17)
for live test runs.

### Step 5 — Seal

- ADR 0050 Status → **Accepted**.
- STATUS.md "Currently active" cleared; "Sealed lateral epics" updated.
- PRODUCT-STATE.md §3.19 updated (persistence + worker + dictation hook + default-off binding).
- Epic bead `mb-bjni` closed with resolution citing seal commit hash.
- NO new `phase-*-complete` tag (LESSONS P5).

---

## §6. Public surface delta (D6 spelled out)

```rust
// src-tauri/src/kg/mod.rs (post-Chunk-4)
pub mod schema;
pub(crate) mod schema_loader;
pub(crate) mod passes;
pub(crate) mod ollama;
pub(crate) mod synonyms;
pub(crate) mod embeddings;
pub mod pipeline;
pub(crate) mod store;        // NEW in 1B
pub(crate) mod worker;       // NEW in 1B

pub use pipeline::{run_pipeline, PipelineResult};
pub use schema::{Entry, Category, EntryType, EntityType, Status, AnswerKey};
pub use store::enqueue_for_filing;   // NEW: the dictation-hook call site
// Possibly:
//   pub use store::queue::QueueRow;  // if the dictation tail wants to inspect outcome
// Decided in Chunk 2.
```

Everything else stays `pub(crate)`. YAGNI applies — Phase 1B does
not pre-export anything 1C might want.

---

## §7. Seal criteria

- [ ] Chunk 1 (this commit) — ADR 0050 Proposed, this brief present,
      epic bead chain present, STATUS.md "Currently active" updated,
      no code changes.
- [ ] Chunk 2 — Migration 024 lands matching ADR 0050 §"DB schema"
      DDL. `src-tauri/src/kg/store/*` ships per D2. `SettingKey::KgGraphEnabled`
      added. Cargo gate green via wrapper.
- [ ] Chunk 3 — `kg::worker` ships with crash-recovery sweep + 30-day
      done-row TTL. Boot wiring spawns iff `KgGraphEnabled = true`. Cargo
      gate green.
- [ ] Chunk 4 — ONE dictation-tail call site lands within the ADR 0050
      §"Dictation-surface authorization clause" boundary. Phase MC's
      five judges (especially `mc-dictation-untouched`) still pass.
- [ ] Chunk 5 — Extended `kg_parity --persist` green at 32/32.
      `kg-graph-off-untouched` invariant test green. ADR 0050 →
      Accepted. STATUS + PRODUCT-STATE updated. Epic closed.
- [ ] **No new ADR beyond 0050.** **No new `phase-*-complete` tag**
      (LESSONS P5 lateral epic).

---

## §8. Risk register

Re-derived from the planning-agent's plan. Each carries a mitigation;
the entries here are the consciously-accepted risks for the epic.

| # | Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|---|
| **R1** | Sealed dictation surface — the single authorized call-site insertion is the smallest possible touch, but any drift outside the boundary is an ADR 0050 §"Dictation-surface authorization clause" violation. | Low | High (Phase MC + dictation regression) | ADR 0050 §"Dictation-surface authorization clause" is the explicit boundary list. Chunk 4's diff must be reviewable as "one let-binding". Phase MC's `mc-dictation-untouched` judge still runs at Chunk 5 seal. |
| **R2** | Worker/crash-recovery interaction with the boot-time conditional spawn. If `KgGraphEnabled` is flipped from false → true via direct settings-table edit AND the app is restarted mid-filing, the recovery sweep needs to handle a clean `processing` → `pending` reset without colliding with the dictation-tail's fresh enqueue path. | Medium | Medium | Recovery sweep runs **before** worker thread spawns. Dictation-tail enqueue is idempotent on `(entry_id)` UNIQUE — a stale `processing` row that gets reset to `pending` will be re-claimed cleanly. Test in Chunk 3 throwaway-crate. |
| **R3** | Per-segment provenance threading through `PipelineResult`. The pipeline currently aggregates `entities: Vec<Entity>` at the dictation level; the store layer needs per-segment info. | Medium | Medium | The pipeline already runs `extract_entities` per-segment internally; Chunk 2's `kg::store::enqueue_for_filing` either (a) consumes a richer `PipelineResult` shape that exposes per-segment entity outputs, OR (b) re-runs the pass-level segmentation deterministically from the dictation text. Chunk 2 picks; (a) preferred. |
| **R3-resolved (Chunk 3)** | Resolution: option (a). `PipelineResult.segment_entities: Vec<SegmentEntities>` added additively. Parity-safe by construction — the `kg_parity` probe's `pipeline_result_to_value` builds the assertion JSON from three hardcoded keys (`entries`, `per_pass_errors`, `new_tag_requests`); a new field on the struct is invisible to it. **Empirically confirmed: 32/32 GREEN both before and after the wiring change (commit `45ac718`).** Chunk 3 also surfaced + closed an orthogonal gap: pre-Chunk-3 production `run_pipeline` was only 4 passes (segment/classify/extract/normalize); `extract_entities` was being run **standalone** by the parity probe but **never** by production callers. Chunk 3 wired it as the 5th pass per ADR 0049 §6. Without this, Chunk 2's `kg_entity_mentions` table would have been dead-on-arrival. | n/a (closed) | n/a (closed) | n/a (closed) |
| **R4** | `cargo test --release` runner is broken on this box (LESSONS P2). The fallback gate is `--no-run` + throwaway-crate. | High (already known) | Low (fallback exists) | Per LESSONS P2: `kg::store::*` is pure-Rust (rusqlite only); runs via throwaway-crate. `kg::worker` may transitively pull `kg::pipeline` → `kg::ollama` (ureq); test surface there is narrow + can be mocked. The `kg-graph-off-untouched` test is a binary probe NOT a `#[test]` (mirrors `kg_parity` precedent). |
| **R5** | Latency budget — per-segment storage costs ~5x row count vs per-dictation. For very long dictations (>100 segments), filing could exceed the ADR 0049 §6 ~1 min target. | Low (snapshot) → Medium (live use) | Medium | Standing P2 bead. NOT gating 1B seal. Phase 1C surfaces real production traffic to measure against; if real, mitigation is a denormalized rollup VIEW or materialized table — NOT discarding per-segment primitive. |
| **R6** | Migration 024 is the largest single migration in the project (4 tables + 2 views + 2 triggers + seed). Complexity → typo risk. | Medium | High (schema drift) | Canonical DDL lives in ADR 0050 §"DB schema". Chunk 2's migration replicates byte-for-byte. The schema-loader test harness exercises every CREATE on every test run. If ADR vs migration diverge in future, ADR wins; new migration reconciles. |
| **R7** | Queue lifetime — `failed` rows accumulate forever in 1B (no UI to surface them). Pre-1C user has no way to see what failed. | Low (1B) → Medium (1C wait) | Low | Accepted for 1B. `tracing::warn!` on every failure path means logs are inspectable. Phase 1C surfaces the failed-filings UI. Done-row 30-day TTL sweep prevents the success side from growing forever. |
| **R8** | Baseline-capture timing — the `kg-graph-off-untouched` test's baseline is captured at Chunk 5 start. If the dictation surface drifts between Chunk 5 start and Chunk 5 seal (e.g. an unrelated dictation refactor lands on `main`), the baseline goes stale. | Low | Medium | Capture baseline as the FIRST action of Chunk 5. Document the baseline commit hash in the test fixture's header comment. If a dictation refactor lands mid-Chunk-5, re-capture and document the reason. |

---

## §9. Standing-bead candidates (carry forward to seal report)

Flag these at Chunk 5 seal for promotion to standing P2/P3 beads:

- **R5 latency budget** — empirical measurement against live production
  traffic. Phase 1C work. P2 standing bead at seal.
- **R7 failed-filings UI surface** — Phase 1C deliverable; track as a
  P2 bead linked from Phase 1C epic.
- **30-day done-row TTL reaping** — confirm in production the policy is
  right; if user complains "I lost my filing history", revisit. P3
  standing bead.

---

## §10. References

- **ADR 0050** — sub-charter (this epic's binding decisions + DDL).
- **ADR 0049** — Phase 0.5 + v1 architectural pivot (parent epic).
  Specifically §6 (v1 binding commitments) + §7 (Wave 1B row) +
  §"Sandbox isolation" close-out (Phase 1A graduation window closed;
  1B opens its own).
- **`docs/knowledge-graph/PHASE-0-5-REPORT.md`** §6 + §7.
- **ADR 0037** — sealed-surface authorization language precedent
  (the boundary table shape ADR 0050 §"Dictation-surface authorization
  clause" mirrors).
- **ADR 0036 + ADR 0040** — sub-charter ADR under a parent epic
  pattern precedent.
- **ADR 0010** — raw transcript immutability; Principle 1 enforcement
  model the audit triggers on mention tables follow.
- **`docs/knowledge-graph/phase-1a-brief.md`** — shape template for
  this brief.
- **`docs/knowledge-graph/parity/README.md`** §3 — per-segment vs
  per-dictation entity provenance (D1 anchor).
- **LESSONS PINNED P4** — session-start ritual (stale-prompt
  triage discipline).
- **LESSONS PINNED P5** — lateral epics seal via ADR + STATUS, NOT
  via `phase-*-complete` tags.
- **LESSONS PINNED P9** — strict IAP on trust-critical gates,
  Pareto-frontier on quality metrics (D8 split).
- **LESSONS PINNED P11** — "tags ≠ entities" (D1 + the two-mention-tables design).
- **LESSONS 2026-05-17** — `cargo test --release` STATUS_ENTRYPOINT_NOT_FOUND
  fallback gate (Chunk 5 step 4).
- **AGENTS.md "Work sizing & workflow selection"** — container choice
  rationale (ADR-chartered lateral epic).
- **AGENTS.md "Build / run / test environment (Windows)"** — cargo
  wrapper requirements.
- **AGENTS.md "Permanently sealed"** — kickoff stale-prompt discipline.
