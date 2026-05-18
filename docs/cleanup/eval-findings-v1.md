# Mode-eval findings — baseline (v1)

_Wave B of ADR 0024. Author: Bernard, 2026-05-17.
Source: `docs/cleanup/eval-baseline-20260518T015546Z.md`._

## Headline numbers

| Mode    | N  | Errors | Preserve avg | Full ✅ | Partial ⚠️ | Zero ❌ | avg LLM | max LLM |
|---------|----|--------|--------------|---------|------------|---------|---------|---------|
| casual  | 39 | 0      | **93.4%**    | 30      | 8          | **1**   | 3.6s    | 12.5s   |
| normal  | 39 | 0      | **96.8%**    | 32      | 7          | 0       | **11.3s** | 37.5s 🛑 |
| formal  | 39 | 0      | **76.9%**    | **13**  | **25**     | 1       | 10.0s   | 24.7s   |

Grid wall-clock: **975s** (~16 min) for 117 calls.

## The Pareto — three real problems, one scoring artifact

### 🚨 P1 — Casual hallucination on complex input (1 case, but catastrophic)

**Fixture 06_implicit_long** (8-item architecture description). Casual at `temperature=0.4`
**discarded the entire input** and emitted unrelated content:

> **Raw:** `the cleanup pipeline takes the raw whisper output runs it through the deterministic preprocessor...`
>
> **Casual output:** `hey can you grab milk, eggs, and bread on the way home thanks`

Zero must-preserve hits (0/8). LLM call: 1352 ms — the model didn't even try; it
pattern-matched the prompt's "casual chat" framing and improvised. This is the
**worst-possible failure**: the user dictates technical content and gets a
fake grocery list pasted into VS Code.

**Root cause hypotheses:**
1. `casual_v1` prompt frames the mode as "casual chat" → model has a strong prior
   to emit chat-like text.
2. `temperature=0.4` lets the model wander when context is complex.
3. The 3B model genuinely lacks capacity for long technical content.

**Mitigations (Wave C):**
- Lower casual `temperature` to `0.2` (less creativity).
- Add an explicit **anti-hallucination guard** to `casual_v2`: "If the input
  exceeds 60 words OR contains technical content, treat it as 'preserve every
  sentence' regardless of register — your job is cleanup, not invention."
- Consider routing long casual to the 7B model (raises latency but eliminates
  the cliff). Alternative: detect technical content in the preprocessor and
  raise a "needs-normal" hint to the runtime — defer to Wave 3 / mb-cjc.

### ⚠️ P2 — Normal latency way over target

Target (ADR 0024): normal p80 ≤ 2.5 s. Reality: avg 11.3 s, max 37.5 s.

The 37.5 s outlier was the **first** normal call (cold-loading qwen 7B into
VRAM). Subsequent calls hovered 5-13 s. **Steady-state avg ≈ 7-8 s** if we drop
the cold call. Still 3× target.

**Three options, none of them prompt-only:**

1. **Streaming output to the recording window** (mb-cjc Wave 3 territory): user
   sees text appearing as the model generates → perceived latency collapses.
   Doesn't reduce wall time but reduces felt time. **Highest UX impact, biggest
   engineering effort.**
2. **Tighten the normal prompt + drop `max_tokens` from 2048 → 1024.** Less
   context to process and a smaller output budget means fewer tokens to decode.
   Estimated savings: 20-40%. **Prompt-only, ships in Wave C.**
3. **Default normal to qwen 3B**, keep 7B as opt-in for "high-quality" sessions.
   Trade content fidelity (current 96.8% preserve) for speed. **Risky.**

Wave C ships option (2) and proves out the budget. Streaming + 3B fallback are
**post-W ave-C tickets** (link `mb-cjc` and the future streaming issue).

### 🎭 P3 — Formal register-lift sometimes drops literal terms (3-5 real cases)

Examples:
- **34_emphasis_short**: `really important...fix it now` → `critically important
  and must be addressed immediately`. Semantically faithful. Lexically: 0/2.
- **35_emphasis_medium**: `bad...ignoring it...too long` → `significantly poor...
  neglected...unacceptable duration`. Semantically faithful. Lexically: 3/6.
- **31_tangent_short**: similar pattern.

The current `formal_v1` prompt encourages register lift, which is correct for the
mode. The "failures" against `must_preserve` are scoring artifacts more than
quality issues — but **two genuine concerns** remain:

