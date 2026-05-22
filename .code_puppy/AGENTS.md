# AGENTS.md — Mockingbird project rules

## Project context

You are working on **Mockingbird**, a local-first voice dictation +
meeting-capture app for Windows (Mac support planned for Phase 9). It
replaces Wispr Flow with a fully local, privacy-respecting implementation.

**The fast onboarding path** (~10 min cold-start, do this every session):

1. `STATUS.md` (~150 lines) — what's sealed, what's in flight, how to resume.
2. `docs/PRODUCT-STATE.md` — comprehensive subsystem map. What ships, by area.
3. `docs/LESSONS.md` PINNED block (top of file) — load-bearing gotchas.
4. **This file** — rules, principles, never-do list.
5. For active phase work: `docs/phases/phase{N}.md` + any wave briefs.
   For active ADR-chartered epics: the chartering ADR (`docs/adr/`).

The canonical full spec is in `PLAN-mockingbird-v2.md` — only re-read it if
the phase doc or PRODUCT-STATE.md is silent on what you need.
The "do not skip" list in PLAN Section 12 is binding and is also
mechanically enforced by `.code_puppy/settings.json` hooks.

## Workflow

You are running inside Code Puppy with the Wiggum `/goal` plugin.
Between iterations, your conversation context is cleared. The
persistent state is:

- This file (AGENTS.md)
- PLAN.md spine + `docs/phases/phase{N}.md`
- `STATUS.md` (the current phase status — you MUST update each iteration)
- The workspace files (code, tests, docs)
- Git history (tags `phase-{N}-complete` mark phase boundaries)
- `docs/LESSONS.md` (append-only notes from prior iterations)
- The hook engine config at `.code_puppy/settings.json`
- **The beads (`bd`) issue database** — the live task queue (see Issue Tracking below)

### At the start of every iteration:

1. Read `STATUS.md` SESSION ANCHOR (top ~30 lines) — what's sealed, what's in-flight.
2. Read `docs/PRODUCT-STATE.md` (skim — 3 min) if you don't already have the
   product model in working memory.
3. Read `docs/LESSONS.md` PINNED block (top of file).
4. Read this file (AGENTS.md).
5. For the current phase / active epic: `docs/phases/phase{N}.md` or the
   chartering ADR.
6. `bd ready` — unblocked work queue. `bd prime` for workflow context.
7. `git log --oneline -20` + `git tag --list "phase-*"` + `git status`.
8. THEN start work.

### Delegation

You are the active agent for a /goal run (usually code-puppy, sometimes
a project JSON agent like migration-author / ui-author / etc.).
Delegate to specialists via `invoke_agent`:

- **planning-agent** (📋) — decompose the deliverables for a new phase
- **qa-kitten** (🐱) — UI / visual verification with Playwright
- **helios** (☀️) — build one-off tools you need
- **agent-creator** — mint new JSON agents (Phase 0 only, usually)
- **Project JSON agents** (migration-author, injection-author, ui-author,
  prompt-tuner, learning-loop-author) — scoped specialists; can also
  be the active agent for an entire phase if the work is narrow

Pack agents (pack-leader, bloodhound, shepherd, terrier, watchdog,
retriever) are **DEPRECATED** in Code Puppy. The framework reserves the
names but ships no implementations. Do not attempt to invoke them.

**`session_id` discipline for `invoke_agent` (LESSONS P8):**

- `session_id` is for **conversational refinement of ONE task**
  (clarify-ask-respond rounds within a single scope of work). Use it
  when you need to follow up with the sub-agent about the same
  deliverable.
- For **serial task handoffs** (Wave N → Wave N+1, or task A → task B
  where each task has its own kickoff), **omit `session_id`**. Each
  dispatch should be its own fresh sub-agent invocation with its own
  clean kickoff anchor. Reusing `session_id` across serial tasks causes
  the sub-agent's session-start ritual to anchor on the first message
  in the session as "the kickoff" — which by dispatch #2+ is stale
  relative to disk, triggering an endless stop-and-surface loop
  (the 2026-05-25 Wave 1A incident; 8 attempts burned).
- Keep dispatch prompts **short and pointer-style**: "implement X per
  `<spec path on disk>`" rather than embedding the full spec. The spec
  lives on disk; embedding it in the prompt makes the prompt body look
  like potential stale charter work to the session-start triage.

