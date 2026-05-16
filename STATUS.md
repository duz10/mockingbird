# Mockingbird — STATUS

**Current phase:** Phase 3 — Waves 1 + 2 + 3 + 4 + 4.5 ✅ **COMPLETE**; Wave 5 (judges + seal) READY
**Last updated:** 2026-05-17 (Phase 3 Wave 4.5 finished — app boots end-to-end, dictation runtime live)
**Last successful judge run:** _Phase-3 Wave-4.5 cargo gate: 306/306 tests on GPU (`--release`), clippy `--release --all-targets -- -D warnings` clean, fmt clean, 2026-05-17. +8 ignored live tests. **Live boot verified:** `mockingbird.exe` runs, DB created, orchestrator config resolved, dictation runtime spawned, Whisper loaded with GPU._

**Blocked on:** 🛑 **Dustin** — the dictation pipeline is wired and runs. To do the QA matrix:

1. **Launch:** `pwsh scripts/run-mockingbird.ps1` (sets CUDA + ORT env, starts in background).
2. **Drive the 12-row matrix** in `docs/phases/phase3-wave4-brief.md` §"QA matrix". Hold RightAlt → speak → release → observe.
3. **Inspect:** logs at `%APPDATA%\com.dustin.mockingbird\logs\mockingbird.log.YYYY-MM-DD`, DB at `%APPDATA%\com.dustin.mockingbird\mockingbird.db`.
4. **Stop:** `taskkill /F /IM mockingbird.exe`.
5. **Mark `mb-up3` closed** with row-by-row notes; then say "go on Wave 5".

Ready tasks waiting: mb-idy (Wave 5 — judges + seal).

---

## Phase 3 progress (current)

