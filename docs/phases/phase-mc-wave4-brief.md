# Phase MC Wave 4 brief — Persist + UI + LLM pass + export (the heavy wave; HUMAN-IN-LOOP)

> Authored end of Wave 3 (commit `21f40d9`). Wave 4 author: read this
> before opening `docs/phases/phase-meeting-capture.md`. The master
> plan is still binding; this brief narrows the design choices Wave 3
> left open and pins per-deliverable signatures + test specs.

---

## What Wave 3 shipped (so you know what to build on)

| module | net status after Wave 3 |
|---|---|
| `meetings/loopback_windows.rs` | `LoopbackCapture` (cpal loopback) **complete** (ADR 0031). 7 tests. Drops cleanly on Stream-side error. |
| `meetings/capture.rs` | `TwinStreamCapture` coordinator **complete**. Owns mic + optional loopback; clock-aligned drain; per-channel sample ring → `MeetingChunker`. 12 tests. Exposes `take_chunk_rx()` (one-shot accessor) — see §1.1 below. |
| `meetings/long_form_stt.rs` | Chunked stitch driver **complete** (ADR 0029). 23 tests (11 integration + 12 pure-helper, split across `long_form_stt_tests.rs` and `long_form_stt_pure_tests.rs` to stay under the 600-line cap). Per-channel rolling `initial_prompt`, CRC32 verify, overlap dedup, global-timeline shift. |
| `meetings/hotkey_installer.rs` | **NEW.** Independent second `WH_KEYBOARD_LL` hook on its own thread (`mockingbird-meeting-hotkey`). 11 + 1-ignored tests. The hook proc ALWAYS calls `CallNextHookEx` so the sealed dictation hook downstream still fires. |
| `hotkey/probe.rs` | Extended with `probe_meeting_main_vk` + `meeting_candidate_chain`. 3 new tests. No edits to existing fns (sealed-file rule respected via additive-only diff). |

Files **untouched** this wave (still sealed): `hotkey/state.rs`,
`hotkey/windows.rs`, `hotkey/driver.rs`, `dictation/`, `injection/`,
`recording_window.rs`, `cleanup/provider.rs`, `cleanup/llm_cleaner.rs`,
migrations 001–010.

Project test count after Wave 3: **~430** (was ~395; +35 net Wave-3-only).
Phase-MC running delta so far: **+103 tests** (Wave 2 +68 + Wave 3 +35).
Master-plan target is +90 to +120 — we're on track and Wave 4 should
add another +25 to +45 to land inside the band.

### 1.1 The `take_chunk_rx()` handoff (subtle but binding)

Wave 3 surfaced a lifetime issue: `TwinStreamCapture` owns the mpsc
`Sender<ChannelChunk>` that mic + sys streams write to, but the
`LongFormStt` consumer needs **sole ownership** of the matching
`Receiver`. The compromise the codebase settled on:

```rust
// capture.rs
pub struct TwinStreamCapture {
    chunk_rx: Option<std::sync::mpsc::Receiver<ChannelChunk>>,
    // ...
}

impl TwinStreamCapture {
    /// Hand off the chunk receiver. **One-shot**: subsequent calls
    /// return `None`. `try_recv_chunks()` returns empty after take.
    pub fn take_chunk_rx(&mut self) -> Option<Receiver<ChannelChunk>> {
        self.chunk_rx.take()
    }
}
```

Wave 4 runtime wiring (§2 below) MUST call `take_chunk_rx()` exactly
once, right after `TwinStreamCapture::start(...)` returns, and pass
the receiver into `LongFormStt::new(...)`. If you find yourself
needing the receiver elsewhere, **stop and ask** — duplicating the
sender is the wrong fix (the mpsc Receiver isn't clonable on purpose;
chunk ordering is the entire point of the driver).

---

## Wave 4 deliverables (8 tasks)

Wave 4 is the **HUMAN-IN-LOOP** wave: the last task (QA matrix)
requires a human at a keyboard with a YouTube tab open. Tasks 1–7
remain autonomous and should be sealed before the QA pass starts.