### At the end of every iteration (before exit):

1. **STATUS.md** — update only if epic state changed (in-flight block) or a phase
   sealed. STATUS.md is intentionally slim now; don't add session diary to it.
2. **`docs/PRODUCT-STATE.md`** — update only if a subsystem shipped or materially
   changed. This is the durable reference, not a journal.
3. **`bd`** — close completed beads (`bd close <id>`), create new ones for any
   work discovered mid-iteration (`bd create ... --type task --priority N`).
4. **`docs/LESSONS.md`** — append a dated entry if a non-obvious thing happened.
   If the finding would change how EVERY future session should work, also
   promote it into the PINNED block at the top.
5. **Cargo gate** via the Windows wrapper (see "Build / run / test environment"
   section below):
   - `powershell -File scripts\cargo-with-cuda.ps1 fmt --check`
   - `powershell -File scripts\cargo-with-cuda.ps1 clippy --release -- -D warnings`
   - `powershell -File scripts\cargo-with-cuda.ps1 test --release` (use `--no-run`
     fallback when the documented launch failure hits; pure-Rust modules go
     through the throwaway-crate recipe — same section)
6. **UI gate:** `npx tsc --noEmit`, `npm test`, `npm run build` (whichever apply).
   `npm run lint` is currently broken pending `mb-yxh` (ESLint v9 migration).
7. **Commit** all changes with a descriptive message referencing the bead id +
   ADR if any.
8. **Tags:** `git tag phase-{N}-complete` ONLY when completing a numbered PLAN §10
   phase. Lateral epics seal via Accepted ADR + STATUS update, NOT by a new tag.
9. The `stop` hook will refuse exit if any of the above failed.

## Coding standards (summary; full version in `docs/QUALITY.md`)

### Rust
- Edition 2021, Rust ≥ 1.77
- `cargo fmt` is law; `cargo clippy --release -- -D warnings` must pass
  (release flag required to reuse whisper-rs-sys CUDA artifacts;
  LESSONS 2026-05-15)
- **All cargo invocations on Windows go through `scripts\cargo-with-cuda.ps1`** —
  plain `cargo` compiles but produces binaries that may not launch.
  See "Build / run / test environment (Windows)" section below.
- `Result<T, E>` everywhere; `unwrap()` only in tests
- `thiserror::Error` for module error types
- `tracing` for logging (not `println!`)
- File size hard limit: 600 lines

### TypeScript / React
- Strict mode TS; no `any` without a `// SAFETY:` comment
- React 19 conventions; no class components
- ESLint config is law
- Tailwind v4 for styling; design tokens from `ui/src/design/tokens.css`
- No `@tanstack/*` — hook will block

### Testing
- Every non-trivial function gets a unit test
- Test files mirror source layout
- `rstest` for parameterized tests
- `proptest` for property tests
- Mock at trait boundaries via `mockall`
- E2E / visual tests via Playwright (qa-kitten authors)

### Documentation
- ADR for any non-trivial architectural decision
- Module-level docs explain WHY, not WHAT
- Doc comments on public APIs
- Append to `docs/LESSONS.md` on non-obvious findings

## Build / run / test environment (Windows)

**This is a Windows + CUDA dev box.** Plain `cargo` invocations compile
but produce binaries that may fail to launch due to missing MSVC + CUDA
runtime env. ALL cargo invocations MUST go through the project wrapper.

### The cargo wrapper

```
powershell -File scripts\cargo-with-cuda.ps1 <cargo-args>
```

Common invocations:
- `powershell -File scripts\cargo-with-cuda.ps1 check`
- `powershell -File scripts\cargo-with-cuda.ps1 clippy --release -- -D warnings`
- `powershell -File scripts\cargo-with-cuda.ps1 test --release`
- `powershell -File scripts\cargo-with-cuda.ps1 fmt --check`
- `powershell -File scripts\cargo-with-cuda.ps1 build --release`

What it does (full details in the script header):
1. Imports MSVC env via `vcvars64.bat`.
2. Pins `CUDA_PATH` + `CUDA_PATH_V12_8` to v12.8 (ADR 0011).
3. Prepends `cmake` to PATH.
4. Caps `CMAKE_BUILD_PARALLEL_LEVEL=4` (whisper-rs CUDA OOMs at 16).
5. Forwards args through `cmd.exe /c` (stream-flattening; LESSONS 2026-05-17).