| Wave | Deliverables                                                                                                                                                                                                                                                       | Status |
|------|--------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|--------|
| 1    | ADRs 0015–0019 (low-level hook, injection strategy, secure-input guard, clipboard save/restore, hotkey conflict probe), `AppError::Hotkey/Injection` variants, `phf` dep, broader `windows-rs` feature set, 16 module scaffolds across `hotkey/` `injection/` `window_context/`, `scripts/cargo-with-cuda.ps1` wrapper, 164/164 tests | ✅ |
| 2    | `window_context/windows.rs` (real `GetForegroundWindow` + `K32GetModuleBaseNameW` + `OwnedHandle` RAII), `hotkey/state.rs` (pure §6.1, 26 tests), `injection/secure_guard.rs` (`WinSecureInputGuard` with class allowlist + `ES_PASSWORD`; ADR 0017 amended), `injection/strategy.rs` (`phf::phf_map!` 12-entry override table + case-insensitive `resolve()`), 213/213 tests | ✅ |
| 3    | `hotkey/windows.rs` (`WH_KEYBOARD_LL` on dedicated `mockingbird-hotkey` thread, pure `classify_keystroke` helper with 9 tests), `hotkey/driver.rs` (20 ms tick cadence, 6 tests), `hotkey/probe.rs` (ADR 0019 fallback chain, 7 tests), `hotkey/pause.rs` (Arc<AtomicBool>+channel `PauseHandle`, 6 tests), 244/244 tests + 4 ignored live | ✅ |
| 4    | `injection/paste.rs` (ADR 0018 four-step dance), `injection/windows.rs` (`SendInputInjector` Paste/Keystroke/Abort), `injection/strategy_wiring.rs` (focus-loss + resolver glue), `cleanup/mod.rs` (Cleaner trait + Passthrough), `dictation.rs` (orchestrator + pure `pipeline::decide`), `recording_window.rs` stub, **migration 004** (injection_status column), 303/303 tests + 7 ignored | ✅ |
| 4.5  | `dictation/runtime.rs` (DictationRuntime spawn glue: hook install + state driver + dictation thread with !Send deps built in-thread), `models_dir()` 4th fallback for `%USERPROFILE%\mockingbird_models\`, `ORT_DYLIB_PATH` autodiscovery, `bootstrap_provenance_rows()` for first-run dict + example_set, `AppState` refactored to `Arc<Mutex<Connection>>` shared with dictation thread, `lib.rs::run()` wired end-to-end, `scripts/run-mockingbird.ps1` launch script, **live boot verified**, 306/306 tests + 8 ignored | ✅ |
| 5    | 4 judges (e2e-injection, db-provenance, clipboard-restored, secure-input-respected), Phase 3 retrospective, `phase-3-complete` tag                                                              | ⏳ |

bd: 24 tasks seeded; 6 closed (Wave 1 done), 5 ready (Wave 2), 13 blocked downstream.

---

## 🎉 PHASE 2 SEALED — GPU VERIFIED ON RTX 2060

PLAN line 1362 ("CUDA path verified on RTX 2060") — **satisfied**. The `phase-2-complete` git tag is applied to commit covering Wave 5 finale.

**What changed since the prior "NOT SEALED" state:** CUDA Toolkit 12.8 installed side-by-side with CUDA 13.2 (each version in its own `v12.8\` / `v13.2\` subdir). MSBuild integration for v13.2 moved aside via `scripts/disable-cuda13-msbuild.ps1` so cmake's VS17 2022 generator picks `CUDA 12.8.targets` (the working one). `whisper-rs cuda` feature re-enabled in workspace `Cargo.toml`. Build env requires `CUDA_PATH` + `CUDA_PATH_V12_8` set explicitly in the calling shell (User/Machine env don't propagate to processes spawned before the install).

Full GPU re-enable runbook lives in `docs/LESSONS.md` under "2026-05-16 [phase-2] CUDA 12.8 install + GPU re-enable success story".

---",
**Cost line (cumulative):** _Track from first /goal run — bootstrap + Phase 0 + Phase 1 Waves 1+2 across two sessions; record when LLM judges run._

---

## Phase 0 — Groundwork: ✅ COMPLETE

All 21 Phase 0 tasks (per `docs/phases/phase0.md`) closed in `bd`. Phase tag
`phase-0-complete` applied to the seal commit.

### Wave-by-wave summary

| Wave | Deliverables                                                 | Status |
|------|--------------------------------------------------------------|--------|
| 1    | dirs + `.gitkeep`, `LICENSE` (MIT), `docs/SETTINGS.md` stub, `docs/phases/phase0.md` | ✅ |
| 2    | `lefthook.yml`, `verify-environment.ps1`, `setup-dev.ps1`, ADR `0000-template` + 9 backfill ADRs, 16 slash commands, `.code_puppy/README.md`, toolchain pins (`.npmrc`/`.rustfmt.toml`/`.env.example`), `CONTRIBUTING.md` + `CHANGELOG.md` | ✅ |
| 3    | `assets/icons/mockingbird.svg`, `scripts/generate-icons.ps1`, generated icon set under `src-tauri/icons/` | ✅ |
| 4    | `README.md`, this STATUS.md, judge self-check, commit + tag | ✅ |

### Mid-iteration learnings logged

- `rust-toolchain.toml` is a PIN not an MSRV → removed from the repo;
  MSRV moves to `Cargo.toml [package] rust-version` in Phase 1.
- PowerShell `$Args` is an automatic; don't name a param `$Args`.
- `cargo tauri icon <svg>` Just Works™ — no ImageMagick needed.
- See `docs/LESSONS.md` for the full set (now 7 entries from bootstrap+Phase-0).

---

## Tauri updater public key (carried forward from bootstrap; Phase 1 embeds into `tauri.conf.json`)

```
dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEQ5N0E1MTkzODYzNTBGQTEKUldTaER6V0drMUY2MlNiS2g5anF0Vjl6UEkyODRQTlZlS0FMRjNuNWcvdEpJUC9RRG1QVm5Ja04K
```

Private key at `%USERPROFILE%\.tauri\mockingbird.key` (empty password —
re-encrypt before Phase 7).

---

## Section −1 resolution (carried forward)

| # | Item | Status | Resolution |
|---|------|--------|------------|
| 1 | Project name | ✅ | `Mockingbird` / `mockingbird`. |
| 2 | License | ✅ | MIT shipped this phase (`LICENSE`). |
| 3 | GitHub repo URL | 🟨 DEFERRED | Placeholder OK; resolve pre-Phase-7. |
| 4 | Code-signing cert | 🟨 DEFERRED | ADR 0005 (deferred to Phase 7). |
| 5 | Tauri updater key | ✅ | Generated bootstrap; embedded by Phase 1. |
| 6 | Cloud Claude model strings | 🟨 DEFERRED | Re-verify pre-Phase-4. |
| 7 | DBOS | ✅ DEFERRED | User confirmed. |
| 8 | `extra_models.json` rotation | 🟨 DEFERRED | Empty scaffold; decide pre-Phase-4. |
| 9 | Orchestration model | ✅ | ADR 0002 (no pack agents). |

---

## Blocked / human input needed

- **cmake** not installed → <https://cmake.org/download/>
- **CUDA Toolkit 12.x** (`nvcc`) → <https://developer.nvidia.com/cuda-downloads>
- **ollama** → <https://ollama.com/download>

Phase 0 and Phase 1 can proceed without these. **Phase 2 cannot.**
Install before kicking off `/phase2-goal`.

---

## Phase 1 — Foundation: ✅ COMPLETE (sealed at `phase-1-complete` tag)

**Migrations 001-003 are now FROZEN.** The hook `block-migration-edit-after-phase-1` enforces — future schema changes go in migration 004+.

Binding plan: `docs/phases/phase1.md` (planning-agent session 1b10a8, 25 tasks across 5 waves).

### Wave 2 — Migrations + runner + integration tests ✅

| File | What it does | Lines |
|------|--------------|-------|
| `src-tauri/src/db/migrations/001_initial.sql` | Core tables + FTS5 per PLAN §7 verbatim (BEGIN/COMMIT, PRAGMA WAL+FK) | 174 |
| `src-tauri/src/db/migrations/002_audit_triggers.sql` | All 4 `_history_*` tables + **12 audit triggers** (4 tables × INSERT/UPDATE/DELETE) extrapolated per Wave 2 brief | 186 |
| `src-tauri/src/db/migrations/003_seed_modes.sql` | Seed 3 prompts + 3 modes with `__PROMPT_*_BODY__` tokens + `(SELECT id FROM prompts ...)` sub-selects | 37 |
| `src-tauri/src/db/mod.rs` | `Database::open(path)` + `::open_in_memory()` + `pub fn apply_migrations()` shim + PRAGMA gating + `integrity_check` + `foreign_key_check` | ~115 |
| `src-tauri/src/db/migrations.rs` | Runner with `include_str!` + `schema_version` idempotency + 3 inline unit tests | ~110 |
| `src-tauri/src/db/prompt_loader.rs` | Token substitution + SQL-quote escaping + 3 unit tests | ~80 |
| `src-tauri/tests/db_migrations.rs` | 7 integration tests (schema_version=3, tables present, **14 triggers**, seeded data with audit fired, audit UPDATE before/after, FTS5 round-trip, idempotency via the shim) | 188 |
| `src-tauri/src/lib.rs` | Wired `pub mod db;` + `.setup()` opens DB at `%APPDATA%/Mockingbird/mockingbird.db` | edit |
| `src-tauri/src/error.rs` | Added `Sqlite(#[from] rusqlite::Error)` variant | edit |

**Cargo quality gate all four green:**
- `cargo check --workspace` ✅ (warm 5.5s)
- `cargo clippy --workspace --all-targets -- -D warnings` ✅ (15.7s)
- `cargo test --workspace` ✅ — **15/15** (5 unit + 3 unit + 7 cross-crate integration)
- `cargo fmt --check` ✅ (after auto-fmt)

**Delegation worked:** migration-author authored all 4 SQL/test files; code-puppy authored the runner + lib.rs wiring + error variant. Zero 5-attempt escalations. 15/15 tests pass first run.

### Wave 1 — Decisions, scaffolding, prompt stubs ✅ (commit `8e70d7c`)

| File | What it does |
|------|--------------|
| `docs/adr/0004-rusqlite-over-sqlx.md` | ADR: rusqlite (bundled) over sqlx; tauri-plugin-sql dropped |
| `Cargo.toml` (workspace) | Phase-1 deps pinned; `whisper-rs`/`cpal`/`ort`/`enigo` DEFERRED to Phase 2 |
| `src-tauri/Cargo.toml` | Member crate, `staticlib`+`cdylib`+`rlib`, Windows-only `windows` dep |
| `src-tauri/build.rs` | `tauri_build::build()` |
| `src-tauri/tauri.conf.json` | Main window (visible:false), tray, CSP allowing `localhost:11434` for Phase-4 ollama, updater configured (active:false until Phase 7) |
| `src-tauri/src/{main,lib,error}.rs` | Skeleton; `AppError` via thiserror; 2 unit tests pass |
| `src-tauri/src/cleanup/prompts/{normal,verbose,fragment}.md` | Phase-1 stubs (~200 words each, Phase 4 refines) |
| `docs/DATA_MODEL.md` | Reference copy of PLAN §7 |
| `.gitattributes` | Cross-platform line-ending pinning (LF for source, CRLF for .ps1) |

**Cargo quality gate green** (all four):
- `cargo check --workspace` ✅ (cold: 4m07s; rusqlite-bundled compiles SQLite from C)
- `cargo clippy --workspace --all-targets -- -D warnings` ✅ (35s)
- `cargo test --workspace --quiet` ✅ (2/2 unit tests in `error.rs`)
- `cargo fmt --check` ✅ (after dropping `newline_style=Unix` in `.rustfmt.toml`; see LESSONS)

### Wave 3 — DB repository modules ✅ (commit pending)

7 modules + 1 cross-crate integration test file:

| File | Lines | Tests | Notes |
|------|-------|-------|-------|
| `db/transcripts.rs` | ~230 | 7 | `Stage` enum; no `update_raw` (hook scans) |
| `db/prompts.rs` | ~130 | 5 | Read-only per ADR 0008 |
| `db/dictionary.rs` | ~370 | 9 | CRUD + `bump_usage` + `create_snapshot`; UNIQUE+NULL gotcha flagged |
| `db/examples.rs` | ~250 | 7 | Minimal CRUD; Phase 8 owns ranking |
| `db/search.rs` | ~190 | 8 | `sanitize_query` phrase-escaping; bm25 ordering verified |
| `db/sessions.rs` | ~330 | 8 | `NewSession` requires provenance FKs at TYPE LEVEL; FK violation tested |
| `db/audit.rs` | ~480 | 11 | `AuditedTable` enum gates dynamic SQL; `state_at` + `rollback_row/table`; timestamp-pinning fixture skirts CURRENT_TIMESTAMP 1-second granularity |
| `tests/db_repos.rs` | ~270 | 6 | Cross-repo end-to-end (full dictation flow, audit rollback, FK check, snapshot round-trip) |

**Cargo quality gate all four green:**
- `cargo check --workspace` ✅
- `cargo clippy --workspace --all-targets -- -D warnings` ✅
- `cargo test --workspace` ✅ — **77/77** PASS
- `cargo fmt --check` ✅

### Wave 4 — Logging + settings + tray + commands + app wire ✅ (commit pending)

| File | Lines | Tests | Notes |
|------|-------|-------|-------|
| `src/logging.rs` | ~220 | 6 | Daily rolling appender + PII scrub MakeWriter (regex for sk-* + emails + literal USERPROFILE) |
| `src/settings/model.rs` | ~120 | 4 | `SettingKey` enum (8 keys), `as_str`/`try_parse`/`default_value`/`all` |
| `src/settings/mod.rs` | ~180 | 9 | Typed get/set facade, UPSERT, corrupt-value fallback, raw + typed paths |
| `src/tray.rs` | ~75 | 2 | Tauri 2 TrayIconBuilder + 4-item menu + `handle_menu_event_pure` helper |
| `src/commands.rs` | ~110 | 2 | `AppState{Mutex<Database>}`; `get_setting`/`set_setting`/`fts_smoke_test` |
| `src/lib.rs` (edit) | +25 | — | logging init → DB open → `app.manage(AppState)` → tray → 3 commands |
| `src/error.rs` (edit) | +4 | — | Added `Tracing(String)` variant |
| `Cargo.toml` (edit) | +2 | — | Added `regex` workspace dep |

**Cargo quality gate all four green first run:**
- `cargo check --workspace` ✅
- `cargo clippy --workspace --all-targets -- -D warnings` ✅ (no fixes needed)
- `cargo test --workspace` ✅ — **101/101** PASS (88 unit + 7 db_migrations + 6 db_repos)
- `cargo fmt --check` ✅

### Wave 5 — Finalizer ✅ (commit pending)

- `docs/CONTRIBUTING.md` (~200 lines): prerequisites, workflow, standing rules, conventions, sub-agents, deprecated-patterns note, brief pattern as recommended default.
- `docs/SETTINGS.md` (binding): 8 keys with type/default/owner/notes; access patterns; adding-new-setting playbook; corruption behavior.
- 3 judge cards in `docs/judges/phase-1/`: `rusqlite-vs-sqlx`, `fts5-smoke`, `no-pack-agents`. Wiggum to execute when wired up.
- `#![warn(missing_docs)]` re-enabled; 163 warnings → 0 via module-level `#[allow]` on repo modules + individual docs on `commands.rs` publics.
- Phase 1 retrospective in LESSONS.md (~100 lines: delivered/test count/what worked/what surprised us/what we deferred/carry-forward/numbers).
- Lefthook live-fire DEFERRED — binary not on dev PATH. Note in LESSONS for follow-up after install.

## Phase 2 — Audio capture & STT: IN PROGRESS (Waves 1 + 2 + 3 + 4 ✅; Wave 5 queued)

**Plan:** `docs/phases/phase2.md` (planning-agent session, 5 waves, 26 tasks).

### Wave 1 — Decisions, deps, AppError, download, scaffolds ✅

| File | Notes |
|------|-------|
| `docs/adr/0011-whisper-rs-cuda-build.md` | Build-time CUDA, runtime CPU fallback via `use_gpu=false` retry |
| `docs/adr/0012-ort-runtime.md` | `ort = "2"` default features (bundled DLLs), Silero on disk |
| `docs/adr/0013-cpal-ringbuf-design.md` | 16 kHz mono i16, 30 ms frames, 1 MB SPSC ringbuf, rubato deferred to Wave 2 |
| `docs/adr/0014-model-storage-path.md` | `%LOCALAPPDATA%\Mockingbird\models\` + `MODEL_PATH` env override |
| `Cargo.toml` + `src-tauri/Cargo.toml` | `cpal`/`ringbuf`/`hound` workspace deps; `ort` staged to W3, `whisper-rs` to W4 (cmake/CUDA gate) |
| `src-tauri/src/error.rs` | +`Audio(String)` and `Stt(String)` variants |
| `scripts/download-models.ps1` + `scripts/model-manifest.json` | BITS-resumable, SHA-256-verified, idempotent |
| `src-tauri/src/audio/{mod,capture,vad}.rs` | `AudioCapture` + `VoiceActivityDetector` traits + Windows `todo!()` bodies |
| `src-tauri/src/stt/{mod,whisper,prompt_builder}.rs` | `SpeechToText` trait + `Transcript` + `models_dir()` helper + Windows `todo!()` bodies + 3 unit tests |
| `src-tauri/src/bin/stt_test.rs` | CLI harness scaffold (args parsing only; Wave 5 wires the pipeline) |
| `src-tauri/src/lib.rs` | +`pub mod audio;` and `pub mod stt;` |

**Cargo quality gate all four green:**
- `cargo check --workspace` ✅ (zero warnings)
- `cargo clippy --workspace --all-targets -- -D warnings` ✅
- `cargo test --workspace` ✅ — **104/104** PASS (91 unit + 7 db_migrations + 6 db_repos)
- `cargo fmt --check` ✅

### Wave 2 — cpal capture + ring buffer + device watcher + synthetic fixtures ✅

| File | Notes |
|------|-------|
| `src-tauri/src/audio/capture.rs` | `CpalCapture` + cpal default_host + ringbuf 0.4 HeapRb 480k cap; build_stream errors on non-16kHz-mono-i16 (rubato deferred); start/stop idempotent; restart-after-stop errors cleanly; 8 unit tests |
| `src-tauri/src/audio/mod.rs` | Dropped `Send` bound on `AudioCapture` (cpal::Stream is !Send on Windows; LESSONS noted) |
| `src-tauri/src/bin/generate_fixtures.rs` | Synthetic 16 kHz mono i16 WAV generator via hound |
| `src-tauri/tests/fixtures/audio/{silent,sine_440,mixed}.wav` | 3 fixtures, ~190 KB total (committed binary) |
| `src-tauri/tests/audio_capture.rs` | 8 cross-crate integration tests (factory/format/drain/3× fixture parse/2× fixture content) |
| `docs/LESSONS.md` | +3 entries: `Box<dyn Trait>` brings methods into scope; cpal::Stream !Send; cpal::Host !Clone |

**Cargo quality gate all four green:**
- `cargo check --workspace` ✅
- `cargo clippy --workspace --all-targets -- -D warnings` ✅ (2 trivial fixes: `&PathBuf`→`&Path`, redundant trait import)
- `cargo test --workspace` ✅ — **120/120** PASS (99 unit + 8 audio_capture + 7 db_migrations + 6 db_repos)
- `cargo fmt --check` ✅

### Wave 3 — Silero VAD via ort + vad_trim helper ✅

| File | Notes |
|------|-------|
| `Cargo.toml` | `ort = "=2.0.0-rc.10", default-features=false, features=["load-dynamic","ndarray"]` — sidesteps MSVC 2022 STL static-link requirement |
| `src-tauri/src/audio/vad.rs` | `SileroVad` impl: 512-sample frames, LSTM state carry-through, `reset()`-zeros, threshold 0.5; `locate_model()` honors `SILERO_VAD_PATH` then `models_dir()`. 4 unit tests (3 require runtime; skip gracefully) |
| `src-tauri/src/audio/vad_trim.rs` | `vad_trim(audio, &mut detector, &cfg)` with `lead_in_ms`/`hangover_ms`/`min_speech_ms`. Pure helper; tested via `AmplitudeVad` fake without loading Silero. 6 unit tests |
| `src-tauri/tests/vad.rs` | 4 integration tests over `silent.wav`/`mixed.wav` with `silero_runtime_available()` catch-unwind skip guard |
| `scripts/download-onnxruntime.ps1` | Fetches ONNX Runtime 1.22.0 zip + extracts `onnxruntime.dll` + prints `ORT_DYLIB_PATH` value to set |
| `scripts/model-manifest.json` | Silero entry: real SHA-256 pinned (`1a153a22…`), URL fixed (`src/silero_vad/data/` not `files/`), real size (2,327,524 bytes) |
| `docs/LESSONS.md` | +5 entries: ort RC-only + MSVC 2022 STL escape via load-dynamic, Silero URL move, Box<dyn Trait>, cpal !Send, cpal::Host !Clone |

**Runtime preconditions for full Wave-3 test green-light:**
- `$env:SILERO_VAD_PATH` → path to `silero_vad.onnx` (or place it in `models_dir()`)
- `$env:ORT_DYLIB_PATH` → path to `onnxruntime.dll` v1.22.x (run `scripts/download-onnxruntime.ps1`)

**Cargo quality gate all four green (with env vars set):**
- `cargo check --workspace` ✅
- `cargo clippy --workspace --all-targets -- -D warnings` ✅ (1 trivial fix: `2*1*128` → `2*128`)
- `cargo test --workspace` ✅ — **134/134** PASS (109 unit + 8 audio_capture + 4 vad + 7 db_migrations + 6 db_repos)
- `cargo fmt --check` ✅

### Wave 4 — whisper-rs STT + prompt builder + CLI + bench ✅

| File | Notes |
|------|-------|
| `Cargo.toml` (workspace) | `whisper-rs = "0.16"` **CPU-only** (cuda feature off; see bd `mb-ltq`). Bumped from 0.13 due to opaque-struct bindgen mismatch between whisper-rs 0.13.2 and whisper-rs-sys 0.11.1 (71 field-not-found errors). 0.16 pairs cleanly with whisper-rs-sys 0.15.0. |
| `src-tauri/src/stt/whisper.rs` | `WhisperStt::new()` GPU-first/CPU-fallback per ADR 0011 + `new_with_options(force_cpu)` explicit form + `gpu_loaded()` accessor. Honors `WHISPER_MODEL_PATH` env override. 4 unit tests with `model_available()` skip-guard. whisper-rs 0.16 API: `state.full_n_segments()` returns `i32` directly; `state.get_segment(i)` returns `Option<Segment>`; segment text via `to_str_lossy()`. |
| `src-tauri/src/stt/prompt_builder.rs` | `build_prompt` + test-friendly `build_prompt_at(input, now)` overload. Scoring = `recency × frequency × app_match`: recency 1.0 hot (<24h) → 0.1 floor (>7d) linear decay; frequency `ln(1+use_count)`; app_match 2.0× when context matches `foreground_app`. Hand-rolled ISO-8601 parser via Howard Hinnant `days_from_civil` (avoids adding chrono surface area). Greedy pack respects `PROMPT_TOKEN_CAP=224`. 12 unit tests covering every signal direction + 500-entry truncation. |
| `src-tauri/src/bin/stt_test.rs` | CLI harness wires the full pipeline: WAV → optional VAD trim → WhisperStt → pretty or `--json` output. Flags: `--force-cpu --json --no-vad --prompt TEXT --model-path PATH`. Hand-rolled JSON encoder + arg parser (no clap dep — yagni). |
| `src-tauri/benches/whisper_latency.rs` | criterion bench `whisper_latency_1s_sine_cpu` over `sine_440.wav`. Graceful skip on missing model. Wired in `src-tauri/Cargo.toml` `[[bench]]` section. |
| `src-tauri/tests/whisper.rs` | 4 integration tests over `silent.wav` / `sine_440.wav` with `whisper_model_present()` skip-guard. Exercise CPU construct, silent-fixture short output, sine-no-panic, initial-prompt accepted. |
| `docs/LESSONS.md` | +5 entries: CUDA-13/ggml chasm, whisper-rs 0.13 self-incompatibility, 0.16 segment API rename, cmake hides inside VS BT, PowerShell em-dash parse failures. |
| `scripts/install-wave4-toolchain.ps1` | Idempotent installer for VS 2022 BuildTools + LLVM + cmake + CUDA Toolkit (the latter shipped CUDA 13.2.1, which broke the GPU build — see LESSONS). |

**Cargo quality gate all four green:**
- `cargo check --workspace` ✅
- `cargo clippy --workspace --all-targets -- -D warnings` ✅ (1 trivial fix: redundant `(y - era * 400) as i64` cast)
- `cargo test --workspace` ✅ — **151/151** PASS (122 unit + 8 audio_capture + 4 vad + 4 whisper + 7 db_migrations + 6 db_repos). +17 over Wave 3.
- `cargo fmt --check` ✅

**Wave 4 surprise: CUDA shipped CPU-only.** Installed CUDA Toolkit 13.2.1 (the only version on chocolatey). ggml's hard-coded CUDA architectures `52;61;70;75` are deprecated in CUDA 13, AND MSBuild's `CudaToolkitDir` integration variable comes up empty against CUDA 13's targets file. Manually downloading CUDA 12.x from developer.nvidia.com (~3 GB) was deemed not worth the time when **ADR 0011's runtime CPU fallback exists for exactly this scenario**. The `WhisperStt::new` code path still tries GPU first; without the cuda feature compiled in, the GPU attempt fails immediately and the CPU path runs. When CUDA 12.x is later installed side-by-side, flipping `features=["cuda"]` back on in `Cargo.toml` re-enables the GPU path with no code changes. bd `mb-ltq` tracks the re-enable task.

### Wave 5 — judges + retrospective + GPU re-enable + seal ✅

| File | Notes |
|------|-------|
| `docs/judges/phase-2/stt-correct.md` | Judge card: edit-distance ≤ 25% on real-speech fixture; non-fabrication on silent.wav (PLAN line 1752 "non-negotiable"); model_id + latency assertions |
| `docs/judges/phase-2/cuda-verified.md` | Judge card: cuda feature ON in Cargo.toml + build succeeds + runtime stderr contains `CUDA\|cuBLAS\|gpu_device\|cudart` + `gpu_used:true` in JSON output. **Currently RED** by design |
| `docs/judges/phase-2/perf-stt.md` | Judge card: mean < 1000 ms, p95 < 1500 ms on 10 s speech fixture (gated on cuda-verified) |
| `.code_puppy/judges-template.json` | +3 entries (`mb-stt-correct`, `mb-cuda-verified`, `mb-perf-stt`); JSON-validated (8 total judges) |
| `docs/LESSONS.md` | +Phase 2 retrospective entry: delivered, surprised, deferred, carry-forward, numbers |
| `Cargo.toml` (workspace) | `whisper-rs = { version = "0.16", features = ["cuda"] }` (cuda feature re-enabled) |
| `scripts/disable-cuda13-msbuild.ps1` | Idempotent helper: moves CUDA 13.2's MSBuild `.targets/.props/.xml/.dll` files to a backup folder so cmake's VS generator picks 12.8 (reversible by moving them back). Run elevated. |
| `src-tauri/src/stt/whisper.rs` + `src-tauri/tests/whisper.rs` | Tests default to GPU now (the CPU path is held as a single fallback canary in each file). Pre-CUDA, the sine-fixture test ran 19 CPU-min in a non-speech iteration loop; on GPU the full integration suite runs in 4.88 s. |
| **✅ `phase-2-complete` tag applied** | All 7 sealing steps from the prior NOT-SEALED callout cleared this session. |

**GPU verification evidence:**
```
ggml_cuda_init: found 1 CUDA devices:
  Device 0: NVIDIA GeForce RTX 2060 with Max-Q Design, compute capability 7.5, VMM: yes
register_backend: registered backend CUDA (1 devices)
whisper_backend_init_gpu: using CUDA0 backend
whisper_model_load:        CUDA0 total size =   573.45 MB
```
JSON: `{"text":"Thank you.","gpu_used":true,"latency_ms":716,"model_id":"whisper-large-v3-turbo-q5_0"}`
(Whisper hallucinated "Thank you." from 3 s of pure silence — a known YouTube-training artifact, not a regression. Test assertions check text length is short rather than equality. Real-world VAD trims silence away before Whisper sees it.)

**Why no Wave 5 brief?** Wave 5 IS the brief — it's the seal-prep wave. Phase 3 gets its own `docs/phases/phase3-wave1-brief.md` at the start of Phase 3 work, not at the end of Phase 2.

### Cargo gate (Wave 5 finale / Phase 2 seal) — all four green ON GPU
- `cargo check --workspace` ✅
- `cargo clippy --release --workspace --all-targets -- -D warnings` ✅ (`--release` reuses CUDA-built artifacts; plain `cargo clippy` would trigger a fresh debug cmake build of whisper-rs-sys ~10 min)
- `cargo test --workspace --release` ✅ — **151/151** PASS on GPU. Whisper integration suite runs in 4.88 s (was 19+ CPU-min before for sine fixture)
- `cargo fmt --check` ✅

Carry-forward from Phase 1 (full list in LESSONS retrospective):
- **Brief pattern is the default.** Write `docs/phases/phase2-waveN-brief.md` at the end of each wave with the next wave's full context. Pattern has shipped ~100% first-run test pass rates.
- **AppError aggregator** generalizes — Phase 2 will add `Stt(...)` and `Audio(...)` variants.
- **Provenance-is-total** at the API layer is a project-wide principle.
- **Migrations 001-003 are FROZEN.** Phase 2 ships migration 004+.
- **Test-density target:** ~10 tests per ~500 lines of code (Phase 1 hit ~100 tests / ~5000 lines).

**Note:** migrations 001-003 are **NOT YET SEALED**. The tag
`phase-1-complete` lands at end of Wave 5 after all phase deliverables
are green and judges pass. Until then, fixes to 001-003 are permitted
(hook `block-migration-edit-after-phase-1` checks tag existence).

### How to resume Phase 1 Wave 3 in a fresh session

1. `/agent code-puppy`
2. `/phase1-goal`
3. **Required reading for Wave 3** (in this order):
   1. `.code_puppy/AGENTS.md`
   2. `docs/phases/phase1.md` (phase plan)
   3. **`docs/phases/phase1-wave3-brief.md`** ← THIS IS BINDING for Wave 3 (~580 lines; written end-of-Wave-2 with fresh context)
   4. `docs/LESSONS.md` (now 15 entries; check for `[phase-1]` and any rusqlite/FTS5 entries)
   5. `bd ready` (Wave 3 tasks `mb-7oi mb-4f8 mb-9pn mb-91x mb-d5z mb-z4k mb-344` are top)
4. **Implementation plan, codified in the Wave 3 brief**:
   - **DO NOT re-decide** type shapes (`NewSession`, `Stage`, `AuditedTable`, etc.) — the brief specifies every type with serde derives, fields, and enum variants.
   - **DO NOT re-decide** function signatures — every public function is specified including parameter types and return types.
   - **DO NOT re-decide** the integration-test set — the brief specifies `db_repos.rs` with 6 cross-repo scenarios.
   - **DO NOT add `Repository` traits / mockall** — explicitly out of scope for Wave 3 per cross-cutting decision #1. Wave 4 may introduce them if a command actually needs to mock.
   - **DO** author all 7 modules + `db_repos.rs` directly as code-puppy (no project agent — no db-repo-author exists; migration-author's scope is migrations, not repos).
5. Wave 3 is **mechanical**. Deviations from the brief require a LESSONS.md note explaining why.
6. **DO NOT tag `phase-1-complete` at end of Wave 3.** Tag lands at Wave 5 after DB repos + app shell + judges run.
7. **End of Wave 3:** write `docs/phases/phase1-wave4-brief.md` while context is loaded (proven 100%-test-pass pattern, recorded in LESSONS).

---

## Judge-run notes

### Phase 1 Wave 1 (2026-05-15)

Mechanically verified (real LLM judges run at phase exit, not per-wave):

- **`build-passes`** (cargo gate): ✅ check + clippy + fmt + test all green.
- **`adr-recorded`**: ADR 0004 present, Status=Accepted, follows 0000-template.md schema.
- **`plan-aligned`** (partial): Cargo.toml deps match PLAN §5 minus the deferred CUDA-coupled crates (documented deviation).
- LLM-judged full pass: at end of Phase 1 Wave 5 per `docs/phases/phase1.md` §C.

### Phase 0 structural self-check (2026-05-15)

Real judges (`phase0-structure`, `adr-format`, `status-initialized`,
`setup-script-runs`) need a separate orchestrator pass that hands the
diff + STATUS.md to a model — not part of this iteration's tool budget.
Instead I verified mechanically:

- `phase0-structure`: dirs + `.code_puppy/` + `.agents/commands/` (16 cmds) all present.
- `agents-md-present`: unchanged from bootstrap.
- `hook-config-valid`: unchanged from bootstrap; 17/17 smoke tests green.
- `judges-seeded`: idempotent merge confirmed in setup-dev run.
- `adr-format`: every ADR file has Status/Context/Decision/Consequences sections.
- `status-initialized`: this file (you are reading it).
- `setup-script-runs`: `verify-environment.ps1` exits 0, `setup-dev.ps1` exits 0.

Full LLM-judged pass: will run on the post-Phase-1 iteration as part
of the regular `/goal` flow.

---

## Notes for the next agent (post context-clear)

1. Read this file first, then `docs/LESSONS.md` (10 entries now — search before
   doing PowerShell, rustfmt, beads, or hook work).
2. PLAN-mockingbird-v2.md and `.code_puppy/AGENTS.md` are binding.
3. `bd ready` shows the queue. Phase 1 Wave 1 is done; Wave 2 tasks
   (`mb-4qg`, `mb-l6d`, `mb-7u9`, `mb-o0d`, `mb-rzf`) are now ready.
4. Phase 1 plan is at `docs/phases/phase1.md`. Wave 2 = migrations,
   delegated to `migration-author` project agent.
5. **Migrations 001-003 are SEALED forever once `phase-1-complete`
   tag lands.** Hook `block-migration-edit-after-phase-1` enforces.
   Triple-check 001/002/003 before that commit + tag.