### 4.1 `meetings/persist.rs` — atomic insert (P0)

Wave 1 scaffolded `MeetingPersistRequest` + `MeetingStatus` + a
`todo!()` stub. Wave 4 fills in the impl against migration 011.

#### Signature (already in code; do not change)

```rust
pub fn persist_meeting(conn: &Connection, req: &MeetingPersistRequest) -> AppResult<i64>;
```

Returns the new `meeting_sessions.id` (i64 rowid).

#### Behaviour

1. **One transaction** opened on `conn`. INSERT into `meeting_sessions`
   (every column from `MeetingPersistRequest`).
2. After the session-row INSERT, **per-channel transcript INSERTs**:
   for each of `formatted_mic`, `formatted_sys`, `formatted_merged`
   that's `Some(_)`, INSERT a `meeting_transcripts` row with
   `stage='formatted'`. For each of `segments_mic`, `segments_sys`
   that's `Some(_)`, INSERT a `raw_segments` row with the segments
   JSON-encoded (via `serde_json::to_string`).
3. **Individual transcript INSERT failures are non-fatal** — log via
   `tracing::warn!` and continue. This mirrors Phase 3 Wave 4.9 Bug A
   (a single bad transcript should not lose the whole session).
4. **Session-row INSERT failure propagates** as
   `AppError::MeetingCapture`. The transaction rolls back; nothing
   persists.
5. Commit the transaction; emit `meetings:session-saved` via the
   Tauri `AppHandle` (the runtime is responsible for the actual emit;
   `persist_meeting` returns the rowid and the runtime does the emit).

#### Test specs (5 tests; in-memory DB)

Live in `src-tauri/src/meetings/persist.rs::tests` (the existing
module). Use `Connection::open_in_memory()` + apply migration 011
via `crate::db::migrations::apply_all(&conn)`.