### Shell: `powershell.exe`, NOT `pwsh`

PowerShell 7 (`pwsh`) is **not on PATH** on this box. Use PS 5.1
(`powershell.exe`) and invoke the wrapper via `-File`:

| ✓ Works                                                | ✗ Fails                                                          |
|--------------------------------------------------------|------------------------------------------------------------------|
| `powershell -File scripts\cargo-with-cuda.ps1 …`       | `pwsh scripts\cargo-with-cuda.ps1 …` (`'pwsh' not recognized`)   |
|                                                        | `powershell -Command "scripts\… ..."` (LESSONS: breaks arg pass) |

LESSONS 2026-05-17 documents both pitfalls under "Finding 4".

### Running the app

Use the launcher script — never `Start-Process target\release\mockingbird.exe` directly:

```
powershell -File scripts\run-mockingbird.ps1
```

The launcher sets `ORT_DYLIB_PATH` and prepends CUDA bin to `PATH` so
the binary can load `onnxruntime.dll` (Silero VAD) and `cudart64_12.dll`
(whisper-rs CUDA) at process start. Launching the exe directly omits
both and produces hard-to-diagnose `STATUS_DLL_NOT_FOUND` or silent
VAD failure.

### Models directory

Runtime model home: `%USERPROFILE%\mockingbird_models\`. Contains:
- `onnxruntime.dll` — ORT 1.22.x for Silero VAD
- `silero_vad.onnx` — Silero VAD weights
- `whisper-large-v3-turbo-q5_0.bin` (or whichever GGUF is configured)

Override parent via `MOCKINGBIRD_MODELS_DIR` env var (`mockingbird_models`
is appended to it).

If the dir is missing/sparse:
- `powershell -File scripts\download-onnxruntime.ps1` — restores ORT (~12 MB)
- `powershell -File scripts\download-models.ps1` — restores Whisper GGUF (~500 MB – 2 GB)

### Known issue: `cargo test --release` launch failure

LESSONS 2026-05-17 (`[phase5-wave-I]`) documents that `cargo test --release`
exits with `STATUS_ENTRYPOINT_NOT_FOUND` (0xc0000139) on this box even
with the wrapper, even with `ORT_DYLIB_PATH` set, and even with the
models dir populated. Root cause is an unidentified DLL ABI mismatch
in the test-binary's load chain. The **app binary itself launches fine** —
only the test runner is affected. Re-confirmed 2026-05-18.

**Sanctioned fallback gate** (use when live test exec is blocked):

1. `… cargo-with-cuda.ps1 check`
2. `… cargo-with-cuda.ps1 clippy --release -- -D warnings`
3. `… cargo-with-cuda.ps1 fmt --check`
4. `… cargo-with-cuda.ps1 test --release --no-run`
   (binaries link clean ⇒ type system, traits, link surface all valid)

For pure-Rust modules with no whisper-rs / ort / cuda deps (e.g.
`meetings/formatter.rs`, `meetings/filler_words.rs`, `cleanup/preprocessor.rs`),
live-test via the throwaway-crate recipe documented in LESSONS 2026-05-17:
copy source into `$env:TEMP\<modname>_tests\`, add only that module's
real deps, run vanilla `cargo test` there. Merge back when green.

For wired modules (touching whisper-rs / ort / cuda): cargo check +
clippy + the human-in-loop QA matrix in each phase doc.

### PowerShell single-quote variable trap

Single-quoted strings in PowerShell do NOT expand variables:

| ✓ Works                                  | ✗ Silently wrong                                                                       |
|------------------------------------------|----------------------------------------------------------------------------------------|
| `Test-Path "$env:USERPROFILE\foo"`       | `Test-Path '$env:USERPROFILE\foo'` (tests for the literal string `$env:USERPROFILE\foo`) |
| `Test-Path -LiteralPath "$home\foo"`     |                                                                                        |

Bernard hit this 2026-05-18 verifying the models dir; reported "missing"
when the folder was in fact present. Use double-quoted strings or
`[System.IO.Path]::Combine($env:USERPROFILE, ...)` for path assembly.

---

## Principles (binding)

1. **Raw data is immutable.** Once `transcripts(stage='raw')` is written,
   never UPDATE it. Hook will veto.
2. **Provenance is total.** Every session row references the exact prompt
   version, dictionary snapshot, and example set used.
3. **Layers are replaceable.** Don't hardcode provider/platform specifics
   outside the dedicated module.
4. **No telemetry.** Crashes log locally. Never phone home.
5. **Cross-platform from day one.** Platform-specific code lives behind
   `#[cfg(target_os)]` traits, even in Windows-only v1.
