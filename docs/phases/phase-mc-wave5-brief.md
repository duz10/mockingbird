# Phase MC Wave 5 brief — Tray toggle, Settings UI, save-as, progress, a11y polish

> Authored end of Wave 4 (commits `bb52526` → `e1bb92c`). Wave 5
> author: read this before opening `docs/phases/phase-meeting-capture.md`.
> The master plan is still binding; this brief narrows the design
> choices Wave 4 left open and pins per-deliverable signatures + test
> specs.

---

## What Wave 4 shipped (so you know what to build on)

| module | net status after Wave 4 |
|---|---|
| `meetings/persist.rs` | `persist_meeting()` writes atomic session-row + per-channel formatted + per-channel raw_segments rows. Per-row failures non-fatal (mirrors P3 W4.9 Bug A). Closed in `9977d6a`. |
| `meetings/runtime.rs` + `meetings/lifecycle.rs` | Full lifecycle: start → capture → stop → long-form-stt → formatter → merge → persist → emit done. Drop marks `interrupted`. `meeting:state` + `meeting:progress` + `meetings:session-saved` events all emitted from `lifecycle.rs`. Closed in `bb52526`. |
| `meetings/llm_pass.rs` | Constructs a fresh `OllamaProvider` via existing arg-less `new()`. Built-in prompts in `meetings/prompts/*.md` (`summary.md`, `action_items.md`, `cleaner_punctuation.md`). Custom prompt via `LlmPassPrompt::Custom(body)`. `LlmPassResult { id, text, latency_ms }` cached in `MeetingRuntimeShared.llm_pass_cache: HashMap<Uuid, String>`. **No DB persistence** — invariant `mc-no-llm-in-critical-path`. |
| `meetings/export.rs` + `meetings/clipboard.rs` | `render_markdown(&MeetingDetail, Option<&str>) -> AppResult<String>` (frontmatter + body; `--include-llm-pass <id>` appends a trailing section). `copy_text_one_shot` is a one-shot `arboard` write (NOT save/restore — see Wave 4 deviation #2). |
| `src-tauri/src/commands/meetings.rs` | All 10 Section MC.6 commands implemented + registered in `commands/mod.rs`. Args structs camelCase via `#[serde(rename_all = "camelCase")]`. Closed in `3381d16`. |
| `ui/src/lib/meetings.ts` + `ui/src/pages/Meetings.tsx` + `MeetingDetail.tsx` + `MeetingRecordBar.tsx` + Sidebar nav + meeting_overlay window | Main-window Meetings page (two-pane list+detail), record bar (source picker + start/stop button + pulsing indicator), LLM-pass UI panel, transcript tabs, Copy / Export / Delete actions. `meeting_overlay` window declared in `tauri.conf.json` (frameless, transparent, alwaysOnTop, skipTaskbar, focus:false, visible:false). Overlay renders source-picker → start → recording chip. Closed in `daf054d` + `e1bb92c`. |
| `docs/phases/phase-mc-qa-matrix.md` | Template authored. HUMAN-IN-LOOP — Dustin runs the 5 scenarios at a real keyboard. Closed in `2aaed00`. |

Files **untouched** this wave (still sealed): `hotkey/state.rs`,
`hotkey/windows.rs`, `hotkey/driver.rs`, `dictation/`, `injection/`,
`recording_window.rs`, `cleanup/provider.rs`, `cleanup/llm_cleaner.rs`,
migrations 001–010.

Project test count after Wave 4: **~455** (vitest +12 for `lib/meetings`
+ Rust ~+20 across persist/runtime/llm_pass/export/commands). Phase-MC
running delta so far: **+125 tests** (Wave 2 +68 + Wave 3 +35 + Wave 4
~+22). Master-plan target is +90 to +120 — we're at the top of the
band; Wave 5's polish should add **+5 to +15** more (settings UI tests
+ tray pure tests + progress wire test).

### 4.1 Wave 4 deviations applied (do not re-litigate)

These were Wave 4 author calls; they're now load-bearing for Wave 5:

1. **`ExportRequest<'a>` deleted** in favor of
   `render_markdown(&MeetingDetail, Option<&str>)`. Wave 4 brief §4.4.
