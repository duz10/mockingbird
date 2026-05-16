# ADR-0008: Prompt versioning — no edits after shipment

- **Status:** Accepted
- **Date:** 2026-05-15
- **Deciders:** Dustin, code-puppy-adeb7b

## Context

The cleanup LLM operates against versioned prompts per mode
(Default/Email/Code/Casual). Without strict versioning, a single
prompt edit changes the output of every replay/eval against historical
transcripts, breaking provenance.

## Decision

Prompts in `src-tauri/src/cleanup/prompts/<mode>_v{N}.md` are
**immutable after shipment**. When a prompt needs to change:

1. Author `<mode>_v{N+1}.md` next to the existing file
2. Update the registry `prompts/index.toml` to map
   `(mode, latest) → v{N+1}`
3. Write a migration INSERT for the `prompts` table (Phase 1+ schema)
   that adds the new version row; do NOT update the old one
4. Each transcript's `prompts.prompt_version` FK keeps pointing at
   the exact version used when it was cleaned, forever

Few-shot examples in prompt files use ONLY:
- Synthetic examples (no PII)
- Or the user's *opt-in* corrected examples (Phase 6+)

Max 8 examples per prompt. Mode is explicit at request time — no
mode inference inside the prompt.

## Consequences

- **Positive:** total provenance — `(transcript, prompt_version)`
  is reproducible at any future point.
- **Negative:** prompt files accumulate over time. Mitigated: SQLite
  is fine with hundreds of small text rows, and old prompt .md files
  on disk are static (~few KB each).
- **Neutral:** the `prompts` skill enforces this at author time
  (via the `prompt-tuner` project agent).

## Alternatives considered

- **Mutate-in-place + git blame for history:** loses the DB-side
  reproducibility guarantee. Replays against pre-edit transcripts
  would silently use the new prompt.
- **Single mega-prompt with mode toggles:** harder to version,
  larger context, conflates concerns.

## Cross-references

- PLAN §8 (cleanup pipeline), §7 (provenance schema)
- `.code_puppy/skills/prompts/SKILL.md`
- `.code_puppy/agents/prompt-tuner.json`
