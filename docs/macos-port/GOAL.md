# ACTIVE GOAL — mac-v1

Persistent objective for the `macos-port` branch. This box has no
`~/.code_puppy/goals.json` store (the Wiggum `/goal` plugin is runtime,
not file-persisted), so this doc + the pin at the top of `STATUS.md` are
the durable goal anchor. Installed by Phase 0 Leaf 0.6 (`code-puppy-edf086`,
2026-06-19). Verbatim text from repo-kennel drawer 48/46.

```
goal: mac-v1
description: |
  Ship Windows-parity macOS v1 of Mockingbird on the macos-port branch.
  Scope = dictation (Phase 3) + meeting capture (Phase 4).
  Out of scope = knowledge-graph / activity (deferred).
  Hard rules:
    1. Never push to main from this Mac. Push only to origin/macos-port.
    2. Never edit *::windows.rs modules; their Windows-cfg gates must stay intact.
    3. Never swap STT off whisper-rs/metal to a Mac-native engine.
    4. Every chunk of work has a bead; every bead has a judge; every judge
       must be green before the bead closes.
    5. File size hard limit 600 lines; cargo fmt + clippy -D warnings is law.
    6. If 5 attempts on the same problem make no progress: STOP, write an ADR
       draft, escalate. Do not push to 10.
acceptance:
  - judge `mac-v1-dictation-e2e` green
  - judge `mac-v1-meeting-capture-e2e` green
  - judge `mac-v1-parity-whisper-metal` green (Phase 5)
  - all phase milestone beads closed
  - origin/macos-port reflects all work
```

## Phase ordering

`1 → 2 → {3, 4} → 5`. Phase 3 (dictation) is PRIORITY 1; Phase 4
(meeting capture) is PRIORITY 2 and the deepest slice.

## Named judges (seeded Phase 0)

All 21 `mac-*` judge IDs live in `~/.code_puppy/judges.json` (merged from
`.code_puppy/judges-template-macos.json` via `scripts/dev/seed-judges.sh`).
Three are deterministic wiggum hooks (`mac-branch-discipline`,
`mac-windows-modules-untouched`, `mac-whisper-feature-locked`); the rest are
probe / manual / composite judges gating their respective phase beads.
