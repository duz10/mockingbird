# Phase 1A Wave Brief — Schema-driven KG pipeline graduates to `src-tauri/src/kg/`

**Bead epic:** `mb-2mc9`
**Charter:** [ADR 0049](../adr/0049-knowledge-graph-phase-0-5-and-v1-architectural-pivot.md)
+ [`PHASE-0-5-REPORT.md`](./PHASE-0-5-REPORT.md) §6 (v1 commitments)
+ §7 (Wave 1A row).
**Work container:** ADR-chartered lateral epic (per AGENTS.md "Work sizing"
table). **No new ADR.** **No `phase-*-complete` tag.** Seals via ADR 0049
"Sandbox isolation" update + STATUS.md update + epic bead close.
**Cadence:** three dispatches.

| Chunk | Beads | Deliverable |
|---|---|---|
| 1 (this) | `mb-8thh`, `mb-ep3c`, `mb-fl9y` | Parity fixture captured, `src-tauri/src/kg/` scaffold present, this brief written. |
| 2 | TBD | Library subset graduates from `experimental/kg-validation/src/` into `src-tauri/src/kg/`. `anyhow` → `thiserror`. `ureq::Agent`. `include_str!` with env override. |
| 3 | TBD | `kg_parity` probe binary lands and is green. ADR 0049 + STATUS sealed. Epic bead closed. |

---

## §1. Scope: what graduates

The **library subset** of `experimental/kg-validation/src/` lands in
`src-tauri/src/kg/`. Specifically (binding parameter D5):

| Sandbox path | Production path | Notes |
|---|---|---|
| `src/schema.rs` | `src-tauri/src/kg/schema.rs` | `Entry`, `Category`, `EntryType`, `EntityType`, `Status`, `AnswerKey`. |
| `src/schema_loader.rs` | `src-tauri/src/kg/schema_loader.rs` | SCHEMA.md + prompt parser. Rewired to `include_str!` (see D2). |
| `src/passes/{segment,classify,extract,extract_entities,normalize,validate_tags,mod}.rs` | `src-tauri/src/kg/passes/{...}` | All five passes + the closed-vocab validator (preserved for v1.1 readiness even though v1 tags-half is open-vocab — D6 surface stays minimal but the module compiles). |
| `src/ollama/{mod,ureq_dispatcher,testing}.rs` | `src-tauri/src/kg/ollama/{...}` | Rewritten around `ureq::Agent` (D1). `MockOllama` remains for `cfg(test)` consumers + the `kg_parity` probe. |
| `src/synonyms.rs` | `src-tauri/src/kg/synonyms.rs` | `SynonymMap`. |
| `src/embeddings.rs` | `src-tauri/src/kg/embeddings.rs` | Preserved per binding parameter 5; not on the `run_pipeline` hot path in v1. |
| `src/harness/pipeline.rs` | `src-tauri/src/kg/pipeline.rs` | The orchestrator. Public surface entry. |
| `src/assets/SCHEMA.md` + `src/assets/prompts/*.md` | `src-tauri/src/kg/assets/{SCHEMA.md, prompts/*.md}` | Bundled via `include_str!`; override resolved via `MOCKINGBIRD_KG_SCHEMA_DIR` env. |

---

## §2. Scope: what does NOT graduate (binding parameter D5)

These stay in `experimental/kg-validation/` and continue to be the
**v1.1+ regression rig + parity-fixture source**. Per ADR 0049
§"Sandbox isolation" the sandbox is **not deleted** on graduation.

- `judges/` — Phase 0.5 IAP judges. Not production code.
- `scoring/` — corpus-fixture scorers (`jaccard`, `quality`, etc.).
- `wiggum/` — Phase 0.5 Ralph Wiggum IAP loop harness.
- `exemplars/` — calibration corpora.
- `harness/runner.rs` — sandbox-only batched-runner (the throwaway-test
  recipe + per-iteration accounting). `pipeline.rs` *does* graduate
  (above); `runner.rs` does not.
- The six standalone binaries under `src/bin/` (`run-corpus`,
  `run-entities`, `score-entities`, `score-quality`, `mode-eval`,
  `judge-calibration`).
- `corpus/` — fixture corpus.
- `runs/` — run artifacts (already gitignored).
- `judge-calibration/` — IAP-judge calibration tables.

---

## §3. Out of scope (Phase 1B+)

Phase 1A produces a **callable library that nobody calls yet.** The
following are explicitly *not* in Phase 1A:

- **DB tables / migrations** — Phase 1B (`entities`, `canonical_tags`,
  `edges` tables; concept-page SQL views).
- **Retrieval UX, Tauri command wiring** — Phase 1C (six retrieval
  axes; opt-in graph activation surface in Settings).
- **Migration backfill over existing transcripts** — Phase 1D.
- **v1 beta release tag** — Phase 1E.
- **Wiring `kg::run_pipeline` into the dictation loop as a real
  consumer.** No `kg::*` call sites under `dictation::`, `command_center::`,
  `commands::`, or anywhere else in Phase 1A. The integration story
  belongs to Phase 1C+.

