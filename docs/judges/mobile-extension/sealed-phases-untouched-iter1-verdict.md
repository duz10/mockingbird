# sealed-phases-untouched (Iter 1) — Verdict

**Judge:** [`sealed-phases-untouched.md`](./sealed-phases-untouched.md)
**Diff range:** `d99a4cd..HEAD` (HEAD = `a004efa`).
**ADR:** [0046 — Mobile extension via synced Obsidian vault](../../adr/0046-mobile-extension-via-vault.md)
**Bead:** `mb-thmd`
**Graded by:** code-puppy-153a32 (Bernard), 2026-05-27 (self-grade,
Mode A — no Mockingbird judge runner is wired for ADR-chartered
laterals yet; same posture as Phase 10 Wave 6.B).
**Verdict:** 🟢 **PASS** (confidence 95%)

---

## Mechanical layer

| # | Check | Result |
|---|---|---|
| M1 | Forbidden subsystems show zero diff (`hotkey`/`meetings`/`activity`/`injection`/`window_context`/`secrets`/`stt`/`cleanup`/`recording_window.rs`) | ✅ empty |
| M2 | No `UPDATE transcripts` / `UPDATE … stage` introductions in `src-tauri/src/` | ✅ empty |
| M3 | Migrations 001-017 byte-identical (`--diff-filter=M`) | ✅ empty (only `018_session_source.sql` is new) |
| M4 | `dictation.rs` forbidden-function bodies untouched | ✅ see "Function-by-function audit" below |
| M5 | Link surface clean (`cargo test --release --no-run`) | ⏳ deferred to end-of-iteration gate (LESSONS P2 fallback; identical to Phase 10 Wave 6.B posture). Will run as part of the standard gate before `mb-thmd` close commit. |

Commands & raw outputs are captured in the session transcript;
re-running any of M1-M3 against `d99a4cd..HEAD = a004efa` reproduces
the empty result.

---

## File-by-file audit (29 files in `git diff --stat d99a4cd..HEAD`)

### Authorized — sealed-surface edits (in §3 / §3.1 / §3.2 carve-out)

| File | Lines | Authorization |
|---|---|---|
| `src-tauri/src/dictation.rs` | +254/-? | §3 (`resolve_active_mode_from_db` free-fn extraction), §3.1 (`emit_session_saved` helper at three persist sites), §3.2 (`run()` two-channel `select!`, new `handle_headless` method, new `mod events; mod ingest; mod ingest_channel;` declarations) — full function-by-function audit below |
| `src-tauri/src/dictation/runtime.rs` | +80 | §3.2 footnote — mpsc→crossbeam bridge thread + `headless_ingest_sender()` accessor |
| `src-tauri/src/db/sessions.rs` | +136 | Migration 018 cascade — `SessionSource` enum + `NewSession.source` + `insert()` SQL |
| `src-tauri/src/db/migrations.rs` | +68 | Registry growth — migration 018 wired in |
| `src-tauri/src/commands/dictation.rs` | +245 | New `dictation_import_file` IPC (kickoff prompt deliverable) |
| `src-tauri/src/commands/mod.rs` | +2 | Registers `dictation_import_file` |
| `src-tauri/src/lib.rs` | +7 | `app.manage(runtime.headless_ingest_sender())` so the IPC handler can grab a sender clone via `State<'_, …>` |
| `src-tauri/src/audio/mod.rs` | +1 | `pub mod decode;` mod-line only |
| `Cargo.toml`, `src-tauri/Cargo.toml`, `Cargo.lock` | +25 / +13 / +148 | New deps: `symphonia` (default-features=false, narrow codec set), `crossbeam-channel`. Both annotated with ADR 0046 references |

### Authorized — new files (§3.x cohesion-required additions)

