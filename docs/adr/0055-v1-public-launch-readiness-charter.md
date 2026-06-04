# ADR-0055: v1 Public Launch Readiness charter

- **Status:** Proposed
- **Date:** 2026-06-04
- **Deciders:** Dustin (project lead), code-puppy (implementor)

## Context

Mockingbird shipped its first beta tag `v0.2.0-beta.1` at `e9e7f8c` (Phase 1F /
`mb-udkb`), then sealed a Tauri-2 state-management bug at `e2fb15b` (Bug 4 /
`mb-1z0m`). The binary now installs and runs cleanly on Dustin's Win11 box,
all gates GREEN, with three follow-up beads filed under Phase 1F
(`mb-jbi0` Win11 smoke matrix, `mb-h1n9` installer + updater verification,
`mb-n40v` marketing-shaped privacy + docs polish).

Dustin's intent shifted from "beta in a private repo for personal use" to
"reference implementation, fork-friendly, not actively maintained for
community PRs". This means a public repo flip is now in scope, and that
flip surfaces a coherent body of work that does not fit either:

- A single bead (it spans ~12-20 files, multi-session, has user-visible
  decisions to lock).
- A new PLAN §10 numbered phase (the work is product launch hardening, not
  a new subsystem from the v2 PLAN).

This is the canonical shape of an **ADR-chartered lateral epic** per
AGENTS.md "Work sizing & workflow selection" (cf. ADRs 0022, 0023, 0032,
0033, 0035, 0036, 0037).

A multi-message planning conversation between Dustin and the planning-agent
locked ten decisions and a seven-wave decomposition; this ADR records them
in one place so the wave dispatches reference a single charter rather than
re-deriving the constraint set.

## Decision

We will execute a single **v1 Public Launch Readiness** lateral epic across
seven sequential waves (LR.0 through LR.6) that brings the existing
`v0.2.0-beta.1` build to a clean public GitHub repo at `duz10/mockingbird`,
under the constraints below.

### Locked decisions

1. **Internal docs gitignored, not relocated.** `docs/adr/`, `docs/phases/`,
   `docs/judges/`, `docs/historical/`, `docs/archive/`, `docs/audits/`,
   `docs/reviews/`, `docs/spikes/`, `docs/cleanup/`, `docs/design/`,
   `docs/knowledge-graph/`, plus `docs/LESSONS.md`, `docs/PRODUCT-STATE.md`,
   `docs/DATA_MODEL.md`, `docs/SETTINGS.md`, `docs/CONTRIBUTING.md`, plus
   top-level `STATUS.md`, `PLAN-mockingbird-v2.md`,
   `mockingbird-activity-capture-plan.md`, `AGENTS.md` all get gitignored
   so they live as maintainer artifacts on disk only. `docs/mobile/` and
   the to-be-created `docs/research/macos-implementation-notes.md` stay
   user-facing.
2. **No Wispr Flow positioning anywhere in user-facing content.** The
   single existing reference at `src-tauri/tauri.conf.json:93`
   (`longDescription`) gets rewritten in LR.2.
3. **Code Puppy + Wiggum + beads + hooks + agents methodology hidden.**
   `.code_puppy/` gitignored. No methodology framing in any external doc.
   One sanctioned line in `ARCHITECTURE.md` reading "Built solo with AI
   coding assistance" is allowed; nothing further.
4. **No em dashes in any user-facing markdown or commit message.** Use
   hyphens, commas, parens, or restructured sentences. Internal docs
   (ADRs, this charter, the audit report) may use em dashes; user-visible
   summary sections in internal docs default to em-dash-free in case they
   get quoted outward.
5. **No mentions of:** Wispr Flow, Code Puppy, Wiggum, beads,
   ADR-as-workflow-concept, LESSONS, the phase model, judges, the hook
   engine, project agents, the `/goal` plugin — in any external-facing
   file (`README.md`, `CHANGELOG.md`, `CONTRIBUTING.md`, `INSTALL.md`,
   `PREREQS.md`, `SECURITY.md`, `PRIVACY.md`, `CODE_OF_CONDUCT.md`,
   `ARCHITECTURE.md`, `LICENSE`, `tauri.conf.json`, GitHub repo metadata).
6. **Unsplash key migrates to DPAPI.** Adds `SecretKind::UnsplashApiKey`,
   wires `unsplash_get_key` / `unsplash_set_key` Tauri commands, rewrites
   `ui/src/components/UnsplashBackground/prefs.ts` to use them, adds a
   one-time `localStorage` migration, updates the i18n string to mention
   DPAPI. Sized 150-250 LoC, one focused session. **This is LR.0.B.**