6. **No shortcuts.** If something is hard to test, refactor until it's
   testable. If something is hard to verify, that's the bug.
7. **Clipboard save/restore around every paste.** The user's clipboard
   is not your scratch space.
8. **Secure-input fields abort injection.** Detect and toast — never
   silently inject into password fields.

## When to stop and ask

If you're about to make a non-trivial architectural decision the PLAN
doesn't cover, **STOP**. Write an ADR draft proposing it before implementing.
The judges will catch missing ADRs anyway, so doing it inline saves an
iteration.

If PLAN.md and the code disagree, PLAN.md wins unless the disagreement is
documented in an ADR that supersedes the relevant PLAN section.

**5-attempt rule**: if you've burned 5 attempts on the same problem with no
progress, **STOP** and escalate via STATUS.md "Blocked / human input needed"
and a beads issue tagged `escalation`. Do not push to 10. The judge prompt
or the deliverable is likely misspecified.

## Work sizing & workflow selection

Pick the **smallest container** that fits the work. Don't reach for phase
machinery when a P3 bead is enough; don't paper over a real lateral epic
with a single bead.

### Work containers (smallest → largest)

| Container | When to use | Tracking | Seal |
|---|---|---|---|
| **Bead-only task** | Tiny work: single bug fix, single-file refactor, UI polish, copy change. <~200 LoC, single session. | `bd create … -t {bug,task,chore}` with optional dependency links. | `bd close <id>` + commit message references `mb-<id>`. No ADR, no tag. |
| **ADR-chartered lateral epic** | Coherent multi-piece work that doesn't fit a sealed phase but isn't a new numbered phase either. ~1-3 sessions, 3-15 files. Examples: MC v1.1 (ADR 0032), MC v1.2 (ADR 0035), three-mode cleanup (ADR 0022). | Charter ADR (Proposed → Accepted). Beads per piece, each referencing the ADR in its description. | ADR Accepted + STATUS.md updated. **NOT** a new `phase-*-complete` tag — those are reserved for PLAN §10 phases. |
| **PLAN §10 phase** | New top-level subsystem from the PLAN. Multi-session, multi-wave, ≥10 files, ≥1 week. | Phase doc at `docs/phases/phase{N}.md`. Wave briefs as you go. Beads per wave's tasks. Judges authored in the final wave. | `git tag phase-{N}-complete`. STATUS.md "Sealed" table updated. |
| **Standing P1/P2** | Long-running quality loop. Never "complete" — picks up whenever there's fixture input. Examples: `mb-ez9` empirical prompt tuning, `mb-xwi` Phase 5/6/7 remaining scope. | One persistent `in_progress` bead with periodic update comments. | Doesn't seal as a unit. Re-anchored when the underlying ADR / phase doc changes. |

### Default rule: bead-first

If you start work on anything that isn't already a bead, `bd create` it
**before the first code edit**. Single-line title is fine — the bead is
the tracking unit, not the spec. Mid-session discoveries that are
out-of-scope for the current bead: `bd create` them immediately, don't
carry them in conversation memory. Memory clears between iterations; the
bd DB doesn't.

### Workflow modes (orthogonal to container)

| Mode | When | Cost | Risk |
|---|---|---|---|
| **Ad-hoc / driver-and-passenger** | Human at the keyboard. Small-to-medium work. Live screenshots, questions, and judgment calls in real time. | Cheapest. No `/goal` overhead. | Easy to skip beads / LESSONS / commits in flow. Mitigated by the "bead-first" default above + end-of-iteration checklist. |
| **`/goal` autonomous** | Human is stepping away. Work is well-scoped (clear deliverable + chartered ADR or wave brief). | Auto-clears context between iterations. Resumable across sessions. | Stale-prompt risk (LESSONS PINNED P4). Up to 10 iterations of LLM cost. |
| **`/goal` + judges (Ralph Wiggum loop)** | High-stakes invariants with explicit pass/fail. Phase MC's 5 judges are the canonical reference (deterministic formatter, lossless stitching, two-channel merge, no-LLM-in-critical-path, dictation-untouched). | Highest — judges author + run cost. | Lowest — invariants mechanically enforced every loop until green. |

