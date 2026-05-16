# ADR-0003: No `@tanstack/*` dependencies (Mini Shai-Hulud IOC)

- **Status:** Accepted
- **Date:** 2026-05-15
- **Deciders:** Dustin, code-puppy-adeb7b

## Context

The May 2026 "Mini Shai-Hulud" npm supply-chain compromise published
84 malicious versions across 42 packages in the `@tanstack/*`
namespace (PLAN Appendix D). The blast radius is too large to audit
case-by-case, and TanStack tooling (Query, Router, Table) is popular
enough that pinning could lull future contributors into a false
sense of safety.

## Decision

The `@tanstack/*` npm namespace is **banned project-wide**, enforced
at three layers:

1. `.npmrc ignore-scripts=true` (cannot run install scripts even if
   slipped in)
2. `.code_puppy/settings.json` hook `block-tanstack` — refuses any
   `package.json` change that adds an `@tanstack/*` dep
3. The `supply-chain` skill instructs contributors to use
   `react-window` for virtualization or hand-roll the component.

## Consequences

- **Positive:** zero exposure to the Mini Shai-Hulud blast radius
  and any successor incidents in the same namespace.
- **Negative:** loss of TanStack ergonomics — `@tanstack/react-table`
  in particular is genuinely well-designed. Mitigated by writing
  ~200 LOC of focused table component when needed.
- **Neutral:** other organizations may use `@tanstack/*` safely;
  this is a Mockingbird-specific choice.

## Alternatives considered

- **Pin pre-compromise versions only:** still requires manual
  vetting at every upgrade; one slip ships malware.
- **Wait for post-mortem and re-evaluate:** open-ended. We'd rather
  rewrite a table than re-vet 42 packages.

## Cross-references

- PLAN Appendix D (IOC list + history)
- `.code_puppy/skills/supply-chain/SKILL.md`
- `scripts/hooks/block-tanstack.py`
