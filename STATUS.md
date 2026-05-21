<!-- ════════════════════════════════════════════════════════════════════
     SESSION ANCHOR — read this block BEFORE any tool call.
     This file is intentionally small. For:
       • What the app currently does, by subsystem  →  docs/PRODUCT-STATE.md
       • Why we made each big call                   →  docs/adr/
       • Non-obvious findings from past sessions     →  docs/LESSONS.md
                                                       (PINNED block at top)
       • Old session-by-session diary                →  docs/archive/STATUS-2026-05-23.md
     ════════════════════════════════════════════════════════════════════ -->

# Mockingbird — STATUS

**Last consolidated:** 2026-05-23 (checkpoint after MC overlay event-delivery hotfix — ADR 0034 / mb-z5y).

## ✅ Sealed (do not re-execute)

| Tag | What it sealed |
|---|---|
| `bootstrap-complete`     | PLAN §0.5 — AGENTS.md, hooks, JSON agents, judges template, skills. |
| `phase-0-complete`       | Rust + Tauri 2 scaffold, hook engine, CI sanity. |
| `phase-1-complete`       | SQLite schema (migrations 001-003), repo layer, settings store. |
| `phase-2-complete`       | Audio capture (cpal) + Whisper-rs CUDA + Silero VAD via ORT. |
| `phase-3-complete`       | Hotkey hook + dictation pipeline + SecureInputGuard + clipboard injection. |
| `phase-4-complete`       | Cleanup provider trait + Ollama provider + prompt loader. |
| `phase-8-complete`       | UI sprint — all six pages + recording overlay (App/Insights/History/Dictionary/Modes/Settings + recording window). |
| `phase-mc-start` (anchor)| Stable diff reference for `mc-dictation-untouched` judge. |
| `phase-mc-complete`      | Meeting Capture subsystem: chord activation, twin-stream capture, long-form chunked Whisper, deterministic formatter, two-channel merge, ephemeral LLM pass, overlay UI, Meetings/MeetingDetail pages, 5 invariant judges. |

**Lateral epics accepted via ADR** (no new phase tag — see `docs/adr/`):

- ADR 0022 — Three-mode cleanup pipeline (casual/normal/formal)
- ADR 0023 — Design Language v1 (warm-earth Liquid Glass + Fraunces)
- ADR 0024 — Empirical mode-prompt tuning + migration 010
- ADR 0025 — Optional Unsplash ambient background (opt-in BYO-key)
- ADR 0032 — MC v1.1 polish (VU meters, LLM-ephemeral notice, MaxDuration UI)
- ADR 0033 — MC chord-collision hotfix (VK_M → VK_OEM_PERIOD + settings actually-read-at-boot + overlay UI wires)
- ADR 0034 — MC overlay event-delivery hotfix (show-before-emit + `emit_to` re-broadcast + defensive latch clear + emit-state observability; fixes mb-z5y)

If a kickoff prompt asks you to re-execute any of the above, **STOP** and surface
the conflict before any tool call. See `.code_puppy/AGENTS.md` § "Permanently sealed".

## 🟢 Currently active

**No in-flight epic.** Last commits: `a4e0ec3` (MC hotfix, closed `mb-x1x`) and
this iteration's mb-z5y / ADR 0034 hotfix (overlay event delivery). Tree clean,
cargo gate green (`fmt --check`, `clippy --release -D warnings`, `test --no-run`),
vitest 55/55 pass. Live-exec verification of the fix is on Dustin (manual: launch
binary, click Start in main meetings page, confirm overlay flips to RECORDING
and Stop button enables).

**Standing P1:** `mb-ez9` — empirical mode-prompt iteration across casual/normal/formal
(in_progress; long-running quality improvement loop, picks up whenever Dustin has
fixture additions to feed the mode_eval rig).

**Standing P2s:**
- `mb-xwi` — Phases 5/6/7 main-phase work from PLAN §10 (Recording UX polish,
  History/Settings/About windows, code signing). The long pole.
- `mb-cjc` — ADR 0022 Wave 3 (LLM-skip for short casual utterances; ~300ms direct-paste path).

**P3 backlog:** see `bd ready` — 6 issues (tray deep-link, Settings.tsx split,
DPAPI for Unsplash key, Unsplash glyph review, ESLint v9 migration, hide-disabled-AI-modes toggle).

## ▶ How to resume

1. **Read this file** (you are here). 30 seconds.
2. **Read `docs/PRODUCT-STATE.md`** — the durable "what does the app actually do today?"
   reference. Replaces 1000+ lines of old session diary. 2-3 min skim.
3. **Read `docs/LESSONS.md` PINNED block** (top of file). The load-bearing
   gotchas (cargo wrapper, test-binary launch bug, stale-prompt incident). 1 min.
4. **Read `.code_puppy/AGENTS.md`** — rules, principles, never-do list. 2 min.
5. **For active phase work:** read `docs/phases/phase{N}.md` and any wave briefs.
   For one-off epics: read the chartering ADR (`docs/adr/`).
6. `bd ready` — what's unblocked.
7. `git log --oneline -20` and `git status` — what shipped recently / dirty tree.
8. **Then start work.** If the kickoff prompt conflicts with sealed-phase state,
   STOP and ask before tool calls.

## 📐 What goes where

| Doc | Update cadence | Purpose |
|---|---|---|
| `STATUS.md` (this file) | End of every iteration | Anchor: what's sealed, what's in-flight, how to resume. Stays slim. |
| `docs/PRODUCT-STATE.md` | When a subsystem ships or materially changes | "Current state of the product" reference. Stable. |
| `docs/LESSONS.md`      | When non-obvious finding emerges | Append-only journal. TOC + PINNED at top, body chronological. |
| `docs/adr/####-*.md`    | Per architectural decision | Immutable once Accepted (supersede via new ADR). |
| `docs/archive/`         | Read-only             | Old STATUS diaries, deprecated docs. |
| `bd` issue DB           | Per task              | Live work queue + dependency graph. |
| `git tag phase-N-complete` | Per phase seal     | Immutable boundary marker. |

## 🛠 End-of-iteration checklist

(Hooks enforce most of this; see `.code_puppy/AGENTS.md` § "At the end of every iteration".)

1. **Update STATUS.md** — flip Currently-active block if epic state changed; otherwise leave alone.
2. **Update PRODUCT-STATE.md** only if a subsystem shipped or materially changed.
3. **Close/create beads** (`bd close <id>` / `bd create ...`).
4. **Cargo gate** (all via `scripts\cargo-with-cuda.ps1`):
   - `fmt --check`
   - `clippy --release -- -D warnings`
   - `test --release --no-run` (live exec blocked on this box — see LESSONS PINNED)
   - `build --release` (when shipping)
5. **UI gate:** `npx tsc --noEmit`, `npm test`, `npm run build`.
6. **LESSONS append** if a non-obvious thing happened.
7. **Commit with a descriptive message** referencing the bead id + ADR if any.
8. **No new phase tag** unless completing a numbered PLAN §10 phase (lateral epics seal via ADR, not tag).