2. **`copy_text_one_shot` lives in `meetings/clipboard.rs`** — single-
   source-of-truth primitive callable from IPC + future tray.
3. **Tray meeting-status entry deferred from Wave 4 to Wave 5** —
   §5.1 below picks it up.
4. **Some integration tests gated `#[ignore]`** if `tauri::test::
   mock_runtime()` was brittle. Document the gate per attribute. Wave
   5 does NOT need to un-gate them; the QA matrix is the e2e proof.
5. **`meeting_overlay` window declared with `visible: false`.** The
   chord → show-window wiring is **§5.6 of THIS brief** — Wave 5 work.

---

## Wave 5 deliverables (5 tasks)

Master plan Wave 5 row lists 3 P1 + 2 P2. We treat all 5 as
sealable-this-wave; defer items that prove too big to Wave 6 risk
register only with explicit ADR.

### 5.1 Tray-menu "Pause meeting hotkey" toggle (P1)

**Files:** `src-tauri/src/tray.rs`, `src-tauri/src/meetings/runtime.rs`
(public `set_meeting_hotkey_paused(bool)`), `src-tauri/src/commands/
meetings.rs` (NEW command `meeting_set_paused`).

#### What it does

Adds a `Pause Meeting Hotkey` checkable menu item to the tray menu,
sitting between the existing `Pause` (dictation pause) and `Settings…`
items. Toggling it injects `ActivationEvent::PauseToggle { paused }`
into the meetings activation channel — which the activation state
machine *already* handles (see `activation.rs:159` "PauseToggle wins
everywhere" and the existing tests `pause_toggle_in_idle_resets_to_idle`
+ `pause_toggle_in_main_pressed_resets_to_idle`). When paused:

* The meetings hotkey hook still installs + chains via
  `CallNextHookEx` — i.e. dictation hook still works.
* `Right Ctrl + M` → activation state machine sees the chord but
  `paused == true` swallows it; no `MeetingToggle` is emitted.