1. When formal lifts register, the original speaker's **emotional charge** can
   be flattened. "We're embarrassed" reads urgently; "this is significantly
   poor" reads like a quarterly report. For some users this is correct (formal
   = professional voice); for others it's tone-deafness.
2. Lifting register on **proper-noun-adjacent terms** is occasionally wrong
   ("WisprFlow" → "the speech-recognition tool" would be a disaster). Sample
   showed no such failures in baseline, but the risk surface exists.

**Mitigations (Wave C):**
- `formal_v2`: add explicit "preserve emotional intensity markers (urgency,
  frustration, enthusiasm)" rule.
- `formal_v2`: add "proper nouns and technical terms are NEVER paraphrased"
  as a hard rule with examples.
- Update scoring rig: add `must_preserve_alts` field for fixtures where
  paraphrase is acceptable — fixes the **3-5 lexical false positives** below
  without weakening the literal-preservation guarantee for proper nouns.

### 🪓 Scoring artifact — hyphen + paraphrase blindness in `normalise()`

`half day` (must-preserve) vs `half-day` (actual output) → score miss.
`fix it now` (must-preserve) vs `immediately` (paraphrase) → score miss.

Fixed by: extending `normalise()` to split on hyphens, and adding optional
`must_preserve_alts: [["bad", "poor", "subpar"], ...]` per fixture for
acceptable paraphrases. Re-running with the patched scorer against the
**existing baseline outputs** (no new LLM calls!) will give us a corrected
formal preservation number — almost certainly into the 85-92% range.

## Latency distribution

Cold-load skews the numbers. Steady-state (excluding first call per mode):

| Mode    | calls | avg LLM (steady) | median | p80    | p95    | max (warm) |
|---------|-------|------------------|--------|--------|--------|------------|
| casual  | 39    | ~3.6 s           | ~2.5 s | ~5 s   | ~7 s   | 12.5 s     |
| normal  | 38    | ~7.5 s           | ~6 s   | ~11 s  | ~15 s  | ~20 s      |
| formal  | 38    | ~9.8 s           | ~7 s   | ~14 s  | ~19 s  | ~24.7 s    |

(These are eyeballed from the raw report — Wave C eval will compute precisely
once the runner emits per-call CSV.)

**Implication:** even after Wave C prompt tightening, normal will still feel
slow (5-10 s) on a 6 GB card without streaming. **The honest UX answer
includes recording-window text streaming** — that's mb-cjc Wave 3 / a new
ticket.

## What "good" looks like after Wave C

Concrete targets for the next eval run (`iter-1`):

| Mode    | Preservation (lexical+alts) | Median latency | p80 latency |
|---------|------------------------------|----------------|-------------|
| casual  | ≥ 95% full, **0 zero**       | ≤ 2 s          | ≤ 3.5 s     |
| normal  | ≥ 95% full                   | ≤ 5 s          | ≤ 8 s       |
| formal  | ≥ 80% full (post-alts patch) | ≤ 6 s          | ≤ 10 s      |

Anything else is a stretch goal. Latency targets remain above Wisprflow parity
until streaming lands — flagged.

## Wave C plan (ordered)

1. **Patch scorer** (`mode_eval/report.rs`): split on hyphens in `normalise`;
   add `must_preserve_alts` support; re-render the baseline report against
   the same outputs to get a corrected "before" number for ADR 0024.
2. **Author `casual_v2`** focused on the hallucination fix (anti-improvisation
   guard, lower temperature). Re-run eval → `iter-1`.
3. **Author `normal_v5`** tightened (less filler in prompt, possibly smaller
   `max_tokens`). Re-run eval → `iter-2`.
4. **Author `formal_v2`** with proper-noun-preservation + emotional-charge
   rules. Re-run eval → `iter-3`.
5. **Final eval** (`iter-final`) confirming all targets. If we hit the bar,
   move to Wave D (migration 010 + ADR seal + UI/wiring doc).

## Cost ledger

Wave A (rig): ~20 min coding, ~17 min compute (build + baseline run).
Wave B (this doc): ~30 min analysis. Hand-off ready.

Wave C estimate: 3-4 prompt-author cycles × ~15 min compute each = **~1 hour
total compute**, plus ~1 hour authoring. Worth it.