7. **GitHub username `duz10`, repo name `mockingbird`** (simplified from
   the earlier `mockingbird-vtt` candidate — Dustin re-locked the simpler
   name).
8. **Private repo throughout, public flip in LR.6.** The repo is created
   private at `duz10/mockingbird` in LR.4, mirrored for CI exercise in
   LR.3, public-flipped in LR.6 with the first public release tag also
   landing in LR.6.
9. **Version stays `v0.2.0-beta.1` for first public release.** The
   `CHANGELOG.md` entry gets a user-friendly rewrite (drop methodology
   framing, drop the "beads against this release" sentence) but the
   semver number does not bump.
10. **Single-line AI acknowledgment in `ARCHITECTURE.md` allowed.** One
    line, no further methodology detail. Exact wording lands in LR.2.

### Seven-wave structure

| Wave | Scope | Sealing artifact |
|---|---|---|
| LR.0.A | Static security audit (this wave): history scan, env audit, key location, cargo audit, npm audit, IOC scan, scrubber regex extension, internal-doc exposure map, Tauri config audit, tracked-binary scan. Reports findings; does not remediate. | Audit report committed at `docs/audits/v1-public-launch-security.md`; sub-bead closed. |
| LR.0.B | Unsplash API key DPAPI migration per locked decision #6. Wires `SecretKind::UnsplashApiKey`, `unsplash_get_key` / `unsplash_set_key` commands, rewrites `prefs.ts`, adds localStorage one-time migration, updates i18n string, tests both sides. | Code lands; sub-bead closed. |
| LR.1 | Aggressive gitignore expansion per locked decision #1. `.code_puppy/`, `AGENTS.md`, `PLAN-mockingbird-v2.md`, `STATUS.md`, internal `docs/` subtrees, stale top-level files (`mockingbird-logo-motion.html`, `verify_iter1_schema.py`, top-level `build_phase*.log`). Plus repo hygiene: `scripts/` split into user-facing (~5 scripts) and `scripts/dev/` (everything else). | Working tree post-gitignore matches the planned repo layout from the planning conversation. Sub-bead closed. |
| LR.2 | User docs rewrite: `README.md`, `CHANGELOG.md`, `CONTRIBUTING.md` polished per the LR.0.A exposure map. New `INSTALL.md`, `PREREQS.md`, `SECURITY.md`, `PRIVACY.md`, `CODE_OF_CONDUCT.md`, `ARCHITECTURE.md` authored. `tauri.conf.json` longDescription rewritten. `docs/research/macos-implementation-notes.md` authored. `mb-n40v` (privacy + docs polish) folds in here. | All user-facing docs em-dash-free per locked #4, methodology-free per #3 and #5. Sub-bead closed. |
| LR.3 | CI + release workflow + stale bot + minimal issue/PR templates. `cargo audit` added to CI. `npm audit` added to CI. ESLint v9 migration (`mb-yxh`) folds in. `mb-h1n9` (installer + updater verification) folds in. | CI matrix GREEN on a private push. Sub-bead closed. |
| LR.4 | `gh repo create mockingbird --private` on `duz10`. Push private. Verify CI runs against the private repo. | Private repo lives at `github.com/duz10/mockingbird` with GREEN CI. Sub-bead closed. |
| LR.5 | GitHub Pages static landing site (project page, not user docs). | Pages site live at `duz10.github.io/mockingbird`. Sub-bead closed. |
| LR.6 | Public flip + first public release tag. Pre-flip audit pass over LR.0.A's exposure map (any em dash, any methodology reference, any Wispr/Code-Puppy/beads/etc mention = blocker). `mb-jbi0` (Win11 smoke matrix) folds in as the final smoke. Cut a release with `v0.2.0-beta.1` artifacts (msi + exe + signatures). | Repo public. Release page live. Epic bead closed. |

### Wave dependency graph

```
LR.0.A -.
         \
LR.0.B ---+--> LR.1 ---+--> LR.2 -.
                       |          \
                       +--> LR.3 --+--> LR.4 --> LR.5 --> LR.6
                       ^                                  ^
                       +----------------------------------+
                       (LR.6 depends on everything)
```

## Consequences

