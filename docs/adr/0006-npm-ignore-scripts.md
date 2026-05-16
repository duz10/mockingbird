# ADR-0006: npm `--ignore-scripts` mandatory

- **Status:** Accepted
- **Date:** 2026-05-15
- **Deciders:** Dustin, code-puppy-adeb7b

## Context

Post-Shai-Hulud (Sept 2025) and Mini Shai-Hulud (May 2026), arbitrary
code execution at npm install time is the dominant supply-chain
attack vector. Packages run `preinstall` / `postinstall` scripts as
the installing user with full machine privileges. The npm advisory
DB does not catch malicious package versions until hours after
publication, by which time CI and developer machines may be
compromised.

## Decision

All npm/pnpm/yarn install commands MUST run with `--ignore-scripts`.
Enforced via three independent layers (defense in depth):

1. `.npmrc` at repo root: `ignore-scripts=true`
2. `.code_puppy/settings.json` hook `block-unsafe-npm` refuses any
   `npm/pnpm/yarn install|ci|i` command without the explicit flag
3. The `supply-chain` skill documents the rationale and lists
   legitimate carve-outs (none exist today).

If a future dep genuinely requires a postinstall script (native
build), the team will: (a) audit the script, (b) vendor the built
artifact, (c) write a follow-up ADR documenting the carve-out.

## Consequences

- **Positive:** install-time RCE blocked by default.
- **Negative:** some packages with legitimate build steps require
  manual workaround. So far we have zero such deps; revisit if it
  becomes a real friction.
- **Neutral:** lockfile (`package-lock.json`) is committed; review
  diffs at every PR.

## Alternatives considered

- **Trust npm:** historically catastrophic. Not an option.
- **Sandbox installs (Docker, etc.):** heavyweight for a desktop dev
  loop; doesn't help when a malicious package then ships to users.

## Cross-references

- PLAN Appendix D
- `.code_puppy/skills/supply-chain/SKILL.md`
- `scripts/hooks/block-unsafe-npm.py`
- `.npmrc`
