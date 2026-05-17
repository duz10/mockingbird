# Phase 4 — Cleanup LLM (the heart)

**Status:** ✅ COMPLETE (Wave 1 — all backend work in one autonomous pass)
**Started:** 2026-05-18
**Sealed:** 2026-05-18 (commit pending; tag `phase-4-complete`)

## Goal (PLAN §10)

> Three modes producing distinguishably different output for the same
> utterance; provider abstraction works for Ollama and Claude; token
> budgets enforced.

**Plus** (WisprFlow-parity bonus, local-only): three additional AI
command modes — `rewrite` / `expand` / `summarize` — disabled by
default, opt-in via Settings (Phase 5 UI).

## What landed

### New modules

| Module | Purpose | Lines |
|---|---|---:|
| `cleanup/provider.rs` | `CleanupProvider` trait + `StubCleanupProvider` for tests | 325 |
| `cleanup/token_budget.rs` | PLAN §8 budget table enforcement + word-boundary truncation | 295 |
| `cleanup/few_shot.rs` | Top-N example selection SQL + budget shrinker + renderer | 290 |
| `cleanup/prompt_builder.rs` | Stitches system + dict + few-shot + foreground + raw | 290 |
| `cleanup/ollama.rs` | Sync `ureq` client for `localhost:11434` | 245 |
| `cleanup/claude.rs` | Sync `ureq` client for `api.anthropic.com` + retry policy | 290 |
| `cleanup/llm_cleaner.rs` | `Cleaner`-trait impl wrapping a `CleanupProvider` | 290 |
| `secrets/mod.rs` | Cross-platform `SecretStore` trait | 90 |
| `secrets/stub.rs` | In-memory `NullSecretStore` for tests | 110 |
| `secrets/windows.rs` | DPAPI-backed store (`CryptProtectData` + `CryptUnprotectData`) | 305 |

All files under the 600-line cap.

### New deps

- `ureq` 2.10 with `tls` + `json` (sync HTTP — see ADR 0021).
- `windows` feature `Win32_Security_Cryptography` (DPAPI).

### New migration

- `005_ai_command_modes.sql` — seeds `rewrite` / `expand` / `summarize`
  prompts + modes rows (enabled=0). Brings schema_version → 5.
- `cleanup/prompts/{rewrite,expand,summarize}.md` — prompt bodies.

### ADRs

- **ADR 0021** — Cleanup provider trait is sync (deviates from PLAN
  §8's `async_trait`). Documents the orchestrator-architecture
  reasoning.

### Wiring

- `dictation/runtime.rs::make_default_cleaner` — looks up the active
  mode's `(model_id, temperature, max_tokens)`, health-checks the local
  Ollama, and either constructs an `LlmCleaner` or falls back to
  `PassthroughCleaner` with a `WARN` log line ("start Ollama + pull
  the model to enable LLM cleanup").

### Tests

- **45+ pure unit tests** across the new modules.
- **3 `#[ignore]`d live tests**: `ollama::live_health_check_succeeds`,
  `ollama::live_cleanup_returns_text`, `claude::live_validate_key_succeeds`
  (gated on env vars / local Ollama).
- **NEW orchestrator integration test**:
  `llm_cleanup_runs_in_orchestrator_and_injects_cleaned_text` —
  drives the full `DictationOrchestrator::run` loop with `LlmCleaner`
  backed by `StubCleanupProvider`. Asserts:
  1. Cleaned text *differs* from raw (LLM actually ran).
  2. `transcripts.cleaned.text` == stub's normal-mode transform.
  3. `transcripts.cleaned.model_used` == `"stub-normal"` (the actual
     model id the provider reported, not just the provider name).

## Test count

- 308 → **383 tests** (+75 from Phase 4).
- 0 → 3 `#[ignore]`d live tests (Ollama health, Ollama cleanup,
  Claude validate-key).
- 4/4 orchestrator integration tests green.

## Judges (Phase 4)

See `docs/judges/phase-4/`:

- `mb-cleanup-modes-differ` — three modes produce distinguishably
  different output for the same input (via the `StubCleanupProvider`
  fixture suite + the orchestrator integration test).
- `mb-cleanup-provider-swappable` — Ollama + Claude both implement the
  same trait; can be swapped without orchestrator changes.
- `mb-secrets-encrypted-at-rest` — DPAPI cipher file does not contain
  plaintext sentinel.
- `mb-token-budget-respected` — `BudgetPlan` math sums correctly +
  truncation lands at word boundaries.

## Carry-forward to Phase 5

- **Per-mode hotkey picker UI.** Migration 005 ships placeholder
  hotkeys (`Ctrl+Win+R/E/S`); the Settings → Modes UI (Phase 5) needs
  to expose these for user editing + conflict-probe via the existing
  `hotkey::probe` module.
- **Cleanup latency to recording window.** `OllamaProvider::cleanup_streaming`
  is a stub for now; Phase 5's recording window can wire it to a
  mid-flight progress indicator.
- **Settings UI for Claude key entry + validation.** `ClaudeProvider::validate_key`
  + `WinDpapiSecretStore::put(SecretKind::ClaudeApiKey, ...)` are
  ready; the Settings → Models tab needs to pair them.
- **Foreground app routed through the cleaner.** `LlmCleaner::run_cleanup`
  currently passes `foreground_app: None` to the prompt builder.
  Phase 5+ (or a 4.x patch) should plumb the current foreground app
  through the orchestrator's `complete()` call.

## What's NOT in Phase 4 (deferred per the brief)

- **Streaming cleanup display** in the recording window → Phase 5.
- **Per-mode provider/model dropdowns** → Phase 5 Settings UI.
- **Claude key entry UI** → Phase 5 Settings UI.
- **Real LLM eval framework** → Phase 8 learning loop.
- **Few-shot example marking from history** → Phase 6 history UI.

## Wave 1 cost line

- ~1,200 LoC new Rust (cleanup/* + secrets/*).
- 75 new tests; 0 deleted; 3 new `#[ignore]`d live tests.
- 1 new ADR (0021).
- 1 new migration (005).
- 6 new prompt files (rewrite/expand/summarize markdown + the existing
  normal/verbose/fragment unchanged).
- fmt + clippy `-D warnings` clean.