- **Positive:** A single charter ADR pins the constraint set so the seven
  wave dispatches stay short and pointer-style (per AGENTS.md
  `session_id` discipline). The locked decision list is the canonical
  reference for every "is this allowed in the external docs?" question
  during LR.2 through LR.6. The wave decomposition lets LR.0.A and LR.0.B
  proceed in parallel (LR.0.B does not depend on LR.0.A findings; the
  audit at LR.0.A turned up no blockers for LR.0.B).
- **Positive:** Gitignoring internal docs rather than relocating them
  preserves on-disk maintainer continuity (every working-memory pointer
  in AGENTS.md, the kennel, and prior STATUS entries keeps pointing at
  the same paths) while presenting a clean public surface. No
  document-path rewrites needed in any internal file.
- **Negative:** A first-time public visitor cannot see ADRs or
  architectural decision history. The trade-off is intentional per the
  "not actively maintained for community PRs" posture; an outside
  contributor who needs that history can fork and ask, or the project
  can selectively un-gitignore individual ADRs later (no migration
  needed; just remove the gitignore line).
- **Negative:** The locked "no em dashes in external docs" rule is a
  style constraint that the LR.2 author and the LR.6 final audit must
  mechanically check. The audit step for this is a one-line
  `Select-String -Pattern '\u2014'` against the user-facing files;
  cheap, but easy to forget.
- **Neutral:** `v0.2.0-beta.1` becomes the first public release tag.
  Future post-launch semver bumps follow normal Keep-a-Changelog
  cadence; no special "public-launch" version designation.
- **Neutral:** Charter ADR itself gets gitignored by LR.1 alongside the
  rest of `docs/adr/`. It lives on disk as a maintainer artifact only.
  This is intentional per locked decision #1.

## Alternatives considered

- **Single mega-bead.** Rejected: ~12-20 file scope, multi-session,
  user-visible decisions that need lockable provenance. A bead title
  cannot carry the locked-decision list. Charter ADR is the right shape.
- **New PLAN §10 numbered phase (Phase 11: Public Launch).** Rejected:
  the work is product hardening, not a new subsystem from the v2 PLAN.
  Per AGENTS.md "Permanently sealed" + LESSONS PINNED P5, numbered
  phases seal via `phase-N-complete` git tags reserved for PLAN §10
  subsystems. This is the canonical lateral-epic shape that the ADRs
  0022 / 0032 / 0035 / 0037 family established.
- **Relocate internal docs under `docs/internal/` instead of
  gitignoring.** Rejected by Dustin in the planning conversation. The
  public repo would still surface the parent `docs/internal/` directory
  name and a tree-view would still show its existence; the cleaner
  story is "internal docs simply do not appear in the public repo".
- **Repo name `mockingbird-vtt` or `mockingbird-dictation`.** Both
  considered. Dustin re-locked the simpler `mockingbird` for the
  username `duz10`. The collision risk with the older
  github.com/mockingbird name namespace is offset by the username
  scoping (`duz10/mockingbird`).
- **Public flip at LR.4 with the repo creation.** Rejected: deferring
  the flip to LR.6 keeps CI exercise (LR.3 -> LR.4) inside the private
  perimeter so the first public push lands with a working CI matrix
  rather than turning red minutes after going public.

## Cross-references

- AGENTS.md "Work sizing & workflow selection" - chartering rationale
- AGENTS.md "Permanently sealed" - why this is NOT a new numbered phase
- LESSONS PINNED P4 - session-start triage (kickoff prompt clearance)
- LESSONS PINNED P5 - phase-tag discipline for lateral epics
- LESSONS PINNED P13 - cargo build --release after manifest touches
  (will fire in LR.0.B and LR.2 if `tauri.conf.json` is touched)
- ADR 0005 (code-signing deferred) - updater pubkey already in
  `tauri.conf.json`; LR.6 release uses existing signing path
- ADR 0006 (npm ignore-scripts) - applies to LR.0.B + LR.2 + LR.3
  npm work
- ADR 0011 (whisper-rs CUDA build) - LR.3 CI must use the cargo wrapper
- ADR 0017 (DPAPI for secrets) - LR.0.B extends this to Unsplash
- ADR 0035 (MC v1.2 stable alpha) - structural precedent for an
  ADR-chartered cleanup epic that does not bump a phase tag
- `docs/audits/v1-public-launch-security.md` - LR.0.A audit output
  (the exposure map there is the input list for LR.1 + LR.2)

---

_This ADR is gitignored by LR.1's gitignore expansion (per locked
decision #1). It lives on disk as a maintainer artifact only and is
referenced from this charter forward in commit messages and bead
descriptions via its `ADR 0055` identifier._
