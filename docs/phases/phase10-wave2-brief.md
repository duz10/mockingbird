# Phase 10 Wave 2 — UIA Deep Snapshots + Multi-Monitor: Brief

**Status:** code complete, awaiting Wave 3 dispatch.
**Authored:** 2026-05-25.
**Charter:** [`docs/phases/phase10.md`](phase10.md) § Wave 2.
**Bead:** `mb-hr1u`.

This brief captures the in-wave decisions the phase doc explicitly
delegated to Wave 2: UIA backend choice, snapshot payload size cap,
multi-monitor approach, and the app-quality matrix observed during
smoke testing.

---

## 1. UIA backend: raw `windows`-rs vs the `uiautomation` crate

**Decision: raw `windows`-rs (already a workspace dep at 0.56), with
the new feature flag `Win32_UI_Accessibility`. No new external
dependency. No new ADR.**

### Audit results

| Criterion | `windows`-rs (raw COM) | `uiautomation` crate |
|---|---|---|
| Already in the workspace? | ✅ Yes (Phase 3, Phase MC, Phase 10 W1B) | ❌ No |
| External-dep audit cost | ✅ Zero | ⚠ Non-trivial (transitive deps, Shai-Hulud-class IOC check, maintenance audit) |
| Surface area required | Small (CoInitializeEx + CUIAutomation + ElementFromHandle + ContentViewWalker + ~5 property gets) | Same, plus a higher-level wrapper API |
| Cross-platform abstraction (Principle 5) | Hidden behind our own `activity::uia::Probe` trait | Hidden behind our own `activity::uia::Probe` trait |
| Lock-in risk | None — `Probe` trait insulates us | Adds an extra abstraction layer above `Probe` |
| Code-cost compared to wrapper | ~150 LoC of unsafe COM in `windows_com.rs` | Probably ~60 LoC of safe-wrapper code |

The crate would save us ~90 LoC of unsafe wrapping but adds a maintenance
surface we don't need. The phase doc's "default lean: `uiautomation`
crate IF audit passes" is conditioned on `IF`; the IF here is a tie at
worst, and consistency with the existing `windows`-rs idioms breaks the
tie toward no-new-dep.

### Why not an ADR

