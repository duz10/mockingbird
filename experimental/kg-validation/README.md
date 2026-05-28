# kg-validation — Mockingbird Knowledge Graph Phase 0 harness

This is the **sandboxed validation harness** chartered by
[ADR 0048](../../docs/adr/0048-knowledge-graph-phase-0-validation.md).
Its job is to answer one question: **can the local 3B-7B models we
ship actually produce trustworthy structured entries from rambling
voice memos?**

It is **not** a production module. It is **not** wired into the main
Mockingbird app. Deleting this folder is a no-op for production.

## Isolation discipline (spec §5 — non-negotiable)

These rules are the architectural contract for this entire epic and
are reproduced here so they are visible at the entry point — not
buried in an ADR.

- **§5.1 — Sandbox folder.** All Phase 0 work (corpus, answer keys,
  harness, prompts, run outputs, scoring) lives under this directory.
  Deleting `experimental/kg-validation/` must leave the production
  app completely untouched. This is the blast-radius container.
- **§5.2 — Reuse by reference, never by copy.** When Phase 0 needs an
  existing production capability, it imports / calls the existing
  production module as-is. It does **not** copy production source
  into the sandbox.
- **§5.3 — Do not modify production files.** Phase 0 may read from,
  import, and call production modules, but writes only inside its
  own sandbox folder. Genuine need to change a production module ⇒
  **stop and flag**, do not silently edit.
- **§5.4 — Deliberate duplication is allowed only for features under
  active change**, and only as a clearly-marked new file inside the
  sandbox that never overwrites the original.

ADR 0048 records the three Phase-0 scope carve-outs (G1 local Ollama
helper, G2 corpus is text-only, G4 determinism knobs). Anything not
covered there is governed by §5 strictly.

## Layout

```
experimental/kg-validation/
├── Cargo.toml          # standalone (own [workspace], no path-dep on mockingbird)
├── README.md           # this file
├── src/
│   ├── lib.rs          # pub mod schema, harness, scoring, passes
│   ├── schema.rs       # Category / EntryType / Entry / AnswerKey  (Wave 0)
│   ├── harness/mod.rs  # Wave 2 home
│   ├── scoring/mod.rs  # Wave 3 home
│   └── passes/mod.rs   # Wave 2 home
├── corpus/
│   ├── dictations/     # raw dictation text (Wave 1)
│   └── answer-keys/    # hand-authored ground truth (Wave 1)
├── prompts/            # per-pass prompts (Wave 2)
└── runs/               # per-invocation harness output (gitignored)
```

## Invocation

The harness binary `run-corpus` lands in Wave 2:

```
cargo run --bin run-corpus
```

**The Windows CUDA wrapper is NOT needed here.** This crate has no
`whisper-rs`, no `ort`, and no CUDA in its dep graph, so plain `cargo`
works directly and live `cargo test` actually launches the test
runner (sidestepping LESSONS PINNED P2).

## Determinism knobs (ADR 0048 §G4)

- Pipeline temperature pinned to **0.2** across every pass.
- `seed` set per-pass where Ollama supports it for the model in use,
  so the §8.5 two-run stability check measures real sampling
  variance rather than greedy-decode noise.
- The LLM tag-equivalence judge runs on a **different model family**
  than the pipeline (e.g. pipeline `qwen2.5:3b` → judge
  `llama3.1:8b`). Same-model judging is the failure mode spec §6.1
  warns about.
