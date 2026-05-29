# KG parity fixtures — Wave 0.5.4 seed-42

This directory pins the **bit-identical re-run gate** for graduating the
schema-driven KG pipeline from `experimental/kg-validation/` into
`src-tauri/src/kg/` (Phase 1A — see `docs/knowledge-graph/phase-1a-brief.md`
and ADR 0049).

It is a deliberate, narrow contract between the sandbox and the
production graduation: *given the same inputs and a deterministic
LLM mock, the production `kg::run_pipeline` must produce JSON
identical to what the sandbox produced when Wave 0.5.4 sealed.* If
it doesn't, the graduation regressed something — most likely a
prompt body, a parser quirk, or pass ordering.

The gate is consumed by Chunk 3's `src-tauri/eval/kg_parity` probe.

---

## §1. What's here

| File | What it is |
|---|---|
| `wave-0.5.4-seed-42.json` | 32-dictation aggregate: per-dictation `pipeline_result` (= `entries` + `per_pass_errors` + `new_tag_requests`) + the Wave 0.5.4 entity-pass output. Sorted by `dictation_id`. The **assertion target**. |
| `wave-0.5.4-seed-42-canned-responses.json` | Per-pass canned model responses, keyed by `dictation_id`. The **MockOllama script**. |
| `aggregate_fixture.py` | The capture script. Idempotent: re-running against the same sandbox source dirs produces the same bytes. |

Sizes (informational, may drift if the script is rerun against a different
source run): roughly 63 kB and 30 kB respectively as of capture day.

---

## §2. Provenance

Both fixtures are derived from sealed Wave 0.5.4 sandbox runs:

- **Pipeline (4-pass)** source:
  `experimental/kg-validation/runs/iter-1-7b-fix/`
  – the 4-pass open-vocab `run_pipeline` run that the entity probe
    cited as its `source-run` in `ENTITY_SUMMARY.md`.
- **Entity (5th pass)** source:
  `experimental/kg-validation/runs/run-7b-entities-seed42/`
  – the standalone `extract_entities` probe over the same 32-dictation
    corpus, scored against `corpus/entity-labels.jsonl` (`corpus_average_jaccard = 54.83%`,
    above the ≥ 50% Wave 0.5.4 bar).
- **Model**: `qwen2.5:7b-instruct-q4_K_M` (the ADR 0049 v1 model pin).
- **Profile**: `mid-confident` — `temperature=0.2`, `seed=42`, `num_ctx=4096`.
- **Captured timestamp**: `2026-06-14T08:00:00Z` (stable; not the wall clock at capture time — the sandbox uses this as the deterministic `captured_iso` so `Entry.captured_iso` is comparable across runs).

Both source dirs are gitignored. They live on the original sealing
machine + (with luck) the dev box. If they're ever lost, see §5
"Restoration".

---

## §3. Per-segment vs per-dictation entity provenance (read this)

A subtle but load-bearing detail for Chunk 3:

The Wave 0.5.4 entity probe ran `extract_entities` **per segment** but
the on-disk artifact at `run-7b-entities-seed42/entities/<id>.json`
only preserved the **per-dictation aggregate** (dedup'd union of all
segment outputs, with a `segment_count` and `segment_failures` field
but no per-segment breakdown). This is by design in the sandbox
harness — the probe was a scoring instrument, not a replay rig.

Implication for the production parity probe:

- The `pipeline_result.entries` portion of the fixture is **fully
  reproducible** byte-for-byte from canned responses, because the
  4-pass `run_pipeline` writes per-segment artifacts.
- The `entities` portion of the fixture is **at-the-aggregate-level
  reproducible only**. Chunk 3's `kg_parity` probe has two options:

    1. **Aggregate-only assertion** (preferred for Phase 1A): the
       production `run_pipeline` fans `extract_entities` per-segment,
       collects the union, and the probe asserts equality at the
       `{name, type, aliases}` set level. The MockOllama returns the
       same canned aggregate string for every per-segment call within
       a given dictation; dedup in the production pass collapses
       duplicates. This is the cheapest path and matches the v1
       semantics where downstream consumers only care about the
       aggregate.

    2. **Re-run-for-per-segment fidelity** (if (1) proves insufficient):
       extend the sandbox harness to persist per-segment entity
       artifacts, re-run the Wave 0.5.4 probe, and rebuild the
       canned-responses file. This is a one-iteration sandbox change;
       does not require touching production code.

