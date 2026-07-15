# PR: `macos-port` → `main` — macOS v1 (dictation + meeting capture on Apple Silicon)

> **Base:** `main` ← **Compare:** `macos-port`
> **Merge-base:** `a9b4c41` · **Range:** `origin/main..macos-port`
> **Do NOT squash** — the per-commit bodies carry the "flag-for-main-merge"
> rationale the review below references.

---

## A. Overview

This PR lands the **macOS port** of Mockingbird: full **dictation** +
**meeting capture** on Apple Silicon, built **from source with Command-Line
Tools only (no full Xcode)**. It is the work of **Phases 0–5 + a polish tail**,
delivered on the `macos-port` branch and **live-validated on the bundled
`.app`** (not just dev builds).

The **Windows / 7B path is preserved byte-identical** wherever it could be: all
new platform behaviour sits behind existing traits + `#[cfg(target_os="macos")]`
gates and `isMac`-gated UI. A **small, deliberate set of shared-code changes**
does touch the Windows build surface — those are the strict-better bug fixes and
structural moves enumerated in **Section C**, which is where your Windows-side
review time should go.

**Scope:** ~45 feature commits (+2 PR-prep commits below), **166 files**,
**~+18,100 / −900 lines**. The bulk is additive macOS platform code and tests.

---

## B. SAFE — invisible to Windows (the bulk; skim only)

These do **not** change Windows runtime behaviour. Listed so you know what's in
the tree.

- **New `*::macos.rs` platform impls behind existing traits:**
  `secrets/macos.rs` (Keychain), `hotkey/macos/` (CGEventTap + keymap),
  `injection/macos.rs` (CGEvent Cmd+V), `injection/secure_guard.rs` (AX secure
  input), `window_context/macos.rs` (NSWorkspace), `permissions/macos.rs`,
  `meetings/sck_macos.rs` (ScreenCaptureKit system audio).
- **`#[cfg(target_os="macos")]`-gated code** throughout (dictation runtime spawn,
  capture/VAD/STT un-gating, RAM-aware model select, fidelity fallback).
- **`isMac`-gated UI** — "Coming soon" treatment for Activity / Knowledge Graph /
  Mobile Sync on Mac, Mac-specific copy (hotkey text, Keychain vs DPAPI,
  permissions onboarding). `isMac` is hydrated once via `host_os()` in the
  zustand store. Windows renders identically to before.
- **macOS-target-gated deps** (Windows/Linux never see them):
  `security-framework`, `core-graphics`, `core-foundation`, `arboard`
  (text-only, `image-data` off), `accessibility-sys`, the `objc2-*` family
  (`objc2`, `objc2-foundation`, `objc2-app-kit`, `objc2-core-media`,
  `objc2-core-audio-types`, `objc2-core-foundation`),
  `objc2-screen-capture-kit`, `dispatch2`, `block2`. Most `objc2-*` were already
  in the tree transitively via Tauri — we just opt into class features.
- **`tauri.macos.conf.json` bundle overlay** (mirrors the existing Windows CUDA
  overlay pattern) — bundles the Whisper GGUF + Silero VAD + `libonnxruntime`
  dylib into `Contents/Resources/models/`, yielding a self-contained ~598 MB
  `.app`. The shared `tauri.conf.json` + Windows MSI/NSIS output are untouched.
- **New additive migrations 027–030** + the override-layer tables:
  `027_prompt_normal_small.sql`, `028_normal_small_v2_and_prompt_label.sql`,
  `029_mode_model_overrides.sql`, `030_mode_prompt_overrides.sql`
  (`mode_model_overrides` / `mode_prompt_overrides`). Forward-only, additive;
  they run on Windows too but only add tables/prompt rows — no destructive DDL,
  no edits to sealed migrations 001–003.
- **Dev-only surface:** ~18 `mac_*` dev-bin smokes + `[[example]]` judge
  entries, `judges_macos_v1.rs` per-module, `scripts/dev/cargo-mac.sh`,
  `download-models.sh` / `download-onnxruntime.sh`, macOS-port wiggum hooks. None
  linked into the shipped `mockingbird` binary.

