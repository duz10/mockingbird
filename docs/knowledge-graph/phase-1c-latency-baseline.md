# Phase 1C — KG filing-pipeline latency baseline

**Wave:** 1C.0 (`mb-plz9`, ADR 0051) — discharges `mb-b3jy`
**Date measured:** 2026-05-30
**Status:** Initial baseline. Re-measure on the triggers in §6.

---

## 1. What was measured

End-to-end wall-clock cost of filing one dictation into the knowledge
graph, broken down per pipeline pass and per stage. The bench bypasses
the worker thread + queue round-trip so the numbers reflect the
*pipeline + store* cost only — i.e. what a user is waiting for when
they want their dictation to appear in retrieval surfaces.

**Bench binary:** `src-tauri/src/bin/kg_latency_bench.rs`
**Bench module:** `src-tauri/src/kg/latency_bench.rs`
**Invocation:**

```text
powershell -File scripts\cargo-with-cuda.ps1 run --release --bin kg_latency_bench
  > docs\knowledge-graph\phase-1c-latency-baseline-raw.csv
```

### Methodology

- **Model:** `qwen2.5:7b-instruct-q4_K_M` (same as
  `kg::worker::DEFAULT_FILING_MODEL`).
- **Ollama:** local daemon at `http://localhost:11434`, default config.
  Model already pulled and warmed (the bench's reachability ping
  triggers a no-op generation before the first measured fixture, so
  cold-start model load isn't billed to the first sample).
- **GenerateOptions:** `temperature=0.2, num_ctx=4096, seed=entry_id`
  (mirrors `kg::worker::process_one` exactly).
- **SQLite:** tempfile-backed, all 24 migrations applied,
  `PRAGMA foreign_keys = ON`. No `:memory:` shortcut — the production
  app uses file-backed SQLite, and we want the file-engine FK behavior.
- **Fixtures:** 5 hand-picked entries from
  `docs/knowledge-graph/parity/wave-0.5.4-seed-42.json`, covering
  segment counts 1, 2, 3, 4, and 5 (5 is the longest in the fixture
  set; no 7-segment dictation exists).
- **Per-fixture flow:** seed `sessions` row → `kg::run_pipeline` →
  `kg::store::apply_filed_outcome` (inside a `tx`, timed separately) →
  next.
- **Hardware:** Dustin's Windows + CUDA dev box. No other GPU/CPU
  load besides the daemon + bench.
- **Sample size:** n=5. This is a baseline, not a stable statistical
  characterization. Run-to-run variance is real; see §6.

---

## 2. Raw CSV

Verbatim from `docs/knowledge-graph/phase-1c-latency-baseline-raw.csv`
(2026-05-30 run):

```csv
fixture_id,segment_count,segment_ms,classify_ms_total,extract_ms_total,extract_entities_ms_total,normalize_ms_total,store_apply_ms,total_pipeline_ms
persona-03-case-01,1,5198,952,5560,3722,0,0,15433
persona-01-case-01,2,1733,1938,10830,5307,0,0,19811
persona-04-case-02,3,2187,3009,18481,9372,0,0,33054
persona-05-case-02,4,3267,4185,23543,14776,0,0,45777
persona-05-case-03,5,3441,5457,31386,18738,0,0,59029
# summary
# samples              = 5
# mean_total_pipeline_ms = 34620
# p50_total_pipeline_ms  = 33054
# p95_total_pipeline_ms  = 59029
# max_total_pipeline_ms  = 59029
```

---

## 3. Summary statistics

### Aggregate `total_pipeline_ms`

| stat   | ms     | seconds |
|--------|--------|---------|
| mean   | 34 620 | 34.6    |
| p50    | 33 054 | 33.1    |
| p95    | 59 029 | 59.0    |
| max    | 59 029 | 59.0    |
| min    | 15 433 | 15.4    |

### Per-pass breakdown (mean across all 5 fixtures, ms)

| pass                | mean ms | % of mean total |
|---------------------|---------|-----------------|
| `segment`           | 3 165   | 9.1 %           |
| `classify_total`    | 3 108   | 9.0 %           |
| `extract_total`     | 17 960  | 51.9 %          |
| `extract_entities_total` | 10 383 | 30.0 %     |
| `normalize_total`   | 0       | 0.0 %           |
| `store_apply`       | 0       | 0.0 %           |
| **sum**             | **34 616** | **~100 %**  |

### Per-segment scaling (back-of-envelope)

| segments | total ms | ms/segment |
|----------|----------|------------|
| 1 | 15 433 | 15 433 |
| 2 | 19 811 |  9 906 |
| 3 | 33 054 | 11 018 |
| 4 | 45 777 | 11 444 |
| 5 | 59 029 | 11 806 |

Steady-state cost per added segment is **~11 s** once the segment
pass (one-shot) and a warm classify/extract pair are paid. The
1-segment outlier (15.4s) is dominated by an unusually heavy
`segment_ms = 5198` — likely just first-fixture cold-start noise the
warmup ping didn't fully absorb. The other four segment-pass
measurements (1.7s, 2.2s, 3.3s, 3.4s) form a tighter cluster.

---

## 4. Implications for 1C UX

Concrete observations from the data, not opinions:

1. **`extract` is the dominant cost** at ~52% of the average. It is
   the largest single LLM call per segment (entry-schema-filling +
   tag extraction). **Optimization priority #1 for Phase 1D+** is
   prompt tuning on `extract` — even a 30% reduction here moves the
   p95 from 59s down to ~50s.

2. **`extract_entities` is the runner-up** at ~30%. Phase 1D backfill
   work will magnify this proportionally. **Optimization priority #2**
   is prompt tuning on the 5th pass.

3. **`normalize` and `store_apply` are effectively free** (0ms in
   the ms-precision bench — i.e. sub-ms). Deterministic-preprocessor
   optimizations on the tag pipeline have zero upside; SQLite write
   amplification is a non-concern for v1.

4. **p95 = 59s sits right at the ADR 0049 §6 budget.** This is
   not catastrophic — the budget was explicitly aspirational, not
   load-bearing — but it means:

   - **Wave 1C.1 (Settings activation UX)** must show a "KG indexing
     in progress" indicator. Fire-and-forget with no feedback is a
     poor UX when the user could be staring at a blank pane for a
     full minute waiting for their dictation to surface in retrieval.
   - **Wave 1C.3 (Dictations retrieval surface)** needs an empty/loading
     state for "newest dictation isn't filed yet". A user typing a
     1-paragraph dictation and immediately reaching for a filter
     chip will be waiting up to a minute before that dictation's
     entities + tags exist.
   - The "spinner OR not" question becomes: spinner with a soft
     deadline (1 minute) and a fallback "still working…" indicator.
     Per ADR 0049 §6, hard failure isn't a thing — the worker just
     keeps trying.

5. **Multi-segment dictations dominate the worst case.** The 5-segment
   fixture (`persona-05-case-03`, 605 chars dictation text) hits 59s.
   That's still within the local-LLM "feels reasonable for a
   background indexing job" budget. The right ceiling check is "did
   the user dictate again before the previous one finished?" — which
   the queue handles natively (FIFO, never drops work).

---

## 5. Comparison to ADR 0049 §6 binding

ADR 0049 §6 spec target: **"~1 min per dictation"** as the
soft latency budget for v1.

| metric | spec | observed |
|--------|------|----------|
| mean   | ≤ ~1 min | 35 s  ✓ comfortably under |
| p50    | ≤ ~1 min | 33 s  ✓ comfortably under |
| p95    | ≤ ~1 min | 59 s  ✓ just under |
| max    | ≤ ~1 min | 59 s  ✓ just under |

**Verdict:** the empirical baseline meets the §6 binding on a 5-sample
representative fixture set. p95 sits within ~2% of the budget though,
so there's no margin — any regression (prompt drift, model swap to a
slower variant, hardware change) needs re-measurement to stay honest.

