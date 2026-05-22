# ADR-0037: Unified Recording Command Center

- **Status:** Proposed
- **Date:** 2026-05-25
- **Deciders:** Dustin (project lead), Bernard / code-puppy (chartering)
- **Charter for:** Phase 10 Wave 1A — Unified Recording Command Center (inserted before Wave 1B Activity-Log Skeleton; numbered phase's first wave seals at `phase-10-wave-1a-complete`).
- **Sibling ADR:** [ADR 0036](0036-activity-capture-sibling-subsystem.md) — Activity Capture sibling-subsystem charter. 0037 is the explicit authorization to make surgical edits to sealed Dictation + Meeting Capture surfaces that 0036's "sealed modules stay sealed" rule would otherwise forbid; this authorization is scoped to the boundary listed below.
- **Companion source flag:** Bernard's Wave 0 post-iteration summary item #1 — "The third recording overlay is now real." This ADR resolves the YAGNI debt ADR 0026 parked there and that flag re-surfaced.

## Context

Phase 10 Wave 0 (commit `613f336`) chartered Activity Capture as a third top-level subsystem under ADR 0036. The wave-0 plan had each subsystem own its own recording overlay window:

- **Dictation** — the existing `recording_window` pip (default top-of-screen area, ADR 0016 § no-focus-theft).
- **Meeting Capture** — the `meeting_overlay` window (mid-screen, larger; ADR 0026 + ADR 0033/0034/0035).
- **Activity Capture (planned)** — a new `recording_indicator` pip at top-right.

Three overlays, three positions, three lifecycle owners, three sets of "what's currently being recorded?" affordances. Bernard's own Wave 0 review explicitly flagged this as a UX smell ("The third recording overlay is now real ... Phase MC's ADR 0026 explicitly said 'YAGNI a shared `WindowConventions` helper until window #3' — Phase 10 is window #3"). Dustin's review confirmed: three overlays is the wrong design. The user's mental model is "I want to start recording something," and the three modes are configuration of that one act, not three unrelated features.

The corrective design — agreed by Bernard ↔ Dustin in the Wave 0.5 planning round — is a **Unified Recording Command Center**: one bottom-center overlay that opens via chord (or tray), shows a mode picker (Dictation / Meeting / Activity), and is the single surface across all three subsystems. The Right Alt push-to-talk dictation fast path stays exactly as it is today (ADR 0017 / ADR 0018) — keyboard-fluent users lose nothing. The Command Center is the entry point for everyone else, and the **only** entry point for Meeting + Activity capture.

### Why bottom-center

- **Implicit mutual exclusion.** Only one overlay can occupy that screen slot at a time. If dictation's pip moves there, and the meeting overlay opens there, and the Command Center opens there, then by geometry alone the system can never have two overlapping "recording" UIs. This is stronger than the z-order discipline ADR 0034 had to enforce post-MC, because it doesn't require any code to enforce — it's enforced by the screen.
- **Out of the way of normal work.** Top-of-screen overlays collide with browser tab strips, IDE tabs, app titlebars; right-side overlays collide with Windows notification toast position. Bottom-center is the least-claimed strip on a normal Windows display, and the strip the user looks at least often when actively typing.
- **Symmetry with platform UX patterns.** Spotlight (macOS) opens mid-screen on `Cmd + Space`; Windows Run opens at `Win + R`; many command palettes (VS Code's `Ctrl+Shift+P`, etc.) open mid-screen. Bottom-center is a sensible local variant — close enough to feel "command palette" without colliding with any existing system surface.

### Why this needs an ADR (not just a Wave 1A internal redesign)

ADR 0036 §Decision items 1 + 2 explicitly seal `dictation/`, `hotkey/`, `injection/`, `recording_window.rs`, `cleanup/provider.rs`, and all of `meetings/` (with the narrow `long_form_stt` library-reuse + `export` composition-only exceptions). The Command Center design requires surgical edits to several of those sealed files — there is no way to wire a unified entry point without touching the existing subsystems' invocation paths. The Permanently-Sealed rule in AGENTS.md is explicit that new work against sealed phases is a charter-an-ADR-first lateral epic, **even when the work is small**. This ADR is that authorization. Outside the boundary listed below, the seal still holds.

## Decision

**Build a Unified Recording Command Center as Phase 10 Wave 1A. It is the single keyboard- and mouse-accessible entry point to the three recording modes. It owns no recording logic itself — it dispatches to the existing Dictation, Meeting Capture, and (Wave 1B+) Activity Capture runtimes via their public start APIs. The seal on Dictation + Meeting Capture stays in force outside the boundary explicitly listed below.**

### 1. Architecture — new code (greenfield, no sealed-surface coupling)

```
src-tauri/src/command_center/
├── mod.rs           # Orchestrator + public API (start_open, on_mode_chosen, on_session_card_stop, on_dismiss).
│                    #   No recording logic — purely a dispatcher.
├── state.rs         # Pure-Rust state machine: Closed → Opening → ShowingModePicker
│                    #   → ShowingSessionCard{kind} → Launching{kind} → Closed.
│                    #   Inputs: ChordEvent, TrayClick, EscKey, ModePicked{kind},
│                    #   SessionCardStop, RuntimeReplied{ok|err}, FirstRunCompleted.
│                    #   Outputs: WindowAction (show/hide), RuntimeAction (start_kind | stop_kind),
│                    #   SettingsAction (mark command_center_seen_v1). Throwaway-crate testable.
└── hotkey.rs        # WH_KEYBOARD_LL hook on its own message-pump thread.
                     #   Always-CallNextHookEx per ADR 0027. Observes ONLY the configured
                     #   command_center_chord VK pair; suppresses Windows key-repeat until
                     #   main-keyup. Fourth global LL keyboard hook in the app.

ui/src/command_center.tsx                 # New Tauri window entry, mirrors recording.tsx
ui/src/command_center/
├── CommandCenter.tsx                     # Component: mode picker | session card | welcome variant
├── CommandCenter.module.css              # Bottom-center positioning, transparent backdrop,
                                          #   non-activating (no focus theft) per ADR 0016 §7
└── (no further subcomponents in Wave 1A — keep the surface flat; split later if a tile or
   the session card grows past the 600-line rule)

src-tauri/src/overlay_conventions.rs      # NEW shared helper. Closes ADR 0026's YAGNI debt.
                                          #   Owns: bottom-center monitor-pick math, the
                                          #   WS_EX_NOACTIVATE | WS_EX_LAYERED | WS_EX_TOPMOST
                                          #   fixup, and the "where exactly is bottom-center on
                                          #   THIS monitor accounting for taskbar" math. The
                                          #   meeting_overlay AND the dictation pip AND the new
                                          #   command_center all import from here. Neutral
                                          #   ground — does NOT live under meetings/ or
                                          #   dictation/, so neither subsystem owns it.
```

The orchestrator in `mod.rs` exposes:

```rust
pub fn open_via_chord();     // hotkey.rs calls this
pub fn open_via_tray();      // tray.rs calls this
pub fn open_via_first_run(); // boot path calls this if command_center_seen_v1 == false
pub fn pick_mode(kind: RecordingKind);
pub fn stop_active_session();
pub fn dismiss();
```

The state machine (in `state.rs`) is the only file that decides what those calls DO; `mod.rs` is glue. This is intentional — the state machine is the file that has to be airtight, and putting it behind a pure-Rust trait-free struct with no I/O makes it cheap to test exhaustively via the throwaway-crate recipe.

### 2. Window — bottom-center, non-activating, transparent

- Tauri window config in `tauri.conf.json` under a new `command_center` key.
- `decorations: false`, `transparent: true`, `alwaysOnTop: true`, `focus: false`, `skipTaskbar: true`, `resizable: false`.
- Initial size: ~480 × 340 (mode picker tall; session card swaps to a shorter ~480 × 180; first-run welcome adds a header band, tall ~480 × 420). Resize-on-content-swap is fine because there's no decoration to flicker.
- Positioning: bottom-center of the monitor where the focused window currently lives at chord-fire time. `overlay_conventions.rs` owns the math.
- Windows extended-style fixup mirrors `recording_window.rs`'s existing `WS_EX_NOACTIVATE | WS_EX_LAYERED | WS_EX_TOPMOST` block. **Now lives in `overlay_conventions.rs` so all three overlays share the implementation.**

### 3. Resolution of the six locked Qs

| # | Decision (verbatim from Wave 0.5 planning) | Implementation hook |
|---|---|---|
| Q1 | Chord: `Right Ctrl + Space`. Validate against ADR 0019 probe. If conflict, fall back to user-configurable setting (no separate ADR — it's just a Settings entry the user changes). | New `SettingKey::CommandCenterChord` (default `"RightCtrl+Space"`). Boot-time conflict probe per ADR 0019 ladder. `command_center/hotkey.rs` reads the live setting at install time + re-installs on change. |
| Q2 | Mutual-exclusion while a session is recording: open the Command Center, show a **SessionCard** with `kind`, `started_at`, and a Stop button. After Stop confirms, return to the mode picker (not auto-dismiss — user can immediately start a different mode). | State machine has a dedicated `ShowingSessionCard{kind}` state; transitions back to `ShowingModePicker` on `RuntimeReplied{ok}` from the stop call. |
| Q3 | Tray entry: yes. "Open Command Center" menu item in `tray.rs` near the top. | `tray.rs` gains one new menu item; click handler calls `command_center::open_via_tray()`. No accelerator string (Tauri-side limitation — the chord runs through our own hook). |
| Q4 | Legacy `Right Ctrl + .` meeting chord: user setting `legacy_meeting_chord_enabled` (bool). Default OFF for new users. **One-shot migration:** on boot, if `meeting_hotkey_chord` exists and is non-default, set `legacy_meeting_chord_enabled = true`. | New `SettingKey::LegacyMeetingChordEnabled` (default `false`). One-shot migration in `meetings/runtime.rs` boot path (mirrors ADR 0033 pattern, also in `meetings/runtime.rs`). Existing Settings → Meetings chord picker stays VISIBLE but disabled when the bool is OFF — so users see what the chord WOULD be. |
| Q5 | First-run: auto-open Command Center with "Welcome, pick a mode" header band above the tiles. Tracked via `command_center_seen_v1` (bool, default `false`; flips to `true` after first dismiss via any path: Esc, mode pick, tray close). | New `SettingKey::CommandCenterSeenV1`. Boot path: if `false`, call `command_center::open_via_first_run()` after main-window init. The Welcome variant is the same `CommandCenter.tsx` component with a top-of-window header band rendered conditionally — DRY, no parallel component. |
| Q6 | Sequencing: insert **Wave 1A** before existing Wave 1, which becomes **Wave 1B — Activity-Log Skeleton**. Cascade dependency links. | `phase10.md` re-numbered; bead `mb-hnl3` re-titled; new Wave 1A bead created and linked. Wave 1B's overlay sub-deliverable changes from "new standalone overlay" to "wires into Command Center mode picker (from Wave 1A); no standalone overlay." |

### 4. Mode picker & SessionCard UX

- **Mode picker (default state when no session is active):** three tiles. Vertical stack, top-to-bottom by frequency of use (Dictation, Meeting, Activity). Each tile:
  - Mode icon (small, top-left).
  - Mode name (bold).
  - One-line description (e.g. "Quick voice-to-text into the focused app").
  - Keyboard hint on the right (`↵` for Enter, `1` / `2` / `3` for direct-key activation, arrow-keys to navigate).
  - The Dictation tile shows the muted note "or just hold Right Alt" — making the existing fast-path discoverable without privileging it visually.
  - Esc dismisses; click outside the window also dismisses; mode pick dispatches via `command_center::pick_mode(kind)` and immediately hides the window.

- **SessionCard (state when ANY recording is already live):** one card.
  - Header: "Currently recording: Dictation / Meeting / Activity" (with mode icon).
  - Subtitle: elapsed time, live-updating at 1 Hz.
  - Big red "Stop Recording" button. Esc still works (dismisses the card, NOT the recording).
  - Below the button, smaller: "Wait — return to picker" (cancels SessionCard, leaves the recording alone, returns Command Center to closed state).
  - After Stop confirms: state machine transitions back to `ShowingModePicker` — does NOT auto-dismiss. User can immediately start a different mode if they want.

- **First-run Welcome variant:** identical mode picker, plus a header band above the three tiles:
  - "Welcome to Mockingbird" (h1).
  - One paragraph: "Three ways to record. Pick one to learn how it works."
  - The mode-picker tiles below behave exactly the same; the header band disappears on second open (i.e. once `command_center_seen_v1 = true`).

### 5. Boundary — what 0037 explicitly authorizes touching in sealed code

This is the explicit authorization for surgical edits Wave 1A will make to files that ADR 0036 §Decision items 1 + 2 sealed. Outside this list, the seal holds.

| File | Surgical change | Why it's surgical (not a redesign) |
|---|---|---|
| `src-tauri/src/recording_window.rs` | Relocate dictation pip default position to bottom-center (via `overlay_conventions.rs`). Add a `suppressed_for_command_center: bool` flag honored when the Command Center window is up. | Position move is a single coordinate calc; suppression flag is a single conditional in the show/hide path. No state-machine change. The 383 dictation tests stay green. |
| `src-tauri/src/meetings/hotkey_installer.rs` | Wrap the legacy-chord install in `if settings.legacy_meeting_chord_enabled { install() }`. | One conditional. The installer itself is unchanged. Migration (below) sets the bool on first boot for existing users. |
| `src-tauri/src/meetings/overlay.rs` | Add a public invocation entry point callable from `command_center::pick_mode(Meeting)`. **Keep the visual contract identical** — same window, same lifecycle, same z-order. Position move via `overlay_conventions.rs` is the only visual change. | New entry point, not a redesign of the overlay. The overlay-event-delivery hotfix work in ADR 0034 is preserved verbatim. |
| `src-tauri/src/meetings/runtime.rs` | Add one-shot legacy-chord migration in the boot path. Pattern verbatim from ADR 0033: read prior setting, write new bool, log INFO with prior + new values. | Already the established pattern; this is a literal copy of the same idiom into a new field. |
| `src-tauri/src/dictation.rs` / `src-tauri/src/dictation/runtime.rs` | Accept a "started-from-command-center" signal so the dictation pip can suppress itself (Command Center window already occupies bottom-center) and the dictation runtime can know to dismiss the Command Center on session start. | One additional parameter on the public start entry point; default value preserves the existing call-site behavior. The state machine doesn't change. |
| `src-tauri/src/settings/model.rs` + `src-tauri/src/commands/settings.rs` | Add `CommandCenterChord`, `CommandCenterSeenV1`, `LegacyMeetingChordEnabled` setting keys. | Additive only; existing keys untouched. |
| `ui/src/pages/Settings.tsx` + `ui/src/pages/SettingsMeetingTab.tsx` | Two new UI rows: General → "Command Center chord" (string picker); Meetings → "Enable direct chord shortcut" (toggle, with chord-picker disabled-but-visible underneath). | Additive UI rows; no existing field touched. |
| `ui/src/lib/i18n/keys/en.json` (or equivalent) | Three new copy strings. | Additive. |
| `src-tauri/src/tray.rs` | One new menu item: "Open Command Center". | Additive. |

**Anything not on this list stays sealed.** If Wave 1A discovers it needs a touch outside this list, that's a successor-ADR amendment to 0037 — not a unilateral expansion.

### 6. One-shot legacy-chord migration

Per Q4. Pattern verbatim from ADR 0033's chord-default-change migration. Run in `meetings/runtime.rs` boot path:

```rust
// Pseudo-code; real impl in Wave 1A.
fn run_legacy_chord_migration(settings: &SettingsRepo) -> AppResult<()> {
    let key = SettingKey::LegacyMeetingChordEnabled;
    if settings.has_been_set(key) {
        return Ok(()); // already migrated; idempotent
    }
    let prior_chord = settings.get_string(SettingKey::MeetingHotkeyChord)?;
    let is_non_default = prior_chord.as_deref()
        .map(|c| c != DEFAULT_MEETING_CHORD)
        .unwrap_or(false);
    let new_val = is_non_default;
    settings.set_bool(key, new_val)?;
    tracing::info!(
        target: "settings.migration",
        ?prior_chord, %new_val,
        "legacy_meeting_chord_enabled one-shot migration applied"
    );
    Ok(())
}
```

The misfire risk (Q4 con) is mitigated by:
1. The INFO log includes the prior chord value + the new bool, so a misfire is visible in the log.
2. The Settings → Meetings tab adds a **"Restore legacy chord behavior"** button that sets `legacy_meeting_chord_enabled = true` regardless of the migration outcome. One-click recovery for any user the migration misfires on. (Wave 1A deliverable; not a separate ADR.)

## Consequences

### Positive

- **Single mental model.** Users learn one chord (or click one tray item) to access all three recording modes. Activity Capture inherits this model from day one rather than being retrofitted post-hoc.
- **YAGNI debt resolved.** ADR 0026 parked the "extract `WindowConventions` helper" decision until window #3 existed. This IS window #3 — extracting now into `src-tauri/src/overlay_conventions.rs` (neutral ground, not under either sealed subsystem) means all three overlays share one implementation of the bottom-center math + the WS_EX_NOACTIVATE fixup. Future overlay #4 (if ever) trivially inherits.
- **Implicit mutual exclusion.** Bottom-center as the single overlay slot means no two recording UIs can visually overlap by geometry. No z-order discipline required; no "stuck overlay" class of bug possible (ADR 0034's class is structurally eliminated for the Command Center surface).
- **Tray discoverability.** Mouse-only / chord-shy users can reach all three modes without a keyboard. This is also the most accessible affordance for a user discovering the app via the tray icon.
- **Migration mirrors a proven pattern.** ADR 0033 already shipped a one-shot chord migration in the same file (`meetings/runtime.rs`) and we know it works. Doing the same thing the same way again is the cheapest correct option.
- **The dictation fast path is preserved verbatim.** Holding Right Alt to dictate still works exactly as it does today. Power users lose nothing.

### Negative

- **Six surgical touches in sealed subsystems.** Authorized by this ADR (boundary list above), but every one of those touches is a place a Wave 1A bug could regress dictation or meeting capture. Mitigation: Wave 1A's live OS smoke matrix (3 invocation paths × 4 current-session states = 12 cells minimum) is the gate; the 383 dictation tests + the 5 Phase MC judges must still pass at Wave 1A seal.
- **One-shot migration misfire risk.** If `meeting_hotkey_chord` was set to a value that *equals* the default-at-the-time-of-the-migration string but differs from the current default (e.g. ADR 0033 changed the default string mid-flight), the migration could either over- or under-fire. Mitigation: INFO log captures the prior value + new bool; "Restore legacy chord behavior" button gives users a one-click recovery. Documented in About / changelog so it's discoverable.
- **Live OS smoke testing burden grows.** Three invocation paths (chord / tray / first-run auto-open) × three modes × four current-session states (none / mid-dictation / mid-meeting / mid-activity) = 36-cell smoke matrix in theory; Wave 1A's brief reduces to the 12 cells that materially differ. Plus: every Wave 2-6 smoke run must include "open Command Center while mid-recording works correctly" as a check. Per LESSONS P7 this is the right cost — judges can't catch live-OS regressions, so the matrix has to grow.
- **Fourth global WH_KEYBOARD_LL hook.** Dictation's hook + Meeting Capture's hook + (Wave 1B planned) Activity's chord hook + (this ADR) Command Center's chord hook = four message-pump threads sitting in `GetMessageW`. Cost: still sub-microsecond per keystroke per pump. ADR 0027's "multiple LL hooks coexist iff each CallNextHookEx" is the binding rule; `command_center/hotkey.rs` complies.
- **The "what's currently happening?" model is now slightly more complex.** Before: each subsystem's overlay told its own story. After: the Command Center owns the "do you want to start something / are you already recording something?" model. State machine has to be airtight on this — the SessionCard must accurately reflect which kind is running, and the Stop button must actually stop the right one. Wave 1A test plan: state machine ≥30 unit tests via throwaway-crate covering every (current-session, user-action) pair.

### Neutral

- **No new top-level dependencies.** `command_center/` uses what's already in `Cargo.toml`. The new UI window uses the same React + Vite stack as `recording.tsx` and `meeting_overlay.tsx`.
- **Doc surface grows.** ADR 0037 + a likely Wave 1A brief (`docs/phases/phase10-wave1a-brief.md`) once Wave 1A is in flight, plus three new copy keys in i18n. Intentional — same pattern as Phase MC.
- **Phase 10's seal-tag scheme gains one tag.** `phase-10-wave-1a-complete` now exists in the scheme, then `phase-10-wave-1b-complete`, then 2/3/4/5/6. Final seal `phase-10-complete` unchanged.

## Alternatives considered

- **Skip the Command Center; keep three separate overlays as Wave 0 planned.** Rejected. Three overlays is what Bernard's own Wave 0 review flagged as a UX smell; Dustin agreed. The "ship now, unify later" path means future overlay refactors have to retrofit three subsystems instead of two — strictly more expensive.

- **Build the Command Center but NOT as a sibling — make it part of `dictation/` (the oldest subsystem).** Rejected. The Command Center is not dictation-shaped; it doesn't capture audio, doesn't paste, has no `modes` table dependency. Putting it under `dictation/` would re-litigate ADR 0026's "sibling vs. extension" decision. Greenfield `command_center/` is the same call as Phase MC's greenfield `meetings/`.

- **Put `overlay_conventions.rs` under `meetings/` (the most recent overlay author) or under `command_center/` (the new owner).** Rejected. Either choice makes one subsystem the owner of a helper the others must reach across module boundaries to use, which the cross-module-coupling hook would flag. Neutral ground at `src-tauri/src/overlay_conventions.rs` (top-level under `src/`) is the right call — it's a utility, not a subsystem.

- **Replace the meeting overlay AND the dictation pip with the Command Center.** Rejected. The meeting overlay's job during a long meeting is not "mode picking" — it's a persistent "you're recording, here's a Stop button" surface. Conflating that with the Command Center's "what do you want to start?" job would re-introduce the same "two jobs in one component" anti-pattern ADR 0026 rejected. SessionCard is the Command Center's response to "I want to interrupt the currently-running thing"; the existing meeting overlay continues to be the during-meeting surface.

- **Make `command_center_chord` enforce some specific format.** Rejected. The existing chord setting validation in `meetings/` already handles the parsing/normalization. Reuse the existing validator; don't reinvent.

- **Skip the first-run auto-open.** Rejected (Q5). The Command Center is the new "what does this app DO?" surface; not surfacing it on first run leaves new users staring at a tray icon with no clear path in. The `command_center_seen_v1` flag is a one-line implementation and recoverable from a Settings reset.

## Sub-decisions deferred

- **Chord conflict-probe ergonomics if `Right Ctrl + Space` collides on some keyboards.** No separate ADR. Defer to the user-configurable `command_center_chord` setting; if a probe reports a collision at boot, log a WARN + leave the default in place and let the user re-pick via Settings. This is the existing behavior for `meeting_hotkey_chord`; same pattern, same code path.

- **Whether the SessionCard's "Wait — return to picker" affordance should also surface for the keyboard-fluent path (e.g. Esc-once = return to picker, Esc-twice = dismiss).** Defer to Wave 1A live testing; if the affordance reads better as a single key, Wave 1A's brief can resolve it without a successor ADR.

- **Whether to add a fourth tile "Stop the currently-recording thing" when nothing is active (no-op tile) vs. only when something IS active (the SessionCard).** Resolved as the latter in this ADR (Q2); reopen only if Wave 1A testing surfaces a real ambiguity.

- **Localizing the Welcome header band.** v1 ships English-only per existing i18n state; deferred to whenever a real localization story lands (not Phase 10 scope).

## Non-Goals (this ADR)

- **Hyper-key emulation / Karabiner-style chord composition.** v1 supports a fixed two-key chord (`mod + key`) per ADR 0019. The setting key is a string for forward-compat, but the parser remains the same.
- **Per-app chord overrides.** The Command Center chord is global; the dictation hotkey is global; no per-app remapping.
- **Cross-platform key remapping for non-US keyboards.** v1 ships with the existing ADR 0019 VK constants. International keyboard layouts that don't have a `Right Ctrl` or `Space` in the expected positions will need to use the chord-picker — same as today.
- **Voice-activated command center invocation.** No "hey Mockingbird, open command center." We are not building a wake-word system. Ever.
- **Animated transitions between mode picker and SessionCard.** A simple crossfade is fine in Wave 1A if it's free; anything beyond that is YAGNI.
- **Drag-to-move on the Command Center window.** Bottom-center is fixed. If a user has a multi-monitor setup that makes bottom-center inconvenient, the chord opens on the monitor with the focused window — that's the only positioning intelligence we ship.

## Cargo gate (binding per LESSONS P2)

Phase 10's existing accepted fallback gate applies to Wave 1A:

- **Pure-Rust modules** (`command_center/state.rs`, `overlay_conventions.rs`'s pure math helpers, the legacy-chord migration's logic extracted into a testable function) → throwaway-crate recipe (LESSONS 2026-05-17).
- **Wired modules** (`command_center/mod.rs`, `command_center/hotkey.rs`, plus every sealed-surface touch enumerated in the Boundary table above) → cargo check + clippy `--release -- -D warnings` + `fmt --check` + `test --release --no-run` + the Wave-1A live OS smoke matrix.

No new gate proposed. The parallel investigation bead `mb-0n8c` is unchanged.

## Cross-references

- **Sibling ADR (this iteration):** ADR 0036 — Activity Capture sibling-subsystem charter. 0037 amends 0036's "sealed modules stay sealed" rule with the explicit boundary list above.
- **Pattern precedent:**
  - ADR 0026 — sibling-subsystem charter pattern. 0037 follows the same Context / Decision / Consequences shape and uses 0026's "two windows is right until window #3" reasoning to justify the `WindowConventions` extraction now.
  - ADR 0033 — one-shot chord-migration pattern. 0037's `legacy_meeting_chord_enabled` migration is a verbatim port of the idiom.
  - ADR 0027 — multiple WH_KEYBOARD_LL hooks coexisting. 0037's chord hook is the fourth installer; same CallNextHookEx discipline.
  - ADR 0034 — overlay event-delivery hotfix. 0037's SessionCard inherits the show-before-emit + `emit_to` defensive pattern.
- **Sealed surfaces refused for redesign:** all of `meetings/` except the four files explicitly named in the Boundary table; all of `dictation/` except the start-entry-point parameter addition; all of `injection/`; all of `hotkey/`; all of `cleanup/`. Migrations 001-012 untouched (Wave 1A adds setting keys via the existing `SettingsRepo` mechanism — no new migration needed).
- **LESSONS:**
  - PINNED P2 (cargo-test broken) — sets the test strategy.
  - PINNED P5 (ADR-vs-tag) — Wave 1A seals via `phase-10-wave-1a-complete` tag (per-wave tag inside a numbered phase, not a lateral epic).
  - PINNED P7 (judges-don't-catch-OS-regressions) — Wave 1A's live OS smoke matrix is the gate that catches what judges can't.
- **Beads:**
  - Parent epic: `mb-a2w9` (unchanged).
  - New Wave 1A bead: created this iteration, type `feature`, P1, blocks Wave 1B (`mb-hnl3` re-titled to "Phase 10 Wave 1B").
  - Wave 1B (`mb-hnl3`) re-titled in this iteration; now blocked-by the new Wave 1A bead.
  - Downstream beads (Wave 2-6) unchanged; their dep-chain cascades naturally.

---

_The `adr-format` judge validates this structure exists in every numbered ADR. Keep section headings stable._