---

## C. CROSS-PLATFORM — TOUCHES THE WINDOWS BUILD SURFACE (review this closely)

Each item: **what changed · why it's safe · what to check on Windows.**

### C1. Meeting formatter idempotency — `mb-7k6` (`d08d3b4`, `meetings/formatter.rs`)
- **What:** `split_into_paragraphs` now preserves `"\n\n"` boundaries and
  `strip_to_fixpoint` re-runs the filler-strip over merged paragraphs, so
  formatting is idempotent and now also strips **cross-segment** fillers.
  Proptest un-ignored + 2 unit tests added.
- **Safe:** pure-Rust shared code, **zero cfg gates** (verified). Existing
  single-input tests are byte-identical; only previously-buggy multi-pass /
  cross-segment cases change (strictly better).
- **Check on Windows:** run the meetings formatter test suite; eyeball one real
  multi-segment meeting transcript for over-stripping of intentional repetition.

### C2. dBFS no-data sentinel → `Option`/null — `mb-x1d` (`05aae1c`, `meetings/levels.rs`, `lifecycle.rs`, UI)
- **What:** `compute_dbfs -> Option<f32>`; `LevelsState` uses an `i32::MIN`
  atomic sentinel; `build_tick_payload` takes `(Option<f32>, Option<f32>)` →
  emits JSON `null` for no-data; the overlay reads `number | null`.
- **Safe:** a real full-scale **0 dBFS** reading is no longer misread as
  "no data". Shared code, zero cfg gates.
- **Check on Windows:** start a meeting, confirm the mic/sys level bars animate
  and that a silent source shows a flat bar (null) rather than a spurious
  full-scale reading.

### C3. Meeting title mid-comma preservation — `mb-zd0` (`5b2c0c3`, `meetings/title.rs`)
- **What:** `trim_token` keeps interior `','`; `trim_token_core` handles
  substance; `finalize` edge-trims and strips wrapping quotes. Stale asserts
  fixed + regression tests.
- **Safe:** shared code, zero cfg gates. Titles like `Sprint review, part 2`
  keep the comma instead of truncating.
- **Check on Windows:** generate a few meeting titles containing commas/quotes.

### C4. `ErrorBoundary` at the UI router root — `mb-e3z` (`b0b4465`, `ui/src/main.tsx`)
- **What:** a top-level class `ErrorBoundary` wraps the router root; catches
  React **render** throws (documented: NOT module-load / event-handler throws)
  and shows a recover UI instead of a white screen.
- **Safe:** additive React 19 wrapper; no behaviour change on the happy path.
- **Check on Windows:** `npm run build` + smoke the app; confirm normal
  navigation is unchanged.

### C5. Ollama lazy self-heal on dictation — `mb-58i` (`2b7b428`, gated to prod cleaner in `c56280d`)
- **What:** at the dictation boundary, `maybe_upgrade_from_passthrough` cheaply
  re-pings Ollama **only when currently on passthrough**, so a cleaner that
  started before Ollama was up self-heals on the next dictation.
- **Safe:** no-op unless already degraded to passthrough; gated to the
  production cleaner (dev/test cleaners unaffected). Cross-platform by design.
- **Check on Windows:** dictate with Ollama down then bring it up mid-session —
  confirm cleanup upgrades without a restart, and no added latency on the
  normal (Ollama-up) path.

### C6. `<html>`-transparent overlay CSS fix — `mb-mac-v1.7` (`8a4baf8`, `ui/src/design/global.css`) + `macos-private-api`
- **What:** the transparent-overlay CSS overlay fix, **plus** enabling the tauri
  `macos-private-api` feature (root `Cargo.toml`) required by
  `app.macOSPrivateApi: true`. **This also fixes the Windows black-corner
  transparency** artifact.
- **Safe:** `macos-private-api` is a no-op on Windows/Linux (source-build path;
  App Store caveat N/A). CSS change is cosmetic.
