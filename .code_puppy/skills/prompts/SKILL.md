---
name: prompts
description: Cleanup LLM prompt design, versioning, and few-shot construction for Mockingbird. Activate this skill whenever you are editing files under `src-tauri/src/cleanup/prompts/`, designing a new mode (Email, Code, Casual…), or working with prompt-tuner.
---

# Mockingbird prompts

## Where prompts live

- `src-tauri/src/cleanup/prompts/<mode>.md` — versioned, immutable once shipped
- `src-tauri/src/cleanup/prompts/index.toml` — registry mapping
  `(mode, version) → file_path`
- `prompts.prompt_version` column in DB — every cleaned row points at one

## Hard rules

1. **Prompt files are versioned**, never edited in place after shipment.
   When you need to change a prompt, write `email_v3.md` next to
   `email_v2.md` and bump the index. The judge prompts (under
   `~/.code_puppy/judges.json`) are versioned the same way.

2. **No PII in examples.** Few-shot examples in prompt files use only
   the user's *opt-in* corrected examples and synthetic substitutes.
   Never paste raw transcripts into prompt files — they're immutable
   in DB for a reason; freezing them into a prompt creates a leak.

3. **Mode is explicit at request time.** The cleanup pipeline always
   knows which mode (Default/Email/Code/Casual) it's running. No mode
   inference inside the prompt — the caller supplies it.

4. **Dictionary substitution happens *before* the LLM call.** Custom
   word replacements (e.g. "Bernarrd" → "Bernard") are applied as
   a deterministic pass; the LLM sees the corrected text and only does
   the harder language work.

## Few-shot construction

- Maximum 8 examples per prompt; more wastes tokens and confuses the
  model.
- Examples are drawn from the user's `examples_set` (Phase 5+), pinned
  by `examples_set_id` for provenance.
- Each example is `{raw}` → `{cleaned}` — no chain-of-thought, the model
  is doing translation, not reasoning.
- New modes start with 3 examples and grow only when an eval shows a
  regression that more examples would fix.

## When prompt-tuner is the right tool

`invoke_agent("prompt-tuner", ...)` for:
- Diagnosing why a mode is regressing on the user's eval set
- Drafting a new prompt version with an A/B plan
- Designing the eval cases for a new mode

Don't invoke prompt-tuner for trivial typo fixes — those are inline edits.

## Cross-references

- PLAN Section 8 — cleanup pipeline
- ADR 0007 (prompt versioning) — write if not present
- skill: data-model (for the provenance columns)