* Manual start/stop from the main-window record bar continues to
  work (it's not gated by the activation channel).

#### Rust signatures

```rust
// meetings/runtime.rs — new public method on MeetingCaptureRuntime
impl MeetingCaptureRuntime {
    /// Idempotent. Injects `ActivationEvent::PauseToggle { paused }`
    /// into the activation channel. Persists the new value to
    /// settings under `meeting_hotkey_paused` (NEW SettingKey — see
    /// §5.1.1 below) so the choice survives app restarts.
    pub fn set_meeting_hotkey_paused(&self, paused: bool) -> AppResult<()>;

    /// Read the current paused state. Reads from in-memory cache,
    /// not settings DB — startup hydrates the cache from settings.
    pub fn is_meeting_hotkey_paused(&self) -> bool;
}
```

```rust
// commands/meetings.rs — NEW command
#[tauri::command]
pub async fn meeting_set_paused(
    state: tauri::State<'_, Arc<MeetingCaptureRuntime>>,
    paused: bool,
) -> Result<(), String>;

#[tauri::command]
pub fn meeting_is_paused(
    state: tauri::State<'_, Arc<MeetingCaptureRuntime>>,
) -> Result<bool, String>;
```

#### `tray.rs` wiring

The tray needs to read + write the paused state. Two paths:

* **Read at menu-open** to set the initial checkmark: query
  `app_handle.state::<Arc<MeetingCaptureRuntime>>().is_meeting_
  hotkey_paused()` inside the `on_menu_event` callback right before
  emitting (Tauri doesn't checkbox-toggle menu items in 2.x without
  rebuilding the menu — see §5.1.2). Simpler: rebuild the menu each
  time the user opens the tray (Windows tray menu rebuild cost is
  ~ms, irrelevant).
* **Write on click:** `handle_menu_event` adds a new arm for
  `"pause_meeting"` that calls `set_meeting_hotkey_paused(!current)`.

#### 5.1.1 New `SettingKey` variant

The user's pause choice must survive restart. Add to
`settings/model.rs`:

```rust
SettingKey::MeetingHotkeyPaused,
// db key: "meeting_hotkey_paused"
// default: serde_json::json!(false)
```

…and add the variant to `ALL` + the `every-key-round-trips` test (it
already iterates `Self::ALL`). One extra round-trip line is the only
test churn here.

#### 5.1.2 Tauri 2.x checkbox-menu-item caveat

Tauri 2 supports `CheckMenuItem` via `tauri::menu::CheckMenuItemBuilder`.
Use it instead of the plain `MenuItemBuilder` for the new "Pause Meeting
Hotkey" entry. Example:

```rust
let pause_meeting = CheckMenuItemBuilder::with_id("pause_meeting", "Pause Meeting Hotkey")
    .checked(is_paused) // read once at menu-build
    .build(app)
    .map_err(map_tauri)?;
```

The handler is the same shape as plain menu items. To update the
checked state after a toggle, either:
* (a) Rebuild the menu via `tray.set_menu(&menu)?` after each toggle,
  OR
* (b) Call `pause_meeting.set_checked(!checked)?` on the cached
  `CheckMenuItem` handle.

Pick (b) if the cached handle is reachable; (a) is the fallback if
the menu items go out of scope after `register()`.

#### Test specs (4 tests)

| file | name | inputs | expected |
|---|---|---|---|
| `tray.rs` | `handle_menu_event_pure_recognizes_pause_meeting_id` | Extend existing pure-test loop with `"pause_meeting"` | `true` |
| `meetings/runtime.rs` | `set_paused_then_get_round_trips` | Spawn runtime; `set_meeting_hotkey_paused(true)`; read | `is_meeting_hotkey_paused() == true` |
| `meetings/runtime.rs` | `set_paused_persists_to_settings` | Spawn with in-memory DB; toggle paused; respawn from same DB | Second `is_meeting_hotkey_paused() == true` |
| `meetings/activation.rs` | `pause_toggle_via_runtime_swallows_subsequent_chord` (extend existing test if natural) | Inject `PauseToggle { paused: true }`; then full chord sequence | No `MeetingToggle` event emitted; state stays IDLE |

If the runtime tests need `tauri::test::mock_app()` and that's still
brittle, gate the bottom two `#[ignore]` and lean on the activation-
pure test (which is already there + green).

---

### 5.2 Settings UI surface for new `SettingKey` variants (P1)

**File:** `ui/src/pages/Settings.tsx` (+ companion module CSS if
the file approaches 600 lines; pre-split if needed).

#### Variants to surface

All declared in Wave 1; Wave 5 adds the UI controls. Defaults shown
for reference (from `settings/model.rs::default_value`).

| variant                       | db key                          | type                                              | default          | UI control                          |
|-------------------------------|---------------------------------|---------------------------------------------------|------------------|--------------------------------------|
| `MeetingHotkeyModifier`       | `meeting_hotkey_modifier`       | string (`"VK_RCONTROL"`/`"VK_LCONTROL"`/`"VK_F13"`)| `"VK_RCONTROL"`  | Dropdown w/ conflict-probe call before save |
| `MeetingHotkeyMainKey` (if it exists; check `settings/model.rs`) | `meeting_hotkey_main_key` | string (`"VK_M"`/`"VK_F13"`/…)                  | (check default)  | Dropdown                              |
| `MeetingDefaultSource`        | `meeting_default_source`        | string (`"mic"`/`"system"`/`"both"`)              | `"mic"`          | Dropdown                              |
| `MeetingFillerStripEnabled`   | `meeting_filler_strip_enabled`  | bool                                              | `true`           | Toggle switch                         |
| `MeetingParagraphGapMs`       | `meeting_paragraph_gap_ms`      | number (500–5000)                                 | `2000`           | Slider w/ ms readout                  |
| `MeetingAudioRetentionDays`   | `meeting_audio_retention_days`  | number\|null                                      | `null` (inherit) | Number input + "Inherit (—)" toggle   |
| `MeetingSpeakerLabelMic`      | `meeting_speaker_label_mic`     | string                                            | `"You"`          | Text input (max 30 chars)             |
| `MeetingSpeakerLabelSys`      | `meeting_speaker_label_sys`     | string                                            | `"Other(s)"`     | Text input (max 30 chars)             |
| `MeetingHotkeyPaused` (NEW)   | `meeting_hotkey_paused`         | bool                                              | `false`          | **Mirror of tray toggle** — same row |

#### Layout

Add a new top-level Settings tab labeled **"Meeting"** sitting
between the existing "Dictation" and "Advanced" tabs. The tab
houses two sections:

* **Activation** — modifier + main-key dropdowns + paused toggle.
  The two dropdowns trigger a Tauri command that runs the existing
  `hotkey/probe.rs::probe_meeting_main_vk` + `meeting_candidate_chain`
  fallback chain; if the chosen combo collides with the dictation
  hotkey, the UI surfaces a `Toast` + disables the Save button
  (`probe_meeting_main_vk` returns `MeetingProbeResult::Collision`
  with the colliding VK). The collision check fires `onChange`, not
  `onBlur`, so the user sees the warning before saving.

* **Transcript** — default source + filler strip + paragraph gap +
  speaker labels.

* **Audio retention** — per-meeting override field with an
  "Inherit from `audio_retention_days`" toggle. When the toggle is
  on, the value sent is `null` (which the DB layer accepts and reads
  as "use the global retention").

#### TS types + IPC

Already partly in place. Settings IPC is `settings.get(key)` /
`settings.set(key, value)` per `ui/src/lib/tauri.ts`. The new fields
are JSON-serialized — booleans as `true`/`false`, numbers as numbers,
strings as strings, `null` for the retention "inherit" sentinel.

A new `settings.probeMeetingHotkey(modifier, mainKey)` IPC command
wraps the Rust probe; signature:

```rust
// commands/settings.rs (or wherever the probe IPC lives)
#[tauri::command]
pub fn probe_meeting_hotkey(
    modifier: String, // VK_NAME
    main_key: String,
) -> Result<MeetingProbeResult, String>;
```

…where `MeetingProbeResult` already exists in `hotkey/probe.rs`.

#### Test specs (vitest, 5 tests)

`ui/src/pages/Settings.meeting.test.tsx` — new file.

| name | setup | expected |
|---|---|---|
| `renders_meeting_tab` | Render `<SettingsPage />` with default settings fixtures; click "Meeting" tab | All 8 controls render with their default values |
| `paragraph_gap_slider_updates_setting` | Drag the slider to 3000 | `settings.set("meeting_paragraph_gap_ms", 3000)` called once |
| `filler_strip_toggle_updates_setting` | Toggle filler-strip off | `settings.set("meeting_filler_strip_enabled", false)` called once |
| `hotkey_collision_disables_save` | Mock `probeMeetingHotkey` to return `{ kind: "Collision", colliding_vk: "VK_RCONTROL" }` | Save button disabled; collision toast visible |
| `retention_inherit_toggle_writes_null` | Click "Inherit" | `settings.set("meeting_audio_retention_days", null)` called once |

If `Settings.tsx` is already ~580 lines (close to cap), extract the
new "Meeting" tab into `pages/SettingsMeetingTab.tsx`. Same pattern
as the Wave-4 MeetingDetail extraction.

---

### 5.3 `meeting_export_markdown` "Save As…" dialog (P1)

**Files:**
* `src-tauri/Cargo.toml` (add `tauri-plugin-dialog`)
* `src-tauri/src/lib.rs` (`.plugin(tauri_plugin_dialog::init())`)
* `src-tauri/src/commands/meetings.rs` (extend `meeting_export_markdown` to invoke save-as dialog when no `dest_path` supplied)
* `ui/src/pages/MeetingDetail.tsx` (button already exists; just verify the IPC call works without `destPath` in real-Tauri mode)

#### Why a plugin

Tauri 2 moves dialogs from core into the `tauri-plugin-dialog` crate.
Wave 4's `commands/meetings.rs` already comments on this (see
`commands/meetings.rs:28`). Adding the plugin is the binding choice
to unblock the Save-As UX.