### Judges — when (regardless of container)

Any work touching the eight binding principles (Principles section above)
benefits from a judge if no existing judge already covers it. A judge is
just a small LLM-grading prompt returning pass/fail + reasoning.
Wave-6-style five-invariant bundles are the canonical pattern; a
**single one-off judge for a single bead is fine** when the invariant is
narrow. Don't reach for a 5-judge bundle when a 1-judge spot-check fits.

### Bug-fix subpattern (the common case)

Most bug fixes are this — no ADR, no judges, no tag:

1. `bd create "subsystem: short symptom" -t bug -p {1=user-blocking, 2=annoying, 3=papercut}`
2. *(optional)* `bd update <id> --status in_progress`
3. Reproduce → fix → test → commit (commit message references `mb-<id>`).
4. `bd close <id> -r "Fixed in <commit>. Root cause: ..."`
5. If the bug surfaced a non-obvious finding, append a body entry to
   `docs/LESSONS.md`.

### Discovery → bead pattern (mid-session)

When you discover work outside the current bead's scope (refactor
opportunity, related bug, missing test, doc gap), the default is
`bd create` immediately, with a one-line title and a sensible priority
(P3 if you're unsure — promote later). Holding it in conversation memory
is the failure mode — discoveries get lost when context clears between
iterations.

## Never do

- Modify a file in `models/` (gitignored downloads)
- Commit `.env`, `*.key`, `*.pfx`, or any file containing secrets
  (hook `block-secret-commit` enforces)
- Add a dependency without checking it works with the cross-platform
  abstraction
- Add any `@tanstack/*` package or any package flagged in the Mini
  Shai-Hulud IOC list (hook + see PLAN.md Appendix D)
- Run `npm install`, `npm ci`, or `pnpm/yarn install` without
  `--ignore-scripts` (hook `block-unsafe-npm` enforces)
- Skip writing tests "because it's a small change"
- Mutate raw transcript rows (hook enforces)
- Edit migrations 001/002/003 after `phase-1-complete` tag exists
  (hook enforces)
- Add telemetry, analytics, or crash-reporting that phones home
- Spend more than 10 iterations on a single goal — stop and ask the
  human to review the judge config or PLAN.md (5-attempt rule above
  catches this earlier in practice)
- Inject into a password field without first checking the
  `SecureInputGuard`
- Paste without saving and restoring the previous clipboard

## Permanently sealed (do not re-execute)

The following are immutable historical work. If a prompt asks you to do
any of these, **STOP and ask the human** — the prompt is almost certainly
stale context (e.g. a copy-paste of an old kickoff message).

- **PLAN Section 0.5 Bootstrap** — sealed at git tag `bootstrap-complete`.
  This file, hook config (`.code_puppy/settings.json`), project JSON agents
  (`.code_puppy/agents/*.json`), judges-template
  (`.code_puppy/judges-template.json`), and project skills
  (`.code_puppy/skills/*/SKILL.md`) are all on disk. The 17-step bootstrap
  checklist is a historical artifact only.
- **PLAN §10 numbered phases that carry a `phase-N-complete` git tag.**
  See `STATUS.md` for the current sealed list (as of last consolidation:
  phases 0, 1, 2, 3, 4, 8 + `phase-mc-complete`). **Check before starting
  work**: `git tag -l "phase-*"`. If a prompt asks you to re-execute a
  sealed phase, stop. If it asks you to *add new work to a sealed phase*,
  that's a new ADR-chartered lateral epic — handle it like ADR 0022/0023/0033
  (charter ADR → bd epic → wave briefs if needed → seal via STATUS + ADR
  acceptance, NOT by re-tagging the phase).
- **ADR-chartered lateral epics with Accepted ADRs.** See `STATUS.md` §
  "Sealed" for the current list (as of last consolidation: ADRs 0022, 0023,
  0024, 0025, 0032, 0033). These ADRs are accepted and their work is shipped;
  reopening requires a successor ADR that supersedes the previous one.

### Session-start ritual (mandatory before any tool call)

This supersedes / hardens the start-of-iteration list above. Even if the
human pastes a custom kickoff prompt that bypasses the normal `/goal` flow,
do all of this BEFORE any other tool call:

1. `cp_read_file STATUS.md` — full file, it's slim now (~150 lines). It tells
   you what's sealed, what's in-flight, and how to resume.
2. **Triage the kickoff prompt against STATUS.** Three cases — pick one and act:

   **(a) Stale wrapper around a clear actionable request** — the most common
   case. The kickoff text references a sealed phase / ADR / bootstrap, but
   the human's actual message ALSO contains a concrete bug report, feature
   ask, or follow-up question (often at the bottom, after "prior context"
   boilerplate, or in a screenshot/log paste). **Treat the stale framing as
   accidental boilerplate / `/goal` template noise and answer the actual
   request.** Do not stop-and-ask just to confirm the obvious. A one-liner
   acknowledgement ("ignoring stale Phase X kickoff, answering the bug
report") is enough.

   **(b) Genuinely ambiguous intent** — the kickoff conflicts with sealed
   state AND there's no clear actionable embedded request, OR the request
   could plausibly mean "reopen the seal". STOP and surface via
   `ask_user_question` before any further tool calls. This is the original
   2026-05-17-incident guard rail.

   **(c) Clean kickoff, no conflict** — proceed normally.

3. Otherwise proceed with the normal iteration ritual above (read PRODUCT-STATE,
   LESSONS PINNED, this file, phase doc / ADR, `bd ready`, `git log`, etc.).

This ritual exists because of two incidents: the 2026-05-17 stale-bootstrap
incident (over-execution of sealed work) AND the 2026-05-23 over-correction
(stopping to ask when the body of the message contained a clear bug report).
See LESSONS PINNED entry **P4**.

## Issue Tracking

**`bd` (beads) is the DEFAULT work-tracking mechanism for Mockingbird.**
Every non-trivial unit of work — features, bugs, refactors, docs,
mid-session discoveries — should have a bead. `STATUS.md` is the
human-readable snapshot at iteration boundaries; `bd` is the live queue
you actually work from.

See "Work sizing & workflow selection" above for *when* to use just-a-bead
vs. ADR-chartered epic vs. PLAN phase. This section covers *how* to use
`bd`.

### Default flow

1. **Start:** `bd ready` (or `bd ready -t bug`, etc.) to pick work. If your
   task isn't already in the queue, `bd create` it **before** the first
   edit. A one-line title is enough.
2. **During:** `bd update <id> --status in_progress` if it'll take more than
   a few minutes. `bd link <a> <b>` if discovery surfaces a dependency.
   `bd create` any out-of-scope discoveries immediately — don't carry them
   in working memory.
3. **End:** `bd close <id> -r "..."` with a resolution message that mentions
   the commit hash and (if applicable) the LESSONS or ADR reference.

### Quick reference

- `bd ready` — find unblocked work (filter with `-t bug`, `-p 1`, etc.)
- `bd create "Title" -t {bug|feature|task|chore} -p {1|2|3}` — create issue
- `bd update <id> --status {open|in_progress|blocked|closed}` — change state
- `bd close <id> -r "resolution note"` — close with reason
- `bd link <a> <b>` — `b` blocks `a` (i.e. `a` depends on `b`)
- `bd show <id>` — full issue detail
- `bd prime` — full workflow context dump (read once per fresh setup)

Issue prefix is `mb-` (set at `bd init`). Reference IDs in commit messages
so `git log <hash> -1 --format=%B` and `bd show <id>` cross-walk cleanly.

### Gotchas (learned the hard way)

- **Avoid non-ASCII in `bd create` titles/descriptions.** Em-dashes, smart
  quotes, and some other unicode characters cause `bd create --description "..."`
  to fail with a non-zero exit code **after** creating the issue (so retrying
  produces a duplicate). Workaround: keep create-time strings ASCII-only and
  use `bd update <id>` to add rich descriptions post-create if needed.
  Lesson logged 2026-05-24 (4 duplicate beads created + closed in one
  iteration before noticing).
- **`git status --short` eats the leading-status character when piped into
  `findstr`.** Use `git status --porcelain=v1` instead — the two-character
  `XY` index/worktree status survives the pipe. Lesson logged 2026-05-24.
- **`findstr /R` regex is anemic** (no `\b`, no `+`, no lookahead). For
  anything beyond "contains a literal substring", pipe to PowerShell
  `Select-String` or `Select-Object` instead. Lesson logged 2026-05-24.
