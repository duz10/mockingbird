# ADR-0033: Phase MC hotfix — chord collision (VK_M ⇨ VK_OEM_PERIOD), source-probe wiring, overlay/Stop UI completion

- **Status:** Accepted
- **Date:** 2026-05-23
- **Accepted:** 2026-05-23 (single-iteration ship, all 5 MC Wave-6 judges preserved)
- **Deciders:** Dustin (project lead), code-puppy/Bernard (implementor, session `code-puppy-b14c19`)
- **Supersedes / extends:** ADR 0019 §MC.1 (chord default), ADR 0032 (MC v1.1 polish — orthogonal scope)
- **bd:** `mb-x1x` (this epic), spawns `mb-dub` (tray deep-link follow-up, P3)

## Context

Phase MC (Meeting Capture) was sealed on 2026-05-22 at git tag
`phase-mc-complete`. Within hours of Dustin running the shipped build
end-to-end on the production Win11 box, four user-reported regressions
surfaced that were **not** caught by the Wave-6 judges (none of which
exercise live audio device probes, real keyboard hooks, or post-start
UI state machines on a real OS). The five judges remain green by
construction after this ADR — the fixes are confined to integration
glue and one settings-default change.

The four issues:

1. **Source-availability probe was a Wave-3 stub.** `meetings::capture::
   probe_sources()` hard-coded `system_available: false` and was never
   replaced with a real cpal check in Wave 4 as the phase plan
   implied. Every box reported "system audio not available" in the
   Meetings page source dropdown, which masked the entire loopback
   path from the user.

2. **Main-window "Start Recording" button latched the Stop button
   disabled.** `ui/src/pages/Meetings.tsx` set
   `startingOrStopping="starting"` in `handleStart` and only cleared
   it on `meeting:state="done" | "error" | "interrupted"`. None of
   those fire on a successful start (`"started"` does, but the handler
   never cleared the latch on that event). The chord-start path went
   through a separate component (`MeetingOverlay`) that did clear
   correctly, masking the asymmetry during pre-seal QA.

3. **Default chord `RCtrl + M` collided with Microsoft 365 Copilot on
   Windows 11.** Microsoft's chord handler fires regardless of whether
   our `WH_KEYBOARD_LL` hook consumes the keystroke (Copilot
   apparently uses a higher-priority injection path or polls the key
   state independently). Local-first + privacy-preserving means we
   can't ship a default the OS vendor steals. Compounding this, the
   pre-hotfix `lib.rs` boot path called
   `MeetingRuntimeConfig::defaults_with()` unconditionally and never
   read the `meeting_hotkey_modifier` / `meeting_hotkey_key` settings
   rows — so the Settings UI chord picker (shipped in MC Wave 5) was a
   no-op. The DB persisted the user's selection, the spawn ignored it.
   Two bugs hiding each other.

4. **Overlay pill didn't appear when meetings were started from the
   main-window button, and the recording-mode pill had no dismiss
   button.** The chord path called `show_overlay()` from inside the
   activation thread; the IPC-driven main-window start path
   (`commands::meetings::meeting_start`) did not. Once the overlay
   was visible, the only exit was "Stop the meeting" — there was no
   way to hide the pill while letting the recording continue (a real
   ergonomic ask when the meeting is long and the pill is in the way
   of a slide).

Per AGENTS.md "Permanently sealed" rule, sealed phases are immutable
and new work against them must be ADR-chartered lateral epics (per the
ADR 0022 / 0023 precedent). This is that ADR for the post-seal
hotfix epic.

## Decision

### 1. Source probe — real cpal device check, cross-platform-gated

`meetings::capture::probe_sources()` calls `cpal::default_host()` and
checks `default_input_device()` / `default_output_device()` for a
valid `default_input_config()` / `default_output_config()`
respectively. The `default_output_config()` path is the WASAPI
loopback source proxy — if it exists, system audio capture is
available.

We deliberately **do not** open a stream during the probe. Opening
WASAPI in `PLAY_LOOPBACK` mode pulls frames and could trip recording-
indicator UIs on the user's box. A config-only check is cheap and
sufficient. The probe runs once at app boot + on the Meetings page's
initial mount.

`#[cfg(target_os = "windows")]` gates the cpal path; non-Windows
returns `{ mic: false, system: false }` per Phase 9's deferred macOS
loopback story. Principle 5 (cross-platform abstraction) is preserved.

### 2. Stop-button latch — clear on `started` event

`ui/src/pages/Meetings.tsx`: the `meeting:state` event handler clears
`setStartingOrStopping(null)` on the `"started"` branch in addition
to the existing `"done" | "error" | "interrupted"` clears. Fires from
both the button and the chord start paths, so a single fix covers
both call sites. 1 LoC + 9 lines of context comment.