#### Plugin dep + init wiring

```toml
# src-tauri/Cargo.toml [dependencies]
tauri-plugin-dialog = { version = "2", default-features = false }
```

Check the Mini Shai-Hulud IOC list (PLAN Appendix D) before adding.
The `tauri-plugin-dialog` crate is in the Tauri org and is on the
known-good list as of `phase-4-complete`.

```rust
// src-tauri/src/lib.rs (or wherever .plugin() chains live)
.plugin(tauri_plugin_dialog::init())
```

#### Command behaviour change

`meeting_export_markdown` already accepts `destPath: Option<String>`.
When `None`, the command currently picks a default path
(`<appdata>/Mockingbird/meetings/exports/<uuid>.md`). Wave 5
**preserves the default-path fallback** but adds a new optional arg:

```rust
#[tauri::command]
pub async fn meeting_export_markdown(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<MeetingCaptureRuntime>>,
    uuid: String,
    dest_path: Option<String>,
    prompt_user_for_path: Option<bool>, // NEW
    include_llm_pass: Option<LlmPassRef>,
) -> Result<MeetingExportResult, String>;
```

When `prompt_user_for_path == Some(true)` and `dest_path == None`,
the command opens a save-as dialog via:

```rust
use tauri_plugin_dialog::DialogExt;
let path = app.dialog()
    .file()
    .add_filter("Markdown", &["md"])
    .set_file_name(&suggested_name)
    .blocking_save_file();
// `path` is Option<FilePath>; None means user cancelled.
```