| name | inputs | expected |
|---|---|---|
| `persist_complete_meeting_round_trips` | Request with all three formatted channels + both segment arrays | Returns `Ok(rowid)`; `SELECT count(*) FROM meeting_sessions WHERE uuid=?` == 1; `SELECT count(*) FROM meeting_transcripts WHERE meeting_session_id=?` == 5 (3 formatted + 2 raw_segments). |
| `persist_mic_only_round_trips` | Request with `formatted_mic=Some, segments_mic=Some`; sys + merged = None | rowid OK; transcripts count == 2. |
| `persist_returns_meeting_capture_error_on_unique_violation` | INSERT a session, then INSERT again with the same `uuid` | `AppError::MeetingCapture(_)`. |
| `persist_skips_individual_transcript_failures` | Inject a deliberately-bad transcript row (e.g. `channel="invalid"` via a hand-rolled INSERT after `persist_meeting` returns; OR mock the `meeting_transcripts` table CHECK constraint by overriding the migration with a stricter table for the test) | Session row still committed; total persisted transcripts == 2 (the two valid). [Note: the master plan calls this out as mirroring Phase 3 Bug A; if you can't construct a meaningful "bad row" given the loose schema, **skip this test and document why in the brief addendum** — the partial-status invariant is also covered by the status_partial test.] |
| `persist_marks_status_partial_when_only_one_channel_formatted` | Request with `formatted_mic=Some, formatted_sys=None, formatted_merged=None, status=Partial` | The `status` column round-trips as `"partial"` and `MeetingStatus::from_db_str` decodes it correctly. |

### 4.2 `meetings/runtime.rs` — full lifecycle wiring (P0)

Wave 1 scaffolded the struct + a 1-test smoke. Wave 4 wires the
lifecycle: start → capture → stop → long-form-stt → formatter →
merge → persist → emit done.

#### New types / signatures

```rust
pub struct MeetingCaptureRuntime {
    shared_conn: Arc<Mutex<Connection>>,
    config: MeetingRuntimeConfig,
    // NEW (Wave 4):
    hotkey: Option<MeetingHotkeyInstaller>,
    activation_thread: Option<JoinHandle<()>>,
    in_flight: Arc<Mutex<Option<InFlightMeeting>>>,
    llm_pass_cache: Arc<Mutex<HashMap<Uuid, String>>>,
    app_handle: tauri::AppHandle,
}

struct InFlightMeeting {
    uuid: Uuid,
    started_at: DateTime<Utc>,
    capture: TwinStreamCapture,
    long_form: thread::JoinHandle<AppResult<LongFormOutput>>,
}

impl MeetingCaptureRuntime {
    /// NEW signature (Wave 1's signature is preserved; add `app_handle`):
    pub fn spawn(
        shared_conn: Arc<Mutex<Connection>>,
        config: MeetingRuntimeConfig,
        app_handle: tauri::AppHandle,
    ) -> AppResult<Self>;

    pub fn start_meeting(&self, source: MeetingSource) -> AppResult<Uuid>;
    pub fn stop_meeting(&self, uuid: Uuid) -> AppResult<()>;
    pub fn llm_pass_cache(&self) -> Arc<Mutex<HashMap<Uuid, String>>>;
}
```

#### Behaviour

1. `spawn` installs the meetings hotkey (via
   `MeetingHotkeyInstaller::install(chord, tx)`), spawns the activation
   thread which reads from the activation rx and calls `start_meeting` /
   `stop_meeting` on each `MeetingToggle`.
2. `start_meeting` constructs `TwinStreamCapture::start(source)` and
   spawns a `LongFormStt` worker on a fresh `JoinHandle`. Returns the
   new `Uuid`. Idempotent: if `in_flight.is_some()`, returns the
   existing uuid + emits a `meeting:state="warn-already-running"` event.
3. `stop_meeting` calls `capture.stop()` (drops the streams; flushes
   final chunks). Joins the long-form worker. Runs `formatter::format`
   per channel + `merge` for merged. Calls `persist_meeting`. Emits
   `meeting:state="done"` + `meetings:session-saved`.
4. **`Drop`**: if `in_flight.is_some()`, force `capture.stop()`, join
   with a 5 s timeout, persist with `status=Interrupted`, log via
   `tracing::warn!`. The 4-hour idle endurance QA test exercises this.

#### Test specs (3 tests; rest covered by the activation/capture/long_form modules)

The runtime's job is glue; the pure modules carry the real coverage.
Aim for 3 wired-end-to-end tests, all `#[cfg(target_os = "windows")]`
because they touch `TwinStreamCapture`:

| name | inputs | expected |
|---|---|---|
| `runtime_spawn_then_drop_is_clean` | Spawn with a dummy in-memory connection + a `tauri::test::mock_app()` handle | No panic; no leaked thread (verify via the mockingbird-meeting-hotkey thread count before/after). |
| `runtime_start_then_stop_persists_session_row` | Spawn; `start_meeting(Mic)` with a `MicCapture` stub that emits 1 s of silence; `stop_meeting(uuid)` | A `meeting_sessions` row exists with `status='complete'` and at least the formatted-mic transcript row. |
| `runtime_drop_during_capture_marks_interrupted` | Spawn; start; drop the runtime before stop | A `meeting_sessions` row exists with `status='interrupted'`. (Use a custom Drop-completion channel to deterministically wait for the persist before the test exits.) |

If `tauri::test::mock_app()` isn't ergonomic enough for the test, gate
these `#[ignore]` and document why — the rest of the wave seal does
not depend on them.

### 4.3 `meetings/llm_pass.rs` — Ollama wrapper (P0)

Wave 1 scaffolded `LlmPassPrompt`, `LlmPassRequest`, `LlmPassResult`.
Wave 4 ships `run_llm_pass`.

#### Signature (already in code; do not change)

```rust
pub fn run_llm_pass(req: &LlmPassRequest, transcript_text: &str) -> AppResult<LlmPassResult>;
```

#### Behaviour

1. Resolve the prompt body:
   - `LlmPassPrompt::BuiltIn(name)` → `include_str!` from
     `meetings/prompts/{name}.md`. The three Wave 1 prompts are
     `summary.md`, `action_items.md`, `cleaner_punctuation.md`.
     Unknown name → `AppError::MeetingCapture("unknown built-in prompt: …")`.
   - `LlmPassPrompt::Custom(body)` → use `body` verbatim.
2. Build the full prompt: `"{system_header}\n\n{prompt_body}\n\n---\n\n{transcript_text}"`.
   The `system_header` is the constant `"You are a meeting-transcript
   assistant. Be concise. Do not invent facts not in the transcript."`
   (or whatever the existing dictation system-header style is — go
   look at `cleanup/ollama.rs` first for parity).
3. Instantiate `OllamaProvider::new()` (existing arg-less constructor).
   **Do NOT extend the `CleanupProvider` trait.** Build a
   `CleanupRequest<'_>` with the assembled prompt as the input text,
   `model_id` (default from settings if `None`), `temperature=0.2`,
   `max_tokens=2048`. Call `provider.clean(&req)`.
4. Generate a fresh `Uuid::new_v4()`. Return `LlmPassResult { id, text,
   latency_ms }`. The runtime caches `text` keyed by `id`.

#### Test specs (4 tests)

| name | inputs | expected |
|---|---|---|
| `builtin_prompt_resolves_to_markdown` | `LlmPassPrompt::BuiltIn("summary")` | The resolved body contains a non-empty markdown header (the `prompts/summary.md` file has one — pin the assertion to a specific snippet of that file, like `"## Goal"`). |
| `builtin_prompt_unknown_name_errors` | `LlmPassPrompt::BuiltIn("nonexistent")` | `AppError::MeetingCapture(_)` whose message contains `"unknown built-in prompt"`. |
| `custom_prompt_passes_through_verbatim` | `LlmPassPrompt::Custom("hello world")` | The assembled prompt body section is exactly `"hello world"`. |
| `run_llm_pass_against_mock_ollama` | Stand up a `wiremock`/`httpmock` Ollama on `127.0.0.1:0`; point `OllamaProvider::with_base_url(...)` at it; canned JSON response with `response="ok"` | `LlmPassResult { text: "ok", .. }`; `latency_ms` ≥ 0. |

The last test needs `wiremock` or `httpmock` — pick whichever is
already in `Cargo.toml` dev-deps. If neither is in, **add httpmock**
(it's simpler) via a dev-dep-only addition; the hook
`block-unsafe-npm` doesn't gate dev-deps in `Cargo.toml`. Document
the addition in the brief's deviations.

### 4.4 `meetings/export.rs` + `meetings/clipboard.rs` (P0)

#### `export.rs::render_markdown`

Wave 1 scaffolded `ExportRequest<'a>` + a `todo!()` stub. Wave 4
ships the impl.

```rust
pub fn render_markdown(req: &ExportRequest<'_>) -> AppResult<String>;
```

But the existing scaffold only has `meeting_uuid` and `llm_pass_text`
fields — Wave 4 needs the **full meeting detail** to render. Either:
(a) extend `ExportRequest` with a `&MeetingDetail` payload, or
(b) make `render_markdown` take a 2nd argument: `&MeetingDetail`.

**Recommendation**: option (b). The signature becomes:

```rust
pub fn render_markdown(detail: &MeetingDetail, llm_pass_text: Option<&str>) -> AppResult<String>;
```

…and `ExportRequest<'a>` gets deleted (YAGNI — it was a Wave 1
placeholder; nothing else references it). Document this as a planned
deviation from Wave 1's scaffold shape.

Output format (frontmatter + body, mirrors the Wispr-Flow export the
human is migrating from):

```markdown
---
title: <title or "Untitled meeting">
uuid: <uuid>
started_at: <ISO-8601>
duration: <H:MM:SS>
source: <mic|system|both>
formatter_version: mc-v1
---

# <title or "Untitled meeting">

<You-labeled paragraphs from formatted_mic>

<Other-labeled paragraphs from formatted_sys>

(or, if both channels present, the merged paragraph stream
with `**You:**` / `**Other(s):**` labels — picks `formatted_merged`
when present, else interleaves by paragraph order.)

## LLM pass output  ← only if `llm_pass_text` is Some
<llm_pass_text>
```

#### `clipboard.rs::copy_text_one_shot`

```rust
/// One-shot UTF-16 clipboard write. **No save/restore** — meeting
/// export is an explicit user-initiated paste-target action, NOT an
/// inline injection. The "clipboard save/restore" binding rule in
/// AGENTS.md applies to dictation paste-injection, not to a user
/// pressing "Copy to clipboard" on a finished meeting transcript.
pub fn copy_text_one_shot(text: &str) -> AppResult<()>;
```

The implementation uses the `arboard` crate (already in Cargo.toml
for the dictation paste path). Just `arboard::Clipboard::new()?
.set_text(text)?`. Two-line function; one test asserting
round-trip on a Windows agent (live clipboard).

#### Test specs (6 tests across export + clipboard)

| file | name | inputs | expected |
|---|---|---|---|
| export | `frontmatter_round_trips` | Detail with title, uuid, started_at, source, total_duration_ms | The rendered markdown's first 7 lines parse as YAML and the parsed map has all 6 keys. |
| export | `mic_only_renders_you_label_only` | Detail with `formatted_mic=Some, sys=None, merged=None` | Body contains `<formatted_mic body>` and does NOT contain `**Other(s):**`. |
| export | `both_renders_merged_when_present` | Detail with all three channels, `formatted_merged=Some("**You:** hi\n\n**Other(s):** hello")` | Body uses the merged stream, not an interleave. |
| export | `llm_pass_section_appended_when_present` | Render with `llm_pass_text=Some("- bullet 1")` | Body ends with `"## LLM pass output\n\n- bullet 1\n"`. |
| export | `untitled_meeting_renders_fallback_title` | Detail with `title=None` | Frontmatter `title: "Untitled meeting"` and the H1 matches. |
| clipboard | `copy_text_round_trips` (`#[ignore]`-gated; live clipboard) | `"hello"` | `arboard::Clipboard::new()?.get_text()? == "hello"`. |

### 4.5 Tauri commands (P0)

Author `src-tauri/src/commands/meetings.rs` and register all 10
commands in `commands/mod.rs::register`. Section MC.6 of the master
plan is the binding spec — paste-and-implement.

#### Layout pointer

```
src-tauri/src/commands/
  mod.rs          (add `pub mod meetings;` + register the 10 commands)
  meetings.rs     (new file)
```

#### Command checklist (one `#[tauri::command]` fn per row)

```
meeting_probe_sources         → MeetingSourceProbe
meeting_start                 → { uuid }
meeting_stop                  → ()
list_meetings                 → Vec<MeetingSummary>
get_meeting_detail            → MeetingDetail
delete_meeting                → ()
search_meeting_transcripts    → Vec<MeetingMatch>
meeting_export_markdown       → { path }
meeting_copy_to_clipboard     → ()
meeting_run_llm_pass          → LlmPassResult
```

Each command takes a `tauri::State<MeetingCaptureRuntime>` (or
`Arc<MeetingCaptureRuntime>`) and the args struct. All return
`Result<T, String>` (the existing IPC convention — `AppError` to
`String` via `Display`).

**Test specs**: 2 tests per command via `tauri::test::mock_runtime()`,
gated `#[cfg(target_os = "windows")]`. Target: ~20 tests across all
10 commands. If the mock-runtime ergonomics are bad, drop to 1 test
per command (smoke + happy-path); document the deferral.

### 4.6 `meeting_overlay` Tauri window + React UI (P0)

Two parts: backend window declaration and the React surface.

#### Backend

- `tauri.conf.json`: add a `"meeting_overlay"` window declaration
  alongside the existing dictation overlay. Same `decorations: false,
  always_on_top: true, transparent: true, skip_taskbar: true` pattern.
- `meetings/overlay.rs::MeetingOverlay::show / hide` — fill in the
  `todo!()` stubs with `window.show()? / hide()?`.

#### React surface

```
ui/src/
  meeting_overlay.tsx                (new entry point; mirrors recording_overlay.tsx)
  meeting_overlay/
    MeetingOverlay.tsx               (source picker + Start + cancel)
    MeetingOverlay.module.css        (or Tailwind; pick whichever the dictation overlay uses)
  pages/
    Meetings.tsx                     (history list page)
    MeetingDetail.tsx                (transcript view + Copy / Export / LLM-pass panel)
  lib/
    meetings.ts                      (typed IPC wrappers around the 10 commands)
  components/
    Sidebar.tsx                      (ADD a "Meetings" nav link)
  App.tsx                            (REGISTER routes for /meetings + /meetings/:uuid)
```

**Visual parity**: mirror the dictation overlay's pill/blob aesthetic;
design tokens come from `ui/src/design/tokens.css` (binding per
AGENTS.md). No new design tokens this wave.

**Test specs**:
- Backend `overlay.rs`: 2 unit tests (show + hide construct + drop
  cleanly with a `tauri::test::mock_app()` — `#[ignore]`-gate if
  the mock-app isn't available).
- React: **qa-kitten (sub-agent) authors Playwright visual tests in
  Wave 5.** Wave 4's React deliverable is functional only; do not
  block on visual test authorship.

### 4.7 Tray "Meeting" affordance (P1, optional Wave-4-shippable)

Master plan defers the tray "Pause meeting hotkey" toggle to Wave 5.
Wave 4 can either:
- (a) Leave the tray alone (defer to W5);
- (b) Add a passive "Meeting: idle / recording" status entry to the
  tray menu (no toggle; just read-only state echo from
  `MeetingCaptureRuntime::status()`).

**Recommendation**: (a) — keep Wave 4 focused on the recording-to-
persistence path. Re-evaluate during Wave 5 brief.

### 4.8 Hands-on QA matrix (P0, HUMAN-IN-LOOP)

Authored as `docs/phases/phase-mc-qa-matrix.md`. The human (Dustin)
runs the 5 scenarios; code-puppy authors the template and reports
collation.

Scenarios:

1. **60 s mic-only meeting.** Expected: `status='complete'`,
   `formatted_mic` non-empty, `formatted_sys` NULL, audio blob written
   under `app_local_data_dir()/audio_blobs/<uuid>.wav`.
2. **60 s system-only meeting** (YouTube tab). Expected: `formatted_sys`
   non-empty, `formatted_mic` NULL.
3. **60 s Both with you-and-a-podcast-talking-over-each-other.**
   Expected: both channels populated; `formatted_merged` interleaves
   by `t0_ms`; `mc-two-channel-merged` judge would pass on this
   transcript (manual eyeball at this stage; judge runs in Wave 6).
4. **10 min mic-only stress.** Expected: completes without OOM; ~20
   chunks emitted; `chunk_count_mic == 20`; persisted size sane.
5. **4-hour idle-loop endurance** (overnight). Expected: no memory
   leak (Task Manager working-set delta < 200 MB between t=0 and
   t=4h). Drop-on-shutdown marks the session `interrupted` if a
   recording is in flight at shutdown.

Each scenario records: start timestamp, end timestamp, final status,
any toast messages, working-set delta. Template lives in
`docs/phases/phase-mc-qa-matrix.md`.

---

## Deviations from `phase-meeting-capture.md` (justified)

1. **`ExportRequest<'a>` is deleted in §4.4.** YAGNI — it was a Wave 1
   placeholder, no call sites. The replacement signature
   `render_markdown(&MeetingDetail, Option<&str>)` is cleaner and one
   fewer type to maintain.
2. **`copy_text_one_shot` lives in `meetings/clipboard.rs`, not inline
   in `commands/meetings.rs`.** Reason: it's a reusable primitive the
   `meeting_copy_to_clipboard` IPC + future tray "copy last meeting"
   affordance will both call. Single source of truth.
3. **Tray meeting-status entry deferred to Wave 5 per §4.7 (a).**
   Wave 4 already has 7 P0 tasks; tray polish belongs in Wave 5's
   "polish + brief" wave.
4. **Mock-runtime test gates may be `#[ignore]`'d.** If
   `tauri::test::mock_runtime()` proves too brittle on Windows
   (LESSONS 2026-02-19 noted similar mock-app pain for the dictation
   overlay), gate the runtime + overlay backend tests `#[ignore]`
   and rely on the QA matrix for end-to-end coverage. Document the
   gate-rationale in each `#[ignore = "…"]` attribute.
5. **`httpmock` (or `wiremock`) as a NEW dev-dep is acceptable in
   Wave 4.** §4.3 test `run_llm_pass_against_mock_ollama` needs it.
   Dev-deps aren't gated by `block-unsafe-npm`; just check the Mini
   Shai-Hulud IOC list (PLAN Appendix D) for the chosen crate.

---

## Cargo gate (must be green at Wave 4 seal)

```pwsh
cd src-tauri
cargo check --all-targets
cargo clippy --release --all-targets -- -D warnings
cargo test --release --no-run     # full link; --no-run because
                                   #   --release executable launch
                                   #   trips 0xC0000139 on this box
                                   #   (LESSONS 2026-05-17)
cargo fmt --check
```

All four must come back clean (zero errors, zero warnings).

For the React surface:

```pwsh
cd ui
npm run lint
npm run typecheck       # if defined
# npm test deferred to Wave 5 with qa-kitten Playwright suite
```

---

## Wave 6 judge prep (read-ahead so Wave 4 doesn't paint into a corner)

Wave 6 lands 5 judges. Wave 4's deliverables must keep these
judge-checkable:

| judge | Wave-4 implication |
|---|---|
| `mc-formatter-deterministic` | Formatter is sealed by Wave 2; Wave 4 must not call into it with side-effectful arguments. |
| `mc-long-form-stitched-losslessly` | Long-form driver is sealed by Wave 3; Wave 4's runtime wires it but does not alter its semantics. |
| `mc-two-channel-merged` | The export's "both channels" path (§4.4) is what this judge reads. Keep the speaker labels consistent and the interleave deterministic. |
| `mc-no-llm-in-critical-path` | §4.2 runtime's `start_meeting → stop_meeting → persist` path MUST NOT call `run_llm_pass`. The LLM pass is invoked ONLY by the `meeting_run_llm_pass` IPC command, post-persist, from the UI. Wave 6 adds runtime instrumentation that asserts the `OllamaProvider` constructor counter is 0 across the critical path. |
| `mc-dictation-untouched` | Cross-checks the sealed-files list. Wave 4 adds new files only; no edits to the seal list. |

---

## Brief checklist (post-wave-4 author updates this section)

Before declaring Wave 4 sealed:

- [ ] All 7 autonomous tasks (4.1–4.6) have green cargo gate + tests landed
- [ ] `docs/phases/phase-mc-qa-matrix.md` template authored
- [ ] Human-in-loop QA scheduled with Dustin (don't run autonomously)
- [ ] `bd close` for all `mb-pdv.*` Wave 4 task IDs
- [ ] Author `docs/phases/phase-mc-wave5-brief.md` for the W4→W5 handoff
- [ ] STATUS.md updated with Wave 4 completion line + Wave 5 anchor

---

*Brief authored by code-puppy (`code-puppy-b14c19`) on 2026-05-20,
end-of-Wave-3 iteration, commit `21f40d9`. Master plan version:
`docs/phases/phase-meeting-capture.md` as of post-`phase-4-complete`
tag.*
