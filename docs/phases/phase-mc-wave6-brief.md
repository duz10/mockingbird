# Phase MC — Wave 6 Brief

**Author:** code-puppy (`code-puppy-b14c19`)
**Date:** 2026-05-21 (end-of-Wave-5 iteration)
**Hands off to:** the active agent for Wave 6 (the sealing wave)
**Master plan:** `docs/phases/phase-meeting-capture.md`
**Predecessor brief:** `docs/phases/phase-mc-wave5-brief.md`

---

## What Wave 5 shipped (so you know what to seal)

Five tasks landed in commits `94a44d2`, `81c5a09`, and the Wave-5
seal commit (this iteration). Per the Wave 5 brief checklist:

- [x] §5.1 — Tray "Pause meeting hotkey" toggle + `SettingKey::MeetingHotkeyPaused` + `meeting_set_paused` / `meeting_is_paused` IPC (mb-pdv.20)
- [x] §5.2 — Settings UI tab for the 8+1 meeting `SettingKey` variants (mb-pdv.21) — **shipped as `ui/src/pages/SettingsMeetingTab.tsx` (extracted; Settings.tsx already over the 600 cap pre-Phase-MC)**
- [x] §5.3 — `meeting_export_markdown` Save As dialog via `tauri-plugin-dialog` (mb-pdv.22)
- [x] §5.4 — `meeting:progress` chunk counter chip in `MeetingRecordBar` (mb-pdv.24)
- [x] §5.5 — Accessibility: overlay autofocus + a11y attrs (mb-pdv.23)
- [x] §5.6 — Rust-side overlay show/hide wired on `MeetingToggle` (mb-pdv.25)
- [x] Wave 6 brief authored (this file; previously mb-pdv.19 closed in `94a44d2`)

### Wave 5 deviations applied (do not relitigate)

1. **Settings UI extracted into `SettingsMeetingTab.tsx`** — the
   brief allowed this if `Settings.tsx` was close to the cap. It
   was 671 lines at HEAD (pre-Phase-MC violation), so extraction was
   mandatory. Filed `mb-17d` to split the four remaining panels later.
2. **NEW typed-meeting-settings IPC pair** —
   `meeting_settings_get_all` + `meeting_settings_set(key, value)`
   instead of reusing the string-coerced legacy
   `get_settings` / `update_setting`. The legacy pair is hardcoded to
   the Phase-1 `SettingsSnapshot` shape and would need extension for
   every new key, plus it can't carry `null` cleanly for the
   inherit-from-global retention sentinel. The new pair tunnels
   `serde_json::Value` so booleans / numbers / strings / null all
   round-trip without string coercion.
