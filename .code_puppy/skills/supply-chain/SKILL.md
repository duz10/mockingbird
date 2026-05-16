---
name: supply-chain
description: npm and Cargo supply-chain hygiene for Mockingbird in the post-Shai-Hulud world. Activate this skill whenever you are adding, upgrading, or auditing a dependency — JS or Rust — or reviewing `package.json`, `package-lock.json`, `Cargo.toml`, `Cargo.lock`.
---

# Mockingbird supply-chain hygiene

## The world we live in (PLAN Appendix D)

- Sept 2025: original Shai-Hulud worm — 187 npm packages compromised
- May 2026: **Mini Shai-Hulud** — 84 versions across 42 `@tanstack/*` packages
- We assume more is coming. Defaults are restrictive.

## Defaults

### npm

- `.npmrc` at repo root: `ignore-scripts=true`
- Every `npm install|ci|i` MUST include `--ignore-scripts`.
  Hook `block-unsafe-npm` refuses bare invocations.
- **`@tanstack/*` is banned** project-wide. Hook `block-tanstack` refuses
  any `package.json` that adds it. Use `react-window` for virtualization;
  hand-roll the rest.
- Lockfile (`package-lock.json`) is committed; review diffs at every PR.
- New deps go through a 2-step add: first review on a branch with
  `npm audit --omit=dev`, then merge.

### Cargo

- `Cargo.lock` is committed (we ship a binary).
- Prefer crates from the [Rust core team](https://github.com/rust-lang)
  or maintainers with 2FA enabled.
- Avoid `*-sys` crates unless they wrap a well-known C library; check
  the build script.
- `cargo audit` runs in CI; fix every warning or write an
  `cargo audit --ignore` entry with a justification commit message.

## Adding a dependency: the checklist

- [ ] What does it do that I can't write in < 200 LOC?
- [ ] Is it actively maintained? (last commit < 12 months)
- [ ] Does it pull in anything from a flagged namespace?
- [ ] Does its README claim any networking the app shouldn't do?
- [ ] Pin the version (exact, not `^` for npm, `=` allowed for Cargo).
- [ ] Document in `docs/dependencies.md` why it's worth the surface area.

## IOC lists to check

- PLAN Appendix D ships a frozen list of compromised packages.
  Update it any time you become aware of a new event.
- For npm: cross-reference against `npm audit` and the npm advisory DB.
- For Cargo: cross-reference against `cargo audit` and RustSec.

## Cross-references

- PLAN Appendix D — full IOC and history
- ADR 0003 (no-tanstack) — write if not present
- ADR 0004 (npm ignore-scripts default) — write if not present
