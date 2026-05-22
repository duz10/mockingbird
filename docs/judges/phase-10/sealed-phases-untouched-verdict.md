# sealed-phases-untouched — Verdict

**Judge:** [`sealed-phases-untouched.md`](./sealed-phases-untouched.md)
**Diff range:** `stable-alpha-v0.1..HEAD` (narrowed per Wave 6.B per
Dustin; mechanically excludes the dictation-polish lateral epic
`dda676a` + the MC v1.2 capability migration `f298a5d` that landed
between `phase-mc-complete` and the Phase 10 first commit).
**Pre-fix base commit:** `95e57cd` (Wave 6.A).
**Graded by:** code-puppy-c7291b (Bernard), Wave 6.B Iteration 1.
**Verdict:** 🟢 **PASS**

---

## Mechanical layer (dry-run rig)

All four mechanical criteria are 🟢 GREEN in
`scripts\dry-run-phase10-judges.ps1`:

| Criterion | Result |
|---|---|
| C1 — Dictation public-API surface diff | empty |
| C2 — Meeting Capture pipeline shape diff | empty |
| C3 — `stage='raw'` UPDATE introductions | none |
| C4 — Migrations 001-014 modifications since `stable-alpha-v0.1` (`--diff-filter=M`) | none |
| C5 — Link surface (`cargo test --release --no-run`) | clean (12 test targets) |

## LLM-grader layer (this verdict file)

**Q1 — Is the Dictation module's public API surface unchanged?**

Empty diff across:
- `src-tauri/src/dictation/*`
- `src-tauri/src/injection/*`
- `src-tauri/src/cleanup/provider.rs`,
  `cleanup/llm_cleaner.rs`, `cleanup/ollama.rs`

**Answer:** YES. No file in any of those paths was modified between
`stable-alpha-v0.1` and `HEAD`. The Dictation FSM, paste-payload
sealing, injection pipeline (clipboard guard / IME / secure-input
guard), and the LLM-cleanup provider trait + Ollama implementation
are byte-for-byte identical to the stable-alpha-v0.1 baseline.

**Q2 — Has the Meeting Capture pipeline shape changed?**

Empty diff across `meetings/capture.rs`, `long_form_stt.rs`,
`formatter.rs`, `merge.rs`, `chunker.rs`, `filler_words.rs`, and
`audio/capture.rs`.

The **one** change inside `src-tauri/src/meetings/` is to
`runtime.rs` (~97 lines added), which adds:

1. The constant `LEGACY_CHORD_MIGRATION_MARKER_KEY` and the helper
   `migrate_legacy_meeting_chord_flag_once(&Connection)`.
2. A guard around `MeetingHotkeyInstaller::install(...)` that
   consults `SettingKey::LegacyMeetingChordEnabled` and either
   installs the chord (legacy users) or skips it (new installs,
   reaching Meeting capture via Command Center).
3. Plumbing to make `self.hotkey` an `Option<HotkeyInstaller>`
   instead of a required field.

**Answer:** YES, the pipeline shape is preserved. The chord-install
branch change is **pre-authorized by ADR 0037 §Q5** (Phase 10
Wave 1A, Command-Center entry point) and does not touch capture,
chunking, STT, two-channel merge, formatter, or audio capture.
The activation thread + `PauseToggle` injection path still run for
programmatic `start_meeting()` / `cancel_meeting()` callers
(Command Center), so Meeting capture itself remains a black-box
identical to MC v1.2 from the perspective of any downstream caller.

**Q3 — `transcripts` table raw-immutability (Principle 1)?**

`git diff stable-alpha-v0.1..HEAD -- src-tauri/src/` piped through
`Select-String 'UPDATE transcripts|UPDATE .* stage'` returns
zero hits. Raw rows are still write-once. The
`block-mutate-raw-transcripts` hook would have refused the
commit even if a regression were attempted.

**Answer:** YES, raw transcripts are immutable.

**Q4 — Migrations 001-014 byte-identical (no modifications)?**

`git diff --diff-filter=M --name-only stable-alpha-v0.1..HEAD -- ...001..014` is empty.

- Migrations 001-010: sealed at `phase-4-complete`.
- Migration 011: sealed at `phase-mc-complete`.
- Migrations 012, 013, 014: **added** during Phase 10 Waves 1B / 2 / 4
  (commits `7333a98`, `9155f40`, `e3f90db`); they show in the diff
  as additions but `--diff-filter=M` correctly excludes additions.
- Migration 015 (Wave 5 / ADR 0043 built-in exclusion rules) is
  also an addition during Phase 10 — out of scope for the
  001-014 seal check.

The `block-sealed-migration-edits` hook is the belt-and-braces
backstop for this check.

**Answer:** YES, the 14 sealed migrations are modification-free.

## Out-of-scope-by-design (informational)

The following files DID change in `stable-alpha-v0.1..HEAD`, but
none of them are part of Phase 10's seal-protected surface — they
are Phase 10's *integration points* and are pre-authorized by the
chartering ADRs (0036 / 0037 / 0040 / 0041 / 0042 / 0043 / 0044):

| File | Authorization |
|---|---|
| `src-tauri/src/commands/mod.rs` | New `activity_*` IPC commands (ADR 0036 §IPC, ADR 0044 §PDF export, ADR 0042 §retention IPC). |
| `src-tauri/src/db/migrations.rs` | Registry growth: migrations 012-015 wired in. ADR 0036 (012), Wave 2 (013), Wave 4 (014), ADR 0043 (015). |
| `src-tauri/src/error.rs` | New `AppError` variants: `ActivityPersist`, `ActivityCapture`, `ActivityAudio`, etc. ADR 0036 + Wave-level. |
| `src-tauri/src/lib.rs` | App-setup additions: `crash_recovery::recover_all` (ADR 0036 §boot recovery), `ActivityCaptureRuntime::spawn` (ADR 0036). |
| `src-tauri/src/recording_window.rs` | Command Center window registration (ADR 0037). |
| `src-tauri/src/settings/model.rs` | New `SettingKey` variants: `LegacyMeetingChordEnabled` (ADR 0037), `ActivityCaptureEnabled` + audio policy (ADR 0036), retention TTLs (ADR 0042). |
| `src-tauri/src/tray.rs` | New tray menu items wiring Activity Capture + Command Center (ADR 0037). |

None of these files appear in the seal-protected lists C1 or C2.
They are Phase 10's authorized footprint by construction.

---

## Final verdict

🟢 **PASS** — Phase 10 stayed within ADR 0037's authorization
boundary. Dictation and Meeting Capture remain sealed; raw
transcripts remain immutable; the 14 sealed migrations are
modification-free. The one in-`meetings/` change is the
pre-authorized ADR 0037 §Q5 chord-gating; everything else lives
outside the protected surface.