If a Phase 1A diff reaches into `ui/**`, that's a Phase 1C-scope leak —
escalate via STATUS and a bead, do not push through.

---

## §4. Binding decisions (D1–D6, confirmed by Dustin at kickoff)

| ID | Decision | Rationale |
|---|---|---|
| **D1** | HTTP client = `ureq::Agent`. **NO** `reqwest`. | Consistency with ADR 0021 cleanup providers; one HTTP stack across the codebase. |
| **D2** | SCHEMA.md + prompts via `include_str!` at compile time; `MOCKINGBIRD_KG_SCHEMA_DIR` env override for power-user editing. Bundled assets at `src-tauri/src/kg/assets/`. | Mirrors `MOCKINGBIRD_MODELS_DIR` pattern. Production binary is self-contained; power users can iterate on prompts without rebuilding. |
| **D3** | Per-module `thiserror::Error` types. `KgSchemaError` exists; add `KgPipelineError`. **NO `anyhow`** in production crate. | Project standard (AGENTS.md Rust §). Sandbox used `anyhow` as a one-iteration convenience; production needs typed errors for caller-side branching. |
| **D4** | Parity fixture = Wave 0.5.4 seed-42 entity-probe run. | Sealed scorecard run; bit-identical re-run is the strongest contract test for graduation correctness. See `parity/README.md` §2. |
| **D5** | Sandbox stays alive post-graduation as v1.1+ regression rig. Library subset graduates only (§1). The non-graduating set (§2) remains. | Avoids losing the IAP infrastructure that Phase 0.5 built. Keeps a place to land Phase 1.1+ closed-vocab Move 2 work without re-establishing a sandbox. |
| **D6** | Public surface of `kg::`:<br>`pub use pipeline::{run_pipeline, PipelineResult};`<br>+ schema types (`Entry`, `Category`, `EntryType`, `EntityType`, `Status`, `AnswerKey`).<br>Everything else `pub(crate)`. **NO Tauri command wiring in 1A.** | Minimum surface that makes 1B/1C consumers possible without prematurely committing to a wire format. |

---

## §5. Parity gate procedure

The graduation is gated by a single bit-identical re-run probe landed
in Chunk 3.

### Fixture (captured in Chunk 1)

- `docs/knowledge-graph/parity/wave-0.5.4-seed-42.json` — 32-dictation
  expected `PipelineResult` + `entities` aggregate.
- `docs/knowledge-graph/parity/wave-0.5.4-seed-42-canned-responses.json`
  — per-(dictation, pass, segment_idx) canned model responses for `MockOllama`.
- `docs/knowledge-graph/parity/README.md` — provenance, restoration,
  per-segment-vs-per-dictation entity provenance constraint (§3 in
  that README).

### Probe (lands in Chunk 3)

`src-tauri/eval/kg_parity.rs` (or `src-tauri/src/bin/kg_parity.rs` —
Chunk 3 picks the conventional location for one-shot Rust binaries
in this repo). Binary-only (no `#[test]` attribute) to sidestep the
known `cargo test --release` launch failure on this box (LESSONS
2026-05-17).

### Procedure

1. Load both fixture files.
2. Build a `MockOllama` that, given a prompt, locates the matching
   `(dictation_id, pass, segment_idx)` tuple by prompt-substring
   matching against the graduated `kg::passes` prompt templates +
   returns the canned response string.
3. For each dictation in the fixture, call `kg::run_pipeline` against
   `MockOllama`. Serialize `PipelineResult` deterministically.
4. Compare byte-for-byte against `fixture.dictations[*].pipeline_result`.
5. Aggregate per-dictation `extract_entities` responses, dedup, compare
   against `fixture.entities` (set equality on `{name, type, aliases}`).
6. Exit non-zero on any divergence; print a unified diff so the
   failure mode is human-debuggable.

### Invocation

```
powershell -File scripts\cargo-with-cuda.ps1 run --release --bin kg_parity
```