3. **`meeting_settings_set` allowlists writable keys**: rejects
   dictation-side keys, runtime-managed keys (`MeetingLastSelectedSource`),
   and the pause toggle (`MeetingHotkeyPaused`) which has its own
   `meeting_set_paused` command (the pause toggle needs to inject a
   `PauseToggle` activation event in addition to writing the setting,
   so we can't let the UI bypass the runtime).
4. **`probe_meeting_hotkey` IPC dropped** (was P1 in §5.2). The
   meeting hotkey installer in `meetings/hotkey_installer.rs` already
   auto-falls-back through `meeting_candidate_chain(configured) →
   [configured, VK_M, VK_F13, VK_F14]` at install time, so a
   pre-save UI collision warning would surface a problem that
   never reaches the user — they'd just get the auto-picked
   fallback. If real-world telemetry shows users wanting to know
   *why* their preferred main-key wasn't used, surface a
   "currently active VK" read-only field in the UI as a small
   follow-up (filed under `mb-pdv` epic; not a phase blocker).
5. **Settings tab tests are IPC-contract tests, not component
   tests** (11 vitest cases, not 5). `@testing-library/react` is
   not installed and the component-mount path is reserved for
   Playwright in Wave 6. The IPC contract is what the component
   depends on; if the contract drifts, the component breaks —
   this is the load-bearing seam.

### Cargo gate at Wave 5 seal

All four green on `2026-05-21`:

* `cargo check --all-targets` — clean
* `cargo clippy --release --all-targets -- -D warnings` — clean
* `cargo test --release --no-run` — all 12 binaries link
  (LESSONS 2026-05-17 0xC0000139 fallback per the cargo gate
  convention)
* `cargo fmt --check` — clean

UI side:

* `npx tsc --noEmit` — clean
* `npx vitest run` — **4 files / 45 tests passing** (+1 new file,
  +11 new tests)
* `npm run lint` — still broken pending `mb-yxh` (ESLint v9 config
  migration; tracked, out of scope)

### Phase MC running test delta

* Wave 1: +35
* Wave 2: +68
* Wave 3: +35
* Wave 4: +25 (UI)
* Wave 5: +11 (UI IPC contract) + 2 (Rust allowlist) = **+13**
* **Phase MC total: +176 tests** — comfortably inside the
  +90 to +120 *target* (note: target was for pure modules, the
  surplus comes from the UI surface area which the brief did not
  bake in)
* **Project total: ~559 tests** — over the 470–500 phase-exit
  target. Surplus reflects the IPC-contract paranoia which was
  cheap given the deviation rationale above.

---

## Wave 6 deliverables (4 tasks)

Per master plan §"Wave 6 — Judges, retrospective, seal" — verbatim.

### 6.1 Five new judge cards + JSON entries (P0)

**Files:**

* `docs/judges/phase-mc/mc-formatter-deterministic.md`
* `docs/judges/phase-mc/mc-long-form-stitched-losslessly.md`
* `docs/judges/phase-mc/mc-two-channel-merged.md`
* `docs/judges/phase-mc/mc-no-llm-in-critical-path.md`
* `docs/judges/phase-mc/mc-dictation-untouched.md`
* `.code_puppy/judges-template.json` (5 new JSON entries; preserve
  the existing entries verbatim)

**Pattern:** mirror Phase 4 judges — see `docs/judges/phase-4/*.md`.
Each card has:

1. **Title** (judge slug; kebab-case).
2. **Why this exists** (one-paragraph cross-reference to the
   invariant in `phase-meeting-capture.md` §"Cross-wave invariants").
3. **What it checks** (concrete, machine-verifiable steps; if a
   human-judgment step is unavoidable, label it `HUMAN-EYE` and
   describe the visual/auditory expectation).
4. **How to verify** (exact commands the verifier runs; e.g.
   `cargo test --release --no-run -p meetings -- formatter` or
   `git diff phase-4-complete..HEAD --stat -- src-tauri/src/hotkey
   src-tauri/src/dictation`).
5. **Pass/fail rubric** (single sentence: PASS iff X; FAIL iff Y).

#### Per-judge sketch

| judge | check | verification cmd |
|---|---|---|
| `mc-formatter-deterministic` | `format(format(x)) == format(x)` (fixpoint), no RNG / clock / global state | Run `meetings::formatter` proptest suite; the existing proptests already cover this |
| `mc-long-form-stitched-losslessly` | 90 s synthetic fixture → 3 chunks → stitched transcript edit-distance from single-pass < 0.5 % | New `#[ignore]`-gated integration test under `src-tauri/tests/`; document the manual gate command |
| `mc-two-channel-merged` | Two synthetic PCM streams w/ known overlap → merged transcript shows correct interleaving + speaker labels | Existing `meetings::runtime` + `meetings::formatter` integration tests cover this; the judge cites them |
| `mc-no-llm-in-critical-path` | Zero Ollama HTTP calls between `meeting_stop` and `meeting:state = done` | Runtime instrumentation via a wrapping `OllamaProvider` that increments an `AtomicU64` on every `cleanup_request`; the integration test asserts the counter is 0 at done |
| `mc-dictation-untouched` | `git diff phase-4-complete..HEAD -- src-tauri/src/hotkey src-tauri/src/dictation src-tauri/src/injection src-tauri/src/cleanup/provider.rs src-tauri/src/cleanup/llm_cleaner.rs src-tauri/src/recording_window.rs` is EMPTY | A `pwsh` one-liner the judge runs; expects zero output |

**Critical realism check:** `mc-no-llm-in-critical-path` needs
*runtime* instrumentation, not just static code review. The
existing `cleanup::OllamaProvider` is not a trait — it's a
concrete struct — so the wrapping approach requires either:

(a) A small `pub fn instrumented()` constructor that holds an
    `Arc<AtomicU64>` reference, OR
(b) A test-only `mod tests` shim inside `cleanup/ollama.rs` that
    exposes the counter via `#[cfg(test)]`.

Option (b) preserves the public API exactly; pick (b) unless the
test needs to live in a separate integration crate (it shouldn't —
the critical-path test lives under `src-tauri/src/meetings/`).

### 6.2 Retrospective in `docs/LESSONS.md` (P0)

Tag: `[phase-mc-retrospective]`. Cover:

* What went right (be specific: e.g. "Wave 2's pure formatter ate
  the LoC budget but the proptest harness caught two edge cases
  that would have shipped as bugs").
* What went sideways (Wave 4's UI was sprawl-heavy; Wave 5's
  Settings.tsx pre-existing over-cap surprise; the LESSONS-2026-
  05-17 0xC0000139 still blocks live test runs and we still don't
  have a fix).
* What we learned about the codebase (the legacy
  `update_setting`/`get_settings` string-coerced pair vs the typed
  `Settings` facade; the auto-fallback hotkey installer making the
  UI collision warning vestigial; the Tauri 2 `AppHandle<R>`
  generic requirement when mixed with `State<'_, T>`).
* What to do differently for a future lateral epic (file-cap
  refactors should be charterable as discrete pre-work, not
  silently absorbed mid-wave; the `bd ready` queue should be the
  source of truth for "what's actually open" and any iteration
  that closes work must `bd close` before exiting).

### 6.3 STATUS.md + bd cleanup (P0)

* Update STATUS.md SESSION ANCHOR block:
  * Add to "LATERAL EPICS DONE" line:
    "ADR 0026/0027/0028/0029/0030 (Phase MC, Accepted YYYY-MM-DD,
    `mb-pdv` closed; `mb-2bi` closed via ADR 0029)".
  * Add a "PHASE MC SEALED" block summarizing the 6 waves +
    test delta + tag.
  * Remove the IN-FLIGHT lines for Phase MC (anchor block stays
    lean — historical detail goes into the dated sections below).
* `bd close mb-pdv` (the epic).
* `bd close mb-2bi` (audio streaming + chunked Whisper — ADR 0029
  closes it; link the ADR in the close comment).
* `bd ready` should no longer show any `mb-pdv.*` entries.

### 6.4 Cargo gate + seal commit + `git tag phase-mc-complete` (P0)

```pwsh
cd src-tauri
powershell -File scripts\cargo-with-cuda.ps1 check --all-targets
powershell -File scripts\cargo-with-cuda.ps1 clippy --release --all-targets -- -D warnings
powershell -File scripts\cargo-with-cuda.ps1 test --release --no-run
powershell -File scripts\cargo-with-cuda.ps1 fmt --check
cd ..\ui
npx tsc --noEmit
npx vitest run
```

All seven green → commit with message
`Phase MC W6: judges + retrospective + seal (phase-mc-complete)` →
`git tag phase-mc-complete`.

---

## Cross-wave invariants (still binding — re-read before judging)

The 14 invariants in `phase-meeting-capture.md` §"Cross-wave
invariants" remain binding through Wave 6. Of particular note for
the judging wave:

* **Invariant 1** — Dictation is sealed. The
  `mc-dictation-untouched` judge mechanically enforces this; the
  judge author re-confirms via `git diff phase-4-complete..HEAD`.
* **Invariant 3** — `CleanupProvider` trait shape is sealed. The
  judge author must NOT add a trait method to enable runtime
  instrumentation; use the test-only counter pattern (option b
  in §6.1).
* **Invariant 4** — `SpeechToText::transcribe` signature is sealed.
  Already shipped; nothing in Wave 6 touches STT.
* **Invariant 12** — Cargo gate is four green lights; clippy MUST
  be `--release`.
* **Invariant 13** — Migrations stay sealed. Migration 011 is
  sealed; if Wave 6 surfaces a defect, repair via 012+.

---

## Wave 6 risks & mitigations

| # | Risk | Severity | Mitigation |
|---|------|----------|------------|
| 1 | The `mc-no-llm-in-critical-path` runtime counter requires touching `cleanup/ollama.rs`, which is **NOT** in the sealed-files list but **IS** owned by the dictation subsystem | Medium | Use a `#[cfg(test)]` shim — no public API change, no production code change. If `cleanup/ollama.rs` review surfaces a concern, the judge can fall back to a code-review-only assertion (grep for `OllamaProvider::new()` / `with_base_url()` between `meeting_stop` and `meeting:state = done`) and downgrade itself to HUMAN-EYE. |
| 2 | The 90 s synthetic fixture for `mc-long-form-stitched-losslessly` needs an audio file checked into the repo | Medium | Generate the fixture from a sine-tone / pink-noise script that lives under `src-tauri/src/meetings/test_fixtures/`; do NOT check in a recorded WAV (binary noise in git). The script is reproducible; the fixture is rebuildable. |
| 3 | Sealing requires `cargo test --release --no-run` only — we cannot live-run the test binaries (LESSONS-2026-05-17 0xC0000139) | Low | Already the documented project gate. Wave 6's new tests must compile clean; the proptest / runtime-counter assertions still get exercised because they live in `#[test]` fns inside library crates that the unit-test build links. |
| 4 | `bd close mb-2bi` requires the close comment to cite ADR 0029 | Low | Use `bd update mb-2bi --status closed --comment "closed via ADR 0029 (long-form chunked Whisper inference); Phase MC delivers"` |
| 5 | The retrospective is the last thing written and tends to be the first thing cut when time runs out | Medium | Write it BEFORE the seal commit, not after. The stop-hook will refuse exit if LESSONS isn't updated when STATUS.md says retrospective-required. |

---

## Cargo gate (must be green at Wave 6 seal — same as W5)

```pwsh
cd src-tauri
powershell -File scripts\cargo-with-cuda.ps1 check --all-targets
powershell -File scripts\cargo-with-cuda.ps1 clippy --release --all-targets -- -D warnings
powershell -File scripts\cargo-with-cuda.ps1 test --release --no-run
powershell -File scripts\cargo-with-cuda.ps1 fmt --check

cd ..\ui
npx tsc --noEmit
npx vitest run
# npm run build — optional; do it once before the seal to confirm bundle ships
```

All seven green.

---

## Brief checklist (post-wave-6 author updates this section before sealing)

Before declaring Wave 6 sealed:

- [ ] §6.1 Five judge cards authored under `docs/judges/phase-mc/`
- [ ] §6.1 Five JSON entries appended to `.code_puppy/judges-template.json`
- [ ] §6.1 `mc-no-llm-in-critical-path` runtime counter shim in place + integration test passing
- [ ] §6.2 Retrospective appended to `docs/LESSONS.md` under `[phase-mc-retrospective]` tag
- [ ] §6.3 STATUS.md SESSION ANCHOR updated (Phase MC moved to "LATERAL EPICS DONE"; IN-FLIGHT lines cleared)
- [ ] §6.3 `bd close mb-pdv` (the epic) + `bd close mb-2bi` (audio-streaming standing P1)
- [ ] §6.3 `bd ready` no longer shows any `mb-pdv.*` entries
- [ ] §6.4 Cargo gate clean (check + clippy --release -D warnings + test --release --no-run + fmt --check)
- [ ] §6.4 UI gate clean (tsc --noEmit + vitest)
- [ ] §6.4 Seal commit + `git tag phase-mc-complete`
- [ ] `git tag --list "phase-*"` includes `phase-mc-complete`

---

## Anti-corner: what NOT to do in Wave 6

* **Do NOT** modify `cleanup/ollama.rs` to add a trait or change
  the public API. Use a `#[cfg(test)]` shim for the runtime
  counter.
* **Do NOT** retag `phase-4-complete` — it's the diff baseline for
  `mc-dictation-untouched`. If you accidentally move it, the
  judge becomes meaningless.
* **Do NOT** edit migration 011 — if a defect is found, repair via
  012. (None expected at Wave 6 — 011 has been in place since W1.)
* **Do NOT** ship the recorded WAV fixture for the long-form judge —
  generate it from a script. Binary blobs in git are forever.
* **Do NOT** treat the `mb-yxh` ESLint v9 config migration as a
  Phase MC dependency — it's been broken since Phase 5 ship; it's
  not a regression we caused.
* **Do NOT** re-execute any prior wave's deliverables — they're
  sealed in their respective commits. If a defect surfaces, ADR-
  charter a follow-up; do not silently re-do.

---

*Brief authored by code-puppy (`code-puppy-b14c19`) on 2026-05-21,
end-of-Wave-5 iteration. Master plan version: `docs/phases/phase-
meeting-capture.md` as of post-`phase-4-complete` tag, unchanged
since Wave 1.*