- **Check on Windows:** confirm overlay corners render transparent (no black
  corners) and no visual regression on the command-center / meeting overlays.

### C7. Persistent Command-Center close-X (`ed9cb6a`, `command_center/CommandCenter.tsx`)
- **What:** the overlay close button now persists — **shows on Windows too**.
- **Safe:** additive UI affordance; no logic change to open/close wiring.
- **Check on Windows:** confirm the close-X appears and dismisses the command
  center as expected.

### C8. `src-tauri/Cargo.toml` — `[target.'cfg(windows)'.dependencies]` header relocation
- **What:** cross-platform crates that had been *inadvertently* scoped under
  `cfg(windows)` during the Windows-only era were moved into `[dependencies]`;
  the `cfg(windows)` block now holds **only** `windows = { workspace = true }`.
- **Safe:** structural no-op for Windows — that target sees `[dependencies]` +
  the `cfg(windows)` block as one union, so the crate set is unchanged. It's the
  macOS target that previously couldn't link them.
- **Check on Windows:** `cargo build --release` resolves identically; diff
  `Cargo.lock` for the Windows target for any unexpected version drift.

### C9. `src-tauri/src/lib.rs` — `resolve_app_data_dir()` cfg-gating + `models_dir()` discovery order
- **What:** `resolve_app_data_dir()` is now `pub(crate)` and cfg-gated
  (windows / macos / other). `models_dir()` discovery order adds macOS `.app`
  Resources + app-data drop-in paths **after** the existing env-var / exe-dir
  checks. The **Windows `LOCALAPPDATA`/`USERPROFILE` branch is preserved
  unchanged** (verified — still `#[cfg(target_os="windows")]`).
- **Safe:** Windows resolution order is byte-identical; macOS paths are appended
  under macos cfg only. Added loud error logging listing every path tried.
- **Check on Windows:** confirm the app still finds models in the usual
  `LOCALAPPDATA` location; the new log line is additive.

### C10. `audio/capture.rs` — `SampleProducer` alias un-gating
- **What:** the `SampleProducer` alias + ringbuf `Split`/`HeapRb` imports moved
  from `cfg(windows)` to `cfg(any(windows, macos))`.
- **Safe:** Windows still compiles the same alias; only broadens the cfg to also
  include macOS. No runtime change on Windows.
- **Check on Windows:** it compiles (it will) — no behaviour to test.

### C11. `MACOSX_DEPLOYMENT_TARGET` / `minimumSystemVersion` — macOS bundle floor
- **What:** pinned `MACOSX_DEPLOYMENT_TARGET` (11.0 for the whisper.cpp release
  build, `mb-d6i`) and `minimumSystemVersion` in `tauri.conf.json` (macOS bundle
  floor). These fix the release `.app` build on Apple Silicon.
- **Safe:** macOS-only build knobs; the Windows MSI/NSIS bundle path does not
  consume them.
- **Check on Windows:** confirm the Windows bundle version/target metadata is
  unaffected by the `tauri.conf.json` addition (it should be macOS-scoped).

### C12. `HotkeyListener` trait gains `Send + Sync` (`hotkey/mod.rs`)
- **What:** `pub trait HotkeyListener: Send` → `Send + Sync` (the listener is a
  `Box<dyn HotkeyListener>` field of the `Send + Sync` managed `DictationRuntime`
  state, ADR 0063).
- **Safe:** all implementors — Windows included — are already trivially `Sync`;
  this is a compile-time bound tightening, not a runtime change.
- **Check on Windows:** it compiles (the Windows `HotkeyListener` impl already
  satisfies `Sync`).

### C13. `uuid_v4_simple` monotonic-counter fix — `mb-gg8` (`meetings/lifecycle.rs`)
- **What:** added a process-local `AtomicU64` sequence mixed into the id hash so
  two calls in the same coarse clock tick still differ. (Code comment tags this
  `mb-mac-v1.5.1`; STATUS tracks it as `mb-gg8` — same fix.)
