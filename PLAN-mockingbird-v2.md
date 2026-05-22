# Project: `Mockingbird` (confirmed name — bootstrap iteration)

A local-first, system-wide voice dictation app. A privacy-respecting, self-improving replacement for Wispr Flow, built with Tauri v2 + whisper.cpp + Ollama. Windows v1 with cross-platform abstractions designed in from day one; macOS as a planned Phase 9.

**Name confirmed:** `Mockingbird` (display) / `mockingbird` (slug). Confirmed by Dustin during bootstrap iteration (Section 0.5 step 1). Tauri identifier placeholder: `com.dustin.mockingbird` — finalize before Phase 7 along with the GitHub repo URL (Section −1 item 3, deferred).

---

## 0. Reading this document

This is the canonical reference for the project. The active Code Puppy agent (usually `code-puppy`, sometimes a project JSON agent) reads this at the start of every Wiggum iteration.

**Build philosophy:** No shortcuts. Quality and future scalability over speed-to-MVP. Every layer must be replaceable independently. The data model is the foundation — get it right before building on top of it. Each phase ships with passing tests, updated docs, unanimous judge approval, **and hook-engine-clean exits** before the next phase begins.

**Hardware target (primary developer):** ROG Zephyrus G14, AMD Ryzen 9 4900HS, 16 GB RAM, RTX 2060 Max-Q 6 GB VRAM, Windows 10/11 x64. The architecture must work well on this; everything more powerful is upside.

**Agent workflow:** This project is built using Code Puppy with the Wiggum plugin's `/goal` convergent loop, the **Hook Engine** for mechanical enforcement, **`code-puppy`** as the default implementor agent, **named specialist agents** (planning-agent, qa-kitten, helios, agent-creator) for phase-specific work, and **project JSON agents** (Appendix G) for tool-scoped work. The human orchestrates by switching the active agent at phase boundaries via `/agent <name>`. Standing rules live in `.code_puppy/AGENTS.md`. Hook enforcement lives in `.code_puppy/settings.json`. Per-phase task state lives in `STATUS.md`. The full agent workflow specification is Section 11. (Note: pack agents like pack-leader are deprecated and not used.)

**How to read this doc, in order:**