The UI passes `promptUserForPath: true` from the new "Export…" button;
silent / programmatic callers pass it omitted to get the existing
default-path behaviour.

#### UI wiring

`ui/src/pages/MeetingDetail.tsx`'s "Export markdown…" button already
exists from Wave 4. Update the IPC call in `Meetings.tsx`'s
`handleExport` callback (which lives in `pages/Meetings.tsx`) to pass
`promptUserForPath: true`. The `lib/meetings.ts` wrapper extends:

```ts
exportMarkdown: (
  uuid: string,
  destPath?: string,
  llmPassId?: string,
  promptUserForPath = true,
) => invoke<{ path: string | null }>("meeting_export_markdown", {
  uuid,
  destPath,
  promptUserForPath,
  includeLlmPass: llmPassId ? { id: llmPassId } : undefined,
}),
```

Return type tightens: the existing `{ path: string }` becomes
`{ path: string | null }` because the user can cancel the dialog. UI
handles the null by showing "Export cancelled" toast (no error log).

#### Test specs (3 tests)

| file | name | inputs | expected |
|---|---|---|---|
| `commands/meetings.rs::tests` | `export_with_dest_path_skips_dialog` | Call with `dest_path=Some(temp)`, `prompt_user_for_path=Some(true)` | Returns path == temp; no dialog opened (can't assert no-dialog directly; smoke-only) |
| `commands/meetings.rs::tests` | `export_with_default_path_writes_to_appdata` | Call with both args None | Returns a path under `<appdata>/Mockingbird/meetings/exports/` |
| `ui/src/lib/meetings.test.ts` | `exportMarkdown_passes_prompt_flag` | Override `meeting_export_markdown` fixture with a spy | Args object contains `promptUserForPath: true` |

The interactive dialog test belongs in Playwright (qa-kitten beat),
not vitest.

---

### 5.4 Live `meeting:progress` chunk counter wired into MeetingDetail (P2)

**Files:** `ui/src/pages/Meetings.tsx` + `ui/src/lib/types.ts`
(extend `MeetingProgressEvent`).

#### Wire

The Rust runtime already emits `meeting:progress` events from
`lifecycle.rs:86`:

```json
{ "channel": "mic" | "system", "chunksDone": N, "chunksTotal": N|null }
```

`MeetingsPage` adds a third `listen<MeetingProgressEvent>` subscriber
alongside `meeting:state` and `meetings:session-saved`. State shape:

```ts
interface MeetingProgressState {
  mic?: { done: number; total: number | null };
  system?: { done: number; total: number | null };
}
```

The progress chip renders inline in the record bar's status row when
the meeting is in the "transcribing" phase (i.e. `recordingPhase ===
"transcribing"`). Format: `"transcribing 4/12"` (master plan §Wave 5
spec, verbatim). When `chunksTotal` is null (i.e. chunks still being
emitted by capture), render `"transcribing 4/?"`.

`MeetingProgressState` is reset to `{}` on each new meeting start
(triggered by `meeting:state === "started"`).

#### TS type addition

```ts
// lib/types.ts
export interface MeetingProgressEvent {
  channel: "mic" | "system";
  chunksDone: number;
  chunksTotal: number | null;
}
```

#### Test specs (2 vitest tests)

| file | name | setup | expected |
|---|---|---|---|
| `ui/src/pages/Meetings.progress.test.tsx` (NEW) | `renders_progress_chip_when_transcribing` | Render `<MeetingsPage />`; emit `meeting:state=started`; emit `meeting:progress` with mic 4/12 | Chip text matches `/transcribing 4\/12/` |
| `ui/src/pages/Meetings.progress.test.tsx` | `clears_progress_on_next_meeting_start` | Emit progress; emit `meeting:state=done`; emit `meeting:state=started` again | Chip not visible |

Use `@testing-library/react` if it's already a dep; if not, mount the
component into a jsdom container and assert via `getByText`.

---

### 5.5 Accessibility pass (P2)

**Files:** `ui/src/meeting_overlay/MeetingOverlay.tsx`,
`ui/src/pages/MeetingDetail.tsx`, `ui/src/pages/Meetings.tsx`.

#### Checklist

1. **Reduced-motion respected on the overlay.** Already done in the
   Wave 4 CSS (`@media (prefers-reduced-motion: reduce) { .pulseDot
   { animation: none; } }`). Audit: confirm the chip's pulse-dot in
   `Meetings.module.css` has the same guard. (It does — see
   `Meetings.module.css:111`. ✅ already.)
2. **Keyboard focus order in MeetingDetail.** Audit: tabs → metadata
   skip → action buttons → LLM panel. Currently the action buttons
   render *above* the tabs in DOM order, so tab-order is: copy →
   export → delete → tab buttons → llm panel. Decide if that's
   correct (probably yes — primary actions first), and add explicit
   `tabIndex` ordering only if the audit shows a problem. Likely
   no-op.
3. **ARIA labels on Copy/Export buttons.** Already set via
   `ariaLabel` prop on `<Button>`. Audit: confirm the `Button`
   primitive maps `ariaLabel` to `aria-label` (it does — see
   `ui/src/components/primitives.tsx`). ✅ already.
4. **Live region for record bar status.** Already done
   (`aria-live="polite"` on `.recordStatus`). ✅ already.
5. **Focus the source picker when the overlay opens.** New: when the
   overlay's CHOOSE mode mounts, programmatically focus the source
   `<select>` so keyboard users don't have to tab in. Use a `ref` +
   `useEffect(() => ref.current?.focus(), [])`. Single change.
6. **Escape closes overlay reliably.** Already done in
   `MeetingOverlay.tsx::onKey`. ✅ already.

#### Test specs (1 vitest test)

| file | name | inputs | expected |
|---|---|---|---|
| `ui/src/meeting_overlay/MeetingOverlay.test.tsx` (NEW) | `focuses_source_select_on_mount` | Render `<MeetingOverlay />` in CHOOSE mode | `document.activeElement === source-select-element` |

Other a11y items are audit-only, no new tests (the pre-existing
behaviour already satisfies them).

---

### 5.6 (Lateral) Rust-side overlay show/hide wiring (carry-forward
from Wave 4 §5.5 deviation)