Reusing an existing dependency is not an architectural decision; it's a
"use what we already have" call. An ADR here would be process-theater.
If we ever switch (e.g. Phase 9 macOS bridge wants the `uiautomation`
crate's macOS-shim companion), a successor ADR can land alongside the
swap.

### `windows` 0.56 feature flags added

```toml
"Win32_UI_Accessibility",      # IUIAutomation et al.
"Win32_Graphics_Gdi",          # MonitorFromWindow, GetMonitorInfoW
"Win32_UI_HiDpi",              # GetDpiForWindow
"Win32_System_SystemInformation", # GetTickCount (idle math)
"Win32_System_Com",            # CoInitializeEx, CoCreateInstance
"Win32_System_Variant",        # BSTR / VARIANT plumbing
"Win32_System_Ole",            # ditto
```

A gotcha discovered during the build: `MONITORINFOF_PRIMARY` lives at
`windows::Win32::UI::WindowsAndMessaging` in this crate version, not
under `Win32::Graphics::Gdi` where the Win32 SDK would put it. Noted
inline in `windows_com.rs`.

---

## 2. Snapshot payload size cap

**Decision: 32 KB per event (`MAX_PAYLOAD_BYTES = 32 * 1024`).**

### Truncation cascade (in `activity::uia::payload::to_payload_json`)

1. Each `visibleTextFragments[i]` is char-truncated to
   `MAX_FRAGMENT_CHARS = 512` chars with a trailing `…`.
2. Serialize. If under cap, ship.
3. If over: cap fragment count to `MAX_TEXT_FRAGMENTS = 256` (first-N,
   not random-N — first fragments are higher in the UIA tree-walk
   order and carry more signal).
4. If still over: drop tail fragments until under cap (or empty).
5. If still over (focused-field value is huge): truncate
   `focusedField.value` to 64 chars + `…`.
6. Final stand: drop `visibleTextFragments` entirely. App + title +
   monitor + control-summary + password-flag + status always survive
   because they're tiny and load-bearing.

### Empirical numbers from smoke testing

| App | Elements visited | Fragments | Payload bytes (raw) | Truncated? |
|---|---|---|---|---|
| Notepad (empty) | 9 | 4 | ~280 | No |
| Notepad (3 paragraphs) | 11 | 5 | ~420 | No |
| Win11 Settings → System → Display | 187 | 64 | ~5.8 KB | No |
| Chrome — github.com (issues list) | 500 (capped) | 256 (capped post-soft-cap) | ~28 KB (post-truncation) | Yes |
| Chrome — Twitter timeline | 500 (capped) | 220 | ~22 KB | Yes |
| VS Code (file open + editor) | 134 | 52 | ~4.1 KB | No |
| Steam (browse store) | 22 (limited tree) | 9 | ~0.8 KB | No |
| Steam (in-game fullscreen) | 0 (no UIA tree) | 0 | ~120 (status:no_payload) | No |

The cap is empirical: it's where the truncation cascade starts kicking in
for browsers without truncating below the "useful signal" threshold for
the Stage-3 abstractor (Wave 3). If Wave 3 finds 32 KB too coarse for a
real summarization corpus, the next move is to lower
`MAX_FRAGMENT_CHARS` to 256 before lowering `MAX_PAYLOAD_BYTES`.

---

## 3. Multi-monitor approach

**Decision: identify by `MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST)`
+ `GetMonitorInfoW` + `GetDpiForWindow`. Persist `monitor.name` (e.g.
`\\.\DISPLAY2`), `monitor.isPrimary`, `monitor.bounds` (left/top/right/
bottom), and `monitor.dpiScale` (e.g. `1.5` for 144 dpi). No
`HMONITOR` int, no monitor index — both are unstable across reboots
and replug events.**

### Schema (within the v2 `snapshot_json`)

```json
"monitor": {
  "name": "\\\\.\\DISPLAY1",
  "isPrimary": true,
  "bounds": { "left": 0, "top": 0, "right": 1920, "bottom": 1080 },
  "dpiScale": 1.0
}
```

`name` is the only stable identifier across reboots; bounds + dpi let
the UI render position context if/when it ever wants to. Wave 2 itself
does not surface per-monitor breakdowns in the UI (that's a v2 feature
per the phase doc § "Multi-monitor scope").

### What we do NOT persist

- `HMONITOR` int — opaque handle; valid only within a process.
- Monitor index — `EnumDisplayMonitors`-order is not stable across
  display configuration changes.
- Per-monitor refresh rate, color profile — out of scope for Wave 2.
- Workspace/virtual desktop id — Win11 virtual-desktops aren't surfaced
  through UIA cleanly; deferred to a future ADR if it becomes signal.

---

## 4. Sampling cadence — keeping 1 Hz polling

The phase doc says Wave 2 may also add `SetWinEventHook(EVENT_SYSTEM_FOREGROUND, ...)`
for instant focus-change response. We **did not** ship the WinEventHook
fast path. Rationale:

- The activity-capture use case is "summarize the session AFTER it
  ends." Sub-second focus-change granularity is not payoff.
- 1 Hz × ~50 µs foreground probe + ~5–30 ms UIA walk = ~30 ms per
  tick worst-case on a busy browser. Well below any UX threshold.
- Adding a WinEventHook introduces a third WH_KEYBOARD_LL-style
  hook lifetime (Phase MC + Phase 3 are the existing two) and the
  invariant judge `ac-no-keystroke-content.md` (Wave 6) would need a
  whitelist entry. YAGNI until Wave 5 polish or post-seal feedback.

If a user reports "I switched apps and the timeline missed it,"
revisit. Until then 1 Hz stays.

---

## 5. Wave 1A deferral #2 — punted to Wave 3

The dictation-runtime direct signal path (Bernard's optional polish:
replace the `cc_update_session` UI-event roundtrip with a direct
constructor parameter to `dictation::start()`) is **NOT** in this
commit.

Rationale: Wave 2's blast radius is exactly `src-tauri/src/activity/*`
+ feature flags in workspace `Cargo.toml`. Reaching into
`src-tauri/src/dictation/*` violates the wave's coherence and adds an
unrelated subsystem to the smoke matrix surface. The UI-mediated path
works today; Wave 3 has runtime-touching work anyway (the abstractor
pipeline) and is the natural seam.

---

## 6. App-quality matrix (smoke test references)

| App | UIA quality | Notes |
|---|---|---|
| Notepad | ★★★★★ | Excellent; `DocumentControlTypeId` + ValuePattern give us the full text. |
| Win11 Settings | ★★★★★ | Best-in-class UIA tree, every setting is a labeled control. |
| Chrome | ★★★★ | Page content surfaces via UIA's chromium accessibility bridge. Long pages get truncated; that's the cap working. |
| VS Code (Electron) | ★★★ | Content panes expose less than Chrome; file-tree is good, editor body is opaque (Monaco renders to canvas). |
| Steam | ★★ | Browser panes give decent UIA; in-game windows expose nothing (expected). Graceful: `status.kind = "no_payload"`. |
| Win11 Lock screen | n/a | `read_last_input_age_ms` returns Err; foreground HWND is zero; no events written. Correct. |
| Browser sign-in form | ★★★★ | `passwordFieldActive=true` when focused, `value` redacted to empty, fragments cleared. Verified per Q5. |

The "★ count" is qualitative — what matters for Phase 10 is that the
Stage-3 abstractor (Wave 3) has *enough* signal to generate a
meaningful Block summary, not that we capture everything. The matrix
above gives Wave 3 confidence that "real apps" yield real payloads.

---

## 7. Files touched this wave

### New
- `src-tauri/src/activity/uia/mod.rs` — Probe trait + factory + StubProbe.
- `src-tauri/src/activity/uia/payload.rs` — pure-Rust ProbeResult + JSON serialization + size-cap cascade. 19 unit tests.
- `src-tauri/src/activity/uia/windows_com.rs` — Windows COM impl. 4 unit tests + relies on the smoke matrix for behavioral coverage.
- `src-tauri/src/activity/activity_level.rs` — pure-Rust idle-tracker FSM + `GetLastInputInfo` probe. 11 unit tests.
- `docs/phases/phase10-wave2-brief.md` — this file.
- `scripts/throwaway-test.ps1` — promoted the throwaway-crate recipe to a reusable script (LESSONS P2). Generalizes across pure modules + optional preamble.
- `scripts/test-activity-level.ps1` — wraps the throwaway runner for `activity_level.rs` (it needs the `windows` crate + a `crate::error` stub).

### Modified
- `Cargo.toml` (workspace) — six new `windows` feature flags (UIA, Gdi, HiDpi, Com, Variant, Ole, SystemInformation).
- `src-tauri/src/activity/sampler.rs` — adds `Probe` + `IdleTracker` integration; new `IdleStart` / `IdleEnd` `SamplerEvent` variants; `ContextSnapshot` gains `snapshot_json` field (sampler now builds the payload, not the runtime).
- `src-tauri/src/activity/runtime.rs` — handles the new variants; uses sampler-built `snapshot_json` verbatim. Two updated tests + one new (`record_event_handles_idle_start_and_idle_end`).
- `src-tauri/src/activity/mod.rs` — exports `activity_level` + `uia` modules.
- `ui/src/lib/activity.ts` — exports `UiaSnapshotV2`, `MonitorInfo`, `FocusedField`, `ControlSummary`, `ProbeStatus`, and a safe `parseSnapshotJson` helper.
- `ui/src/pages/Activity.tsx` — adds `SnapshotDetails` subcomponent: collapsed-by-default `<details>` panel rendering monitor + control-summary one-liner, focused field, and a small fragment preview.
- `ui/src/pages/Activity.module.css` — adds `.snapshot*` classes.
- `ui/src/i18n/en.json` — adds 12 `activity.snapshot.*` keys.

---

## 8. Gate results

| Gate | Result |
|---|---|
| `cargo-with-cuda.ps1 check` | ✅ clean |
| `cargo-with-cuda.ps1 clippy --release -- -D warnings` | ✅ clean |
| `cargo-with-cuda.ps1 fmt --check` | ✅ clean |
| `cargo-with-cuda.ps1 test --release --no-run` | ⏳ in progress at commit time (long first build chain; rerun + log in commit body once it lands) |
| Throwaway-crate `uia_payload` | ✅ 19/19 |
| Throwaway-crate `activity_level` | ✅ 11/11 |
| `npx tsc --noEmit` | ✅ clean |
| `npm test` (vitest) | ✅ 63/63 |
| `npm run build` | ✅ clean (5 entry points; main 177 KB / 51 KB gzip) |

Live smoke matrix (≥10 min sweep across Notepad / Settings / Chrome /
VS Code / Steam / sign-in form / multi-monitor) is **awaiting Dustin's
sign-off**. The matrix above is from a Bernard-driven sweep; it's a
rehearsal, not the seal-condition smoke matrix per the phase doc.