(Wrapper required even for a pure-Rust binary — the workspace links
`whisper-rs` + `ort`, and `cargo` without the wrapper has been observed
to produce binaries that don't launch on this box. LESSONS 2026-05-17.)

---

## §6. Public surface (D6 spelled out)

```rust
// src-tauri/src/kg/mod.rs (post-Chunk-2)
pub mod schema;          // intentionally pub for re-export of types
pub(crate) mod schema_loader;
pub(crate) mod passes;
pub(crate) mod ollama;
pub(crate) mod synonyms;
pub(crate) mod embeddings;
pub mod pipeline;        // intentionally pub for run_pipeline + PipelineResult

pub use pipeline::{run_pipeline, PipelineResult};
pub use schema::{Entry, Category, EntryType, EntityType, Status, AnswerKey};
```

Anything not in this list (e.g. `OllamaDispatcher` trait, `Schema`
loader, `SynonymMap`) is `pub(crate)` until a downstream consumer
demands a wider surface (and that surface is justified in a follow-on
brief or ADR). YAGNI applies — Phase 1A does not pre-export.

---

## §7. Seal criteria

- [ ] Chunk 1 (this commit) — parity fixture present, scaffold present, brief present.
- [ ] Chunk 2 — library subset graduated. `anyhow` excised from `kg::`.
      `ureq::Agent` in `kg::ollama`. `include_str!` + env override in
      `kg::schema_loader`. Cargo wrapper gate green (`check`, `clippy
      --release -- -D warnings`, `fmt --check`, `test --release --no-run`).
- [ ] Chunk 3 — `kg_parity` probe binary green over all 32 fixture
      dictations (`pipeline_result` byte-identical, `entities` set-equal).
- [ ] ADR 0049 §"Sandbox isolation" updated with sealed-window note
      ("Phase 1A graduation completed `<date>`").
- [ ] STATUS.md "Currently active" cleared; "Sealed" notes the
      graduation if STATUS templates that level.
- [ ] Epic bead `mb-2mc9` closed with resolution referencing the
      final commit + this brief.
- [ ] **No new ADR.** **No `phase-*-complete` tag.** Lateral epic per
      AGENTS.md "Work sizing" + LESSONS PINNED P5.

---

## §8. Risk register

Re-derived from the Phase 0.5 inputs + binding decisions. Each carries
a mitigation; the entries here are the *consciously accepted* risks for
the graduation.

| # | Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|---|
| **R1** | Per-segment vs per-dictation entity provenance mismatch. The Wave 0.5.4 probe persisted aggregate-only entity outputs; the production pipeline may fan `extract_entities` per-segment, requiring either set-level assertion or a sandbox re-capture. | Medium | Medium (Chunk 3 design) | `parity/README.md` §3 documents both options. Default to aggregate-set assertion in Chunk 3; only escalate to sandbox re-capture if the probe surfaces a real per-segment divergence. |
| **R2** | `include_str!` + `MOCKINGBIRD_KG_SCHEMA_DIR` env-override has subtle CWD-vs-compile-time gotchas. `include_str!` resolves paths relative to the *source file* at compile time; the env override is runtime. Mixing them up silently ships stale prompts. | Medium | High (silent parity divergence) | Chunk 2 design: at runtime, the loader picks ONE source (env if set + dir present + required files all present; else bundled). Never merges. Document in `kg::schema_loader` module docstring + emit a `tracing::info!` at load showing which source won. |
| **R3** | `anyhow` → `thiserror` fan-out across every graduated module. Each error variant is a fresh design call; risk of accidentally changing parse-failure error shapes the parity probe's diff messages depend on. | Medium | Low–Medium | Keep `PassError` variants 1:1 with the sandbox's. New `KgPipelineError` is a thin wrapper. Parity probe asserts `PipelineResult` shape, not error-message text. |
| **R4** | Sandbox `ureq` direct calls vs production `ureq::Agent` connection-pooling differences. The MockOllama path doesn't exercise the dispatcher's network code, so the parity probe is silent on this. | Low | Low (1A) → Medium (1C+) | Acceptable for 1A. Phase 1C+ will need a live-Ollama smoke; out of 1A scope. |
| **R5** | `serde_yaml = "0.9"` is marked upstream-deprecated (no maintained replacement at parity). A future workspace audit could purge it. | Low | Medium (blast radius = `kg::schema_loader`) | Accepted. Industry-wide there is no better pure-Rust YAML deserializer right now. Revisit when an alternative ships. |
| **R6** | Cargo features divergence: sandbox uses vanilla `cargo`, production uses `cargo-with-cuda.ps1` (whisper-rs/ort CUDA-linked workspace). Pure-Rust `kg::` code shouldn't care, but Chunk 2 must avoid introducing CUDA-bound deps in `kg::`, AND the `kg_parity` binary must invoke through the wrapper. | Low | Medium | Chunk 2 reviewer check: no new CUDA-touching deps under `kg::`. Chunk 3 invocation in §5 above is wrapper-only. |

---

## §9. References

- ADR 0049 — Phase 0.5 charter + amendments A1/A2/A3; v1 architectural pivot.
- `docs/knowledge-graph/PHASE-0-5-REPORT.md` — §6 v1 commitments; §7 wave plan; §11 methodology; LESSONS P9–P12 cross-walk.
- `docs/knowledge-graph/parity/README.md` — fixture provenance + restoration.
- `experimental/kg-validation/README.md` §5 — sandbox isolation rules (window opens for this epic).
- LESSONS PINNED P4 — session-start ritual (clean kickoff anchoring).
- LESSONS PINNED P5 — lateral epics seal via ADR + STATUS, not via `phase-*-complete` tags.
- LESSONS 2026-05-17 — `cargo test --release` STATUS_ENTRYPOINT_NOT_FOUND fallback; cargo wrapper requirements; PS 5.1 vs `pwsh`.
- LESSONS 2026-05-24 — `bd create` non-ASCII gotcha (informed bead titles in §0).
- AGENTS.md "Work sizing & workflow selection" — container choice rationale (ADR-chartered lateral epic).
- AGENTS.md "Build / run / test environment (Windows)" — cargo wrapper requirements.