**Files:** `meetings/lifecycle.rs` (or wherever `MeetingHotkeyInstaller`'s
`MeetingToggle` arrives in the runtime).

#### What's missing

Wave 4 declared the `meeting_overlay` window with `visible: false`.
The React side listens for a `meeting:overlay-open` event to flip
into CHOOSE mode + (optionally) self-show via `getCurrentWindow().
show()`. **Nothing on the Rust side emits that event yet.**

Wave 5 wires:

```rust
// meetings/lifecycle.rs or runtime.rs activation handler
ActivationEvent::MeetingToggle => {
    // If a meeting's in flight already, the existing start_meeting
    // idempotency path handles it. If we're idle, show the overlay
    // first so the user can pick a source; don't auto-start.
    if !is_meeting_in_flight() {
        if let Some(w) = app_handle.get_webview_window("meeting_overlay") {
            // Position the overlay near the cursor (center for now;
            // Wave 6 polish can revisit). Show without stealing focus
            // (focus:false in tauri.conf.json handles this).
            let _ = w.show();
            let _ = app_handle.emit("meeting:overlay-open", ());
        }
    } else {
        // A meeting is recording; the chord is the "stop" trigger.
        // We could either: (a) emit overlay-open and let the React
        // side render its Recording chip, or (b) directly call
        // stop_meeting. Master plan says push-to-start, push-to-stop
        // — go with (b) for parity with the manual-stop UX.
        let uuid = current_in_flight_uuid();
        let _ = runtime.stop_meeting(uuid);
    }
}
```