- **Safe:** cross-platform correctness improvement; on Apple Silicon `--release`
  the old code could return identical `as_nanos()` in a tight loop and produce
  duplicate meeting ids. Windows was unaffected in practice but gets the same
  robustness.
- **Check on Windows:** meeting-id generation tests pass (they will).

---

## D. ADRs (decision record — `docs/adr/`, gitignored; listed for reference)

- **0056** — macOS secrets storage via the Keychain (`security-framework`)
- **0057** — macOS global dictation hotkey via `CGEventTap`
- **0058** — macOS paste injection via `CGEvent` Cmd+V + portable clipboard save/restore
- **0059** — macOS secure-input detection via AX `AXSecureTextField` (+ `IsSecureEventInputEnabled`)
- **0060** — macOS window context via `NSWorkspace.frontmostApplication`
- **0061** — macOS first-launch permissions onboarding
- **0062** — macOS dictation-backend un-gating (capture / VAD / STT)
- **0063** — macOS `DictationRuntime` spawn + secure-guard factory
- **0064** — macOS RAM-aware cleanup-model selection
- **0065** — macOS small-model prompt hardening (tier-gated `normal_small`)
- **0066** — macOS Modes effective-model control + per-mode override layer
- **0067** — macOS user-editable per-mode LLM prompts (edit + revert)
- **0068** — macOS Phase 4b: ScreenCaptureKit system-audio capture

---

## E. Post-merge follow-ups (recommendations — NOT done in this PR)

1. **Add a macOS CI lane (#1 recommendation).** A GitHub Actions matrix
   (`windows-latest` + `macos-latest`) so the cross-platform surface in Section C
   can't silently regress. Today there's no automated macOS build gate.
2. **Retire the port-only wiggum hooks.** After merge there is no `macos-port`
   branch, so `block-push-to-main` and `block-windows-rs-edit-on-macport` are
   moot — remove them. **Keep** `block-stt-swap` and the quality/secret hooks.
3. **Post-v1 backlog beads** (deferred, honest "Coming soon" on Mac today):
   Activity (`mb-4u2`), Knowledge Graph (`mb-0cg`), Mobile Sync (`mb-uo9`),
   AI command modes on Mac, autostart wiring (`mb-29s`), 7B / PC v6 hardening
   (`mb-lxl`).
4. **Note:** the gitignored per-machine carry docs (`docs/macos-port/GOAL.md`
   etc. that are maintainer-local) stay local; they are not part of the merge.

---

## F. Validation done

- **Dictation + meeting capture live-validated on the bundled `.app`** (the real
  user experience — Right-Option dictation + Both-source meeting with mic +
  system audio both attributed).
- **Metal STT parity confirmed:** `mac-v1-parity-whisper-metal` judge PASSES,
  **WER 0.0000 / CER 0.0000** on `jfk.wav` (confirmed Metal backend, no silent
  CPU fallback).
- **Windows byte-identical verified structurally** throughout (no `*windows.rs`
  touched; platform code behind cfg gates / traits; the Section C changes are
  the deliberate, enumerated exceptions).
- **All gates green:** `cargo fmt`, `clippy --all-targets -D warnings` (with AND
  without `metal`), `cargo test`, `tsc --noEmit`, vitest (167), `npm run build`,
  and the KG Playwright specs (headless, `--workers=1`).

---

## G. How to open the PR (you do this on GitHub after Windows-side review)

```bash
# from the repo, branch already pushed to origin/macos-port:
gh pr create \
  --base main \
  --head macos-port \
  --title "macOS v1 — dictation + meeting capture on Apple Silicon" \
  --body-file docs/macos-port/PR-SUMMARY.md
```

Or via the web UI: **New pull request → base `main` ← compare `macos-port`**,
then paste this file's contents as the description.

**Do NOT merge until Windows-side validation of Section C passes.** Prefer a
**merge commit (no squash)** so the per-commit "flag-for-main-merge" bodies
survive.
