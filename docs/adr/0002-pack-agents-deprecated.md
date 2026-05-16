# ADR-0002: Pack agents deprecated; `code-puppy` is the orchestrator

- **Status:** Accepted
- **Date:** 2026-05-15
- **Deciders:** Dustin, code-puppy-adeb7b

## Context

PLAN.md §11 and Appendix G referenced "pack agents" (pack-leader,
bloodhound, shepherd, terrier, watchdog, retriever) as the
multi-agent orchestration layer. During bootstrap kickoff, Dustin
confirmed those names are **reserved but unimplemented** in current
Code Puppy and should not be enabled.

## Decision

The active agent for any `/goal` run is **`code-puppy`** (this puppy)
or one of the project JSON agents (migration-author, injection-author,
ui-author, prompt-tuner, learning-loop-author). Delegation goes
through `invoke_agent(...)` to the framework agents that DO exist:

- `planning-agent` — phase decomposition
- `qa-kitten` — Playwright UI verification
- `helios` — build missing tools
- `agent-creator` — mint new project JSON agents

`puppy.cfg` setting `enable_pack_agents=true` from PLAN §10 Phase 0
is explicitly NOT applied.

## Consequences

- **Positive:** one orchestrator, one chain of responsibility, easier
  to reason about which agent owns what.
- **Negative:** loss of theoretical specialization (e.g., a dedicated
  "code review" pack member). Mitigated by the project JSON agents,
  which are scoped specialists in everything but name.
- **Neutral:** the names are reserved for future Code Puppy versions.
  If pack agents return, revisit this ADR.

## Alternatives considered

- **Wait for pack agents to ship:** unbounded, blocks our timeline.
- **Hand-roll equivalents:** YAGNI — code-puppy + 5 project agents
  cover every Phase-1-through-Phase-8 role.

## Cross-references

- PLAN §11 (workflow), Appendix G (project agents)
- STATUS.md "Section −1 resolution" table, item 9