Picking option (1) until proven inadequate is the Phase 1A position;
revisit during Chunk 3 if the probe surfaces a real per-segment
divergence.

---

## §4. Consuming the fixture (Chunk 3 contract sketch)

```rust
// src-tauri/eval/kg_parity.rs (Chunk 3 — not yet written)
//
// 1. Load wave-0.5.4-seed-42-canned-responses.json.
// 2. Build a MockOllama that, given a prompt, locates the matching
//    (dictation_id, pass, segment_idx) tuple by prompt-substring
//    matching against the sandbox's prompt templates (which we'll
//    have graduated into kg::passes by then) + returns the canned
//    response string.
// 3. For each dictation in wave-0.5.4-seed-42.json:
//      a. Call kg::run_pipeline against MockOllama with the
//         dictation_text + captured_iso.
//      b. Serialize the resulting PipelineResult.
//      c. Compare byte-for-byte against the expected
//         pipeline_result from the fixture.
// 4. Aggregate per-dictation extract_entities responses across
//    segments, dedup, compare against fixture.entities (set
//    equality on {name, type, aliases}).
// 5. Exit non-zero on any divergence; print a unified diff so the
//    failure mode is human-debuggable.
```

The probe is binary-only (no `#[test]` attribute) to sidestep the
known `cargo test --release` launch failure on this box
(LESSONS 2026-05-17). Invoke via:

```
powershell -File scripts\cargo-with-cuda.ps1 run --release --bin kg_parity
```

---

## §5. Restoration

If the source run dirs are ever missing:

```
# 1. Restart Ollama with the v1 model:
ollama run qwen2.5:7b-instruct-q4_K_M
# 2. Re-run the 4-pass pipeline over the corpus:
cd experimental/kg-validation
cargo run --release --bin run-corpus -- --out-dir runs/iter-1-7b-fix --seed 42 \
  --model qwen2.5:7b-instruct-q4_K_M --temperature 0.2
# 3. Re-run the entity probe over the same corpus:
cargo run --release --bin run-entities -- --out-dir runs/run-7b-entities-seed42 \
  --source-run runs/iter-1-7b-fix --seed 42 \
  --model qwen2.5:7b-instruct-q4_K_M --temperature 0.2
# 4. Re-aggregate:
python ../../docs/knowledge-graph/parity/aggregate_fixture.py
# 5. git diff should be empty if model + seed + corpus + prompts
#    are unchanged. Non-empty diff = a real semantic change to one
#    of those inputs.
```

(Exact binary names may differ; check `experimental/kg-validation/src/bin/`
for the current set.)

---

## §6. Invariants the fixture encodes

The bit-identical assertion implicitly pins:

- The five-pass topology (segment → classify → extract → normalize → extract_entities) and the per-pass argument shapes.
- The prompt bodies for each pass (any reshape will diverge the canned response matching).
- The parser JSON shapes (`Entry`, `Classification`, `Extraction`, `EntityExtraction`).
- The synonym/normalize behaviour the sandbox encoded (no closed-vocab in `iter-1-7b-fix`, so empty `new_tag_requests` is expected).
- The `captured_iso` propagation contract.

If any of those need to change after Phase 1A seals, the change *must*
include a re-capture of this fixture in the same commit — the parity
gate is the safety net, not a static check.

---

## §7. What this fixture is NOT

- **Not a quality benchmark.** Quality is scored by the sandbox's
  scoring rigs (jaccard against the corpus labels). The parity gate
  asks "does the production pipeline produce the same outputs?",
  not "are those outputs good?".
- **Not a regression test for LLM drift.** With a real Ollama dispatcher,
  seed=42 is necessary but not sufficient for byte-identity (model
  bytes + GPU non-determinism enter). The MockOllama path strips
  the LLM entirely, so the gate isolates pipeline-code correctness.
- **Not a substitute for Phase 1B+ tests.** Once `kg::` has DB tables,
  retrieval, etc., those will need their own tests. This fixture
  covers the library-only contract Phase 1A delivers.