1. Section −1 for pre-flight decisions (read once, then they're done)
2. Section 0–3 for orientation (you are here)
3. Section 11 for how the /goal + Wiggum workflow operates and which agent does what
4. Appendix A for the standing rules the agent always follows
5. Appendix E for the hook-engine rules that mechanically enforce Section 12
6. `docs/phases/phase{N}.md` for the current phase's deliverables and exit criteria
7. Whichever architectural sections (4–9.1) the current phase touches
8. Section 12 for non-negotiable invariants

**Document split (read carefully):** Sections 4–9.1 are architectural reference. Section 10 is the *spine* of the phase plan — short. Each phase has a companion `docs/phases/phase{N}.md` that contains the full deliverables list, judge config, entry/exit criteria, and slash-command body. Slash commands point at the phase doc, not the whole PLAN.md. This keeps per-iteration context cost bounded.

---

## 0.5. Bootstrap iteration (the agent runs this before Phase 0 proper)

This iteration handles everything in pre-flight Section −1 and Appendix I 
that the agent can do without human input, AND walks the human through 
anything that requires a decision. It exists because some of these items 
must be in place before iteration 2 starts (notably AGENTS.md and the hook 
config — Code Puppy auto-loads both at iteration start).

The bootstrap iteration is special: the kickoff prompt provides all standing 
rules inline so the agent doesn't need AGENTS.md to be on disk yet. By the 
end of bootstrap, every file the agent needs in subsequent iterations is 
committed.

### Bootstrap deliverables (single iteration, no judges)

The agent works through this ordered checklist. Where input is required, it 
stops and asks via ask_user_question.

1. Confirm project name. Read PLAN.md line 1. If a placeholder, ask. 
   Replace throughout and commit before proceeding.

2. Verify Code Puppy environment.
   - Run /agent (no arg) and confirm the shipped agents are present: 
     code-puppy, planning-agent, qa-kitten, helios, agent-creator.
   - If pack-leader / bloodhound / etc. appear in the list, note them 
     but do NOT use them — the framework devs confirmed pack agents 
     are deprecated as of 0.0.506. This plan uses code-puppy + named 
     specialists exclusively.
   - Run !cat ~/.code_puppy/puppy.cfg to inspect config. The 
     enable_pack_agents flag is irrelevant now; leave whatever value 
     is there.

3. DBOS is OPTIONAL and deferred. Skip unless the human has explicitly 
   installed dbos (`pip install dbos`). Note in STATUS.md as "deferred — 
   not required for solo dev workflow." If the human did install it, 
   run /dbos on and confirm /dbos status reports enabled.

4. Verify model availability. Query reachable models against PLAN.md 
   Section 5 strings. If any stale, ask the human for substitutes and 
   update PLAN.md.

5. Verify build prerequisites: rustc, cargo, node, npm, cmake, nvcc. For 
   any failure, surface install instruction (with official URL) and ask 
   whether to continue (e.g., CPU-only without nvcc) or pause.

6. Verify WebView2 runtime on Windows. Report missing.

7. Write .code_puppy/AGENTS.md from PLAN.md Appendix A with project name 
   substituted. Commit.

8. Write .code_puppy/settings.json (hook engine) from Appendix E. Commit.

9. Write all hook scripts under scripts/hooks/ per Appendix E summaries. 
   Each: reads JSON on stdin, applies rule, exits 0/1/2. Test each one 
   with synthetic input. Commit.

10. Write the five project JSON agents under .code_puppy/agents/ per 
    Appendix G. Delegate to agent-creator. Commit.

11. Write the six project skills under .code_puppy/skills/<name>/SKILL.md 
    per Section 11.3. Write these directly (you, the active agent). Commit.

12. Seed judges. Write .code_puppy/judges-template.json per Appendix C, 
    then write scripts/seed-judges.ps1 that merges it into 
    ~/.code_puppy/judges.json. Run the seed script. Verify with /judges 
    that all judges are present (all disabled by default).

13. Generate Tauri updater key pair. tauri signer generate -w 
    ~/.tauri/<name>.key. Capture public key. Stage for inclusion in 
    tauri.conf.json (which doesn't exist until Phase 0 scaffolds it). 
    Surface private key path to human with backup reminder.

14. Confirm Section −1 items resolved. For any deferred (e.g., code-signing 
    CA — Phase 7), record deferral in STATUS.md with target phase.

15. Initialize STATUS.md with phase outline, empty Phase 0 tasks section, 
    bootstrap completion checkbox.

16. Commit + tag. Final commit: "chore: bootstrap iteration complete". 
    Tag bootstrap-complete.

17. Hand off. Print summary, surface anything blocked on the human, 
    suggest next command (/plan-phase 0 then /phase0-goal).

### Why this is a single iteration without judges

This is setup work — the artifacts produced are themselves the means by 
which subsequent iterations will be judged. There's no good way to judge 
bootstrap with the same machinery that depends on bootstrap. A simple 
"did all 17 steps complete?" check suffices; the human reviews the 
bootstrap commit before kicking off Phase 0.

### Agent routing within bootstrap

- code-puppy (the active agent receiving /goal) orchestrates and does 
  most of the file scaffolding directly (steps 7, 8, 9, 11, 12, 15).
- invoke_agent("agent-creator", ...) mints the JSON agents (step 10).
- code-puppy runs environment verification shell commands directly 
  (steps 2, 4, 5, 6).
- invoke_agent("helios", ...) only if a needed tool doesn't exist yet.
- The agent stops and asks the human for any item requiring a decision.

---

## −1. Pre-flight decisions (resolve before Phase 0)

These nine decisions can't be deferred. The agent should refuse to start Phase 0 if any are unresolved.

1. **Final project name.** Spreads into Cargo identifier, Tauri `identifier` field (e.g., `com.yourname.Mockingbird`), Windows installer ProductName, registry path, `%APPDATA%/<name>/` directory, code-signing certificate Common Name. Decide once.
2. **License.** MIT default; confirm and write `LICENSE` text in Phase 0.
3. **GitHub repo URL.** Needed for Tauri auto-updater endpoint, README badges, ADR provenance links.
4. **Code-signing certificate path.** Buy/renew before Phase 7; choose CA (DigiCert, Sectigo, SSL.com). The cert's Subject CN must match the installer's ProductName. Decision goes in ADR 0005.
5. **Tauri updater key pair.** Generated in Phase 0 via `tauri signer generate`. Public key → `tauri.conf.json`. Private key → secret store (NOT in git). One key pair per project lifetime.
6. **Cloud LLM model identifiers (verify on day of Phase 4).** Anthropic model strings drift. Plan defaults: `claude-haiku-4-5-20251001`, `claude-sonnet-4-6`, `claude-opus-4-7`. Verify with `curl https://api.anthropic.com/v1/models` (BYO key) before Phase 4 starts; update Section 5 if the strings have moved.
7. **DBOS durable execution (OPTIONAL — defer unless multi-day unattended runs are anticipated).** DBOS gives crash-resume *within* a single iteration. Wiggum + git already give crash-resume *between* iterations, which covers most solo-dev failure modes. Skip during bootstrap; come back later if needed. To enable: `pip install dbos`, then `/dbos on` inside Code Puppy. Until then, leave off — the agent works fine without it.
8. **Round-robin LLM keys for long phase runs.** Code Puppy supports `extra_models.json` with per-call rotation. Decide whether Phase 4 and Phase 8 will rotate across (a) one Anthropic key, (b) one Anthropic key + one local Ollama, or (c) multi-key fleet (Anthropic + Cerebras + Bedrock). Affects `~/.code_puppy/extra_models.json`.
9. **Orchestration model: /goal + Wiggum, not pack agents.** Confirmed with the Code Puppy devs (May 2026): pack agents (pack-leader, bloodhound, shepherd, terrier, watchdog, retriever) are deprecated and removed from the framework. The reserved names remain in `PACK_AGENT_NAMES` and the `enable_pack_agents` flag still exists, but no implementations are or will be shipped. The canonical workflow is now `/goal <prompt>` (Wiggum convergent loop) with `code-puppy` as the implementor agent, delegating to named specialists (`planning-agent`, `qa-kitten`, `helios`, `agent-creator`) and project JSON agents (Appendix G) via `invoke_agent`. The human orchestrates by switching the active agent (`/agent <name>`) at phase boundaries to match the work. See Section 11.

---

## 1. Goals (v1)

1. **Hold-to-speak dictation** in any Windows app: press hotkey, speak, release, polished text appears at cursor.
2. **Three modes**, each its own hotkey, each its own cleanup prompt.
3. **Fully local by default** — Whisper STT and Ollama LLM cleanup both run on-device. No audio or text leaves the machine unless the user opts into a cloud cleanup provider.
4. **Per-mode model selection** — each of the three modes can independently use a local Ollama model or a cloud Claude model.
5. **Provenance-first data model** — every dictation event preserves raw audio metadata, raw transcript, cleaned transcript, final inserted text, model used, prompt version used, and dictionary version used. Audit log on every mutation.
6. **History view** with search, side-by-side raw/cleaned/final comparison, and ability to mark sessions as canonical style examples.
7. **Custom dictionary** that biases Whisper via `initial_prompt` and informs the cleanup LLM via prompt context.
8. **Polished UX** matching Wispr Flow's feel: floating recording window, waveform, status indicators, mode display, cancel handling, tray menu, undo last dictation. Visual polish bar measured against `docs/DESIGN.md` tokens (Section 9.1).
9. **Export / import** as the cross-machine data portability story (no cloud sync in v1).
10. **Standardized developer onboarding** — any developer can clone the repo, run the setup script, and have a working build in under 30 minutes.
11. **Mechanical rule enforcement.** Section 12 "do not skip" rules are enforced by the Code Puppy Hook Engine (Appendix E), not by trusting the implementor to remember them.

## 2. Non-goals (v1)

- macOS / Linux ship targets (architecture must support them; implementation deferred to Phase 9+).
- Mobile app.
- Cloud sync across devices.
- Meeting transcription / diarization.
- Audio retention by default (off; toggleable in settings).
- Whisper fine-tuning / LoRA (schema supports it; implementation deferred).
- The nightly learning loop is **designed** in v1 (schema supports it) but **implemented** in Phase 8 as a stretch.
- Voice activation / hands-free mode (push-to-talk only).
- Multi-language switching mid-dictation (English-only in v1; i18n string-table scaffold present for future).
- Telemetry / crash reporting that phones home. Ever.

## 2.1. Platform support

**v1 target: Windows 10/11 x64.** Build, test, ship Windows first.

**Cross-platform abstractions are required from day one.** Every platform-specific concern lives behind a Rust trait with `#[cfg(target_os = "...")]` impls. macOS and Linux files exist as `todo!()` stubs so the type system reminds us where the platform boundary is. This adds ~5–10% to the Windows build cost and turns Phase 9 into "fill in the stubs," not "refactor everything."

| Concern | Trait | Windows impl | macOS impl (Phase 9) | Linux impl (community) |
|---|---|---|---|---|
| Text injection | `Injector` | Win32 `SendInput` + clipboard paste | `CGEventCreateKeyboardEvent` + Accessibility API | `xdotool` / `ydotool` |
| Foreground window detection | `WindowContext` | `GetForegroundWindow`/`GetWindowText` | `NSWorkspace.frontmostApplication` | X11 / Wayland |
| Secret storage (API keys) | `SecretStore` | DPAPI via `windows-rs` | Keychain via `security-framework` | libsecret |
| Hotkey hold/release detection | `HotkeyListener` | `SetWindowsHookEx(WH_KEYBOARD_LL)` via `windows-rs` | `CGEventTap` | `evdev` |
| Hotkey default modifiers | const per-platform | `Ctrl+Win` family | `Ctrl+Cmd` / `Cmd+Option` family | `Ctrl+Super` family |
| GPU acceleration | build flag | CUDA | Metal | CUDA / Vulkan |
| Permissions checks at startup | `PermissionGate` | Mic only | Mic + Accessibility + Input Monitoring | Mic only |
| Secure-input detection (do not inject) | `SecureInputGuard` | `GetGUIThreadInfo` + foreground class check | `IsSecureEventInputEnabled` | best-effort |
| Boot-on-login | `Autostart` | Run-key OR Task Scheduler | LaunchAgent plist | systemd user unit |

**Phase 9 deliverables:** macOS builds pass, Mac first-run wizard handles Accessibility permission grant (Mac's worst UX point), Apple Developer Program enrolled, app notarized.

Linux is community-supported: PRs that fill in the Linux trait impls are accepted; the team does not commit to maintaining them.

**Note on hotkey detection:** `tauri-plugin-global-shortcut` fires on press only, which is insufficient for hold-to-record. The `HotkeyListener` trait above wraps a real low-level keyboard hook so we get both key-down and key-up events. This is non-negotiable — without it the state machine in Section 6 cannot be built. See `docs/phases/phase3.md` for the implementation detail.

---

## 3. Architecture overview

Five layers. Each replaceable independently.

```
┌─────────────────────────────────────────────────────────────┐
│  Layer 5: Provenance & storage (SQLite + audit log + FTS5) │
├─────────────────────────────────────────────────────────────┤
│  Layer 4: Text injection (platform trait: Win32 / Mac AX)   │
├─────────────────────────────────────────────────────────────┤
│  Layer 3: Cleanup LLM (Ollama local | Claude API)           │
├─────────────────────────────────────────────────────────────┤
│  Layer 2: Speech-to-text (whisper.cpp + GPU + Silero VAD)   │
├─────────────────────────────────────────────────────────────┤
│  Layer 1: Activation + audio capture (low-level hook + cpal)│
└─────────────────────────────────────────────────────────────┘
```

**End-to-end flow on hotkey hold:**

1. User holds `Ctrl+Win` (Normal mode).
2. Layer 1 receives key-down via low-level keyboard hook, starts mic capture (cpal), displays floating recording window with live waveform.
3. User speaks. User releases key. Low-level hook fires key-up.
4. Layer 1 stops capture, passes PCM buffer to Layer 2.
5. Layer 2 trims silence via Silero VAD (ONNX), runs whisper.cpp with `initial_prompt = <recent dictionary terms>`. Returns raw transcript in ~0.3–0.8s on RTX 2060.
6. Layer 3 receives raw transcript. Constructs prompt from mode system prompt + recent few-shot examples for this mode (token-budgeted) + dictionary terms + foreground app/window context + raw transcript. Sends to configured provider. Returns cleaned text in ~0.2–0.5s.
7. Layer 4 picks paste vs keystroke per app. Saves clipboard, injects cleaned text at cursor, restores clipboard. Aborts gracefully if a secure-input field is focused.
8. Layer 5 writes the full provenance trail to SQLite in one transaction.
9. Recording window dismisses with 220 ms fade. Total latency target: < 1.5s end-to-end for a 5-second utterance.

---

## 4. Project layout

```
Mockingbird/
├── PLAN.md                          # this file — read every iteration
├── STATUS.md                        # current phase + task checklist — updated every iteration
├── README.md                        # entry point — three reader paths
├── CONTRIBUTING.md                  # architecture overview for contributors
├── CHANGELOG.md
├── LICENSE                          # MIT
├── .gitignore
├── .env.example
├── .npmrc                           # ignore-scripts=true (Appendix D)
├── .tool-versions                   # mise/asdf pins: Rust, Node
├── rust-toolchain.toml              # Rust version pin
├── .rustfmt.toml
├── .eslintrc.cjs
├── lefthook.yml                     # pre-commit + pre-push hooks (fmt, clippy, test)
├── Cargo.toml                       # workspace
├── package.json
├── .code_puppy/
│   ├── AGENTS.md                    # standing rules — read every iteration (Appendix A)
│   ├── settings.json                # HOOK ENGINE config (Appendix E) — mechanical enforcement
│   ├── judges-template.json         # recommended judge roster (Appendix C)
│   ├── agents/                      # project JSON agents (Appendix G)
│   │   ├── migration-author.json
│   │   ├── injection-author.json
│   │   ├── ui-author.json
│   │   ├── prompt-tuner.json
│   │   └── learning-loop-author.json
│   ├── skills/                      # project skills (Section 11.3)
│   │   ├── data-model/SKILL.md
│   │   ├── injection-recipes/SKILL.md
│   │   ├── supply-chain/SKILL.md
│   │   ├── quality/SKILL.md
│   │   ├── prompts/SKILL.md
│   │   └── design-tokens/SKILL.md
│   └── README.md                    # explains the .code_puppy/ contents
├── .agents/
│   └── commands/                    # custom slash commands (Appendix B)
│       ├── phase0-goal.md
│       ├── phase1-goal.md
│       ├── phase2-goal.md
│       ├── phase3-goal.md
│       ├── phase4-goal.md
│       ├── phase5-goal.md
│       ├── phase6-goal.md
│       ├── phase7-goal.md
│       ├── phase8-goal.md
│       ├── plan-phase.md            # /plan-phase N — invokes planning-agent for phase N
│       ├── qa-window.md             # /qa-window — qa-kitten visual check
│       ├── smoke.md                 # /smoke — full pipeline health check
│       ├── run-tests.md
│       ├── verify-env.md
│       ├── lint.md
│       └── seed-judges.md
├── src-tauri/
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   ├── build.rs
│   ├── icons/                       # generated by `tauri icon`
│   ├── tests/                       # Rust integration tests
│   ├── benches/                     # criterion perf budgets
│   └── src/
│       ├── main.rs
│       ├── lib.rs
│       ├── error.rs                 # crate-wide error types
│       ├── audio/
│       │   ├── mod.rs
│       │   ├── capture.rs           # cpal mic capture
│       │   └── vad.rs               # Silero VAD (ONNX) wrapper
│       ├── stt/
│       │   ├── mod.rs
│       │   ├── whisper.rs           # whisper-rs binding
│       │   └── prompt_builder.rs    # initial_prompt from dictionary (token-budgeted)
│       ├── cleanup/
│       │   ├── mod.rs
│       │   ├── provider.rs          # trait CleanupProvider
│       │   ├── ollama.rs            # local provider (streaming, /api/pull, health)
│       │   ├── claude.rs            # cloud provider (streaming, retries, cost meter)
│       │   ├── prompts/
│       │   │   ├── fragment.md
│       │   │   ├── normal.md
│       │   │   └── verbose.md
│       │   ├── few_shot.rs          # example selection (rank × recency, token-budgeted)
│       │   └── token_budget.rs      # shared budget calculator
│       ├── hotkey/
│       │   ├── mod.rs               # trait HotkeyListener (hold + release)
│       │   ├── state.rs             # hold-to-record state machine
│       │   ├── windows.rs           # WH_KEYBOARD_LL hook
│       │   ├── macos.rs             # todo!() stubs
│       │   └── linux.rs             # todo!() stubs
│       ├── injection/
│       │   ├── mod.rs               # trait Injector
│       │   ├── windows.rs           # SendInput
│       │   ├── macos.rs             # todo!() stubs
│       │   ├── linux.rs             # todo!() stubs
│       │   ├── paste.rs             # clipboard save/restore + paste helper
│       │   ├── strategy.rs          # per-app paste vs keystroke choice
│       │   └── secure_guard.rs      # trait SecureInputGuard
│       ├── window_context/
│       │   ├── mod.rs               # trait WindowContext
│       │   ├── windows.rs
│       │   ├── macos.rs             # todo!() stubs
│       │   └── linux.rs             # todo!() stubs
│       ├── secrets/
│       │   ├── mod.rs               # trait SecretStore
│       │   ├── windows.rs           # DPAPI
│       │   ├── macos.rs             # todo!() stubs
│       │   └── linux.rs             # todo!() stubs
│       ├── permissions/
│       │   ├── mod.rs               # trait PermissionGate
│       │   ├── windows.rs
│       │   └── macos.rs
│       ├── autostart/
│       │   ├── mod.rs               # trait Autostart (Run key | Task Scheduler)
│       │   └── windows.rs
│       ├── db/
│       │   ├── mod.rs
│       │   ├── migrations/
│       │   │   ├── 001_initial.sql        # core tables + FTS5 (see Section 7)
│       │   │   ├── 002_audit_triggers.sql # audit triggers for all four audited tables
│       │   │   ├── 003_seed_modes.sql     # seed `modes` + `prompts` rows
│       │   │   └── ...
│       │   ├── sessions.rs
│       │   ├── transcripts.rs
│       │   ├── dictionary.rs
│       │   ├── examples.rs
│       │   ├── prompts.rs
│       │   ├── audit.rs             # includes rollback_to_snapshot()
│       │   └── search.rs            # FTS5 query helpers
│       ├── export_import/
│       │   ├── mod.rs
│       │   ├── archive.rs           # .Mockingbird zip format
│       │   ├── manifest.rs
│       │   └── merge.rs
│       ├── settings/
│       │   ├── mod.rs               # typed wrappers over key/value table
│       │   └── model.rs             # known setting keys + defaults
│       ├── logging.rs               # tracing setup, rotation, PII scrubbing
│       ├── tray.rs
│       └── commands.rs              # all #[tauri::command] handlers
├── ui/
│   ├── package.json
│   ├── vite.config.ts
│   ├── tsconfig.json
│   ├── index.html
│   ├── src/
│   │   ├── main.tsx
│   │   ├── App.tsx
│   │   ├── windows/
│   │   │   ├── Recording.tsx
│   │   │   ├── Settings.tsx
│   │   │   ├── History.tsx
│   │   │   ├── FirstRun.tsx
│   │   │   ├── Dictionary.tsx
│   │   │   └── ExportImport.tsx
│   │   ├── components/
│   │   │   ├── Waveform.tsx
│   │   │   ├── StatusDot.tsx
│   │   │   ├── ModeBadge.tsx
│   │   │   ├── VirtualList.tsx      # hand-rolled (banned: @tanstack/*)
│   │   │   └── ...
│   │   ├── design/
│   │   │   ├── tokens.css           # design tokens (Section 9.1)
│   │   │   ├── motion.ts            # animation specs
│   │   │   └── sounds/              # optional audio cues
│   │   ├── ipc/                     # typed Tauri command wrappers
│   │   ├── i18n/                    # English-only v1, future-ready
│   │   └── styles/
│   ├── tests/                       # Playwright (qa-kitten authors these)
│   └── public/
├── models/                          # gitignored — downloaded at first run
│   ├── ggml-large-v3-turbo-q5_0.bin
│   └── silero_vad.onnx
├── scripts/
│   ├── setup-dev-windows.ps1
│   ├── setup-dev-macos.sh           # Phase 9 stub
│   ├── verify-environment.ps1
│   ├── download-models.ps1          # resumable + SHA256-verified
│   ├── seed-judges.ps1
│   └── generate-tauri-keys.ps1      # Phase 0: tauri signer generate
├── docs/
│   ├── DESIGN.md                    # design system overview (Section 9.1)
│   ├── ARCHITECTURE.md              # broken-out Sections 3 + 8
│   ├── DATA_MODEL.md                # broken-out Section 7
│   ├── PROMPTS.md
│   ├── HOTKEYS.md
│   ├── PROVIDERS.md
│   ├── DEPENDENCIES.md
│   ├── DEVELOPMENT.md
│   ├── TROUBLESHOOTING.md
│   ├── EXPORT_IMPORT.md
│   ├── GLOSSARY.md
│   ├── QUALITY.md
│   ├── SETTINGS.md                  # every settings key + default + migration
│   ├── SCRIPT_ALLOWLIST.md
│   ├── KNOWN_ISSUES.md              # secure-input fields, anti-cheat, etc.
│   ├── LESSONS.md                   # appended-to by agents on non-obvious findings
│   ├── phases/                      # per-phase docs (replaces 38K-token PLAN re-read)
│   │   ├── phase0.md
│   │   ├── phase1.md
│   │   ├── phase2.md
│   │   ├── phase3.md
│   │   ├── phase4.md
│   │   ├── phase5.md
│   │   ├── phase6.md
│   │   ├── phase7.md
│   │   └── phase8.md
│   └── adr/
│       ├── README.md
│       ├── _template.md
│       ├── 0001-tauri-over-electron.md
│       ├── 0002-sqlite-over-postgres.md
│       ├── 0003-windows-first-cross-platform-traits.md
│       ├── 0004-rusqlite-over-sqlx.md
│       ├── 0005-code-signing-ca.md  # produced before Phase 7
│       └── ...
└── tests/
    ├── e2e/                         # Playwright (via qa-kitten)
    ├── integration/
    └── fixtures/
        ├── audio/                   # known phrases for STT correctness
        └── transcripts/             # canned for cleanup determinism
```

---

## 5. Tech stack & dependencies

**App shell:** Tauri v2.x (Rust + WebView2). ~22 MB binary, ~50 MB RAM at idle.

**Frontend:** React 19 + Vite + TypeScript + Tailwind CSS v4. Hand-rolled shadcn-style components. Custom canvas waveform. **`react-window`** for virtualized lists (banned: `@tanstack/*` per Appendix D).

**Rust crates (Cargo.toml, src-tauri):**

```toml
[dependencies]
tauri = { version = "2", features = ["tray-icon"] }
tauri-plugin-global-shortcut = "2"   # registration only; hold-detection uses our HotkeyListener
tauri-plugin-store = "2"
tauri-plugin-sql = { version = "2", features = ["sqlite"] }
tauri-plugin-notification = "2"
tauri-plugin-clipboard-manager = "2"
tauri-plugin-fs = "2"
tauri-plugin-dialog = "2"
tauri-plugin-updater = "2"
tauri-plugin-autostart = "2"
cpal = "0.15"
ringbuf = "0.4"
hound = "3"
whisper-rs = { version = "0.13", features = ["cuda"] }
ort = "2"                            # ONNX Runtime — for Silero VAD
enigo = "0.2"                        # cross-platform keyboard injection fallback
rusqlite = { version = "0.32", features = ["bundled"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
reqwest = { version = "0.12", features = ["json", "stream"] }
tokio = { version = "1", features = ["full"] }
zip = "2"
sha2 = "0.10"
anyhow = "1"
thiserror = "2"
tracing = "0.1"
tracing-subscriber = "0.3"
tracing-appender = "0.2"             # log rotation
chrono = { version = "0.4", features = ["serde"] }
uuid = { version = "1", features = ["v4", "serde"] }

[target.'cfg(target_os = "windows")'.dependencies]
windows = { version = "0.58", features = [
  "Win32_UI_Input_KeyboardAndMouse",
  "Win32_Foundation",
  "Win32_UI_WindowsAndMessaging",
  "Win32_Security_Cryptography",
  "Win32_System_DataExchange",
  "Win32_System_Memory",
  "Win32_System_Threading",
] }

[target.'cfg(target_os = "macos")'.dependencies]
# Phase 9: core-graphics, security-framework, objc2

[dev-dependencies]
mockall = "0.13"
tempfile = "3"
pretty_assertions = "1"
rstest = "0.23"
criterion = "0.5"                    # perf budgets
proptest = "1"                       # property tests for migration round-trips
```

**External assets (downloaded at first run, not bundled):**

| Asset                                | Size    | Source                                                                 | Purpose                |
| ------------------------------------ | ------- | ---------------------------------------------------------------------- | ---------------------- |
| `ggml-large-v3-turbo-q5_0.bin`       | ~1.5 GB | huggingface.co/ggerganov/whisper.cpp                                   | Whisper STT (CUDA)     |
| `silero_vad.onnx`                    | ~2 MB   | github.com/snakers4/silero-vad (releases → `silero_vad.onnx`)          | VAD                    |
| Ollama (installer)                   | ~600 MB | ollama.com                                                             | Local LLM runtime      |
| `qwen2.5:3b-instruct-q4_K_M`         | ~2.0 GB | `ollama pull`                                                          | Default cleanup LLM    |
| `gemma2:2b-instruct-q4_K_M`          | ~1.6 GB | `ollama pull`                                                          | Fragment-mode cleanup  |
| `phi3.5:3.8b-mini-instruct-q4_K_M`   | ~2.2 GB | `ollama pull`                                                          | Optional alternative   |

Every download uses the resumable downloader in `scripts/download-models.ps1` with SHA-256 manifest verification. Manifests are checked into the repo at `scripts/model-manifest.json`.

**Disk budget:** ~10 GB for the default stack. Free 30 GB before install.

**Cloud provider option (BYO key):** Anthropic Claude API. Models per pre-flight decision 6. Opt-in per mode.

## 5.1. Developer setup & distribution

**README.md opens with three reader paths:**

> **I just want to use the app** — Download installer from Releases. Run it. Follow setup wizard.
>
> **I want to build it from source** — Run `scripts/setup-dev-windows.ps1`, then `npm run tauri dev`. Full instructions: `docs/DEVELOPMENT.md`.
>
> **I want to understand or contribute** — Start with `PLAN.md`, then `CONTRIBUTING.md`, then `.code_puppy/AGENTS.md`.

**`scripts/setup-dev-windows.ps1`** is idempotent and:

1. Verifies Rust ≥ 1.77 (from `rust-toolchain.toml`); prompts to install rustup if missing
2. Verifies Node ≥ 20 (from `.tool-versions`); prompts if missing
3. Verifies Visual Studio Build Tools (MSVC, Windows SDK)
4. **Verifies CUDA Toolkit 12.x AND cmake AND nvcc on PATH** (offers CPU fallback). Required for `whisper-rs` CUDA build.
5. **Verifies WebView2 Evergreen runtime** (preinstalled on Win11, often missing on Win10 LTSC / fresh installs). If missing, opens MS download page.
6. Verifies Long Paths enabled in Windows (`HKLM\SYSTEM\CurrentControlSet\Control\FileSystem\LongPathsEnabled = 1`). Tauri builds break on default 260-char limit.
7. Checks for Ollama at `%LocalAppData%\Programs\Ollama\ollama.exe`; opens download page if missing
8. `ollama pull qwen2.5:3b-instruct-q4_K_M`
9. `ollama pull gemma2:2b-instruct-q4_K_M`
10. Downloads `ggml-large-v3-turbo-q5_0.bin` to `models/` (resumable, SHA-256 verified)
11. Downloads `silero_vad.onnx` to `models/` (resumable, SHA-256 verified)
12. `npm ci --ignore-scripts` — `.npmrc` makes this the default but the flag is explicit for safety. `npm ci` alone runs lifecycle scripts (the actual Shai-Hulud attack vector); see Appendix D.
13. `cargo fetch --locked`
14. Runs DB migrations against dev profile DB
15. Generates Tauri updater key pair if missing (`tauri signer generate`) — writes public key to `tauri.conf.json`, private key to `~/.tauri/<name>.key` (never committed)
16. Verifies Code Puppy + Wiggum installed; seeds `.code_puppy/judges-template.json` into `~/.code_puppy/judges.json`
17. Verifies pack agents enabled: appends `enable_pack_agents=true` to `~/.code_puppy/puppy.cfg` if absent
18. Prints readiness summary or list of failures

**`scripts/verify-environment.ps1`** is the read-only diagnostic. Outputs machine-readable JSON + human-readable summary so Code Puppy can consume it during build sessions.

**Distribution to end users:**

- Signed Windows installer (NSIS), produced via `npm run tauri build`
- Code signing certificate (~$200–400/yr; ADR 0005 captures CA choice)
- Tauri auto-updater + GitHub Releases endpoint
- Mac DMG + notarization (Phase 9; Apple Developer Program $99/yr)
- **Per-user install only** (no all-users / elevated install in v1) — keeps DPAPI scoping clean and avoids UAC complications.

**Known SmartScreen note:** A newly-signed installer triggers SmartScreen warnings until reputation builds. Document in `docs/KNOWN_ISSUES.md` and link from the release notes.

**`docs/DEPENDENCIES.md`** documents each dependency's role, license, and rationale.

---

## 6. The three modes

Each mode is a row in the `modes` table: id, slug, hotkey, prompt_id, provider, model_id, temperature, max_tokens. Mode rows are seeded by migration 003.

### Mode 1: Normal (default)

- **Hotkey:** `Ctrl+Win` (hold) — preserves Wispr Flow muscle memory
- **Default model:** local `qwen2.5:3b-instruct-q4_K_M`
- **Purpose:** Everyday dictation. Somewhat professional baseline. Light cleanup.

```
You are a dictation cleanup assistant. Your job is to take a raw speech-to-text
transcript and produce clean, send-ready text.

Rules:
- Remove filler words: um, uh, like, you know, basically, kind of, sort of
- Add proper punctuation and capitalization
- Handle mid-sentence corrections: when the speaker says one thing and then
  restates it ("I went to the store, I mean the market"), keep only the
  corrected version
- Preserve the speaker's voice and word choice — do not rewrite for style
- Do not summarize, expand, or invent details
- Do not add or remove sentences
- Keep the same level of formality the speaker used
- If the speaker explicitly says punctuation ("period", "comma", "new
  paragraph"), apply it

Context about the speaker:
{dictionary_terms}

Recent examples of clean output from this user (for style reference only):
{few_shot_examples}

The speaker is dictating into: {foreground_app} — {foreground_window_title}

Raw transcript:
{raw_transcript}

Output the cleaned text only. No preamble, no commentary, no quotes.
```

### Mode 2: Verbose

- **Hotkey:** `Ctrl+Shift+Win` (hold)
- **Default model:** local `qwen2.5:3b-instruct-q4_K_M`
- **Purpose:** Technical, detailed content. Preserve nuance, technical terms, precise language.

```
You are a dictation cleanup assistant for technical and detailed content.
Your job is to take a raw speech-to-text transcript and produce clean text
that preserves the speaker's full meaning and technical precision.

Rules:
- Remove filler words but preserve all technical content, qualifiers, and hedges
- Add proper punctuation and capitalization
- Handle mid-sentence corrections by keeping the corrected version
- Preserve the speaker's voice, word choice, and level of detail — do not
  simplify or shorten
- Long sentences are acceptable when content warrants them
- Do not summarize. Output word count should approximate spoken word count
  minus filler.
- Preserve all technical terms exactly as spoken
- If the speaker explicitly says punctuation, apply it

Context about the speaker:
{dictionary_terms}

Recent examples of clean verbose output from this user:
{few_shot_examples}

The speaker is dictating into: {foreground_app} — {foreground_window_title}

Raw transcript:
{raw_transcript}

Output the cleaned text only. No preamble, no commentary, no quotes.
```

### Mode 3: Fragment

- **Hotkey:** `Ctrl+Alt+Win` (hold)
- **Default model:** local `gemma2:2b-instruct-q4_K_M`
- **Purpose:** Terse output. Quick notes, code comments, terminal commands, bullet lists.

```
You are a dictation cleanup assistant for terse, fragment-style output.
Compress raw speech into the shortest correct text that captures intent.

Rules:
- Remove all filler and redundancy aggressively
- Strip courtesy phrases, hedges, and meta-talk ("I want to write", "let me
  think", "what I mean is")
- If the content is a list, output as bulleted markdown (one "- " per item)
- If a single thought, output a single concise sentence or fragment
- If a command (terminal, code), output it literally with no prose framing
- No greeting, no sign-off, no padding
- Preserve technical terms exactly as spoken
- If input is empty after stripping filler, output nothing

Context about the speaker:
{dictionary_terms}

The speaker is dictating into: {foreground_app} — {foreground_window_title}

Raw transcript:
{raw_transcript}

Output the cleaned fragment only.
```

### 6.1. Hotkey state machine

The Windows shortcuts `Win+Ctrl+→/←/D/F4/Shift+B` are reserved by the OS. Our hold-detection coexists with them because we register at the low-level keyboard hook layer and only trigger on combo *hold* > 80 ms (filters out the OS's tap-based shortcuts). First-run wizard runs a conflict-detection probe; if a chosen hotkey collides with a detected reserved shortcut on the user's machine, the wizard offers `F23` / `F24` / `Ctrl+Shift+Space` alternatives.

```
IDLE
  └─ on key_down(any mode hotkey, held > 80 ms) → RECORDING(mode)
       (taps < 80 ms ignored — passes through to OS for native shortcuts)

RECORDING(mode)
  ├─ on key_up                    → PROCESSING(mode, audio_buffer)
  ├─ on Escape (< 30s recorded)   → CANCELLED (discard audio)
  ├─ on Escape (≥ 30s recorded)   → CONFIRM_CANCEL toast (3s timeout = continue)
  └─ on duration > 300s           → STOPPED (auto-stop, treat as key_up)

PROCESSING(mode, audio)
  ├─ VAD trim
  ├─ Whisper transcribe        → raw_transcript (immutable on write)
  ├─ Cleanup LLM call(mode)    → cleaned_text
  ├─ Secure-input check        → ABORT if secure field focused (toast + log)
  ├─ Text inject               → final_text (clipboard saved+restored)
  └─ Persist to DB (atomic)    → IDLE
  └─ on error                  → ERROR_STATE → IDLE

ERROR_STATE
  └─ Tray notification with retry option
  └─ Recording window flashes red 400 ms then fades
```

**Two mode hotkeys held simultaneously:** first wins; second ignored until first released.
**Same hotkey re-pressed during PROCESSING:** ignored (gentle audio cue if sound feedback on).
**Pause-dictation tray toggle:** sets `state.paused=true`; key-down events no-op until cleared.

---

## 7. Data model (SQLite)

Lives at `%APPDATA%/<name>/<name>.db`. WAL mode. Schema versioned via `tauri-plugin-sql` migrations. **Migrations are append-only — never edit a migration after Phase 1 closes.** The hook engine enforces this (Appendix E).

### Migration 001 — core tables + FTS5

```sql
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE schema_meta (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

INSERT INTO schema_meta (key, value) VALUES ('schema_version', '1');

CREATE TABLE prompts (
  id          INTEGER PRIMARY KEY,
  mode_slug   TEXT NOT NULL,
  version     INTEGER NOT NULL,
  body        TEXT NOT NULL,
  created_at  TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  UNIQUE(mode_slug, version)
);

CREATE TABLE modes (
  id            INTEGER PRIMARY KEY,
  slug          TEXT NOT NULL UNIQUE,
  display_name  TEXT NOT NULL,
  hotkey        TEXT NOT NULL,
  provider      TEXT NOT NULL,
  model_id      TEXT NOT NULL,
  prompt_id     INTEGER NOT NULL REFERENCES prompts(id),
  temperature   REAL NOT NULL DEFAULT 0.3,
  max_tokens    INTEGER NOT NULL DEFAULT 2048,
  enabled       INTEGER NOT NULL DEFAULT 1,
  created_at    TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at    TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE dictionary (
  id            INTEGER PRIMARY KEY,
  term          TEXT NOT NULL,
  canonical     TEXT,
  source        TEXT NOT NULL,
  confidence    REAL NOT NULL DEFAULT 1.0,
  app_context   TEXT,
  use_count     INTEGER NOT NULL DEFAULT 0,
  last_used_at  TEXT,
  created_at    TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  UNIQUE(term, app_context)
);

CREATE INDEX idx_dictionary_term ON dictionary(term);
CREATE INDEX idx_dictionary_use ON dictionary(use_count DESC, last_used_at DESC);

CREATE TABLE dictionary_snapshots (
  id          INTEGER PRIMARY KEY,
  term_ids    TEXT NOT NULL,
  created_at  TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE style_examples (
  id          INTEGER PRIMARY KEY,
  mode_slug   TEXT NOT NULL,
  session_id  INTEGER,
  raw_input   TEXT NOT NULL,
  ideal_output TEXT NOT NULL,
  app_context TEXT,
  source      TEXT NOT NULL,
  rank        REAL NOT NULL DEFAULT 0,
  enabled     INTEGER NOT NULL DEFAULT 1,
  created_at  TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_style_examples_mode ON style_examples(mode_slug, enabled, rank DESC);

CREATE TABLE example_sets (
  id           INTEGER PRIMARY KEY,
  mode_slug    TEXT NOT NULL,
  example_ids  TEXT NOT NULL,
  created_at   TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE sessions (
  id                       INTEGER PRIMARY KEY,
  uuid                     TEXT NOT NULL UNIQUE,
  mode_id                  INTEGER NOT NULL REFERENCES modes(id),
  hotkey_pressed           TEXT NOT NULL,
  started_at               TEXT NOT NULL,
  recording_ended_at       TEXT NOT NULL,
  processing_completed_at  TEXT,
  status                   TEXT NOT NULL,
  error_message            TEXT,
  foreground_app           TEXT,
  foreground_window_title  TEXT,
  audio_duration_ms        INTEGER NOT NULL,
  audio_blob_path          TEXT,
  prompt_id                INTEGER REFERENCES prompts(id),
  dictionary_snapshot_id   INTEGER REFERENCES dictionary_snapshots(id),
  example_set_id           INTEGER REFERENCES example_sets(id),
  stt_latency_ms           INTEGER,
  cleanup_latency_ms       INTEGER,
  injection_latency_ms     INTEGER
);

CREATE INDEX idx_sessions_started ON sessions(started_at DESC);
CREATE INDEX idx_sessions_mode ON sessions(mode_id, started_at DESC);
CREATE INDEX idx_sessions_app ON sessions(foreground_app, started_at DESC);

CREATE TABLE transcripts (
  id           INTEGER PRIMARY KEY,
  session_id   INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  stage        TEXT NOT NULL,                -- 'raw' | 'cleaned' | 'final'
  text         TEXT NOT NULL,
  model_used   TEXT,
  created_at   TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  UNIQUE(session_id, stage)
);

CREATE INDEX idx_transcripts_session ON transcripts(session_id);

-- FTS5 search across all transcript stages
CREATE VIRTUAL TABLE transcripts_fts USING fts5(
  text,
  content='transcripts',
  content_rowid='id',
  tokenize='porter unicode61'
);

CREATE TRIGGER transcripts_fts_insert AFTER INSERT ON transcripts BEGIN
  INSERT INTO transcripts_fts(rowid, text) VALUES (new.id, new.text);
END;

CREATE TRIGGER transcripts_fts_delete AFTER DELETE ON transcripts BEGIN
  INSERT INTO transcripts_fts(transcripts_fts, rowid, text) VALUES('delete', old.id, old.text);
END;

-- transcripts(stage='raw') is IMMUTABLE — no fts update trigger needed
-- (cleaned/final are also written once; learning loop edits style_examples instead)

CREATE TABLE corrections (
  id            INTEGER PRIMARY KEY,
  session_id    INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  before_text   TEXT NOT NULL,
  after_text    TEXT NOT NULL,
  detection_method TEXT NOT NULL,
  classification   TEXT,
  classified_at    TEXT,
  created_at       TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE settings (
  key    TEXT PRIMARY KEY,
  value  TEXT NOT NULL
);

CREATE TABLE learning_runs (
  id                          INTEGER PRIMARY KEY,
  started_at                  TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  completed_at                TEXT,
  sessions_analyzed           INTEGER,
  corrections_classified      INTEGER,
  examples_added              INTEGER,
  examples_removed            INTEGER,
  dictionary_terms_added      INTEGER,
  eval_correction_rate_before REAL,
  eval_correction_rate_after  REAL,
  rolled_back                 INTEGER NOT NULL DEFAULT 0,
  notes                       TEXT
);

UPDATE schema_meta SET value = '1' WHERE key = 'schema_version';
```

### Migration 002 — audit triggers (all four audited tables)

JSON-patch audit triggers on `modes`, `prompts`, `dictionary`, `style_examples`. Pattern shown for `dictionary`; **the same three triggers (INSERT/UPDATE/DELETE) must be created for all four tables** with table-appropriate column projections.

```sql
CREATE TABLE _history_dictionary (
  id        INTEGER PRIMARY KEY,
  row_id    INTEGER NOT NULL,
  operation TEXT NOT NULL,
  patch     TEXT NOT NULL,
  at        TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
-- ... matching _history_modes, _history_prompts, _history_style_examples ...

CREATE TRIGGER dictionary_audit_insert AFTER INSERT ON dictionary
BEGIN
  INSERT INTO _history_dictionary (row_id, operation, patch)
  VALUES (NEW.id, 'INSERT', json_object(
    'term', NEW.term, 'canonical', NEW.canonical, 'source', NEW.source,
    'confidence', NEW.confidence, 'app_context', NEW.app_context
  ));
END;

CREATE TRIGGER dictionary_audit_update AFTER UPDATE ON dictionary
BEGIN
  INSERT INTO _history_dictionary (row_id, operation, patch)
  VALUES (NEW.id, 'UPDATE', json_object(
    'before', json_object('term', OLD.term, 'canonical', OLD.canonical, 'confidence', OLD.confidence, 'enabled', OLD.enabled),
    'after',  json_object('term', NEW.term, 'canonical', NEW.canonical, 'confidence', NEW.confidence, 'enabled', NEW.enabled)
  ));
END;

CREATE TRIGGER dictionary_audit_delete AFTER DELETE ON dictionary
BEGIN
  INSERT INTO _history_dictionary (row_id, operation, patch)
  VALUES (OLD.id, 'DELETE', json_object('term', OLD.term));
END;

UPDATE schema_meta SET value = '2' WHERE key = 'schema_version';
```

### Migration 003 — seed modes + prompts

```sql
-- Seed initial prompt rows (versioned)
INSERT INTO prompts (mode_slug, version, body) VALUES
  ('normal',   1, '<normal prompt body from cleanup/prompts/normal.md>'),
  ('verbose',  1, '<verbose prompt body from cleanup/prompts/verbose.md>'),
  ('fragment', 1, '<fragment prompt body from cleanup/prompts/fragment.md>');

-- Seed modes rows pointing at version-1 prompts
INSERT INTO modes (slug, display_name, hotkey, provider, model_id, prompt_id, temperature, max_tokens) VALUES
  ('normal',   'Normal',   'Ctrl+Win',       'ollama', 'qwen2.5:3b-instruct-q4_K_M', 1, 0.3, 2048),
  ('verbose',  'Verbose',  'Ctrl+Shift+Win', 'ollama', 'qwen2.5:3b-instruct-q4_K_M', 2, 0.3, 4096),
  ('fragment', 'Fragment', 'Ctrl+Alt+Win',   'ollama', 'gemma2:2b-instruct-q4_K_M',  3, 0.2, 1024);

UPDATE schema_meta SET value = '3' WHERE key = 'schema_version';
```

The actual prompt bodies are loaded from `cleanup/prompts/*.md` via `include_str!` at build time and injected by a Rust-side migration helper, not by hand-pasting into SQL. This keeps the prompt source of truth in the Markdown files.

### Three data invariants (binding)

1. **Raw transcripts are immutable.** Once `transcripts(stage='raw')` is written, never UPDATE it. **Enforced by the hook engine** (Appendix E hook `block-raw-transcript-edit`).
2. **Provenance is total.** Every `sessions` row references the exact `prompt_id`, `dictionary_snapshot_id`, and `example_set_id` used. Missing any = `status='error'`.
3. **Deletions are soft where possible.** Use `enabled` flags. Real deletes only on user-initiated "purge history."

### Rollback via `_history_*`

`db/audit.rs` exposes `rollback_to_snapshot(table, row_id, timestamp) -> Result<()>` and `rollback_table_to_timestamp(table, timestamp) -> Result<()>`. Implementations replay the audit log in reverse from `now()` to the target timestamp, applying inverse operations (INSERT→DELETE, DELETE→INSERT, UPDATE→reverse UPDATE). Used by the Phase 8 learning loop on eval regression, and by manual "undo last change" actions in Settings → Advanced.

## 7.1. Export / import (multi-device story)

V1 cross-machine portability. No cloud sync; clean local export/import.

### `.Mockingbird` archive format

```
my-data-2026-05-15.Mockingbird
├── manifest.json
├── database.sqlite
├── settings.json              # non-secret settings
├── dictionary.csv             # human-readable + re-importable standalone
└── style_examples.json        # human-readable + re-importable standalone
```

**`manifest.json`:**

```json
{
  "format_version": 1,
  "app_version": "0.1.0",
  "schema_version": 3,
  "exported_at": "2026-05-15T22:14:00Z",
  "source_machine_hash": "sha256:abc...",
  "includes": ["database", "settings", "dictionary", "style_examples"],
  "audio_blob_count": 0,
  "session_count": 1247
}
```

**Explicitly NOT in archive:** API keys (re-enter; machine-bound by design), audio blobs (separate export), Whisper/Ollama models.

### Partial exports

- "Export dictionary" → `dictionary.csv` only
- "Export style examples" → `style_examples.json` only
- "Export history (date range)" → JSON dump

### Import behaviour

1. Read manifest; if `schema_version` older, run migrations on imported DB before merging.
2. **Pre-import snapshot:** copy current DB to `%APPDATA%/<name>/backups/pre-import-{timestamp}.db`. Reversible.
3. User picks strategy: Replace everything / Merge archive-wins / Merge local-wins.
4. Dictionary merged by `(term, app_context)`.
5. Style examples merged by content hash.
6. Sessions/transcripts append (UUIDs unique).
7. Re-prompt for API keys if any provider was `claude`.

### CLI

```
Mockingbird export --output my-data.Mockingbird [--include database,dictionary,...]
Mockingbird import my-data.Mockingbird --strategy replace|archive-wins|local-wins [--dry-run]
Mockingbird export-dictionary --output dictionary.csv
Mockingbird export-style-examples --output examples.json --mode normal
```

Full format spec: `docs/EXPORT_IMPORT.md`.

---

## 8. Cleanup LLM: provider abstraction

```rust
#[async_trait]
pub trait CleanupProvider: Send + Sync {
    async fn cleanup(
        &self,
        prompt: &str,
        raw_transcript: &str,
        model_id: &str,
        temperature: f32,
        max_tokens: u32,
    ) -> Result<CleanupResult, CleanupError>;

    fn provider_name(&self) -> &'static str;
    fn supports_model(&self, model_id: &str) -> bool;
}

pub struct CleanupResult {
    pub text: String,
    pub model_used: String,
    pub latency_ms: u64,
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
}
```

### Token budget (binding)

`cleanup/token_budget.rs` enforces a per-request budget. Default for local 3B models: 8192 tokens total. Allocation (sums to ≤ 7000, leaving 1192 for the LLM's response):

| Block                 | Budget (tokens) | Source                                      |
|-----------------------|-----------------|---------------------------------------------|
| System prompt         | 600             | mode prompt body (fixed, version pinned)    |
| Dictionary terms      | 600             | top-N by recency × frequency × app-match    |
| Few-shot examples     | 1500            | top-5 by rank × recency, **capped at 1500** |
| Foreground app/title  | 100             | trimmed                                     |
| Raw transcript        | 4200            | trimmed at boundary if oversized            |

The budget calculator returns an error (`PromptOverBudget`) if the raw transcript alone exceeds the remaining budget; the cleanup layer then falls back to a "raw-only" prompt with no few-shot block. Logged at `WARN`.

### Local provider (Ollama)

- HTTP client to `http://localhost:11434/api/chat` with streaming
- Health check on app start: `GET /api/tags` (5s timeout)
- Setup wizard offers Ollama install if missing
- Tray notification with one-click model pull if model missing (`POST /api/pull` with progress)
- Auto-detect `%LocalAppData%\Programs\Ollama\ollama.exe`; if Ollama service is installed but not running, attempt one auto-start before showing an error
- Streaming responses (token-by-token to log, but injection waits for completion)

### Cloud provider (Claude)

- BYO API key, stored via `SecretStore` (Windows DPAPI; Keychain on Mac)
- **Key validated on entry** by hitting `GET /v1/models` — only stored if 200
- Models per pre-flight decision 6
- Streaming, with retry on 429/503 (exponential backoff, max 3 retries, jitter)
- Per-mode cost meter in Settings → Modes, updated from `usage` field in response

### Per-mode provider selection

Settings → Modes shows per-mode provider + model picker. Per-mode override is a first-class feature; mixing local and cloud across modes is supported. Model lists are fetched live:
- Local: `ollama list` parsed
- Cloud: `GET /v1/models` from Anthropic

---

## 9. UX specification

For visual/motion/typography/color/sound details see Section 9.1 (Design system).

### Recording window (floating, frameless, always-on-top, non-activating)

`tauri.conf.json` window config:

```json
{
  "label": "recording",
  "url": "recording.html",
  "decorations": false,
  "transparent": true,
  "alwaysOnTop": true,
  "focus": false,
  "skipTaskbar": true,
  "acceptFirstMouse": false,
  "resizable": false,
  "shadow": true,
  "width": 320, "height": 80,
  "visible": false
}
```

After window creation, Windows-specific fixup applies `WS_EX_NOACTIVATE | WS_EX_LAYERED | WS_EX_TOPMOST` via `SetWindowLongPtrW` and `SetWindowPos(... SWP_NOACTIVATE)` to guarantee no focus theft. Tested under Phase 5's `no-focus-theft` judge.

- Positioned bottom-center of the **monitor under the cursor at hotkey-down time** (multi-monitor aware via `MonitorFromPoint`)
- 320×80 px, rounded corners (12 px), drop shadow, semi-transparent dark surface
- Color-coded status dot (grey/red/amber/green/red-flashing) — animated pulse per Section 9.1
- Mode badge (Normal/Verbose/Fragment in colored pill)
- Live audio waveform (canvas, 60 Hz, last 2s)
- Duration counter (mm:ss after 60s)
- "Esc to cancel" hint
- Slide-in 220 ms ease-out on appear; fade 800 ms after injection completes
- Esc: cancel immediately if <30s recorded; confirmation toast if ≥30s (3s timeout = continue)
- DPI-aware (per-monitor v2)

### System tray

- Idle / Recording / Processing / Error states each have distinct icons (16/24/32/48 px PNG, packaged into `.ico`)
- Click → opens main window (history view)
- Right-click → Pause dictation, Mode submenu, Open history/dictionary/settings, Quit

### History window

- Three-pane: filter sidebar / virtualized session list (react-window) / detail pane
- Detail pane: Raw → Cleaned → Final side-by-side with diff highlighting
- Metadata strip: mode, model, latency breakdown, prompt version, dictionary version
- Actions: Mark as style example, Add term to dictionary, Re-process with..., Copy final, Delete
- Top search bar: SQLite FTS5 across all transcript stages; snippet highlighting in results

### First-run setup wizard

1. Welcome + privacy summary (concrete copy in `docs/DESIGN.md`)
2. Microphone permission + device picker + 3-second level test
3. Hotkey check — probe Windows reserved shortcuts; offer alternatives if conflicts detected
4. Whisper + Silero model downloads (resumable, SHA-256 verified, progress per file)
5. Ollama check (install if missing — opens download page; resume wizard after user returns)
6. Pull default Ollama models (progress UI)
7. Optional Claude API key entry (validated on entry; skippable)
8. Test dictation (records 3s, runs full pipeline, shows result; retry on failure)
9. Done — minimize to tray with "boot on login?" opt-in toggle

### Settings window

Tabs: General, Modes, Models, Dictionary, History, Advanced.

- **General:** hotkey conflicts re-check, autostart toggle, sound feedback toggle, reduced-motion toggle, theme (system/light/dark), log folder opener.
- **Modes:** per-mode provider, model, temperature, max_tokens, prompt version selector (with diff vs default).
- **Models:** live Ollama model list + per-model VRAM estimate; "move LLM to CPU" toggle (mandatory feature per Section 12 item 12).
- **Dictionary:** CRUD with `app_context` scoping.
- **History:** retention policy, "purge history" with confirmation, audio retention toggle.
- **Advanced:** export/import button, learning loop on/off + history table, "view logs folder", "view DB folder", "rollback last learning run" button, telemetry-off badge ("This app sends no data to Anthropic or us. Crash logs are local only.").

### Export/import window

- Export button → file dialog → `.Mockingbird` produced (progress bar for large DBs)
- Import button → file picker → strategy chooser → dry-run preview (counts of new/conflicting rows) → confirm
- Partial export submenu

---

## 9.1. Design system & tokens

The product is visual-quality-sensitive. This section is the source of truth for what "polished" means; the `recording-window-renders` and `first-run-flow` judges measure against it.

### Color tokens (CSS custom properties, `ui/src/design/tokens.css`)

```css
:root {
  /* Surfaces */
  --surf-0: oklch(0.16 0.01 240);              /* deepest dark */
  --surf-1: oklch(0.20 0.01 240 / 0.92);       /* recording window bg */
  --surf-2: oklch(0.24 0.01 240);              /* card */
  --surf-3: oklch(0.30 0.01 240);              /* hover */
  --on-surf: oklch(0.96 0.01 240);
  --on-surf-muted: oklch(0.78 0.01 240);

  /* Status palette */
  --status-idle:        oklch(0.65 0.01 240);  /* grey */
  --status-recording:   oklch(0.66 0.18 25);   /* red */
  --status-processing:  oklch(0.78 0.16 75);   /* amber */
  --status-ok:          oklch(0.72 0.18 145);  /* green */
  --status-error:       oklch(0.62 0.22 25);   /* red-flash */

  /* Mode palette */
  --mode-normal:    oklch(0.70 0.14 230);      /* blue */
  --mode-verbose:   oklch(0.72 0.16 285);      /* violet */
  --mode-fragment:  oklch(0.78 0.14 95);       /* yellow-green */

  /* Radii */
  --r-1: 4px; --r-2: 8px; --r-3: 12px; --r-pill: 999px;

  /* Spacing scale (4-pt) */
  --s-1: 4px; --s-2: 8px; --s-3: 12px; --s-4: 16px; --s-5: 24px; --s-6: 32px;

  /* Type scale */
  --font-sans: "Inter", "Segoe UI", system-ui, sans-serif;
  --font-mono: "JetBrains Mono", ui-monospace, monospace;
  --type-xs: 11px/14px;
  --type-sm: 13px/18px;
  --type-md: 15px/22px;
  --type-lg: 18px/26px;
  --type-xl: 24px/32px;

  /* Elevation */
  --shadow-1: 0 1px 2px oklch(0 0 0 / 0.20);
  --shadow-2: 0 4px 12px oklch(0 0 0 / 0.28);
  --shadow-3: 0 12px 32px oklch(0 0 0 / 0.40);  /* recording window */

  /* Motion */
  --ease-out: cubic-bezier(0.22, 1, 0.36, 1);
  --ease-spring: cubic-bezier(0.34, 1.56, 0.64, 1);
  --dur-1: 120ms;
  --dur-2: 220ms;
  --dur-3: 400ms;
  --dur-fade-out: 800ms;
}

@media (prefers-color-scheme: light) {
  :root { /* light overrides */ }
}

@media (prefers-reduced-motion: reduce) {
  :root { --dur-1: 0ms; --dur-2: 0ms; --dur-3: 0ms; --dur-fade-out: 0ms; }
}
```

### Motion specs

| Element                       | Property              | Duration   | Easing       |
|-------------------------------|-----------------------|------------|--------------|
| Recording window appear       | translateY + opacity  | 220 ms     | --ease-out   |
| Recording window dismiss      | opacity               | 800 ms     | --ease-out   |
| Status dot pulse (recording)  | opacity 1.0 ↔ 0.55    | 1100 ms    | linear loop  |
| Status dot pulse (processing) | rotate via conic-grad | 1400 ms    | linear loop  |
| Status dot flash (error)      | opacity 1.0 ↔ 0.3 ×3  | 400 ms     | --ease-out   |
| Mode badge color crossfade    | color                 | 120 ms     | --ease-out   |
| Waveform render               | canvas redraw         | 60 Hz cap  | n/a          |
| Tab content switch (Settings) | opacity + 4px slide   | 180 ms     | --ease-out   |

`prefers-reduced-motion: reduce` zeros all durations.

### Sound design (opt-in, default off)

Three short cues (WAV, 22 kHz mono, ≤ 200 ms each), placed in `ui/src/design/sounds/`:
- `start.wav` — soft chirp on RECORDING enter
- `done.wav` — gentle tick on successful injection
- `error.wav` — low buzz on error

User toggles per cue in Settings → General.

### Typography

System sans-serif by default (`Segoe UI` on Windows, `system-ui` fallback). Inter shipped as optional WOFF2 if user enables custom font in advanced settings. JetBrains Mono for transcript previews.

### Iconography

Single-source SVG sprite at `ui/src/design/icons.svg`. Tray icons baked from same source via `scripts/build-tray-icons.ps1` at four sizes × four states = 16 PNGs → 4 ICOs.

### Accessibility

- All interactive elements reachable by keyboard with visible focus ring (`--status-recording` outline, 2 px offset)
- ARIA labels on icon-only buttons
- Live regions for recording state announcements (screen-reader users hear "Recording started" / "Injecting cleaned text")
- Contrast: all text on `--surf-1` ≥ 4.5:1, large text ≥ 3:1
- Reduced-motion respected
- High-contrast mode: detects Windows high-contrast theme via `forced-colors: active`; switches to system colors

### Internationalization scaffold

All user-visible strings in `ui/src/i18n/en.json`. Single hook `useT(key)` looks up by key. English-only v1; structure ready for community translation.

---

## 10. Build phases

Each phase is one or more Code Puppy `/goal` invocations. **The phase deliverables, entry criteria, judge config, and exit criteria live in `docs/phases/phase{N}.md`** — this section is the spine. The full per-phase doc is what the slash command points at.

**Each phase MUST end with:**
- All judges returning `complete=True`
- All hook-engine `stop` checks passing (fmt, clippy, tests, STATUS.md staleness)
- A commit on the main branch with a descriptive message
- A `phase-N-complete` git tag (`git tag phase-N-complete`)
- `STATUS.md` updated to mark the phase complete and prepare the next phase's task list
- Any architectural decisions captured as ADRs in `docs/adr/`
- A `docs/LESSONS.md` append entry if the phase surfaced a non-obvious finding

Below is the spine. For each phase, read `docs/phases/phase{N}.md` for the full detail.

### Phase 0: Groundwork

**Goal:** Empty repo with all scaffolding, hook engine, AGENTS.md, ADR template, setup scripts, README skeleton, STATUS.md, git initialized, judges seeded, **hook engine wired, project agents minted, project skills installed.**

**Key deliverables beyond the original plan:**
- `.code_puppy/settings.json` (Appendix E) — hook engine config
- `.code_puppy/agents/*.json` (Appendix G) — project JSON agents
- `.code_puppy/skills/*/SKILL.md` — project skills (Section 11.3)
- `lefthook.yml` — pre-commit (fmt + lint) and pre-push (test) hooks
- `extra_models.json` — round-robin LLM config per pre-flight decision 8
- `puppy.cfg` entry `enable_pack_agents=true`
- `tauri signer generate` run; public key in `tauri.conf.json`, private key out of band
- App icon source SVG + generated icon set
- WebView2 runtime check in `verify-environment.ps1`
- DBOS db URL configured (pre-flight decision 7)

**Judges:** `phase0-structure`, `agents-md-present`, `hook-config-valid`, `judges-seeded`, `adr-format`, `status-initialized`, `setup-script-runs`.

**Exit criteria:** Every deliverable present, `git log --oneline` shows Phase 0 commit, tag `phase-0-complete` exists, STATUS.md says Phase 0 complete, hook engine ran clean on the final commit.

### Phase 1: Foundation

**Goal:** Tauri v2 app opens to tray, SQLite migrations 001-003 applied, settings round-trip, FTS5 search smoke test passes.

**Key deliverables:**
- `src-tauri/Cargo.toml`, `tauri.conf.json` with the recording-window config block from Section 9
- `src-tauri/src/db/migrations/001_initial.sql` (per Section 7, **including FTS5**)
- `src-tauri/src/db/migrations/002_audit_triggers.sql` (**all four audited tables**, not just dictionary)
- `src-tauri/src/db/migrations/003_seed_modes.sql` with prompts loaded from `cleanup/prompts/*.md`
- Migration runner with idempotency + integrity check
- `src-tauri/src/logging.rs` — tracing rotation + PII scrubbing
- `src-tauri/src/settings/model.rs` — typed setting keys (documented in `docs/SETTINGS.md`)
- Tray with placeholder menu
- ADR 0004: rusqlite vs sqlx

**Judges:** `build-passes`, `tests-pass`, `lint-clean`, `migrations-applied`, `fts5-smoke`, `adr-recorded`, `plan-aligned`, `status-updated`.

### Phase 2: Audio capture & STT

**Goal:** Hold a key in a CLI test → speak → see a transcript. CUDA path verified on RTX 2060.

**Key deliverables:**
- `audio/capture.rs` (cpal, 16 kHz mono, ring buffer, default-device-changed handler)
- `audio/vad.rs` (Silero ONNX via `ort` crate, real model file)
- `stt/whisper.rs` (whisper-rs CUDA build, GPU init logged, CPU fallback path)
- `stt/prompt_builder.rs` (224-token `initial_prompt` with recency × frequency × app-match scoring)
- `scripts/download-models.ps1` — resumable, SHA-256-verified
- Audio fixture set in `tests/fixtures/audio/` (synthesized via TTS — Helios builds the generator if absent)
- CLI test harness: `cargo run --bin stt_test -- path/to/file.wav`
- `criterion` bench for STT latency (perf budget: < 1s for 10s audio on RTX 2060)

**Judges:** `build-passes`, `tests-pass`, `lint-clean`, `stt-correct`, `cuda-verified`, `perf-stt`, `adr-recorded`, `plan-aligned`, `status-updated`.

### Phase 3: Global hotkey + text injection

**Goal:** Hold `Ctrl+Win`, speak, release, see raw transcript appear in Notepad — across the standard target app set.

**Key deliverables:**
- `hotkey/mod.rs` — `HotkeyListener` trait (returns key-down AND key-up events)
- `hotkey/windows.rs` — `SetWindowsHookEx(WH_KEYBOARD_LL)` implementation with 80 ms hold threshold
- `hotkey/state.rs` — state machine per Section 6
- `injection/windows.rs` — `SendInput` with Unicode support
- `injection/paste.rs` — **clipboard save before paste, restore after** (binding)
- `injection/strategy.rs` — per-app paste vs keystroke choice with a per-app override table
- `injection/secure_guard.rs` — `SecureInputGuard` trait + Windows impl (detects password fields via `GetGUIThreadInfo` + class name); aborts injection with a tray toast
- `window_context/windows.rs` — `GetForegroundWindow` / `GetWindowText` + process name lookup
- Hotkey conflict detection routine + first-run wizard step
- Sessions + raw transcripts persisted per event
- E2E manual test report covering Notepad, Word, Outlook, Slack, Teams, Chrome (Gmail), VS Code, Claude Desktop, terminal (cmd + PowerShell + Windows Terminal), Cursor

**Judges:** `build-passes`, `tests-pass`, `lint-clean`, `e2e-injection`, `db-provenance`, `clipboard-restored`, `secure-input-respected`, `adr-recorded`, `plan-aligned`, `status-updated`.

### Phase 4: Cleanup LLM (the heart)

**Goal:** Three modes producing distinguishably different output for the same utterance; provider abstraction works for Ollama and Claude; token budgets enforced.

**Key deliverables:**
- `cleanup/provider.rs`, `ollama.rs`, `claude.rs` per Section 8
- `cleanup/token_budget.rs` enforcing the budget table in Section 8
- `secrets/windows.rs` with DPAPI; Claude key validated on entry
- Mode prompts in `cleanup/prompts/*.md` via `include_str!`
- `modes` and `prompts` tables loaded from migration 003 (verified)
- `cleanup/few_shot.rs` selecting top-5 examples per mode by rank × recency, bounded to 1500 tokens
- End-to-end: STT → cleanup → inject cleaned text
- `transcripts(stage='cleaned')` and `transcripts(stage='final')` persisted
- Fixture-based deterministic test: 3 modes × 5 canned raw transcripts, comparing outputs structurally (Normal: cleaned-similar-length, Verbose: preserves length, Fragment: < 50% length); Claude provider mocked

**Judges:** `build-passes`, `tests-pass`, `lint-clean`, `modes-differ`, `provider-swappable`, `secret-storage`, `token-budget-respected`, `perf-cleanup`, `adr-recorded`, `plan-aligned`, `status-updated`.

### Phase 5: Recording UX

**Goal:** Floating window with waveform, status dot, mode badge — matching `docs/DESIGN.md` tokens. **qa-kitten** authors the Playwright spec.

**Key deliverables:**
- Tauri window config per Section 9 + Windows extended-style fixup
- `ui/src/windows/Recording.tsx` (canvas waveform, status dot per Section 9.1 motion, mode badge, duration counter, Esc hint)
- `ui/src/design/tokens.css` + motion module
- Audio level streamed via Tauri events at 60 Hz
- Esc-cancel with 30s confirmation rule
- Multi-monitor cursor-based positioning
- Per-monitor DPI awareness
- Optional sound cues wired (default off)
- qa-kitten Playwright spec in `ui/tests/recording-window.spec.ts` with screenshot baselines

**Judges:** `build-passes`, `tests-pass`, `lint-clean`, `recording-window-renders`, `no-focus-theft`, `design-tokens-applied`, `accessibility-basic`, `adr-recorded`, `plan-aligned`, `status-updated`.

### Phase 6: History & data UI

**Goal:** Full provenance browsable and searchable. Style examples markable. Dictionary editable.

**Key deliverables:**
- `ui/src/windows/History.tsx` (virtualized list via `react-window`, three-pane layout)
- Tauri commands: `list_sessions`, `get_session_detail`, `search_transcripts` (FTS5 snippet output)
- Detail pane: raw/cleaned/final side-by-side with diff highlighting + metadata strip (mode, model, latency, prompt version, dictionary version)
- "Mark as style example" → `style_examples` insert (with `enabled=1`, `source='user_marked'`)
- "Add term to dictionary" with selection picker + app-context capture
- `ui/src/windows/Dictionary.tsx` (CRUD; app-context filter)
- qa-kitten Playwright spec covering: search returns expected sessions, marked example appears in next dictation prompt (log-verifiable), dictionary CRUD

**Judges:** `build-passes`, `tests-pass`, `lint-clean`, `provenance-visible`, `search-works`, `example-loop-closed`, `adr-recorded`, `plan-aligned`, `status-updated`.

### Phase 7: Polish

**Goal:** Feels finished. Signed installer. Auto-updater wired.

**Key deliverables:**
- `ui/src/windows/FirstRun.tsx` per Section 9 (9 steps; resume after Ollama install)
- `ui/src/windows/Settings.tsx` (all six tabs)
- "Undo last dictation" hotkey + handler
- Boot-to-tray (Tauri autostart plugin; opt-in toggle)
- Optional sound feedback toggle
- Mini recording window mode
- Error notification handling (Ollama down → auto-restart attempt; Claude 401/429/503 → clear toast with action)
- `ui/src/windows/ExportImport.tsx`
- `src-tauri/src/export_import/*` per Section 7.1
- Tauri auto-updater configured + `update.json` endpoint at GitHub Releases
- Code signing wired into `tauri build` (cert path from `~/.tauri/<name>.pfx`, password from env)
- NSIS installer customizations: license page, per-user install only, autostart toggle in installer
- ADR 0005: chosen code-signing CA
- qa-kitten Playwright spec covering each settings tab persisting + export/import round-trip

**Judges:** `build-passes`, `tests-pass`, `lint-clean`, `first-run-flow`, `export-import-roundtrip`, `error-handling`, `installer-signed`, `adr-recorded`, `plan-aligned`, `status-updated`.

### Phase 8: Learning loop (stretch)

**Goal:** Nightly job that detects user corrections and improves the system without intervention. Rolls back on regression.

**Key deliverables:**
- Correction detection: clipboard monitor + undo detection within 60s → `corrections` row
- Windows Task Scheduler integration (`schtasks.exe /create`) for `cargo run --bin learn` nightly 2 AM
- `learn` binary:
  1. Pull unclassified corrections from last 7 days
  2. Classify each via local LLM (larger model OK; Ollama)
  3. Update `dictionary` for `new_vocab` classifications
  4. Promote high-quality `(raw → final)` pairs to `style_examples`
  5. Prune `style_examples` per mode to ~50 (lowest rank disabled, not deleted)
  6. Run eval: replay last 24h, compute correction rate (corrections / sessions per mode)
  7. Commit changes only if correction rate didn't go up; else call `rollback_table_to_timestamp` for each touched table
  8. Insert row in `learning_runs`
- Settings → Advanced → "View learning history" + "Rollback last run" button
- Tests: simulated 50-session dataset runs through full job; synthetic regression triggers rollback

**Judges:** `build-passes`, `tests-pass`, `lint-clean`, `correction-detection`, `learning-eval`, `rollback-works`, `adr-recorded`, `plan-aligned`, `status-updated`.

### Phase 9: Cross-platform sweep (macOS)

Deferred. See §2.1 "Platform support" for the binding deliverables (macOS builds pass, Mac first-run handles Accessibility-permission grant, app notarized). All `#[cfg(target_os = "macos")]` `todo!()` stubs become real impls; the cross-platform abstractions designed in from day one mean this phase is mostly "fill in the stubs," not "refactor everything." Not currently scheduled.

### Phase 10: Activity Capture (sibling subsystem)

**Charter ADR:** [ADR 0036](docs/adr/0036-activity-capture-sibling-subsystem.md). **Phase doc:** [`docs/phases/phase10.md`](docs/phases/phase10.md). **Source plan:** [`mockingbird-activity-capture-plan.md`](mockingbird-activity-capture-plan.md).

**Goal:** A third top-level user-visible subsystem. The user starts a session, works normally, and at session end gets a chronological human-readable summary of what they did. Three independent capture layers (UIA accessibility events; optional microphone + chunked Whisper; OPTIONAL post-seal screenshot + OCR) feed one staged summarization pipeline (merge → segment → block → abstract → assemble). Local-first, session-scoped (Start / Pause / Stop), AI is an enhancement-not-a-dependency. Mirrors Phase MC's container shape exactly: numbered phase + chartering ADR + per-wave seal tags + final `phase-10-complete` tag.

**Key constraints (binding — full text in ADR 0036 §Decision):**
- Sibling subsystem under `src-tauri/src/activity/` + `ui/src/pages/Activity{,Detail}.tsx`. Reuses **only** four shared primitives: `audio::AudioCapture`, `meetings::long_form_stt` (library reuse — no extension), `cleanup::OllamaProvider` (via existing `new()` + `CleanupRequest<'_>` — no `CleanupProvider` trait extension), and the migration runner / SQLite repo layer.
- Sealed modules: ALL of `dictation/`, `hotkey/`, `injection/`, `recording_window.rs`, `cleanup/provider.rs`, `cleanup/llm_cleaner.rs`, migrations 001–011, AND all of `meetings/` except `long_form_stt` (read-only library reuse) and `export` (composition only, not edit). Enforced by an extended `block-cross-module-coupling` pre-commit hook (Wave 1).
- Activation: via the **Unified Recording Command Center** authored in Wave 1A (ADR 0037). Chord `Right Ctrl + Space` (proposed; conflict probe per ADR 0019; user-configurable). The Command Center is a single bottom-center overlay with a mode picker for Dictation / Meeting / Activity, replacing the originally-planned per-subsystem `recording_indicator` overlay. Activity Capture has no chord of its own — invocation is through the Command Center's Activity tile. Adds one new `WH_KEYBOARD_LL` hook thread (the Command Center's; activity capture inherits invocation, no new chord).
- Persistence: migration 012 adds `activity_sessions`, `activity_events` (immutable per Principle 1 — trigger enforces), `activity_blocks` (editable derived layer; Q7 cosmetic-only), `activity_transcript_segments` (Wave 4). Schema includes nullable `project_id` + `project_label` on `activity_sessions` from day 1 per Q6 (no IPC / UI in v1). FTS5 on `activity_blocks.generated_abstract` lands in migration 013 (Wave 3). Encryption-at-rest decision deferred to ADR 0038 (Wave 5) — candidates: SQLCipher / DPAPI-per-row / app-layer AES-GCM. (Originally reserved as 0037; renumbered after 0037 was taken by the Command Center charter in Wave 0.5.)
- Exclusion list honored at CAPTURE time (Wave 5), not display time. Defaults: 1Password, Bitwarden, KeePass, browser titles matching `(?i)\b(bank|login|password|signin)\b`, lock screen, UAC. Plus UIA `UIA_IsPasswordPropertyId` snapshot-drop on every sampled focused control — strictly stronger than dictation's `SecureInputGuard` (ADR 0017) because it works across any UIA-exposing app.
- Windows-only v1; cross-platform abstraction (`AccessibilitySnapshot` trait) required from day one with macOS / Linux `todo!()` stubs (Principle 5). macOS impl is the Phase 9 sweep, not Phase 10.
- **Permanently out of scope (Non-Goals — successor ADR required to revisit):** always-on / 24-7 background capture; mobile or browser-extension capture; cloud sync; employee monitoring; real-time live summarization; pixel-perfect screen replay; inline correction → learning loop; project-tagging UI in v1.

**Wave structure (full deliverables in `docs/phases/phase10.md`):**

1. **Wave 1A — Unified Recording Command Center.** Chartered by ADR 0037. New `src-tauri/src/command_center/` module (`mod.rs` orchestrator, pure-Rust `state.rs` state machine, `hotkey.rs` chord hook), new `src-tauri/src/overlay_conventions.rs` shared helper (closes ADR 0026's YAGNI debt), new `command_center` Tauri window + `ui/src/command_center.tsx` + `ui/src/command_center/CommandCenter.tsx` (mode picker / SessionCard / first-run welcome variants). Tray entry in `tray.rs`; settings keys `CommandCenterChord`, `CommandCenterSeenV1`, `LegacyMeetingChordEnabled`; one-shot legacy-meeting-chord migration (mirrors ADR 0033). Surgical edits to sealed Dictation + Meeting Capture surfaces authorized by ADR 0037 §Boundary. Seal tag: `phase-10-wave-1a-complete`.
2. **Wave 1B — Activity-log skeleton (Layer 1 titles-only).** Blocked-by Wave 1A. Migration 012, `activity/` module skeleton (`mod.rs`, `lifecycle.rs`, `sampler.rs`, `persist.rs`, `exclusion.rs` stub, `uia_macos.rs` / `uia_linux.rs` `todo!()` stubs), `commands/activity.rs`, activity invocation wired into the Command Center's mode picker (NO standalone overlay; the Command Center's SessionCard renders the recording surface when `kind=activity`), `Activity.tsx` + `ActivityDetail.tsx` (raw timeline only). Seal tag: `phase-10-wave-1b-complete`.
3. **Wave 2 — UIA deep snapshots + multi-monitor.** Choose `windows-rs` raw COM vs `uiautomation` crate (in-wave audit + brief). `activity/uia.rs` (`AccessibilitySnapshot` trait + Windows impl), `activity/activity_level.rs` (`GetLastInputInfo`-backed coarse idle signal — **no keystroke content captured, ever**), full snapshot payload on `activity_events.snapshot_json`, drill-down UI, multi-monitor enumeration. Seal tag: `phase-10-wave-2-complete`.
4. **Wave 3 — Summarization pipeline.** `activity/segmenter.rs` (Stage 1, pure Rust), `activity/blocker.rs` (Stage 2, pure Rust), `activity/abstractor.rs` (Stage 3 — Ollama via the existing `OllamaProvider`), `activity/assembler.rs` (Stage 4, pure Rust), `activity/prompts/*.md` via `include_str!`, Block CRUD (rename / merge / split / delete / rewrite — `activity_blocks` only; `activity_events` stays immutable), Markdown export, graceful degradation when Ollama is unavailable, migration 013 (additive FTS5 on Block-abstracts). Seal tag: `phase-10-wave-3-complete`.
5. **Wave 4 — Audio layer (Layer 2).** Reuse `audio::AudioCapture` (separate instantiation, no shared state) + `meetings::long_form_stt` (library call). New `activity/audio.rs` orchestrator. Per-session opt-in toggle. Persistent mic-live indicator on the recording overlay. Audio-aware abstractor prompt variant. UIA-exclusion triggers also pause Layer 2. Seal tag: `phase-10-wave-4-complete`.
6. **Wave 5 — Hardening & polish.** ADR 0038 (encryption-at-rest decision — weigh SQLCipher / DPAPI / AES-GCM and pick; renumbered from 0037 after the Command Center charter took 0037). Exclusion list full impl + UIA-password-bit snapshot-drop. Retention policy (configurable auto-delete). Crash recovery (orphaned `in_progress` row promotion → summarize what survived). PDF export + work-report mode. Settings tab "Activity Capture" mirroring the Meetings tab. In-app privacy statement. Seal tag: `phase-10-wave-5-complete`.
7. **Wave 6 — Invariant judges + final seal.** Five judges in `docs/judges/phase-10/`: `ac-raw-events-immutable`, `ac-no-keystroke-content`, `ac-exclusion-honored-at-capture`, `ac-no-llm-in-critical-path`, `ac-summary-degrades-gracefully`. Live-OS smoke matrix on a real Win11 box (per LESSONS PINNED P7 — judges don't catch OS-integration regressions). STATUS + PRODUCT-STATE updates. Seal tag: **`phase-10-complete`**.

**Wave 7 (OPTIONAL, post-seal — NOT part of `phase-10-complete`)**: Layer 3 periodic screenshots + local OCR for accessibility-blind apps. Successor ADR (likely 0039). Lateral-epic shape per AGENTS.md — sealed via STATUS update + ADR Accepted, NOT by a new phase tag.

**Cargo gate (binding):** the existing accepted fallback gate per LESSONS PINNED P2. Pure modules → throwaway-crate recipe (LESSONS 2026-05-17). Wired modules → `cargo-with-cuda.ps1 check` + `clippy --release -- -D warnings` + `fmt --check` + `test --release --no-run` + per-wave human-in-loop smoke matrix in `phase10.md`. **No new gate proposed by Phase 10.** A parallel investigation bead is open against the `cargo test --release` `STATUS_ENTRYPOINT_NOT_FOUND (0xc0000139)` root cause; its resolution is independent and not blocking.

**Judges (final wave only):** `ac-raw-events-immutable`, `ac-no-keystroke-content`, `ac-exclusion-honored-at-capture`, `ac-no-llm-in-critical-path`, `ac-summary-degrades-gracefully`. Plus the existing cross-phase judges (`adr-format`, `status-updated`, etc.) where applicable.

**Exit criteria for `phase-10-complete`:** All Wave 1A / 1B / 2 / 3 / 4 / 5 / 6 seal tags placed; all five Wave-6 judges green; live-OS smoke matrix all-green on a real Win11 box (signed off in STATUS); ADR 0036 + ADR 0037 + ADR 0038 all Accepted; `STATUS.md` Sealed table includes Phase 10; `docs/PRODUCT-STATE.md` Activity Capture subsystem section authored.

---

## 11. Agent workflow (/goal + Wiggum + Hook Engine)

### Tools and where they fit

- **Code Puppy** — the framework. Reads `.code_puppy/AGENTS.md` (combined with `~/.code_puppy/AGENTS.md`) at the start of every iteration. Auto-load is implemented in `code_puppy/agents/_builder.py`.
- **Wiggum plugin** — provides `/goal <prompt>` (convergent loop with judges) and `/wiggum <prompt>` (mechanical repeat loop). `/goal` is THE primary orchestration primitive for this project. Iteration boundaries clear context; persistent state is disk + git + STATUS.md. No automatic max-iteration cap — abandon if > 10.
- **`code-puppy` (the agent)** — the default general-purpose implementor. This is the agent that should be active when you run `/goal` for any build phase. It can read/write files, run shell commands, and delegates to specialists via `invoke_agent`.
- **Named specialist agents** — planning-agent (📋), qa-kitten (🐱), helios (☀️), agent-creator. Used at specific points by switching the active agent OR via `invoke_agent` from inside a /goal run.
- **Project JSON agents** — `.code_puppy/agents/*.json` define tool-scoped specialists for this codebase. See Appendix G. They show up in `/agent` like any other agent; switch into them for entire scoped phases or call them via `invoke_agent` from within a /goal run.
- **Hook Engine** — `.code_puppy/settings.json` declares pre/post/start/stop hooks that mechanically enforce binding rules. See Appendix E.
- **token_ratio_learner + extra_models.json** — rotates LLM keys for long runs.
- **frontend_emitter** — streams `tool_call_start/complete`, `agent_invoked`, `stream_event` to any consumer. Useful for a live progress dashboard.
- **Judges** — independent read-only LLM agents at `~/.code_puppy/judges.json`. Each judge gets read-only tools. Returns `{complete: bool, notes: str}`.
- **Git** — durable memory. Commits = checkpoints. Phase boundaries get tags (`phase-N-complete`).
- **STATUS.md** — the persistent task tracker that survives context clearing.
- **docs/phases/phase{N}.md** — full per-phase spec. The slash command points at it.
- **docs/LESSONS.md** — agents append non-obvious findings here, reducing re-discovery cost.

**On pack agents:** Pack agents (pack-leader, bloodhound, shepherd, terrier, watchdog, retriever) are DEPRECATED in Code Puppy. The framework reserves the names but ships no implementations — the devs confirmed this. Do not attempt to invoke them. All orchestration runs through code-puppy (or a project JSON agent) as the receiver of `/goal`, with delegation to the named specialists above.

### Orchestration model (the human drives boundaries)

The human orchestrates by switching the active agent at phase boundaries and at specific moments within a phase. Inside a /goal run, the active agent handles its own delegation via `invoke_agent`.

Standard rhythm for a phase:

1. `/agent planning-agent` — switch to the planner.
2. `/plan-phase N` (slash command from Appendix B) — decompose Phase N into STATUS.md tasks. Commit.
3. `/agent code-puppy` — switch back to general implementor (or to a project JSON agent if the phase is single-domain — e.g. `/agent migration-author` for Phase 1's DB work).
4. `/phase{N}-goal` — kicks off the Wiggum /goal loop. The active agent reads PLAN + STATUS, executes, judges verify, loops until convergent.
5. For phases that include UI: at the end of an iteration that touches `ui/`, `/agent qa-kitten` and `/qa-window` to run Playwright screenshot verification. Then `/agent code-puppy` to resume the /goal loop if judges flagged remediation.
6. When Phase N's judges all pass + `phase-N-complete` tag is placed, loop back to step 1 for Phase N+1.

### Role assignments (which agent does what)

| Phase moment | Active agent (switch with `/agent <name>`) | Why |
|---|---|---|
| Start of phase: decompose deliverables into STATUS.md tasks | **planning-agent (📋)** | Built for this; produces ordered execution plans. |
| Phase 0–8 main work (Wiggum /goal loop) | **code-puppy** (or a scoped project JSON agent if the phase is narrow) | General implementor; delegates to specialists via invoke_agent. |
| Phase 1 (mostly DB migrations) | **migration-author** (project JSON agent) | Tool-scoped; can't accidentally edit out-of-scope files. |
| Phase 3 (mostly hotkey + injection) | **injection-author** (project JSON agent) | Scoped to injection/, hotkey/, window_context/. |
| Phase 4 (prompts only) | **prompt-tuner** when tuning prompts; code-puppy for everything else | Scoped writes to prompts/*.md. |
| Phase 5–7 (UI implementation) | **ui-author** (project JSON agent) for UI changes; code-puppy for the Rust side | Scoped to ui/. |
| Phase 5–7 (UI visual verification at end of iteration) | **qa-kitten (🐱)** | Playwright + screenshots. |
| Phase 8 (learning loop) | **learning-loop-author** (project JSON agent) | Scoped to bin/learn.rs and learning/. |
| Building one-off helper tools (audio fixture generator, etc.) | **helios (☀️)** | Universal Constructor. |
| Phase 0 step: mint project JSON agents | **agent-creator** | Writes `.code_puppy/agents/*.json` from spec. |

Inside a single /goal iteration, the active agent handles its own delegation — it doesn't need you to switch mid-loop. It calls `invoke_agent` for sub-tasks. The agent switching happens at the boundaries.

### The Wiggum /goal loop

Wiggum sets `clear_context: True` between iterations. **The active agent sees nothing from prior iterations except what lives on disk, in git, and via STATUS.md.** This is the central constraint.

```
/goal <phase prompt>
  └─ iteration 1
      ├─ AGENTS.md auto-loaded by Code Puppy
      ├─ Hook engine `session_start` fires — prints current phase, last 5 commits, last judge verdicts
      ├─ active agent reads PLAN.md, STATUS.md, docs/phases/phase{N}.md, runs `git log --oneline -20`
      ├─ active agent executes work, calling invoke_agent for specialists as needed
      │   (e.g. invoke_agent("qa-kitten", ...) for Playwright; invoke_agent("helios", ...) for a missing tool)
      ├─ Each tool call passes through hook engine pre_tool_use — vetoes possible
      ├─ Each tool call's result passes through post_tool_use — logging, post-checks
      ├─ active agent updates STATUS.md, commits
      ├─ Hook engine `stop` fires — refuses exit if fmt/clippy/test fail
      ├─ All enabled judges run in parallel
      ├─ if every non-abstaining judge returns complete=True → DONE; tag `phase-N-complete`
      └─ if any return complete=False → next iteration receives:
              {goal_prompt}
              
              Judge remediation notes:
              [judge_name] FAIL
                <specific remediation instructions>
              ...
  └─ iteration 2
      ├─ context cleared
      ├─ active agent re-discovers state from disk + git + STATUS.md
      ├─ reads remediation notes
      └─ executes remediation
```

**Termination:** unanimous non-abstaining judge approval, Ctrl+C, or `/goal_stop`. Abandon if iteration count exceeds 10 on a single phase — that signals a judge prompt or a deliverable is misspecified.

### Per-phase setup (the human runs these before `/goal`)

1. Read PLAN.md spine + `docs/phases/phase{N}.md` for the phase you're about to start.
2. `/agent planning-agent` then `/plan-phase N` — produces/refreshes the STATUS.md task list for phase N. Commit.
3. Verify `STATUS.md` reflects the current phase.
4. `/agent code-puppy` (or `/agent <project-json-agent>` if the phase is narrow — see role table).
5. Configure judges via `/judges` TUI:
   - Enable the judges listed in `docs/phases/phase{N}.md`.
   - Disable judges from earlier phases that no longer apply.
   - Verify each judge's model responds.
6. Run `/phase{N}-goal` to kick off `/goal`.

### Judge prompt design (CRITICAL)

Because there's no automatic max-iteration safety net, judges must be:

- **Single-criterion.** One judge = one specific check.
- **Concrete and verifiable.** Prefer a shell command (`cargo test`, `cargo clippy`, file existence check) over LLM inspection. The default judge prompt explicitly prefers concrete verification.
- **Strict.** Marking incomplete on slight ambiguity is much cheaper than a false pass.
- **Helpful in failure.** Notes must tell the implementor exactly what to do to fix it. "Tests fail" is bad; "`db::migration::test_up_to_date` failed: expected schema_version=3, got 2. Run `cargo test db::migration -- --nocapture` to see details." is good.
- **Tier-appropriate model.** Use Haiku-tier for deterministic shell-driven judges (build, test, lint). Use Sonnet-tier for subjective judges (modes-differ, visual judges, first-run-flow). See Appendix C for per-judge tiering.

### STATUS.md format

```markdown
# STATUS

**Current phase:** Phase 1: Foundation
**Started:** 2026-05-15
**Last updated:** 2026-05-15T22:30:00Z (iteration 3)

## Phase outline

- [x] Phase 0: Groundwork (completed 2026-05-15, tag phase-0-complete)
- [ ] Phase 1: Foundation (in progress)
- [ ] Phase 2: Audio capture & STT
- [ ] Phase 3: Global hotkey + text injection
- [ ] Phase 4: Cleanup LLM
- [ ] Phase 5: Recording UX
- [ ] Phase 6: History & data UI
- [ ] Phase 7: Polish
- [ ] Phase 8: Learning loop (stretch)

## Phase 1 tasks

- [x] Initialize Tauri v2 project with React+TS+Vite
- [x] Apply base Tauri plugins (sql, store, notification, etc.)
- [x] Implement db/mod.rs with rusqlite migration runner
- [ ] Apply migration 001 (core tables + FTS5)
- [ ] Apply migration 002 (audit triggers — all 4 tables)
- [ ] Apply migration 003 (seed modes + prompts)
- [ ] Implement settings/mod.rs with get/set commands
- [ ] System tray with placeholder menu
- [ ] Phase 1 exit verification

## Phase progress notes

- Settled on rusqlite over sqlx for embedded simplicity (ADR 0004)
- FTS5 included in migration 001 so Phase 6 doesn't need a schema change
- Migration runner uses tauri-plugin-sql's mechanism

## Blocked / human input needed

- (none)

## Cost & token usage this phase

- Iteration 1: ~14K input, ~6K output (sonnet)
- Iteration 2: ~12K input, ~5K output (sonnet)
- Iteration 3: in progress

## Last successful judge run

- 2026-05-15T22:14:00Z: build-passes ✓, tests-pass ✓, lint-clean ✓, migrations-applied ✗ (FTS5 missing — added in iteration 3)
```

### `/goal` invocation pattern per phase

Goal prompts are short and reference PLAN.md by section + the phase doc. Slash commands expand to the file contents.

Example (`phase1-goal.md`):

```
Complete Phase 1 (Foundation) per PLAN.md Section 10 and docs/phases/phase1.md.

Read AGENTS.md, PLAN.md (spine), docs/phases/phase1.md, and STATUS.md
before starting. Run `git log --oneline -20` and `git status`.

Confirm the hook engine config at .code_puppy/settings.json is loaded
(SessionStart hook should have printed phase context).

For sub-tasks that benefit from a specialist, use invoke_agent:
- planning-agent → if STATUS.md task list is stale or missing
- qa-kitten → any UI verification (none expected in Phase 1)
- helios → if you need a tool that doesn't exist yet
- Alternatively, the human may have switched the active agent to a 
  project JSON agent (e.g. migration-author for Phase 1) — in which 
  case work happens directly within scope.

When you believe Phase 1 is complete:
1. Run `cargo fmt`, `cargo clippy -- -D warnings`, `cargo test`
2. Run `npm run build` and `npm test`
3. Verify `npm run tauri dev` opens the app to tray with no errors
4. Verify migrations 001, 002, 003 all run cleanly on a fresh DB
5. Verify FTS5 search returns hits on seed data
6. Ensure STATUS.md shows all Phase 1 tasks complete + cost line + last-judge line
7. Ensure Phase 1 ADRs are present in docs/adr/
8. Commit a final "Phase 1 complete" commit and tag `phase-1-complete`
9. Exit so judges can verify

The judges enforce the actual exit criteria. The hook engine will refuse
exit if fmt/clippy/test fail. Read judge remediation notes carefully
if marked incomplete.
```

### Critical agent rules (also in AGENTS.md)

1. **Always re-read PLAN.md spine and STATUS.md at the start of each iteration.** You don't remember anything from the previous iteration.
2. **Read only the current phase doc** (`docs/phases/phase{N}.md`), not all of them — keeps context cost bounded.
3. **Always update STATUS.md and commit at the end of each iteration.** Even if work is incomplete — write progress notes.
4. **Never skip writing tests.** The `tests-pass` judge AND the `stop` hook will fail you.
5. **Never modify a raw transcript row.** Period. The hook engine vetoes this at the tool layer.
6. **Write an ADR for any non-trivial architectural decision.** `adr-recorded` judge checks the diff.
7. **Run linters before exiting.** `lint-clean` judge + `stop` hook are strict.
8. **If a judge fails repeatedly with the same complaint, escalate.** Either the judge prompt is wrong, or the deliverable in PLAN.md is misspecified. Don't burn 20 iterations on a broken judge.
9. **Append to docs/LESSONS.md when something surprises you.** Future iterations save time reading it.
10. **Delegate to specialists via `invoke_agent`** when one fits the task (planning-agent for decomposition, qa-kitten for UI verification, helios for tool building). Don't try to do everything yourself when a specialist is the right tool.

### 11.1. Hook engine configuration

See Appendix E for the full `.code_puppy/settings.json`. It declares these mechanically-enforced rules:

- `block-raw-transcript-edit` — pre_tool_use vetoes any `edit_file` / `create_file` targeting a path that would mutate `transcripts(stage='raw')` data files
- `block-migration-edit-after-phase-1` — pre_tool_use vetoes edits to `db/migrations/00[12].sql` once `git tag phase-1-complete` exists
- `block-unsafe-npm` — pre_tool_use vetoes `npm install|ci|i` without `--ignore-scripts` (Appendix D primary defense)
- `block-tanstack` — pre_tool_use vetoes any `package.json` edit adding an `@tanstack/*` dep
- `block-secret-commit` — pre_tool_use vetoes `git commit` if `git diff --cached` contains plausible secrets (high-entropy strings, `.env`, `*.key`, `*.pfx`)
- `block-force-push` — handled by the `force_push_guard` plugin (enable in puppy.cfg)
- `block-destructive-shell` — handled by `destructive_command_guard` plugin (enable)
- `session-start-briefing` — prints current phase, last 5 commits, last judge verdicts on session start
- `stop-quality-gate` — refuses session exit if `cargo fmt --check && cargo clippy -- -D warnings && cargo test --quiet` doesn't pass
- `post-commit-status-check` — warns if STATUS.md wasn't touched in the same commit

### 11.2. Project JSON agents

See Appendix G for the full JSON definitions. Five agents:

- `migration-author` — tools restricted to read + create/edit on `src-tauri/src/db/migrations/*.sql` and `db/migrations/` only. Used for Phase 1.
- `injection-author` — tools restricted to `src-tauri/src/injection/**` and `src-tauri/src/window_context/**`. Used for Phase 3.
- `ui-author` — tools restricted to `ui/src/**` + `ui/tests/**`. Used for Phase 5–7.
- `prompt-tuner` — read access broad, write access only to `src-tauri/src/cleanup/prompts/*.md`. Used during Phase 4 + ongoing.
- `learning-loop-author` — write access to `src-tauri/src/bin/learn.rs` and `src-tauri/src/learning/**`. Used for Phase 8.

Note on tool restrictions: `json_agent.py` filters which tools are available to the agent, but does NOT enforce per-argument scoping (e.g., "can only edit files matching glob X"). Per-argument scoping is implemented by the **hook engine** at pre_tool_use. So the JSON agent narrows the tool set; the hook narrows the arguments.

### 11.3. Project skills

Project skills installed in `.code_puppy/skills/<name>/SKILL.md`. The agent calls `list_or_search_skills` and `activate_skill` to pull these on demand rather than re-reading the full PLAN.md each iteration.

- `data-model` — Section 7 distilled: schema, three invariants, audit pattern, rollback function. ~80 lines.
- `injection-recipes` — Win32 SendInput patterns, per-app strategy table, clipboard save/restore recipe, secure-input detection. ~120 lines.
- `supply-chain` — Appendix D actionable rules. ~60 lines.
- `quality` — Section 17 + lint commands + perf budgets. ~80 lines.
- `prompts` — Section 6 mode prompts (canonical text) + token budget table. ~100 lines.
- `design-tokens` — Section 9.1 tokens + motion + accessibility checklist. ~120 lines.

Each skill has a Claude-Code-compatible frontmatter (`name`, `description`) so the agent's skill-search returns it on relevant keywords.

---

## 12. Critical "do not skip" notes

Every item below is **also enforced by a hook in Appendix E** where mechanically possible. Documentation alone is not the defense — the hook is.

1. **Write migrations 001, 002, 003 before any data is written.** Audit triggers must exist before the first INSERT or we lose history of the earliest records. Modes/prompts seed must exist before Phase 4 can run cleanup.
2. **Persist `prompt_id`, `dictionary_snapshot_id`, `example_set_id` on every session.** Without these, the system is not reproducible and the learning loop is impossible.
3. **`raw` transcripts are immutable.** Never UPDATE them. Even if Whisper changes its mind, write a new row. Hook `block-raw-transcript-edit` enforces.
4. **VAD before Whisper.** Whisper hallucinates badly on silence. Trimming via Silero first is non-negotiable.
5. **The `initial_prompt` is only 224 tokens.** Implement smart selection (recency × frequency × app-match), not "dump everything."
6. **Per-mode model selection is a v1 feature, not v2.** Three rows in `modes` + a settings UI — it's the entire reason this architecture beats OSS alternatives.
7. **Provider trait must exist from Phase 4.** Route everything through the trait. New providers later become a 1-file change.
8. **DB transactions wrap the full session write.** Session + raw + cleaned + final + dictionary_snapshot + example_set must all commit atomically or all roll back. Partial state is worse than no state.
9. **Settings encryption for the Claude API key is mandatory.** Use Windows DPAPI via `windows-rs`. Never store plaintext. The `secret-storage` judge inspects the settings DB for the entered key.
10. **The recording window must be non-activating.** If it steals focus, text injection breaks. Use `WS_EX_NOACTIVATE`, `SWP_NOACTIVATE`, and Tauri's `focus: false` + `skipTaskbar: true`.
11. **Test text injection in: Notepad, Word, Outlook, Slack, Teams, Chrome (Gmail), VS Code, Claude Desktop, terminal (cmd + PowerShell + Windows Terminal), Cursor.** Each has quirks; the per-app strategy table will grow.
12. **Don't let Ollama and Whisper fight over VRAM.** With Whisper q5 (1.5 GB) + Qwen 3B q4 (2 GB) you're fine on 6 GB, but the settings toggle to move LLM to CPU is mandatory for users with larger models.
13. **No telemetry. Period.** Crashes log locally only. Settings has "open logs folder."
14. **Don't make the learning loop block dictation.** Separate process via Task Scheduler; main app uses whatever example set / dictionary is current at request time.
15. **Cross-platform abstractions from day one, even in Windows-only v1.** macOS later is "fill in the stubs," not "refactor everything."
16. **Supply chain hygiene is binding.** `.npmrc` sets `ignore-scripts=true` repo-wide. All installs use `npm ci --ignore-scripts`. Pin exact versions. Commit `package-lock.json`. Avoid `@tanstack/*` entirely. Hook `block-unsafe-npm` + `block-tanstack` enforce. Full spec: Appendix D.
17. **Clipboard save/restore around every paste.** The user's clipboard is not your scratch space. `paste.rs` saves the current clipboard contents, writes the cleaned text, pastes, then restores. Hook `block-bare-paste` warns on `set_clipboard` calls outside the helper.
18. **Secure-input fields abort injection.** Detect password fields and UAC dialogs before injecting. Show a tray toast; never silently inject into a password prompt.
19. **Migrations are append-only after Phase 1 ships.** Once `phase-1-complete` tag exists, hook `block-migration-edit-after-phase-1` vetoes edits to 001/002/003. New schema changes go in new migrations.
20. **Hold-detection requires a low-level keyboard hook, not the global-shortcut plugin.** The plugin fires on press; we need key-up too. `windows-rs` + `SetWindowsHookEx(WH_KEYBOARD_LL)`.
21. **Pin every dependency exactly.** `save-exact=true` in `.npmrc`. Rust deps pinned in `Cargo.lock` and committed.

---

## 13. Open questions / decisions deferred

These are non-blocking but should be revisited at the indicated phase.

1. **Microsoft Store listing** — revisit after v1.0 has ~30 days of reputation. Deferred.
2. **Update channel split (stable vs beta)** — Tauri updater supports multiple manifests. Decide before Phase 7 if a beta channel is desired.
3. **Multi-monitor recording window placement** — chosen: cursor-position monitor. Closed. (Was Section 13 open question 5 in v1.)
4. **Audio device hot-swap** — chosen: tray notification, cancel session, fall back to default device. Closed.
5. **Hotkey conflicts** — chosen: detect on startup + first-run wizard probe. Closed.
6. **Boot trigger choice (Run key vs Task Scheduler)** — chosen: Tauri autostart plugin (Run key) for v1; Task Scheduler reserved for Phase 8 nightly job only. Closed.
7. **Crash report opt-in** — out of scope for v1. Logs are local only.
8. **Phase 9 macOS target date** — deferred to post-v1 retrospective.

---

## 14. Reference / inspiration

Studied to inform this design:

- **Wispr Flow** (closed source) — docs.wisprflow.ai for feature inventory, per-app personalization styles
- **Superwhisper** (closed source, Mac) — recording window UX docs
- **TypeWhisper** (open source) — per-app workflows, dictionary, HTTP API for automation
- **Whispering** (open source, Svelte/Tauri) — Tauri patterns, transformation pipeline
- **OpenWhispr** (open source, Electron) — local Whisper integration, agent integration
- **Muesli** (open source, Mac) — agent-friendly SQLite schema, CLI-as-API pattern
- **whisper.cpp** (ggerganov) — streaming + VAD examples
- **silero-vad** (snakers4) — ONNX VAD source
- **sqlite-history-json** (Simon Willison) — JSON-patch audit log pattern
- **Code Puppy** (mpfaffenberger/code_puppy) — the agent framework; pack agents, hook engine, judges
- **DSPy** (Stanford) — programmatic prompt optimization, applied informally via the learning loop

Academic references:
- "Zero-shot Domain-sensitive Speech Recognition with Prompt-conditioning Fine-tuning" (MediaTek Research, 2023) — up to 33% WER reduction from prompt-conditioning on Whisper.
- "Improving Rare-Word Recognition of Whisper in Zero-Shot Settings" (2025) — contextual biasing without fine-tuning.

---

## 15. Definition of done (v1)

**On a fresh Windows machine, a new user can complete the setup wizard in under 5 minutes and dictate productively into all of their daily apps with quality matching or exceeding Wispr Flow, with zero data leaving their machine by default, with their dictation history fully browsable, searchable, and reproducible, and with the recording window visually polished to the standards in `docs/DESIGN.md`.**

Phase 8 (learning loop) is not required for v1 ship, but the schema must support it.

Quantitative gates (measured on the RTX 2060 target box):
- STT latency < 1s for 10s of audio (Whisper large-v3-turbo q5)
- Cleanup latency < 800 ms with local Ollama 3B model
- End-to-end (key-up → text-inserted) < 1.5s for typical 5s utterance
- App idle RAM < 100 MB
- App startup to tray < 2s
- First-run wizard completion < 5 minutes (on a machine with no model assets)

---

## 16. Glossary

Pin down terms so the agent doesn't drift terminology across iterations.

- **Session** — one dictation event, from hotkey-down to text-injected (or cancelled/errored). One row in `sessions`.
- **Transcript stage** — one of `raw` (Whisper output), `cleaned` (after LLM cleanup), `final` (what was actually injected). Stored as separate rows in `transcripts` keyed by `(session_id, stage)`.
- **Mode** — Normal / Verbose / Fragment. Each is a row in `modes` with its own hotkey, prompt, provider, and model.
- **Provider** — Local (Ollama) or Cloud (Claude API). Selected per-mode. Abstracted behind the `CleanupProvider` trait.
- **Cleanup LLM** — the LLM used for Layer 3, distinct from Whisper (Layer 2).
- **Initial prompt** — Whisper's vocabulary-biasing input, max 224 tokens. Built from the dictionary in `prompt_builder.rs`. Not the same as the cleanup LLM's system prompt.
- **System prompt** — the cleanup LLM's mode-specific prompt template. One per mode. Versioned via the `prompts` table.
- **Few-shot example** — a `(raw_input, ideal_output)` pair stored in `style_examples`. Up to 5 per mode at request time, capped at 1500 tokens collectively.
- **Style example** vs **dictionary term** — different. Dictionary terms are vocabulary used for Whisper biasing AND injected into cleanup prompt context. Style examples are full input/output pairs used as few-shot demonstrations in the cleanup prompt only.
- **Snapshot** — a frozen reference set used at request time. `dictionary_snapshot_id` references the exact dictionary terms used; `example_set_id` references the exact style examples used.
- **Correction** — a user edit detected after dictation. Stored in `corrections`. Drives the learning loop (Phase 8).
- **Audit log** — the `_history_*` tables. JSON-patch deltas on every INSERT/UPDATE/DELETE for the audited tables.
- **Implementor** (a.k.a. active agent) — the Code Puppy agent doing the actual work in a Wiggum iteration. Usually `code-puppy`, sometimes a project JSON agent (migration-author, ui-author, etc.) when the phase is scoped. Delegates to specialists (planning-agent, qa-kitten, helios) via `invoke_agent`.
- **Active agent** — the agent currently selected via `/agent <name>`. Receives `/goal` and `/wiggum` invocations. Default after launch is whatever `default_agent` is set to in puppy.cfg (usually `code-puppy` or `planning-agent`).
- **/goal** — Wiggum convergent loop. Active agent executes the prompt, judges verify, loops until unanimous pass or human stop. This project's primary orchestration primitive.
- **Pack agents (DEPRECATED)** — formerly intended orchestration layer (pack-leader, bloodhound, shepherd, terrier, watchdog, retriever). Removed from Code Puppy; names reserved but no implementations ship. Do not use.
- **Specialist agent** — a named agent (planning-agent, qa-kitten, helios, agent-creator) for a specific job.
- **JSON agent** — a tool-restricted agent declared in `.code_puppy/agents/*.json` for project-specific work.
- **Judge** — a separate read-only Code Puppy agent that verifies one criterion at the end of an iteration.
- **Iteration** — one pass through a Wiggum `/goal` loop. Ends when the implementor exits, after which judges run.
- **Goal** vs **Wiggum mode** — `/goal` is convergent (loops until judges pass); `/wiggum` is dumb repeat.
- **Abstain (judge verdict)** — judge couldn't render a verdict due to infrastructure failure. Excluded from the unanimous-pass tally.
- **Phase** — a chunk of work in Section 10 (Phase 0 through Phase 8). Each phase has its own `docs/phases/phase{N}.md`.
- **STATUS.md** — the persistent task tracker that survives context-clearing between iterations.
- **AGENTS.md** — standing rules loaded by Code Puppy at the start of every iteration.
- **Hook engine** — Code Puppy's pre/post/start/stop hook system. `.code_puppy/settings.json`. Mechanically enforces binding rules.
- **DBOS** — durable execution plugin. Checkpoints LLM/MCP/tool calls; survives crashes.
- **ADR** — Architecture Decision Record, in `docs/adr/`. Michael Nygard format.
- **Skill** — a `SKILL.md` in `.code_puppy/skills/<name>/` discoverable via `list_or_search_skills`.

---

## 17. Quality standards

Summary. Full document at `docs/QUALITY.md`. The `lint-clean` judge enforces most of this mechanically; the `stop` hook is the second line of defense.

### Rust

- Edition 2021, Rust ≥ 1.77
- `.rustfmt.toml` configuration is law; `cargo fmt --check` must pass
- `cargo clippy -- -D warnings` must pass; no allow-by-default in the workspace
- Error handling: `Result<T, E>` everywhere; `.unwrap()` only in tests
- Per-module error types via `thiserror::Error`
- `tracing` crate for logging, not `println!` (except in CLI tools)
- Async via `tokio`; no `block_on` outside `main` / tests
- Public APIs have doc comments (`///`)
- Module-level docs (`//!`) describe the module's role
- File size: hard limit 600 lines per file

### TypeScript / React

- Strict mode TS; no `any` without a `// SAFETY:` comment explaining why
- React 19 conventions; no class components
- `.eslintrc.cjs` configuration is law
- Tailwind v4 for styling; no inline styles except for genuinely dynamic values
- Tauri commands wrapped in typed IPC helpers in `ui/src/ipc/`
- No `@tanstack/*` dependencies (hook-enforced)

### Testing

- Every non-trivial function gets a unit test
- Integration tests for cross-module flows in `tests/integration/`
- E2E tests via Playwright in `ui/tests/`, authored by qa-kitten
- Test files mirror source layout
- `rstest` for parameterized tests
- `proptest` for properties that should hold across inputs (esp. migration round-trips, FTS5 tokenization)
- Mock at trait boundaries via `mockall`
- Test fixtures in `tests/fixtures/` (esp. known audio in `tests/fixtures/audio/`)
- No flaky tests committed; fix or remove

### Documentation

- Every non-trivial architectural decision is recorded as an ADR
- ADR format: Michael Nygard (Context, Decision, Consequences)
- Module-level docs explain WHY, not just WHAT
- README, CONTRIBUTING, DEVELOPMENT, DEPENDENCIES kept current
- `docs/LESSONS.md` appended-to whenever an agent encounters a non-obvious issue
- Inline comments only where the code can't speak for itself

### Commit discipline

- Conventional commits (`feat:`, `fix:`, `chore:`, `docs:`, `refactor:`, `test:`)
- One logical change per commit
- Commit message body explains WHY
- Phase boundaries marked with explicit commits AND git tags (`phase-N-complete`)
- STATUS.md updated in every commit that ends an iteration

### Performance budgets (enforced via `criterion` benches)

- STT latency: < 1s for 10s of audio on RTX 2060 (Whisper large-v3-turbo q5)
- Cleanup latency: < 800 ms with local Ollama 3B model
- End-to-end (key-up → text-inserted): < 1.5s for typical 5s utterance
- App idle RAM: < 100 MB
- App startup to tray: < 2s

### Logging

- `tracing` configured in `logging.rs`
- Daily-rotating file appender via `tracing-appender` at `%APPDATA%/<name>/logs/`
- 7-day retention by default (configurable in Settings → Advanced)
- PII scrubbing: transcript text never logged at `INFO` or above (use `DEBUG` only, off by default)
- "Open logs folder" button in Settings → Advanced

---

## Appendix A — `.code_puppy/AGENTS.md` template

This is the literal content for `.code_puppy/AGENTS.md`. Auto-loaded by Code Puppy at the start of every iteration (per `code_puppy/agents/_builder.py:load_puppy_rules`). Combined with `~/.code_puppy/AGENTS.md`; project rules take precedence.

```markdown
# AGENTS.md — Mockingbird project rules

## Project context

You are working on `Mockingbird` (or whatever the final name is), a local-first
voice dictation app for Windows (with Mac support planned for Phase 9). It
replaces Wispr Flow with a fully local, privacy-respecting implementation.

The complete architecture, data model, and build plan are in PLAN.md.
**Read the PLAN.md spine every iteration** before starting work. The
"do not skip" list in PLAN.md Section 12 is binding and is also
mechanically enforced by `.code_puppy/settings.json` hooks.

For the current phase, read `docs/phases/phase{N}.md` (not the whole
PLAN — keep token budget reasonable).

## Workflow

You are running inside Code Puppy with the Wiggum `/goal` plugin.
Between iterations, your conversation context is cleared. The
persistent state is:

- This file (AGENTS.md)
- PLAN.md spine + `docs/phases/phase{N}.md`
- STATUS.md (the current phase status — you MUST update each iteration)
- The workspace files (code, tests, docs)
- Git history (tags `phase-{N}-complete` mark phase boundaries)
- docs/LESSONS.md (append-only notes from prior iterations)
- The hook engine config at .code_puppy/settings.json

### At the start of every iteration:

1. Read AGENTS.md (this file)
2. Read PLAN.md (spine)
3. Read `docs/phases/phase{N}.md` for the current phase
4. Read STATUS.md to see what's been completed
5. Read docs/LESSONS.md — search for anything tagged the current phase
6. Run `git log --oneline -20` and `git tag --list "phase-*"`
7. Run `git status` for uncommitted changes
8. THEN start work

### Delegation

You are the active agent for a /goal run (usually code-puppy, sometimes
a project JSON agent like migration-author / ui-author / etc.).
Delegate to specialists via invoke_agent:

- planning-agent (📋) — decompose the deliverables for a new phase
- qa-kitten (🐱) — UI / visual verification with Playwright
- helios (☀️) — build one-off tools you need
- agent-creator — mint new JSON agents (Phase 0 only, usually)
- Project JSON agents (migration-author, injection-author, ui-author,
  prompt-tuner, learning-loop-author) — scoped specialists; can also
  be the active agent for an entire phase if the work is narrow

Pack agents (pack-leader, bloodhound, shepherd, terrier, watchdog,
retriever) are DEPRECATED in Code Puppy. The framework reserves the
names but ships no implementations. Do not attempt to invoke them.

### At the end of every iteration (before exit):

1. Update STATUS.md — check off completed tasks, add progress notes,
   update cost line, blocked-on section, last-judge-run line
2. If a non-obvious thing happened, append to docs/LESSONS.md
3. Run `cargo fmt`, `cargo clippy -- -D warnings`, `cargo test`
4. Run `npm run lint`, `npm test` (whichever apply to your changes)
5. Commit all changes with descriptive commit messages
6. If phase is complete: `git tag phase-{N}-complete` after the final commit
7. The `stop` hook will refuse exit if any of the above failed —
   resolve before trying to exit again

## Coding standards (summary; full version in docs/QUALITY.md)

### Rust
- Edition 2021, Rust ≥ 1.77
- `cargo fmt` is law; `cargo clippy -- -D warnings` must pass
- `Result<T, E>` everywhere; `unwrap()` only in tests
- `thiserror::Error` for module error types
- `tracing` for logging (not `println!`)
- File size hard limit: 600 lines

### TypeScript / React
- Strict mode TS; no `any` without a SAFETY comment
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
- Append to docs/LESSONS.md on non-obvious findings

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
doesn't cover, STOP. Write an ADR draft proposing it before implementing.
The judges will catch missing ADRs anyway, so doing it inline saves an
iteration.

If PLAN.md and the code disagree, PLAN.md wins unless the disagreement is
documented in an ADR that supersedes the relevant PLAN section.

If you've iterated 10+ times on a single phase, STOP. The judge prompt
or the deliverable is misspecified. Surface the issue in STATUS.md's
"Blocked on / human input needed" section.

## Never do

- Modify a file in `models/` (gitignored downloads)
- Commit `.env`, `*.key`, `*.pfx`, or any file containing secrets
  (hook `block-secret-commit` enforces)
- Add a dependency without checking it works with the cross-platform
  abstraction
- Add any `@tanstack/*` package or any package flagged in the Mini
  Shai-Hulud IOC list (hook + see PLAN.md Appendix D)
- Run `npm install`, `npm ci`, or `pnpm/yarn install` without
  `--ignore-scripts` (hook `block-unsafe-npm` enforces).
- Skip writing tests "because it's a small change"
- Mutate raw transcript rows (hook enforces)
- Edit migrations 001/002/003 after `phase-1-complete` tag exists
  (hook enforces)
- Add telemetry, analytics, or crash-reporting that phones home
- Spend more than 10 iterations on a single goal — stop and ask the
  human to review the judge config or PLAN.md
- Inject into a password field without first checking the
  SecureInputGuard
- Paste without saving and restoring the previous clipboard
```

---

## Appendix B — Custom slash command templates

Files in `.agents/commands/` (or `.claude/commands/` — both directories are scanned by `customizable_commands` plugin in priority order: `~/.code-puppy/commands` → `.claude/commands` → `.github/prompts` → `.agents/commands`). Filename without `.md` becomes the command name.

### `phase0-goal.md`

```
Complete Phase 0 (Groundwork) per PLAN.md Section 10 and docs/phases/phase0.md.

Read AGENTS.md, PLAN.md (spine), and docs/phases/phase0.md carefully.
Confirm hook engine is loaded (SessionStart hook should have printed
phase context). The .code_puppy/AGENTS.md content is in PLAN.md
Appendix A — copy it verbatim. The hook config is in Appendix E. The
judges-template.json is in Appendix C. The project JSON agents are
in Appendix G. The custom command stubs are in Appendix B.

Delegate via invoke_agent:
- agent-creator → mint the project JSON agents in .code_puppy/agents/
- helios → only if a needed tool doesn't exist
- planning-agent → if STATUS.md task list is stale
- Otherwise do the file scaffolding yourself (you are code-puppy or a
  project JSON agent active for this phase) and run cargo + npm checks
  directly.

Initialize git. Generate Tauri updater keys via `tauri signer generate`.
Seed judges. (DBOS is deferred — skip; pack agents are deprecated — skip.)

Make the first commit "chore: initial scaffolding (Phase 0)" when
all deliverables are in place. Tag `phase-0-complete`. Update STATUS.md.

The judges enforce the actual exit criteria.
```

### `phase1-goal.md` through `phase8-goal.md`

Same template, substituting the phase number and pointing at `docs/phases/phase{N}.md`. Each command:

1. Tells the agent to read AGENTS.md + PLAN.md spine + `docs/phases/phase{N}.md` + STATUS.md.
2. Tells it to verify the hook engine is active.
3. Delegates: planning-agent if STATUS task list is stale; then specialist agents per the phase.
4. Lists the final-step checklist (fmt, clippy, test, build, ADRs, STATUS update, commit, tag).
5. Reminds it that judges + hook engine enforce exit criteria.

### Utility commands

`plan-phase.md`:
```
Invoke the planning-agent to decompose Phase {N} into ordered subtasks.

Read PLAN.md (spine) and docs/phases/phase{N}.md. The planning-agent
should:
1. Produce a checklist of subtasks matching the phase's deliverables.
2. Sequence them by dependency.
3. Identify which subtasks suit which specialist agent.
4. Write the result into STATUS.md under "Phase {N} tasks".
5. Commit STATUS.md with message "plan: Phase {N} task decomposition".
```

`qa-window.md`:
```
Invoke qa-kitten to run the Playwright spec for the current phase's
UI deliverables. Save screenshots into ui/tests/screenshots/. Compare
against baselines (if they exist) and report diffs.
```

`smoke.md`:
```
Run a full pipeline smoke test:
1. Spawn Mockingbird in dev mode (`npm run tauri dev` in background).
2. Use `cargo run --bin pipeline_test -- tests/fixtures/audio/hello.wav`
   to run the full STT → cleanup → (mock-inject) → DB pipeline.
3. Report per-layer latency.
4. Verify a sessions row was created with full provenance.
5. Tear down.
```

`run-tests.md`:
```
Run the full test suite and report results.

1. `cargo fmt --check` — report any formatting issues
2. `cargo clippy -- -D warnings` — report any lint issues
3. `cargo test` — report pass/fail counts and any failures
4. `cargo bench --no-run` — verify benches compile
5. `npm run lint` — report ESLint issues
6. `npm test` — report pass/fail counts
7. `npx playwright test` (if any specs exist) — report visual diffs

Summarize at the end: pass everywhere, or specific failures to address.
```

`verify-env.md`:
```
Run `scripts/verify-environment.ps1` and report the output verbatim,
then summarize whether the environment is ready for development. If
any check fails, suggest the fix from `docs/TROUBLESHOOTING.md`.
```

`lint.md`:
```
Run all linters and auto-fix anything that's auto-fixable:

1. `cargo fmt`
2. `cargo clippy --fix --allow-dirty -- -D warnings` (where safe)
3. `npm run lint -- --fix`

Then commit the fixes with message "chore: lint pass".
```

`seed-judges.md`:
```
Read `.code_puppy/judges-template.json` and merge its judges into
`~/.code_puppy/judges.json`. Existing judges with matching names are
NOT overwritten — only new ones are added.

After seeding, list the judges that are now configured and which are
enabled vs disabled.
```

---

## Appendix C — Judges template (`.code_puppy/judges-template.json`)

Recommended judge roster. Seeded into `~/.code_puppy/judges.json` via `scripts/seed-judges.ps1`. Each phase's "Judges" line in `docs/phases/phase{N}.md` enables a subset.

**Model tiering:** deterministic checks use a small/cheap model since they're really running shell commands. Subjective checks (modes-differ, no-focus-theft, design-tokens-applied) use a stronger model. Replace `haiku`/`sonnet` placeholders with the exact model strings your Code Puppy config knows.

```json
{
  "judges": [
    {
      "name": "build-passes",
      "model": "haiku",
      "enabled": false,
      "prompt": "Verify the project builds cleanly. Run `cargo build` and `npm run build`. Pass only if both exit zero with no errors (warnings acceptable but should be noted). If either fails, mark incomplete with the specific error messages and remediation instructions."
    },
    {
      "name": "tests-pass",
      "model": "haiku",
      "enabled": false,
      "prompt": "Verify all tests pass. Run `cargo test` and `npm test`. Pass only if every test passes with zero failures and zero ignored tests (unless ignored is documented). If any test fails, mark incomplete with the failing test name(s) and a hint at what's wrong. The `stop` hook should already catch this — if you're seeing failures, the hook is misconfigured."
    },
    {
      "name": "lint-clean",
      "model": "haiku",
      "enabled": false,
      "prompt": "Verify the codebase passes all linters. Run `cargo fmt --check`, `cargo clippy -- -D warnings`, and `npm run lint`. Pass only if all three exit zero. If any fail, list the specific files and lints that need attention."
    },
    {
      "name": "migrations-applied",
      "model": "haiku",
      "enabled": false,
      "prompt": "Verify the DB schema is correctly applied. Delete any existing dev DB, run the migration suite, and check: schema_version meta value matches latest migration (3 at Phase 1); all expected tables exist (sessions, transcripts, dictionary, modes, prompts, style_examples, example_sets, dictionary_snapshots, corrections, settings, learning_runs); audit history tables exist for the audited tables (modes, prompts, dictionary, style_examples); triggers exist for INSERT/UPDATE/DELETE on all four audited tables; FTS5 virtual table `transcripts_fts` exists with insert/delete triggers; modes table has three rows (normal, verbose, fragment) with matching prompts rows. If anything is missing, mark incomplete with the specific gap."
    },
    {
      "name": "fts5-smoke",
      "model": "haiku",
      "enabled": false,
      "prompt": "Insert a known string into transcripts, query transcripts_fts for it, expect a hit. Pass only if MATCH returns the expected row id. Also verify delete trigger: delete the transcript row, query again, expect no hit."
    },
    {
      "name": "hook-config-valid",
      "model": "haiku",
      "enabled": false,
      "prompt": "Verify .code_puppy/settings.json exists and matches PLAN.md Appendix E. Specifically check that at minimum these hook names are present: block-raw-transcript-edit, block-unsafe-npm, block-tanstack, block-secret-commit, session-start-briefing, stop-quality-gate. Mark incomplete if any are missing or the JSON is malformed."
    },
    {
      "name": "judges-seeded",
      "model": "haiku",
      "enabled": false,
      "prompt": "Verify ~/.code_puppy/judges.json exists and includes all judges from .code_puppy/judges-template.json. Run `cat ~/.code_puppy/judges.json | jq '.judges | length'` and compare to the template. Mark incomplete if any are missing."
    },
    {
      "name": "adr-recorded",
      "model": "sonnet",
      "enabled": false,
      "prompt": "Inspect the diff since the last 'Phase N complete' tag. For every non-trivial architectural decision visible in the diff (choice of library, design pattern, schema decision, naming convention that will spread), verify a matching ADR exists in docs/adr/. ADRs must have all sections: Context, Decision, Consequences. If a non-trivial decision is undocumented, mark incomplete and name the missing ADR. Trivial changes (renaming a variable, fixing a typo, adding tests) do not require ADRs."
    },
    {
      "name": "status-updated",
      "model": "haiku",
      "enabled": false,
      "prompt": "Verify STATUS.md reflects the current state. Check: current phase correctly named; phase outline has [x] for completed phases (with tag references) and [ ] for incomplete; current phase's task list has [x] for tasks visibly completed in the workspace; 'Last updated' timestamp is recent; 'Cost & token usage this phase' section exists and has entries; 'Last successful judge run' line is present; 'Blocked / human input needed' section exists (may be empty). If STATUS.md is stale or contradicts the workspace, mark incomplete with specifics."
    },
    {
      "name": "plan-aligned",
      "model": "sonnet",
      "enabled": false,
      "prompt": "Verify the project layout matches PLAN.md Section 4 for the files this phase touches. Check that files are in their specified directories, named per spec, and that no unauthorized files exist outside the spec without justification. If a file is misplaced, named incorrectly, or absent, mark incomplete with the specific divergence."
    },
    {
      "name": "phase0-structure",
      "model": "sonnet",
      "enabled": false,
      "prompt": "Verify Phase 0 groundwork is complete. Check: every directory in PLAN.md Section 4 exists (even if empty); .code_puppy/AGENTS.md matches Appendix A content; .code_puppy/settings.json matches Appendix E (hook config); .code_puppy/judges-template.json exists; .code_puppy/agents/ contains the five project JSON agents per Appendix G; .code_puppy/skills/ contains the six project skills per Section 11.3; .agents/commands/ has phase0-goal.md through phase8-goal.md plus utility commands; STATUS.md exists with the phase outline; README.md, CONTRIBUTING.md, LICENSE, .gitignore, .tool-versions, rust-toolchain.toml, .npmrc, lefthook.yml all exist; docs/adr/_template.md and the Phase 0 ADRs exist; scripts/setup-dev-windows.ps1, verify-environment.ps1, download-models.ps1, seed-judges.ps1, generate-tauri-keys.ps1 all exist; tauri updater public key is in tauri.conf.json; tag phase-0-complete exists. List any missing piece."
    },
    {
      "name": "agents-md-present",
      "model": "haiku",
      "enabled": false,
      "prompt": "Verify .code_puppy/AGENTS.md exists and matches Appendix A. Use `diff` against the canonical content if available. Mark incomplete if missing or substantively different."
    },
    {
      "name": "setup-script-runs",
      "model": "haiku",
      "enabled": false,
      "prompt": "Run `scripts/verify-environment.ps1` and check that it completes with a 'ready' status. Mark incomplete if any required environment item is flagged missing."
    },
    {
      "name": "stt-correct",
      "model": "sonnet",
      "enabled": false,
      "prompt": "Verify the Whisper STT pipeline produces correct output. Run the CLI test harness on the fixture audio in tests/fixtures/audio/. Compare to the expected transcript. Mark incomplete if the output doesn't match within reasonable tolerance, or if CUDA acceleration isn't visible in the logs (look for 'CUDA' or 'cuBLAS' mentions during model load)."
    },
    {
      "name": "cuda-verified",
      "model": "haiku",
      "enabled": false,
      "prompt": "Run `cargo run --bin stt_test -- tests/fixtures/audio/hello.wav 2>&1 | grep -i 'cuda\\|cublas'`. Pass only if at least one CUDA/cuBLAS line appears. Mark incomplete if STT ran on CPU."
    },
    {
      "name": "perf-stt",
      "model": "haiku",
      "enabled": false,
      "prompt": "Run `cargo bench --bench stt_latency`. Pass only if the mean latency is < 1.0s for a 10s audio fixture on the configured GPU. Mark incomplete with the actual latency if it regressed."
    },
    {
      "name": "modes-differ",
      "model": "sonnet",
      "enabled": false,
      "prompt": "Verify the three cleanup modes produce distinguishably different output on the canned fixtures in tests/fixtures/transcripts/. For each fixture, the test must produce three outputs. Pass only if: Normal is cleaned but ≈ same length as input; Verbose preserves all technical content (length similar to or greater than Normal); Fragment is < 50% the Normal length and may include bullets. Use the structural assertions in tests/integration/cleanup_modes.rs. If any two modes produce indistinguishable output by these metrics, mark incomplete with specifics."
    },
    {
      "name": "provider-swappable",
      "model": "haiku",
      "enabled": false,
      "prompt": "Verify the provider abstraction works. Inspect cleanup/provider.rs for the trait definition. Inspect cleanup/ollama.rs and cleanup/claude.rs for trait implementations. Run a unit test that swaps the provider at runtime via the trait object (mocked is fine for Claude). Mark incomplete if either provider hardcodes its model name outside its own module, or if the swap doesn't actually work in test."
    },
    {
      "name": "token-budget-respected",
      "model": "haiku",
      "enabled": false,
      "prompt": "Run the token budget unit tests in tests/integration/token_budget.rs. Verify that with worst-case input (long dictionary + 5 long examples + long transcript), the assembled prompt is <= 7000 tokens, and that PromptOverBudget falls back to a raw-only prompt. Mark incomplete if any case exceeds budget without triggering fallback."
    },
    {
      "name": "perf-cleanup",
      "model": "haiku",
      "enabled": false,
      "prompt": "Run `cargo bench --bench cleanup_latency` against a running local Ollama on Qwen 3B. Pass only if mean latency < 800ms. Mark incomplete with the actual latency."
    },
    {
      "name": "secret-storage",
      "model": "sonnet",
      "enabled": false,
      "prompt": "Verify the Claude API key is stored encrypted, not plaintext. Inspect secrets/windows.rs for DPAPI usage. Set a test key via the Tauri command, then `Get-Content $env:APPDATA\\Mockingbird\\Mockingbird.db` (or sqlite dump) and grep for the test key string. The plaintext key MUST NOT appear in the settings DB. Mark incomplete if plaintext storage is found anywhere."
    },
    {
      "name": "e2e-injection",
      "model": "sonnet",
      "enabled": false,
      "prompt": "Verify end-to-end injection works in the standard target apps. This is a manual checklist judge — read the implementor's notes in STATUS.md or the latest commit message for an injection-testing report. The report must show successful injection in at least: Notepad, VS Code, terminal (cmd or PowerShell), and one browser-based field (Gmail or Slack web). Mark incomplete if the report is missing or shows failures without explanation."
    },
    {
      "name": "clipboard-restored",
      "model": "haiku",
      "enabled": false,
      "prompt": "Run the clipboard round-trip test in tests/integration/clipboard.rs: set a sentinel string in clipboard, run an injection, verify the sentinel is restored after injection completes. Pass only if sentinel survives."
    },
    {
      "name": "secure-input-respected",
      "model": "haiku",
      "enabled": false,
      "prompt": "Verify SecureInputGuard fires. Inspect injection/secure_guard.rs and verify the Windows impl checks for password fields. Run a test that simulates a secure-input context and verifies the Injector returns Err(SecureInputBlocked). Mark incomplete if injection proceeds into a simulated password field."
    },
    {
      "name": "db-provenance",
      "model": "haiku",
      "enabled": false,
      "prompt": "Verify every session row has complete provenance. Query the sessions table for the most recent 10 rows. Each must have non-null prompt_id, and (where applicable to the phase) dictionary_snapshot_id, example_set_id. Mark incomplete if any session is missing provenance — that's a critical data integrity bug."
    },
    {
      "name": "recording-window-renders",
      "model": "sonnet",
      "enabled": false,
      "prompt": "Verify the floating recording window renders correctly. Invoke qa-kitten to run ui/tests/recording-window.spec.ts. It must capture screenshots of the window in idle/recording/processing/error states and compare against ui/tests/screenshots/*-baseline.png with a tolerance. Mark incomplete if any state's diff exceeds tolerance or if any component (waveform canvas, status dot, mode badge, duration counter, Esc hint) is missing."
    },
    {
      "name": "no-focus-theft",
      "model": "sonnet",
      "enabled": false,
      "prompt": "Verify the recording window doesn't steal focus. Inspect tauri.conf.json for the recording window: must have skipTaskbar: true, focus: false. Inspect src-tauri/src/windows/recording.rs for WS_EX_NOACTIVATE and SetWindowPos(SWP_NOACTIVATE) calls. Run the test in tests/integration/focus_test.rs that opens the recording window while a Notepad-like text field is focused and verifies the text field keeps focus. Mark incomplete if focus-theft prevention is missing."
    },
    {
      "name": "design-tokens-applied",
      "model": "sonnet",
      "enabled": false,
      "prompt": "Verify the UI uses design tokens from ui/src/design/tokens.css, not hardcoded values. Run `grep -rn 'rgb\\|#[0-9a-fA-F]\\{3,6\\}' ui/src --include='*.tsx' --include='*.css'` excluding tokens.css. Pass only if results are empty or limited to genuinely dynamic computed colors (e.g., waveform). Mark incomplete with the offending lines."
    },
    {
      "name": "accessibility-basic",
      "model": "sonnet",
      "enabled": false,
      "prompt": "Run the qa-kitten accessibility audit (uses axe-core via Playwright) against each window. Pass only if zero serious/critical violations. Mark incomplete with the violations list."
    },
    {
      "name": "provenance-visible",
      "model": "sonnet",
      "enabled": false,
      "prompt": "Verify the history UI surfaces full provenance. Inspect ui/src/windows/History.tsx for the three-pane layout, the side-by-side raw/cleaned/final view, and the metadata strip (mode, model, latency, prompt version, dictionary version). Run qa-kitten on ui/tests/history.spec.ts which clicks into 3 different sessions and asserts all three transcript stages are visible and metadata is accurate. Mark incomplete if any session shows partial provenance."
    },
    {
      "name": "search-works",
      "model": "haiku",
      "enabled": false,
      "prompt": "Verify SQLite FTS5 search across transcripts. Inspect db/migrations/001 for the transcripts_fts virtual table and triggers. Run search queries against test data: full-word match, partial match, multi-word query, and Porter-stemmed query. Mark incomplete if any returns wrong or no results."
    },
    {
      "name": "example-loop-closed",
      "model": "sonnet",
      "enabled": false,
      "prompt": "Verify the style-example feedback loop works. Test: insert a session via test harness, mark it as a style example via the Tauri command, run another dictation in the same mode, inspect the prompt sent to the cleanup LLM via the log — the marked example must appear in the few-shot block. Mark incomplete if it doesn't."
    },
    {
      "name": "first-run-flow",
      "model": "sonnet",
      "enabled": false,
      "prompt": "Verify the first-run setup wizard works end-to-end on a clean profile. Delete %APPDATA%/Mockingbird if present. Launch the app. Walk through (qa-kitten can drive this): privacy summary, mic check, hotkey check + conflict probe, model downloads, Ollama check, model pulls, optional Claude key entry with API validation, test dictation, done with autostart toggle. The implementor should have a flow-test report in the latest commit message confirming each step works. Mark incomplete if any step is broken or missing."
    },
    {
      "name": "export-import-roundtrip",
      "model": "sonnet",
      "enabled": false,
      "prompt": "Verify export/import round-trips data correctly. Test: create test data (3 sessions, 5 dictionary terms, 2 style examples), export to .Mockingbird archive, wipe the DB, import the archive with 'Replace everything' strategy, verify the data matches the original exactly (same sessions, same transcripts, same dictionary, same examples, same metadata). Then re-test with 'archive-wins' and 'local-wins' merge strategies on overlapping data. Mark incomplete on any data loss or mismatch."
    },
    {
      "name": "error-handling",
      "model": "sonnet",
      "enabled": false,
      "prompt": "Verify graceful error handling for: (1) Ollama service stopped — attempting dictation should produce a clear tray notification with a 'restart Ollama' suggestion and the app should try one auto-restart of the service before erroring; (2) Claude API returning 401 — should produce a clear notification with a 'check API key in settings' suggestion; (3) Claude API returning 429 — should retry with backoff up to 3 times; (4) Model file missing — should surface a download prompt. Mark incomplete if any case crashes the app or shows a useless error."
    },
    {
      "name": "installer-signed",
      "model": "haiku",
      "enabled": false,
      "prompt": "Run `npm run tauri build` and inspect the resulting .msi/.exe with `signtool verify /pa /v <path>`. Pass only if the signature is valid and the publisher matches the configured CN. Mark incomplete with the signtool output if it fails."
    },
    {
      "name": "correction-detection",
      "model": "sonnet",
      "enabled": false,
      "prompt": "Verify user-correction detection works. Test: dictate a session via test harness, edit the inserted text within 60 seconds via simulated clipboard activity, verify a corrections row appears with the correct before_text and after_text. Mark incomplete if detection is missing or captures wrong data."
    },
    {
      "name": "learning-eval",
      "model": "sonnet",
      "enabled": false,
      "prompt": "Verify the nightly learning job works on synthetic data. Test: seed the DB with 50 sessions and 10 corrections of known types (new_vocab + style_drift), run `cargo run --bin learn`, verify expected outcomes: new vocabulary terms added to dictionary; high-quality pairs promoted to style_examples; pruning kept the per-mode count under 50; an eval row written to learning_runs with non-null before/after correction rates. Mark incomplete if any step is wrong."
    },
    {
      "name": "rollback-works",
      "model": "sonnet",
      "enabled": false,
      "prompt": "Verify the learning loop rolls back on a regression. Test: craft a synthetic dataset where the learning change increases correction rate; run the job; verify rollback fires (the new examples are removed via _history_style_examples replay, the dictionary is reverted via _history_dictionary replay, a learning_runs row with rolled_back=1 is written). Mark incomplete if rollback doesn't fire on regression."
    }
  ]
}
```

**How to use this template:**

1. `scripts/seed-judges.ps1` merges these into `~/.code_puppy/judges.json` (existing judges with matching names aren't overwritten — only new ones added).
2. Before each phase, enable that phase's judges and disable previous phases' phase-specific judges via `/judges` TUI.
3. Standard judges (`build-passes`, `tests-pass`, `lint-clean`, `adr-recorded`, `plan-aligned`, `status-updated`) stay enabled across most phases.

---

## Appendix D — Supply chain hygiene

Specifically motivated by the **TanStack / Mini Shai-Hulud incident** (May 11, 2026, CVE-2026-45321, CVSS 9.6), which compromised 84 versions of 42 `@tanstack/*` npm packages plus 160+ others including `@mistralai/*`, `@uipath/*`, `@squawk/*`, `@draftlab/*` (also reached 2 PyPI packages). The malicious packages carried **valid SLSA provenance certificates** signed by the legitimate publishing pipelines — signature checking alone would not have caught it. The malware self-propagates via OIDC token theft and triggers a destructive payload (`rm -rf ~/`) when it detects credential rotation.

This appendix is binding (referenced from Section 12 item 16 and from the AGENTS.md "Never do" list). The hook engine mechanically enforces the primary defenses.

### The actual attack vector: lifecycle scripts

Shai-Hulud and its variants don't compromise you when a package is downloaded — they compromise you when the package's `preinstall`, `install`, or `postinstall` script runs. **`npm ci` runs lifecycle scripts by default. So does `pnpm install`. So does `yarn install`.**

**The minimum defense is `--ignore-scripts`.** This project enforces it three ways:

1. Repo-wide `.npmrc` setting `ignore-scripts=true`
2. All install commands in scripts/docs use the flag explicitly: `npm ci --ignore-scripts`
3. Hook `block-unsafe-npm` vetoes any `agent_run_shell_command` matching `npm (install|ci|i)` without the flag

The `.npmrc`:

```ini
# .npmrc
ignore-scripts=true
audit-level=moderate
fund=false
save-exact=true
```

### Packages this project avoids

**Do not add any `@tanstack/*` package to dependencies, even after the affected versions are deprecated.** Hook `block-tanstack` vetoes additions at the `package.json` edit level. Use `react-window` for virtualization, hand-rolled solutions for form/table libs.

Other compromised namespaces — `@mistralai/*`, `@uipath/*`, `@squawk/*`, `@draftlab/*` — review carefully before adding any version. Most have no plausible reason to appear in this project.

### npm hygiene rules (ordered by importance)

1. **`.npmrc` sets `ignore-scripts=true`.** Repo-wide default, committed at root.
2. **Installs use `npm ci --ignore-scripts` explicitly.** Hook-enforced.
3. **Script allowlist process.** If a build legitimately requires a package's install script, add an entry to `docs/SCRIPT_ALLOWLIST.md` (package, version, what the script does, why it's needed, who reviewed it, when). Then `npm rebuild <package> --foreground-scripts` for that one package after the main `npm ci --ignore-scripts`. Never run a broad install with scripts globally enabled.
4. **Pin exact versions** in `package.json`. `save-exact=true` makes any future `npm install <pkg>` write `"1.2.3"` rather than `"^1.2.3"`.
5. **Commit `package-lock.json`.** Treat lockfile diffs in PRs as security-sensitive review items.
6. **Audit before adding any new package.** Check publish timestamp, recent issues, maintainer list. If updated in the last 72 hours and the change is non-trivial, wait.
7. **Run `npm audit signatures` and `npm audit` after install.** Backstop only.
8. **No `bun` on PATH during installs.** The TanStack malware specifically invoked `bun run tanstack_runner.js`.
9. **Watch `.claude/` and `.vscode/` directories.** Both used as persistence locations. Periodically inspect `~/.claude/` and `~/.vscode/extensions/` for unexpected files (`router_runtime.js`, `setup.mjs`).

### GitHub Actions rules (when CI is added)

1. **Pin actions to commit SHAs**, not floating refs.
2. **Never use `pull_request_target` for untrusted code execution.** Pwn-request pattern was the TanStack entry vector.
3. **Don't share Actions caches across trust boundaries.** Scope caches by `github.ref` or `github.workflow_sha`.
4. **OIDC tokens scoped tightly.**

### If you suspect compromise

1. **Do NOT immediately revoke GitHub tokens.** Malware watches for revocation and runs `rm -rf ~/` on detection. **Take the machine offline FIRST**, then investigate.
2. Audit `~/.claude/`, `~/.vscode/`, `~/.npm/_cacache/` for unexpected files.
3. Block these domains: `git-tanstack.com`, `*.getsession.org`, `api.masscan.cloud`, `83.142.209.194`.
4. From a clean machine, rotate every reachable credential: npm tokens, GitHub PATs, AWS/GCP/Azure, K8s service accounts, SSH keys, Vault.
5. Inspect `package-lock.json` for affected versions; restore to known-good if possible.
6. Wipe the compromised host.

### References

- TanStack official postmortem: https://tanstack.com/blog/npm-supply-chain-compromise-postmortem
- StepSecurity: https://www.stepsecurity.io/blog/mini-shai-hulud-is-back
- Snyk: https://snyk.io/blog/tanstack-npm-packages-compromised/
- Wiz: https://www.wiz.io/blog/mini-shai-hulud-strikes-again-tanstack-more-npm-packages-compromised
- CVE-2026-45321 / GHSA-g7cv-rxg3-hmpx

This appendix is revisited whenever a new supply chain incident reaches comparable scale.

---

## Appendix E — Hook engine config (`.code_puppy/settings.json`)

This is the literal content for `.code_puppy/settings.json`. The Code Puppy hook engine reads this on session start. Hooks fire on the listed event types; exit code 1 vetoes; exit code 2 provides feedback without blocking. The plugins `destructive_command_guard`, `force_push_guard`, `shell_safety`, `file_permission_handler` should also be enabled in `~/.code_puppy/puppy.cfg`.

The actual hook commands live in `scripts/hooks/` and are simple Python files reading JSON on stdin.

```json
{
  "version": 1,
  "PreToolUse": [
    {
      "matcher": "edit_file|create_file|delete_file|replace_in_file",
      "hooks": [
        { "type": "command", "name": "block-raw-transcript-edit",
          "command": "python3 scripts/hooks/block-raw-transcript-edit.py",
          "timeout": 5000 },
        { "type": "command", "name": "block-migration-edit-after-phase-1",
          "command": "python3 scripts/hooks/block-migration-edit-after-phase-1.py",
          "timeout": 5000 },
        { "type": "command", "name": "block-tanstack",
          "command": "python3 scripts/hooks/block-tanstack.py",
          "timeout": 5000 }
      ]
    },
    {
      "matcher": "agent_run_shell_command",
      "hooks": [
        { "type": "command", "name": "block-unsafe-npm",
          "command": "python3 scripts/hooks/block-unsafe-npm.py",
          "timeout": 5000 },
        { "type": "command", "name": "block-bare-paste",
          "command": "python3 scripts/hooks/warn-bare-clipboard-set.py",
          "timeout": 5000 }
      ]
    },
    {
      "matcher": "agent_run_shell_command",
      "hooks": [
        { "type": "command", "name": "block-secret-commit",
          "command": "python3 scripts/hooks/block-secret-commit.py",
          "timeout": 10000 }
      ]
    }
  ],
  "PostToolUse": [
    {
      "matcher": "agent_run_shell_command",
      "hooks": [
        { "type": "command", "name": "post-commit-status-check",
          "command": "python3 scripts/hooks/post-commit-status-check.py",
          "timeout": 5000 }
      ]
    }
  ],
  "SessionStart": [
    {
      "hooks": [
        { "type": "command", "name": "session-start-briefing",
          "command": "python3 scripts/hooks/session-start-briefing.py",
          "timeout": 8000 }
      ]
    }
  ],
  "Stop": [
    {
      "hooks": [
        { "type": "command", "name": "stop-quality-gate",
          "command": "python3 scripts/hooks/stop-quality-gate.py",
          "timeout": 120000 }
      ]
    }
  ]
}
```

### Hook script summaries (full scripts in `scripts/hooks/`)

**`block-raw-transcript-edit.py`** — reads tool input JSON on stdin. If the target path matches `**/transcripts.rs` AND the diff appears to UPDATE a `raw` stage row, exit 1 with message `[BLOCKED] Raw transcripts are immutable (Section 12.3). Write a new row instead.`

**`block-migration-edit-after-phase-1.py`** — if `git tag --list "phase-1-complete"` returns non-empty AND the target file matches `src-tauri/src/db/migrations/00[123]_*.sql`, exit 1: `[BLOCKED] Migrations 001/002/003 are frozen after Phase 1. Add a new migration instead.`

**`block-tanstack.py`** — if the target is `package.json` AND the new content contains `@tanstack/`, exit 1: `[BLOCKED] @tanstack/* is banned per Appendix D. Use react-window or hand-roll.`

**`block-unsafe-npm.py`** — if the command matches `^\s*(npm|pnpm|yarn)\s+(install|ci|i)\b` AND does NOT include `--ignore-scripts`, exit 1: `[BLOCKED] npm install/ci must use --ignore-scripts (Appendix D). Edit .npmrc or add the flag.`

**`warn-bare-clipboard-set.py`** — if the command writes to clipboard outside `paste.rs`, exit 2 (warn, don't block): `[WARN] Use paste.rs helpers — clipboard save/restore is mandatory.`

**`block-secret-commit.py`** — if the command starts with `git commit`, run `git diff --cached --no-color` and scan for: high-entropy strings, lines matching `(api[_-]?key|secret|password|token)\s*[:=]`, file additions matching `\.(env|key|pfx|pem)$`. Exit 1 with the offending line numbers.

**`post-commit-status-check.py`** — if the command was `git commit`, check whether `STATUS.md` was in the commit. If not, exit 2 (warn): `[WARN] STATUS.md was not updated in this commit — the next iteration may lose track of progress.`

**`session-start-briefing.py`** — prints to stderr a short briefing: current phase from STATUS.md, last 5 commits, last `phase-*-complete` tag, last successful judge run line, any "Blocked / human input needed" entries.

**`stop-quality-gate.py`** — runs in sequence: `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test --quiet`. If any fails, exit 1 with the failing tool's last 20 lines of output: `[BLOCKED] Quality gate failed. Run the failing tool, fix, then exit again.`

The Phase 0 deliverable list includes writing each of these scripts. Each is ~20–40 lines of Python.

---

## Appendix F — Design tokens (`ui/src/design/tokens.css`)

See Section 9.1 for the full token definitions. This appendix exists so the `design-tokens-applied` judge has a fixed reference.

The canonical token set lives in `ui/src/design/tokens.css` and the matching motion module in `ui/src/design/motion.ts`. Sound files in `ui/src/design/sounds/` (start.wav, done.wav, error.wav). The judge runs `grep -rn 'rgb\|#[0-9a-fA-F]{3,6}' ui/src --include='*.tsx' --include='*.css' | grep -v 'tokens.css'` and expects only allowlisted exceptions (waveform canvas computed colors).

`docs/DESIGN.md` mirrors Section 9.1 with screenshots of each component state once Phase 5 produces them. The `recording-window-renders` judge compares qa-kitten screenshots against `ui/tests/screenshots/*-baseline.png` with a per-pixel tolerance of 2% to avoid font-rendering flakes.

---

## Appendix G — Project JSON agents (`.code_puppy/agents/*.json`)

Five tool-restricted agents. Note: `json_agent.py` restricts which tools are *available*, but per-argument scoping (e.g. "only edit files matching glob X") is enforced by the **hook engine** at `pre_tool_use`, not by the JSON agent itself. So each JSON agent below has a paired hook check in the hook scripts.

### `migration-author.json`

```json
{
  "name": "migration-author",
  "display_name": "Migration Author 🧱",
  "description": "Writes and reviews SQLite migrations for the Mockingbird DB. Restricted to db/migrations/ files.",
  "system_prompt": "You author SQLite migrations for Mockingbird. Read PLAN.md Section 7 and skills/data-model. Write append-only migrations. Never edit a closed migration. Always include a schema_meta version bump. Always wrap multi-table changes in a transaction. Include rollback notes in a comment header.",
  "tools": ["list_files", "read_file", "grep", "create_file", "edit_file", "agent_run_shell_command", "list_or_search_skills", "activate_skill"]
}
```

Paired hook: `pre_tool_use` on `edit_file|create_file` checks that the path matches `src-tauri/src/db/migrations/*.sql` or `tests/integration/db_*.rs`. Other paths are denied with a message.

### `injection-author.json`

```json
{
  "name": "injection-author",
  "display_name": "Injection Author 💉",
  "description": "Writes the text injection layer and platform abstractions. Restricted to injection/, window_context/, hotkey/.",
  "system_prompt": "You author the text injection and hotkey layers for Mockingbird. Read PLAN.md Section 6, 9, skills/injection-recipes. Clipboard MUST be saved and restored around every paste. Secure-input fields MUST abort injection. Hold-detection uses a low-level keyboard hook on Windows, not the global-shortcut plugin. Cross-platform: Windows impl real; macOS/Linux are todo!() stubs.",
  "tools": ["list_files", "read_file", "grep", "create_file", "edit_file", "agent_run_shell_command", "list_or_search_skills", "activate_skill"]
}
```

Paired hook: scope to `src-tauri/src/{injection,window_context,hotkey}/**` and `tests/integration/{injection,clipboard,hotkey,secure_input}_*.rs`.

### `ui-author.json`

```json
{
  "name": "ui-author",
  "display_name": "UI Author 🎨",
  "description": "Writes the React UI windows and components. Uses design tokens, never hardcoded colors. Restricted to ui/src/ and ui/tests/.",
  "system_prompt": "You author the React UI for Mockingbird. Read PLAN.md Section 9, 9.1, skills/design-tokens. ALL colors via tokens.css custom properties — no hex/rgb literals in components. Use react-window for virtualization (never @tanstack). Tauri commands wrapped in typed helpers in ui/src/ipc/. Strict TS; no `any` without a SAFETY comment. Accessibility: ARIA labels, keyboard nav, focus rings.",
  "tools": ["list_files", "read_file", "grep", "create_file", "edit_file", "agent_run_shell_command", "list_or_search_skills", "activate_skill"]
}
```

Paired hook: scope to `ui/**`.

### `prompt-tuner.json`

```json
{
  "name": "prompt-tuner",
  "display_name": "Prompt Tuner ✍️",
  "description": "Tunes the three cleanup mode prompts. Restricted to cleanup/prompts/*.md and a few test fixtures.",
  "system_prompt": "You tune the cleanup mode prompts (normal, verbose, fragment). Read PLAN.md Section 6, 8, skills/prompts. Every prompt change MUST come with a new row in the prompts table (don't UPDATE existing rows — INSERT a new version). Update tests/fixtures/transcripts/ to reflect expected new output. The modes-differ judge will validate distinguishable output.",
  "tools": ["list_files", "read_file", "grep", "create_file", "edit_file", "agent_run_shell_command", "list_or_search_skills", "activate_skill"]
}
```

Paired hook: scope to `src-tauri/src/cleanup/prompts/*.md`, `tests/fixtures/transcripts/**`, and (for the migration insert) `src-tauri/src/db/migrations/0*_prompt_*.sql`.

### `learning-loop-author.json`

```json
{
  "name": "learning-loop-author",
  "display_name": "Learning Loop Author 🔁",
  "description": "Writes the Phase 8 nightly learning binary and its eval/rollback paths.",
  "system_prompt": "You write the Phase 8 learning loop. Read PLAN.md Section 7 (rollback API), Section 10 Phase 8. Never modify raw transcripts (hook will veto). The loop MUST: (1) compute before/after correction rate; (2) only commit changes if correction rate didn't go up; (3) roll back via _history_ replay on regression; (4) write a learning_runs row regardless. Run as a separate cargo bin via Task Scheduler — never block dictation.",
  "tools": ["list_files", "read_file", "grep", "create_file", "edit_file", "agent_run_shell_command", "list_or_search_skills", "activate_skill"]
}
```

Paired hook: scope to `src-tauri/src/bin/learn.rs`, `src-tauri/src/learning/**`, and `tests/integration/learning_*.rs`.

---

## Appendix H — Phase doc template (`docs/phases/_template.md`)

Each phase doc follows this template. Phase 0 produces all nine doc files (phase0.md through phase8.md) from this template.

```markdown
# Phase {N}: {Name}

**Goal:** {one-sentence outcome}

**Entry criteria:**
- Phase {N-1} complete (tag `phase-{N-1}-complete` exists)
- Judges enabled per the list below
- STATUS.md updated with Phase {N} task list

## Deliverables

### Code
- {list every file or module produced this phase}

### Tests
- {unit tests}
- {integration tests}
- {qa-kitten Playwright specs, if any}
- {criterion benches, if any}

### Docs
- {ADRs produced}
- {doc files added/updated}

### Other
- {scripts, fixtures, model downloads}

## Specialist routing

- **planning-agent** invoked at phase start for task decomposition
- **{agent name}** for {specific subtasks}
- ...
- **qa-kitten** for {visual judges, if applicable}

## Judges (enable via /judges TUI)

- {judge name 1}
- {judge name 2}
- ...

## Hook engine notes

- {any hook-specific behavior relevant to this phase}

## Exit criteria

- All judges in the list above return complete=True
- `stop` hook passes (fmt, clippy, test all green)
- All deliverables present
- ADRs for any non-trivial decisions
- STATUS.md marked phase complete
- Commit "Phase {N} complete"
- Tag `phase-{N}-complete`

## Lessons learned (append after phase ends)

- {entries appended by agents during the phase}
```

---

## Appendix I — Open follow-ups for the implementor agent on day 1

A short, prioritized list the Pack Leader should resolve in the first session, before kicking off Phase 0.

1. Confirm the final project name (placeholder: `Mockingbird`). Update PLAN.md, all directory references, and Tauri identifier. Commit.
2. Resolve every item in Section −1 (pre-flight decisions).
3. Verify pack agents are enabled and discoverable (`/agents` should show pack-leader, bloodhound, shepherd, terrier, watchdog, retriever, planning-agent, qa-kitten, helios, agent-creator).
4. Verify Code Puppy + Wiggum are functional with a smoke test: `/goal say hello and stop`.
5. Generate Tauri updater key pair; commit public key into `tauri.conf.json` once the Phase 0 scaffolding lands.
6. Confirm DBOS Postgres/SQLite is reachable; run `DBOS_SYSTEM_DATABASE_URL=... cargo run --bin dbos_smoke` (helios builds the smoke binary if it doesn't exist yet).
7. Write the eight hook scripts under `scripts/hooks/` (Appendix E) and verify each by triggering a denial in a dry run.
8. Author the six project skills (Section 11.3) in `.code_puppy/skills/`.
9. Mint the five project JSON agents (Appendix G) via agent-creator.
10. Seed judges. Verify `/judges` TUI shows all of them with the right tier model.

Once these are green, Phase 0 itself begins.

---

*End of PLAN-Mockingbird-v2.md.*





