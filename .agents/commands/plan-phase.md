---
description: Delegate phase decomposition to planning-agent. Produces docs/phases/phaseN.md.
---

You are kicking off **planning** for a new phase. The implementor
(code-puppy) is about to wake up and needs a binding plan.

## Usage

`/plan-phase {N}` where N ∈ {0..8}.

## What this should do

1. Read `PLAN-mockingbird-v2.md` § "Phase {N}" + Section 4 (layout)
   + Section 12 (do-not-skip).
2. Read `STATUS.md` to see what's already done.
3. Read `docs/LESSONS.md` — search for the phase tag.
4. Invoke `planning-agent` with a tight prompt: "decompose Phase {N}
   into 10–25 tasks with dependency graph, exit criteria, judges,
   risks, and iteration estimate."
5. Write the result to `docs/phases/phase{N}.md` (the binding plan).
6. Seed beads with the tasks (with proper `bd link` dependencies).
7. Commit + push.

After this, the human runs `/agent code-puppy` then
`/phase{N}-goal` to start implementation iterations.
