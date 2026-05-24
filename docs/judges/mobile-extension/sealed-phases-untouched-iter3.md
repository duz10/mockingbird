# Judge: sealed-phases-untouched (ADR 0046 Iter 3)

**Target:** the **diff** between commit `2b5583d` (ADR 0046 Iter 2 SEAL
anchor — outbound projection complete; inbound side untouched) and
`HEAD` (Iter 3 implementation: Wave 0 spike docs + Phase 0 cleanup +
Waves 3.1 / 3.2 / 3.3).

**Question:** Did ADR 0046's Iteration 3 implementation stay inside the
authorization boundary defined by ADR 0046 §3 (sealed Phase-3
dictation untouched), §3.2 (the `HeadlessIngestRequest` channel
amendment is *consumed*, not re-shaped), and the Iter 2 vault
subsystem boundary (outbound surface is left as-is)?

This judge is the structural analog of Iter 1's
[`sealed-phases-untouched.md`](./sealed-phases-untouched.md). Iter 3
follows the same posture as Iter 1: a tightly scoped, ADR-named
extension into the dictation persist-event surface — but at a much
greater arm's-length than Iter 1, because Iter 1 already shipped the
clean `HeadlessIngestRequest` channel + ingest helper that Iter 3 only
needs to *call*.

The new top-level `src-tauri/src/inbox/` subsystem is the centerpiece
of Iter 3. The kickoff's discovery note documents the design choice:
"Inbound and outbound vault flows are conceptually independent (only
the path is shared); keeping them separate keeps the export-job's
`VaultRuntime` and the inbox's `InboxRuntime` as parallel peers driven
by the same settings gate." This judge mechanically verifies that
parallel-peer structure was respected.

## Authorization boundary — Iter 3

### Authorized edits

**New top-level subsystem (every file is a clean add):**

| Path | Authorization |
|---|---|
| `src-tauri/src/inbox/mod.rs`     | Iter 3 / mb-9lgi — module declarations + re-exports |
| `src-tauri/src/inbox/watcher.rs` | Iter 3 / mb-9lgi — `notify-debouncer-full` wrapper + size-stability state machine (Wave 0 Finding 3) |
| `src-tauri/src/inbox/courier.rs` | Iter 3 / mb-txmy — symphonia decode → `HeadlessIngestRequest` enqueue → atomic archive on success / fail-route on error |
| `src-tauri/src/inbox/runtime.rs` | Iter 3 / mb-3ivf — `InboxRuntime` settings-gated lifecycle (parallel peer to `VaultRuntime`) |

**Surgical edits to existing files (every hunk must trace to a named authorization):**

| File | Lines | Authorization |
|---|---|---|
| `src-tauri/src/lib.rs` | +24/-2 | (a) `pub mod inbox;` declaration. (b) `InboxRuntime::new(headless_ingest_tx)` construction parallel to `VaultRuntime`. (c) `app.manage(Arc<InboxRuntime>)` registration. (d) Hoist of `runtime.headless_ingest_sender()` out of the prior single-line `app.manage` so it can be cloned for the inbox path. |
| `src-tauri/src/commands/vault.rs` | +7 | `vault_settings_set` IPC also drives `inbox.refresh_config()` because the InboxRuntime is gated by the SAME `MobileSyncEnabled` + `VaultPath` keys as `VaultRuntime`. Mirror reconciliation pattern, no new IPC. |
| `Cargo.toml`, `src-tauri/Cargo.toml` | +10 / +8 | `notify-debouncer-full = "0.3"` (workspace + crate). ADR §6 mandate over raw `notify`; rationale tied to Wave 0 spike Finding 3. |
| `Cargo.lock` | +182 | Dependency resolution cascade. |
| `.gitignore` | +3 | Whitelist `docs/spikes/**/*.log` for the committed JSONL evidence backing the findings doc. |

**Docs / status / bead-DB (always-authorized):**