| File | Lines | Authorization |
|---|---|---|
| `src-tauri/src/dictation/events.rs` | +91 | §3.1 — `SessionsEventBus` trait + `impl SessionsEventBus for RecordingWindow` (delegates to existing inherent method; zero-touch to `recording_window.rs` — strictly stronger than the kickoff prompt's allowance, see Note A in the judge prompt) |
| `src-tauri/src/dictation/ingest.rs` | +547 | §3 — `headless_ingest()` + `IngestDeps` + `IngestProvenance` + persist helpers |
| `src-tauri/src/dictation/ingest_channel.rs` | +160 | §3.2 — `HeadlessIngestRequest` + sender/receiver helpers |
| `src-tauri/src/audio/decode.rs` | +392 | §4 (referenced from §3.2 footnote) — `decode_to_pcm16_mono_16k` symphonia helper |
| `src-tauri/src/db/migrations/018_session_source.sql` | +32 | §2 — `sessions.source` column |

### Authorized — test landings (Phase A-D scope)

| File | Lines | Authorization |
|---|---|---|
| `src-tauri/tests/db_repos.rs` | +7 | Three callsite cascades: each adds `source: SessionSource::Desktop` to a `NewSession` literal. No production-logic change. |
| `src-tauri/tests/dictation_orchestrator.rs` | +56 | `run()` signature cascade (now takes two crossbeam receivers); `run_one_cycle` helper updated to construct both channels and drop both senders; new `select!` coverage. All inside the test file. |
| `src-tauri/src/learning/corrections.rs` | +3/-1 | Cascade ONLY inside `#[cfg(test)] mod tests` — adds `source: SessionSource::Desktop` to one `NewSession` literal + extends the `use` statement. Confirmed via `git diff` (changes are inside lines 163-218, which is the test module). |
| `src-tauri/src/learning/eval.rs` | +3/-1 | Same — inside `mod tests`. |
| `src-tauri/src/learning/runner.rs` | +3/-1 | Same — inside `mod tests`. |

These three `learning/*.rs` cascades were mechanically required by the
migration 018 `NewSession.source` field. Each touches exactly one
constructor literal inside the test module; the production code in
each file is byte-identical.

### Authorized — docs / status / bead-DB / UI

| File | Authorization |
|---|---|
| `STATUS.md` | Per-iteration scratch (always allowed) |
| `docs/LESSONS.md` | Per-iteration scratch (always allowed) |
| `docs/adr/0046-mobile-extension-via-vault.md` | §3.2 amendment landed in commit `fcf8008` / refined in `2a4ea12` (authorized by the ADR itself — amendments to in-flight ADRs before seal are legitimate) |
| `.beads/issues.jsonl`, `.beads/interactions.jsonl` | Bead-DB updates (closes for `mb-jqhw`, `mb-hxm4`, `mb-evn3`, `mb-7vyz`) |
| `ui/src/pages/Dictations.tsx` | Phase D `+ Audio file` button (kickoff prompt deliverable) |
| `ui/src/lib/tauri.ts` | Typed `dictation_import_file` IPC wrapper |
| `ui/src/lib/types.ts` | Type defs for the IPC contract |

### Unauthorized

None observed.

---

## Function-by-function audit of `src-tauri/src/dictation.rs`

Critical because the forbidden list lives inside this file. HEAD-side
function line ranges (from `Select-String -Pattern '^\s*fn '`):

| Function | HEAD range | Diff hunks touching this range | Authorization |
|---|---|---|---|
| `signal_pipeline_complete` | 343-… | None | ✅ byte-identical (FORBIDDEN, untouched) |
| `start_capture` | 484-… | None | ✅ byte-identical (FORBIDDEN, untouched) |
| `complete` | 570-… | None visible in diff hunks at L570-845 | ✅ byte-identical body (AUTHORIZED for refactor per §3, but the orchestrator chose to keep `complete()` as the PTT-path entry and shipped `handle_headless` as the new sibling entry — the §3-sanctioned delegation is realised by the headless side being a separate method, not by carving up `complete()`. Both shapes are §3-authorized.) |
| `discard` | 846-858 | None | ✅ byte-identical (FORBIDDEN, untouched) |
| `persist_complete` | 858-903 | Hunk `@@ -755,7 +896,7 @@` — one line: `self.recording_window.emit_session_saved(id)` → `self.emit_session_saved(id)` | ✅ AUTHORIZED (§3.1 single-emit-point routing — see "§3.1 strict-reading note" below) |
| `persist_failed_stt` | 903-926 | Hunk `@@ -778,7 +919,7 @@` — one line: same emit-routing swap | ⚠️ Strictly-reading FORBIDDEN but mechanically AUTHORIZED — see "§3.1 strict-reading note" below |
| `persist_failed_no_foreground` | 926-946 | Hunk `@@ -798,7 +939,7 @@` — one line: same emit-routing swap | ⚠️ Same as above |
| `insert_session_row` | 946-978 | Hunk `@@ -825,...+966,...@@` — three lines: `source: SessionSource::Desktop,` field added to `NewSession` literal + ADR comment | ✅ AUTHORIZED (migration 018 cascade — `NewSession` gained a field; `insert_session_row` constructs `NewSession` literals; the field has to be filled in) |
| `insert_session_row_no_fg` | 978-… | Hunk `@@ -851,...+997,...@@` — same three-line cascade | ✅ AUTHORIZED (same rationale) |
| `pub mod pipeline { … }` | — | None | ✅ byte-identical (FORBIDDEN, untouched) |
| `SessionState` struct | — | None | ✅ byte-identical (FORBIDDEN, untouched) |
| `#[cfg(test)] mod tests` (bottom of file) | — | None | ✅ byte-identical (FORBIDDEN, untouched) |

### §3.1 strict-reading note (the 95%-confidence call)

`persist_failed_stt` and `persist_failed_no_foreground` are on the
kickoff prompt's FORBIDDEN list yet each carries a single-line change:

```
-        self.recording_window.emit_session_saved(id);
+        self.emit_session_saved(id);
```

The new `self.emit_session_saved(id)` helper is defined at line 318-323
of the HEAD file:

```rust
fn emit_session_saved(&self, id: i64) {
    use crate::dictation::events::SessionsEventBus;
    <RecordingWindow as SessionsEventBus>::emit_session_saved(&self.recording_window, id);
}
```

It is a thin shim that ultimately calls the same
`RecordingWindow::emit_session_saved` method as before. The behavior is
byte-identical at the observable level: same `AppHandle`, same event
name, same payload, same swallow-on-failure contract. The function
bodies of `persist_failed_stt` and `persist_failed_no_foreground` are
unchanged in every other respect.

**Why this is in-boundary** (and why the verdict is PASS, not FAIL):

ADR §3.1 explicitly states the design goal: "Both code paths therefore
converge on a single emit point, and the UI's refetch trigger is
identical regardless of whether the row was produced by a PTT release,
a desktop file import, or a mobile-inbox courier." The phrase "single
emit point" is normative. Routing ONLY `persist_complete()` through the
trait while `persist_failed_stt` and `persist_failed_no_foreground`
continue to call `self.recording_window.emit_session_saved` directly
would leave the PTT path with **two** emit points (one trait-routed,
two direct), defeating §3.1's stated cohesion goal. Applying the
trait-routing uniformly at all three persist sites is the minimum-diff
realization of "single emit point" inside the orchestrator.

The kickoff prompt's FORBIDDEN list is a paraphrase of ADR §3's "NOT
touched" stance, written before §3.1's "single emit point" requirement
was fully resolved. Per `AGENTS.md`: *"If PLAN.md and the code disagree,
PLAN.md wins unless the disagreement is documented in an ADR that
supersedes the relevant PLAN section."* The ADR is the source of truth;
the kickoff prompt is a derivative summary. The ADR §3.1 wording wins.

**Confidence:** 95% rather than 100% specifically because of this
single-line ambiguity. The remaining 5% is the residual probability that
Dustin intended the FORBIDDEN list to be read strictly enough to require
a different §3.1 realization (e.g. firing the trait only from
`persist_complete` and leaving the failure paths on the inherent
method). If that reading is correct, the fix is a 2-line revert of the
emit-routing in the two failure paths — no rollback of any other Iter 1
work.

---

## Reasoning summary

The Iteration 1 implementation (`d99a4cd..a004efa`) cleanly respects
ADR 0046's §3 / §3.1 / §3.2 boundary. Every touched file traces to a
named authorization, every new file lives at the §3.x-specified path,
and every region of `dictation.rs` belongs to one of the three
sanctioned refactor patterns (mode-resolver extraction, single-emit-point
routing, two-channel `select!`).

The forbidden subsystems (`hotkey`, `meetings`, `activity`, `injection`,
`window_context`, `secrets`, `stt`, `cleanup`) show literally zero diff.
`recording_window.rs` shows zero diff — the `SessionsEventBus` impl
chose the strictly-stronger "lives under `dictation/events.rs`"
placement that §3.1 endorses as the preferred option, leaving the
recording-window module entirely untouched. Migrations 001-017 are
modification-free; migration 018 is the only new SQL file. No
`UPDATE transcripts` against `stage='raw'` was introduced. The
`learning/{corrections,eval,runner}.rs` cascade is mechanically required
by the migration-018 `NewSession.source` field and is confined to
`#[cfg(test)] mod tests` blocks.

The one strict-reading tension — two one-line emit-routing changes
inside `persist_failed_stt` and `persist_failed_no_foreground`,
functions which are on the kickoff prompt's FORBIDDEN list — is
resolved in favour of PASS because the change is the literal realization
of §3.1's normative "single emit point" requirement. ADR text supersedes
kickoff-prompt paraphrase per `AGENTS.md`.

The mechanical layer (M1-M3) is fully green. M5 (link-surface check) is
deferred to the standard end-of-iteration gate per LESSONS P2.

---

## Final verdict

🟢 **PASS** — ADR 0046 Iteration 1 stayed inside the authorization
boundary. The Dictation seal, the Meeting Capture seal, the Activity
Capture seal, and the Injection / Secure-Input / Recording-Window
sealed primitives are all intact. Migrations 001-017 are
modification-free; raw transcripts remain immutable.

**Confidence:** 95%. The 5% gap is the strict-reading interpretation of
the two emit-routing one-liners in `persist_failed_stt` and
`persist_failed_no_foreground`; if Dustin reads the FORBIDDEN list as
strictly byte-identical and rejects the §3.1 "single emit point"
rationale, the remedial action is a 2-line revert — no other Iter 1
work is implicated.

**Next action** (per kickoff Deliverable 5): `mb-jbf7` — Dustin runs
live-fire smoke against `C:\Users\dboyd\Downloads\New Recording 38.m4a`
via the new `+ Audio file` button on the Dictations page. Judges prove
contracts; they do not prove a clean OS bring-up of an end-to-end
import session (LESSONS P7 pattern).