### 3. Chord default flip + settings-wiring fix + one-shot migration

**New default:** `VK_RCONTROL + VK_OEM_PERIOD` (the `.>` key). Chosen
from a 4-candidate audit (Right Ctrl + `\`, `;`, `.`, or F8) for
collision-safety with Microsoft 365 apps and ergonomic accessibility
(right-pinky reach, almost never bound). Dustin picked the period
explicitly.

**Settings wiring:** `lib.rs` now calls
`MeetingRuntimeConfig::from_settings(&conn, chunk_base_dir)` at boot,
replacing the pre-hotfix `defaults_with()` path. The new function
reads `MeetingHotkeyModifier`, `MeetingHotkeyKey`,
`MeetingMaxDurationSeconds`, and `MeetingDefaultSource` from the
settings DB and falls back to documented defaults on parse error /
missing row. Bad rows log a warning + use the default rather than
panicking the meeting subsystem.

**VK-name parser:** New module `meetings::vk_names` translates string
names (`"VK_OEM_PERIOD"`) to Windows VK codes (`0xBE`) and back. Covers
modifiers, function keys (F1–F24), OEM punctuation safe-chord set
(`.`, `,`, `;`, `/`, `` ` ``, `\`, `-`, `=`), and A–Z + 0–9. 22 unit
tests, including round-trip lossless coverage of the entire supported
set. Single boundary for the string-to-code mapping; settings stay
JSON-friendly for `sqlite3` inspection.

**One-shot migration:** `upgrade_legacy_chord_default_once` writes a
sentinel settings row (`_internal_mc_chord_copilot_hotfix_v1`) on first
boot post-hotfix and, if `meeting_hotkey_key` is exactly the literal
JSON string `"VK_M"`, rewrites it to `"VK_OEM_PERIOD"`. Idempotent:
re-running does nothing once the marker is present. Respects user
re-pick: if a user goes to Settings post-migration and selects `VK_M`
explicitly, that choice survives the next boot (marker is present, no
further mutation).

**Settings UI picker:** `SettingsMeetingTab.tsx` extends
`MAIN_KEY_OPTIONS` with `VK_OEM_PERIOD` (now first), `VK_OEM_COMMA`,
`VK_OEM_1`, `VK_OEM_5` plus a `MAIN_KEY_LABELS` map for human-friendly
display (`"Period  ."` not `"VK_OEM_PERIOD"`). A passthrough sentinel
`<option>` is rendered when the persisted value isn't in the curated
list, so a user who hand-edited the DB sees their value rather than a
silently-empty picker.

### 4. Overlay auto-show from main-window start + recording-mode × button

**Rust side:** New `meetings::overlay::force_show_for_recording(app)`
calls `window.show()` on the overlay window **without** emitting the
`meeting:overlay-open` event. The event is the CHOOSE-mode trigger;
re-emitting it from a direct-start path would flicker the overlay
between CHOOSE and recording modes. `commands::meetings::meeting_start`
calls the new helper after the meeting runtime acknowledges the start.

**UI side:** `MeetingOverlay.tsx`'s recording-mode JSX adds a × button
between the Stop button and the pill's right edge. Distinct visual
style (no red, smaller icon) so users can't confuse it with Stop.
Click calls the existing `handleCancel` → `hideOverlay()` (just hides
the Tauri window; doesn't touch the recording state).
`meetingOverlay.dismiss` i18n string added.

### 5. What we **did not** do (deferred)

- **"Reconfigure meeting chord…" tray menu item.** The current
  Settings tray entry already opens the main window but lands on
  Insights — no hash-router deep-link plumbing exists. Adding a
  dedicated tray entry without deep-link is redundant. Deferred to
  `bd: mb-dub` (P3), which will land the `app:navigate` event +
  React-router subscriber + the tray entry as one piece.

## Consequences

### Positive

- System audio (loopback) is now actually offered on any Windows box
  with a working playback device — the loopback feature is no longer
  silently dead.
- Stop button works after main-window start. Symmetric behavior
  between chord-start and button-start paths.
- New default chord doesn't collide with Microsoft Copilot. Users
  with the legacy `VK_M` default get migrated automatically on the
  next launch — no manual settings fiddling required.
- Settings UI chord picker is no longer a no-op; the persisted choice
  actually drives the activation hook at next boot.
- Overlay pill appears for **all** start paths and can be dismissed
  without killing the recording. Matches Dustin's mental model.

### Negative / costs

- The settings-DB sentinel row (`_internal_mc_chord_copilot_hotfix_v1`)
  is technical debt — one more row in the settings table that has no
  semantic meaning to the user or to future code. It's annotated +
  scoped to this one migration, and the cost is one row, but it sets
  a (minor) precedent for inline-migration markers in `settings`
  rather than going through a real schema migration. Acceptable
  because the change is a default-value flip, not a schema shape
  change.
- `VK_OEM_PERIOD` is keyboard-layout-dependent. On a US ANSI layout
  the physical key is unambiguously `.>`; on AZERTY or other layouts
  the same VK code maps to a different glyph. We mitigate by always
  surfacing the canonical VK name in the Settings UI plus the human
  label, and by relying on Windows' VK abstraction (the hook fires on
  the VK regardless of label). No keyboard-layout judge added — the
  user's first cross-layout report is the trigger to broaden the
  approach (probably a `KeyboardLayout` parameter + per-layout key
  preview in the picker).

### Neutral

- 1004 LoC delta across 11 files, +33 tests in the Rust lib (mostly
  `vk_names` round-trips + `from_settings` matrix coverage). Test
  density delta well above the project's 10-tests-per-500-LoC target.
- All five Phase MC Wave-6 judges remain green: the changes are
  confined to non-sealed files. `mc-dictation-untouched` specifically
  passes — no `dictation/`, `hotkey/state.rs`, `hotkey/windows.rs`,
  `hotkey/driver.rs`, `injection/`, `recording_window.rs`,
  `cleanup/{provider,llm_cleaner}.rs`, or migrations 001–010 were
  modified.

## Implementation

**Files changed (with brief role):**

| File | Role |
|---|---|
| `src-tauri/src/meetings/capture.rs` | Real cpal probe |
| `src-tauri/src/meetings/vk_names.rs` | **new** — VK name ⇄ code mapping (22 tests) |
| `src-tauri/src/meetings/mod.rs` | Declare `vk_names` |
| `src-tauri/src/meetings/runtime.rs` | `from_settings`, `upgrade_legacy_chord_default_once`, `short_label`, `format_hotkey_label`, +11 tests |
| `src-tauri/src/meetings/hotkey_installer.rs` | `ChordConfig::default` flip + doc update |
| `src-tauri/src/meetings/overlay.rs` | `force_show_for_recording` |
| `src-tauri/src/settings/model.rs` | Default value `"VK_OEM_PERIOD"` + doc update |
| `src-tauri/src/commands/meetings.rs` | `meeting_start` calls `force_show_for_recording` |
| `src-tauri/src/lib.rs` | Boot path calls `from_settings` instead of `defaults_with` |
| `ui/src/pages/Meetings.tsx` | Clear `startingOrStopping` on `started` event |
| `ui/src/pages/SettingsMeetingTab.tsx` | Extended `MAIN_KEY_OPTIONS` + `MAIN_KEY_LABELS` + passthrough sentinel |
| `ui/src/meeting_overlay/MeetingOverlay.tsx` | × dismiss button in recording-mode pill |
| `ui/src/i18n/en.json` | `meetingOverlay.dismiss` |

**Gate status at acceptance:**

- `cargo fmt --check` — clean (one rustfmt nit auto-fixed)
- `cargo clippy --release -- -D warnings` — clean (one `is_ok()` lint
  auto-fixed during the gate run)
- `cargo test --release --no-run` — clean compile (live exec blocked
  by the LESSONS 2026-05-17 `STATUS_ENTRYPOINT_NOT_FOUND` known issue
  on this box; per AGENTS.md the `--no-run` proof is the gate)
- `npx tsc --noEmit` — clean
- `npm test` — 55/55 passing
- `npm run build` — vite production bundle clean
- ESLint — blocked pre-hotfix by `mb-yxh` (ESLint v9 migration), not in scope

**Sealing commit:** the single bug-fix commit that lands this ADR plus
the 11-file diff. No new phase tag (the original Phase MC seal
remains authoritative); the ADR is the record.

## References

- AGENTS.md "Permanently sealed" + "ADR-chartered lateral epics"
- ADR 0019 §MC.1 (original chord default — superseded by this ADR for
  the default value only; the fallback ladder F13/F14 still applies)
- ADR 0022, ADR 0023 (lateral-epic-on-sealed-phase precedent)
- ADR 0032 (MC v1.1 polish — orthogonal scope, both accepted same day)
- `docs/phases/phase-meeting-capture.md` (the phase plan)
- LESSONS 2026-05-17 (`STATUS_ENTRYPOINT_NOT_FOUND` known issue)
- bd: `mb-x1x` (this epic), `mb-dub` (deferred tray deep-link)