| File | Authorization |
|---|---|
| `STATUS.md`, `docs/PRODUCT-STATE.md`, `docs/LESSONS.md` | Per-iteration bookkeeping |
| `docs/historical/2026-05-23-mobile-extension-original-kickoff.md` | Phase 0 cleanup move (commit `d010bdf`); original kickoff doc relocated under `docs/historical/` with supersession preamble |
| `docs/mobile/ios-shortcut.md` | Iter 3 user-facing setup doc (already shipped in Iter 2 in skeleton form; amended in Iter 3 Wave 0 per Finding 5 with the Action 3 `Open App: Obsidian` mitigation) |
| `docs/spikes/iter3-sync-layer-{observation-plan,findings}.md` | Wave 0 spike artifacts |
| `docs/spikes/iter3-logs/round-{1,2,3,4,4b}*.log` | Wave 0 spike JSONL log fixtures |
| `scripts/watch-vault.ps1` | Wave 0 spike helper script (lives under `scripts/`, not `src-tauri/src/`) |
| `.beads/issues.jsonl`, `.beads/interactions.jsonl` | Bead-DB updates (closes for `mb-9lgi`, `mb-txmy`, `mb-3ivf`, `mb-s8s2`, etc.) |

### Forbidden edits — must be byte-identical

**Entire subsystems (zero diff allowed under these paths):**

- `src-tauri/src/dictation.rs` — Iter 1's §3 / §3.2 carve-out is sealed.
  Iter 3 only *consumes* the `HeadlessIngestRequest` channel; it does
  not modify the orchestrator's `run()` loop, `handle_headless`,
  `complete()`, `discard()`, or any of the persist helpers.
- `src-tauri/src/dictation/ingest.rs` — Iter 3's courier USES
  `headless_ingest()` from outside; no internal changes.
- `src-tauri/src/dictation/ingest_channel.rs` — `HeadlessIngestRequest`
  schema must not have changed.
- `src-tauri/src/dictation/events.rs` — `SessionsEventBus` from Iter 1
  stays as-is.
- `src-tauri/src/dictation/runtime.rs` — `headless_ingest_sender()`
  accessor from Iter 1's §3.2 amendment is *called* by `lib.rs`, not
  modified.
- `src-tauri/src/vault/**` — Iter 2's outbound subsystem. Iter 3 may
  *not* edit `vault/mod.rs`, `vault/layout.rs`, `vault/manifest.rs`,
  `vault/project.rs`, `vault/export_job.rs`, or any other vault module
  (the kickoff allowed a trivial mod-line addition; in practice the
  implementation needed none).
- `src-tauri/src/recording_window.rs` — Iter 1's `SessionsEventBus`
  adapter (in `dictation/events.rs`, not here) is untouched.
- `src-tauri/src/hotkey/**`
- `src-tauri/src/meetings/**`
- `src-tauri/src/activity/**`
- `src-tauri/src/injection/**`
- `src-tauri/src/window_context/**`
- `src-tauri/src/secrets/**`
- `src-tauri/src/stt/**`
- `src-tauri/src/cleanup/**`

**Permanent sealed surfaces:**

- Migrations 001-018 (all `.sql` files) — modification-free. **No new
  migration in Iter 3** (the courier ingest path reuses migration 018's
  `sessions.source` column from Iter 1, writing `'mobile-inbox'`).
- `transcripts(stage='raw')` — no new `UPDATE` statements.

## The judge's task

1. Read the diff (`git diff 2b5583d..HEAD`).
2. Classify each touched path as **AUTHORIZED** (in the tables above)
   or **FORBIDDEN** (not in any table OR matches a forbidden path).
3. Output a verdict in the format below.

## Mechanical sanity-checks (supplement the LLM grader)

Run these and report results alongside the LLM verdict. ANY non-empty
output for M1-M4 is a flag for investigation:

1. **All forbidden subsystems show zero diff:**
   ```powershell
   git diff --stat 2b5583d..HEAD -- `
     src-tauri/src/dictation `
     src-tauri/src/hotkey `
     src-tauri/src/meetings `
     src-tauri/src/activity `
     src-tauri/src/injection `
     src-tauri/src/window_context `
     src-tauri/src/secrets `
     src-tauri/src/stt `
     src-tauri/src/cleanup `
     src-tauri/src/recording_window.rs `
     src-tauri/src/db/migrations
   ```
   Expected: empty.

2. **Iter 2 vault subsystem is byte-identical:**
   ```powershell
   git diff --stat 2b5583d..HEAD -- src-tauri/src/vault
   ```
   Expected: empty (or at most trivial mod-line additions in
   `vault/mod.rs`).

3. **`HeadlessIngestRequest` channel + dictation modules byte-identical:**
   ```powershell
   git diff --stat 2b5583d..HEAD -- src-tauri/src/dictation
   ```
   Expected: empty. (Iter 3's courier *consumes* the Iter 1 channel
   without modifying its definition or its handler.)

4. **No `UPDATE` against `stage='raw'`:**
   ```powershell
   git diff 2b5583d..HEAD -- src-tauri/src/ | `
     Select-String -Pattern 'UPDATE transcripts|UPDATE .* stage'
   ```
   Expected: empty (or matches only in comments / `stage='cleaned'`).

5. **Link surface clean** (LESSONS P2 fallback — live exec blocked on
   this box): the standard end-of-iteration gate
   (`cargo-with-cuda.ps1 check` + `clippy --release -- -D warnings`)
   should already have run during Wave 3.3 SEAL. The kickoff for this
   judge dispatch is "docs + bd state only — no cargo or UI gate"; the
   prior Wave 3.3 commit (`eea2484`) is the cargo proof.

## Verdict format

```
## Verdict: PASS | FAIL
## Authorized edits observed: <list>
## Forbidden edits observed: <list, ideally empty>
## Reasoning: <2-4 paragraphs explaining the classification of each
              touched file, with emphasis on (a) the new inbox/
              subsystem fitting the parallel-peer pattern next to
              vault/, (b) lib.rs + commands/vault.rs surgical edits
              tracing to authorized boundaries, (c) Iter 2's vault
              subsystem and Iter 1's dictation channel both being
              consumed-not-modified>
## Confidence: <0-100%>
```

## On failure

- **DO NOT close `mb-ksau`.**
- **DO NOT proceed to close `mb-1yxp` or commit the verdict.**
- Stop and surface the specific unauthorized edit(s). The Iter 3
  implementation either needs a rollback OR an ADR amendment to
  authorize the unforeseen edit.
- If criterion 1 (forbidden subsystems) trips: revert the cross-cut
  edit. Iter 3 explicitly excludes meetings / activity / injection /
  hotkey / secure-input / recording-window / stt / cleanup changes.
- If criterion 2 (vault subsystem) trips: Iter 2 is sealed; Iter 3
  must not reshape it. File a successor ADR or revert.
- If criterion 3 (dictation modules) trips: the `HeadlessIngestRequest`
  channel and its handler are Iter 1's frozen surface. Reshaping them
  here would invalidate Iter 1's seal.
- If criterion 4 (raw transcripts) trips: rip out the `UPDATE` and
  append a new `stage='cleaned'` row instead. The
  `block-mutate-raw-transcripts` hook should also have caught it at
  write time.

## Cross-references

- **Iter 1 judge:** [`sealed-phases-untouched.md`](./sealed-phases-untouched.md)
- **Iter 1 verdict:** [`sealed-phases-untouched-iter1-verdict.md`](./sealed-phases-untouched-iter1-verdict.md)
- **Structural ancestor:** `docs/judges/phase-10/sealed-phases-untouched.md`
- **Chartering ADR:** [`docs/adr/0046-mobile-extension-via-vault.md`](../../adr/0046-mobile-extension-via-vault.md)
  (§3 + §3.2 + §6 are the load-bearing sections)
- **Binding principles touched by the boundary:** Principle 3 (layers
  are replaceable) and the `phase-{N}-complete` tag convention.