---

## 6. Re-measurement policy

Re-run `kg_latency_bench` and update this doc when **any** of the
following happens. The re-run is cheap (~3 min for n=5); the failure
mode of skipping it is shipping a UX assumption against stale numbers.

1. **Model swap** — `BENCH_MODEL` constant in `kg::latency_bench` or
   `DEFAULT_FILING_MODEL` in `kg::worker` changes. Even a same-family
   swap (e.g. `qwen2.5:7b → qwen2.5:14b`) changes the entire
   latency profile.
2. **Prompt overhaul** — any non-trivial edit to the bundled prompt
   bodies under `src-tauri/src/kg/assets/`. Token-count changes
   directly drive `*_ms_total`.
3. **Pipeline-pass change** — adding, removing, or fundamentally
   restructuring a pass. The CSV column shape itself will need
   updating in this case; bump it.
4. **Hardware migration** — Dustin moves to a different box, or the
   CUDA stack changes major version, or the daemon binds to a
   different runtime.
5. **`GenerateOptions` tuning** — `temperature`, `num_ctx`, or seed
   strategy changes affect both per-pass cost and variance.
6. **Phase 1D backfill measurements** — backfill will need its own
   baseline (different topology: bulk-mode vs. one-at-a-time). This
   doc is the per-dictation reference; backfill gets its own doc.

When re-measuring, **append** a dated sub-section under §2 with the
new CSV and updated §3 stats rather than overwriting — historical
comparison is the point.

---

## 7. References

- ADR 0049 (KG Phase 0.5 + v1 pivot) §6 — latency budget binding.
- ADR 0050 (KG Phase 1B persistence + dictation hook) — defines the
  worker `process_one` shape this bench mirrors.
- ADR 0051 (KG Phase 1C charter) — the wave plan this discharges.
- `docs/knowledge-graph/phase-1c-brief.md` — Wave 1C.0..1C.5 plan.
- `docs/knowledge-graph/parity/wave-0.5.4-seed-42.json` — fixture
  source.
- `mb-b3jy` (bd) — empirical latency-budget bead; closed by this doc.
- `mb-plz9` (bd) — Wave 1C.0 charter bead.