The cancel button in CHOOSE mode hides the window. The recording
chip's Stop button calls `meeting_stop`; the React side already
self-hides on `meeting:state == done`.

#### Test specs (1 wired test, `#[ignore]`-gateable)

| file | name | inputs | expected |
|---|---|---|---|
| `meetings/lifecycle.rs::tests` | `chord_idle_shows_overlay_window` | Mock app handle; inject `MeetingToggle` while `in_flight.is_none()` | The overlay window's `show()` was called once + `meeting:overlay-open` event emitted (assert via test-side event sink) |

If `tauri::test::mock_app()` can't be coaxed into providing a
named webview window for the test, gate `#[ignore = "tauri mock-
app doesn't surface named windows; covered by QA matrix"]` and
defer to the QA matrix scenario 1 (mic-only meeting via chord).

---

## Wave 6 brief (write at end of Wave 5)

End of Wave 5 → author `docs/phases/phase-mc-wave6-brief.md`. Wave
6's deliverables (from master plan):

* 5 judge cards + JSON entries
* Retrospective in `docs/LESSONS.md`
* STATUS.md update + bd close all Phase-MC issues
* Cargo gate green → seal commit → `git tag phase-mc-complete`
* Close `mb-2bi`

The Wave 6 brief mostly pins:
* Judge prompt text per the 5 judges (master plan §Wave 6)
* Verification commands for each judge (mirror Phase 4 judges' pattern)
* The `mb-2bi` close-out reason (Phase MC's ADR 0028+0029 closes it)

---

## Deviations from `phase-meeting-capture.md` (justified)

1. **New `SettingKey::MeetingHotkeyPaused` added in §5.1.1.** The
   master plan doesn't list this variant explicitly, but it implies
   one (the tray toggle must persist across restarts). Single new
   key, defaults to `false`, follows the same `every-key-round-trips`
   test pattern as the Wave 1 batch.
2. **New `meeting_set_paused` + `meeting_is_paused` IPC commands.**
   The master plan listed 10 commands in Section MC.6; this brings
   the total to 12. The two new commands are tiny wrappers around
   `MeetingCaptureRuntime` methods — same shape as the existing 10.
   Not surfacing them in MC.6 would force the tray to call into the
   runtime via a backdoor, which violates the IPC-as-trust-boundary
   convention.
3. **`tauri-plugin-dialog` added as a NEW dep in §5.3.** Tauri 2
   moved dialogs out of core. The crate is in the Tauri org and
   appears on the known-good list as of `phase-4-complete`. If the
   Mini Shai-Hulud IOC list (PLAN Appendix D) flags it at Wave 5
   author-time, stop and surface the conflict — there is no
   alternative path for native save-as on Tauri 2.x.
4. **`meeting_export_markdown` return-type tightens from
   `{ path: string }` to `{ path: string | null }`.** Required by
   the user-cancelled-dialog case. Existing callers (Wave 4) get
   `null` only when they pass `promptUserForPath: true`; default-
   path callers always get `string`. Effectively backwards-
   compatible for programmatic callers.
5. **Overlay show/hide wiring is Wave 5, not Wave 4.** Wave 4
   declared the window `visible: false`; Wave 5 wires the chord
   handler to show it. The Wave 4 author noted this in `e1bb92c`'s
   commit message ("Wave 5 wires the Rust activation hook to .show()
   this window on chord").

---

## Cargo gate (must be green at Wave 5 seal)

```pwsh
cd src-tauri
powershell -File scripts\cargo-with-cuda.ps1 check --all-targets
powershell -File scripts\cargo-with-cuda.ps1 clippy --release --all-targets -- -D warnings
powershell -File scripts\cargo-with-cuda.ps1 test --release --no-run   # full link;
                                                                       # --no-run is the LESSONS 2026-05-17 fallback for 0xC0000139
powershell -File scripts\cargo-with-cuda.ps1 fmt --check
```

All four must come back clean (zero errors, zero warnings).

For the React surface:

```pwsh
cd ui
npx tsc --noEmit          # type-check
npm test                  # vitest unit suite (expecting +8 to +15 new)
npm run build             # vite production bundle
# npm run lint stays broken pending mb-yxh (ESLint v9 config migration).
```

---

## Brief checklist (post-wave-5 author updates this section)

Before declaring Wave 5 sealed:

- [ ] §5.1 Tray pause-meeting toggle landed; 4 tests added; `SettingKey::MeetingHotkeyPaused` round-trips
- [ ] §5.2 Settings UI tab landed; 5 vitest tests added
- [ ] §5.3 `tauri-plugin-dialog` added; `meeting_export_markdown` save-as wired; 3 tests added
- [ ] §5.4 `meeting:progress` chip renders in record bar; 2 vitest tests added
- [ ] §5.5 a11y audit complete; overlay autofocus landed; 1 vitest test added
- [ ] §5.6 Rust-side overlay show/hide wired on `MeetingToggle`; 1 test added (or `#[ignore]`'d with rationale)
- [ ] Cargo gate clean (check + clippy --release -D warnings + test --release --no-run + fmt --check)
- [ ] UI gate clean (tsc --noEmit + vitest + vite build)
- [ ] `bd close` for all `mb-pdv.*` Wave 5 task IDs
- [ ] Author `docs/phases/phase-mc-wave6-brief.md` for the W5→W6 handoff
- [ ] STATUS.md anchor block updated with Wave 5 completion line

---

## Wave 6 judge prep (read-ahead so Wave 5 doesn't paint into a corner)

Wave 6 lands 5 judges. Wave 5's deliverables must keep these
judge-checkable:

| judge | Wave-5 implication |
|---|---|
| `mc-formatter-deterministic` | The Settings UI exposes `MeetingFillerStripEnabled` + `MeetingParagraphGapMs` + speaker labels. These flow into `formatter::format` via the formatter's existing config arg. The judge asserts byte-identical output for the same `(transcript, config)` pair; the UI change does NOT alter that contract — it just plumbs config values from the DB. |
| `mc-long-form-stitched-losslessly` | Untouched by Wave 5. |
| `mc-two-channel-merged` | Untouched by Wave 5. The speaker labels are pre-pended in the formatter; the merge logic doesn't change. |
| `mc-no-llm-in-critical-path` | §5.3's save-as wiring goes through the existing `meeting_export_markdown` command which does NOT call into Ollama. §5.1's pause-toggle path is also LLM-free. §5.6's overlay show/hide is LLM-free. Wave 6's runtime-counter instrumentation will assert this. |
| `mc-dictation-untouched` | §5.1's tray change adds a NEW menu item; it does NOT modify the existing "Pause" (dictation pause) handler. §5.2's Settings tab adds new variants only; the existing dictation settings keys round-trip unchanged. §5.6's overlay-show wiring is in `meetings/`; no edits to `dictation/`. Audit the diff carefully — judge will read `git diff phase-4-complete..HEAD --stat` and complain if it sees `dictation/` lines. |

---

*Brief authored by code-puppy (`code-puppy-b14c19`) on 2026-05-21,
end-of-Wave-4 iteration. Master plan version: `docs/phases/phase-
meeting-capture.md` as of post-`phase-4-complete` tag.*
