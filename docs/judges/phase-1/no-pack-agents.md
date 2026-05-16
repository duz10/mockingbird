# Judge: no-pack-agents (Phase 1)

**Target:** `.code_puppy/AGENTS.md`, `.code_puppy/skills/**`, `docs/**`, `STATUS.md`

**Question:** Are pack agents (`pack-leader`, `bloodhound`, etc.) absent from all agent invocations, configurations, and operational docs in this phase?

**Rationale:** Pack agents are deprecated in Code Puppy per upstream confirmation; the standing rule for this iteration is to use the explicit-delegation model (`code-puppy` as implementor, invoking `planning-agent`/`qa-kitten`/`helios`/`agent-creator` and the project JSON agents directly). Drift back to pack agents would adopt an unmaintained orchestration pattern and lose the explicit-handoff trail.

**Pass criteria:**

```bash
grep -ri "pack-leader\|bloodhound\|pack_leader\|pack agent" \
     .code_puppy/ docs/ STATUS.md
```

Returns only entries that **document the deprecation** (e.g.,
"Pack agents are DEPRECATED" notices). Zero entries actually
invoking or configuring a pack agent.

**On failure:**

- Note in `docs/LESSONS.md`.
- Audit recent agent invocations in the session log.
- If a sub-agent unexpectedly references pack agents, file an
  upstream issue and stop using that sub-agent until clarified.

**Last run:** _Wave 5 (judge cards minted; Wiggum to execute when wired up). Bootstrap confirmed deprecation in `STATUS.md`; Phase 0 mint of `migration-author`/`injection-author`/etc. followed the explicit-delegation pattern._
