# LESSONS

Append-only notes from prior iterations. Each entry: date, phase tag,
short title, and the non-obvious finding. Search this before starting
a new iteration in the same area.

Format:
```
## YYYY-MM-DD [phase/iteration] short title
- Context: what we were doing
- Finding: the non-obvious thing
- Action: what to do differently next time
```

---

## 2026-05-17 [phase-3-wave-4.5] `target/release/mockingbird.exe` fails with `STATUS_DLL_NOT_FOUND` from any cwd that isn't the build dir

- **Context:** Wave 4.5 smoke-tested the wired-up binary by running it directly. Got exit code `0xC0000135` (DLL_NOT_FOUND) within 100 ms, no logs, no panic message.
- **Finding 1:** `[lib] crate-type = ["staticlib", "cdylib", "rlib"]` means the exe links against `mockingbird_lib.dll` (cdylib), which Windows looks for via standard DLL search order. Running the exe from any cwd other than `target\release\` fails the load — the cdylib isn't on PATH.
- **Finding 2:** Even with cwd = `target\release\`, the binary still failed because `whisper-rs = { features = ["cuda"] }` (root `Cargo.toml` line 66) makes the build dlopen `cudart64_*.dll` at process start. CUDA 12.8 is installed at `C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.8\` but its `bin\` is NOT on system PATH — only the `cargo-with-cuda.ps1` wrapper adds it for cargo invocations.
- **Finding 3:** The diagnostic was painful because `windows_subsystem = "windows"` (set in `main.rs` for release builds) suppresses console output. Loader-time failures show only the exit code, with no stderr text.
- **Action:** Created `scripts/run-mockingbird.ps1` that (a) sets cwd to `target\release`, (b) prepends CUDA 12.8 `bin\` to PATH, (c) sets `ORT_DYLIB_PATH`, (d) sets `SILERO_VAD_PATH` + `WHISPER_MODEL_PATH` for log clarity. Phase 6 packaging will copy the CUDA + ONNX DLLs next to the exe so this is moot in the installer; for dev workflow, use the launch script.
- **Generalisation:** When diagnosing `STATUS_DLL_NOT_FOUND` on a Tauri-cdylib release binary, the suspect order is: (1) the cdylib next to the exe, (2) any `features = ["cuda"]` deps, (3) `ORT_DYLIB_PATH` for `ort` crate, (4) Visual C++ runtime. Loader Snaps (`gflags /i mockingbird.exe +sls`) would print each DLL probe to a debugger — overkill for dev but invaluable if (1)-(4) don't explain it.

## 2026-05-17 [phase-3-wave-4.5] `cargo test --lib` doesn't see `ensure_ort_dylib_set()` so VAD tests need explicit env

- **Context:** After Wave 4.5 patches, 3 VAD tests failed with `ort 2.0.0-rc.10 is not compatible with the ONNX Runtime binary found at \`onnxruntime.dll\`; expected GetVersionString to return '1.22.x', but got '1.17.1'`.
- **Finding:** `dictation::runtime::ensure_ort_dylib_set()` only runs from `lib.rs::run()` (the Tauri entry point). `cargo test` is a separate binary that links `mockingbird_lib` directly and exercises `audio::vad::tests::*` without going through `run()`. So the env autodiscovery doesn't fire and Windows resolves `onnxruntime.dll` via default search order — finding a stale 1.17.1 DLL somewhere on the system.
- **Action:** Document the env requirement: `cargo test` needs `ORT_DYLIB_PATH` set explicitly. Wave 5 (or Phase 6) should either add a `#[ctor]`-style init for tests OR mark the affected VAD tests as `#[ignore]` to keep `cargo test` clean. For now: the launch script + cargo wrapper both set the env; bare `cargo test` is the only caller that needs manual setup.
- **Generalisation:** Env-var autodiscovery in `lib.rs::run()` covers the production path but creates a silent test-only gap. If a runtime env var is required, it should be enforced uniformly — either via a shared `init_runtime_env()` function called from both `run()` and a test-init hook, or via test attributes (`#[ignore]` for env-dependent tests).



## 2026-05-17 [phase-3-wave-4] ADR scope ≠ task brief scope

- **Context:** The Wave 4 brief said "no migration 004 — ADR 0010 binding." Implementation discovered the orchestrator needs per-row `injection_status` persistence — without storage, the cross-app QA matrix can't be objectively verified after the fact.
- **Finding:** Re-read ADR 0010. It says raw-`transcripts` rows are immutable; says NOTHING about migration count. The "no migration 004" line in the brief was the brief author (me) conflating two different bindings: "raw transcripts immutable" (real) + "migrations append-only" (real but means "don't EDIT existing migrations, new ones fine"). Asked Dustin via `ask_user_question`; he picked (A) "add migration 004."
- **Action:** Migration 004 lands the nullable `injection_status` column. Brief discipline TODO: when writing a brief that cites an ADR as binding, **paste the binding sentence verbatim from the ADR into the brief**. Paraphrasing introduces drift.

## 2026-05-17 [phase-3-wave-4] `windows-rs` import names drift between consts

- **Context:** Implementing `paste.rs` test for `CF_UNICODETEXT`. First attempt: `use windows::Win32::System::DataExchange::CLIPBOARD_FORMAT;` to wrap our constant in the typed newtype. Build broke.
- **Finding:** In `windows-rs 0.56`, `CLIPBOARD_FORMAT` lives in `Win32::System::DataExchange` as a `pub struct CLIPBOARD_FORMAT(pub u16)` — but it's NOT re-exported at the module's top level for direct `use`. You have to import via its enclosing alias path or just use raw u32. For internal-only test usage, a raw `u32` constant + a comment-encoded invariant is simpler.
- **Action:** Dropped the typed wrapper test; kept `CF_UNICODETEXT_ID: u32 = 13` as a plain constant with a comment locking in the Win32 ABI guarantee (CF_UNICODETEXT has been 13 since NT 3.51). **Generalisation:** when a windows-rs type ISN'T at the top-level of its module, don't fight it for one test — fall back to documented raw values.

## 2026-05-17 [phase-3-wave-4] `&mut dyn Trait` arguments are easy to miss in trait composition

- **Context:** Orchestrator `complete()` first attempted `trim_speech(&samples, self.audio.sample_rate(), &TrimConfig::default())`. Build failed: `trim_speech` takes `&mut dyn VoiceActivityDetector`, not `u32 (sample_rate)`.
- **Finding:** I had glossed the VAD shape based on partial recall. The orchestrator needs to own a `Box<dyn VoiceActivityDetector>` AS A SEPARATE FIELD from the audio capture. The two are independent traits that happen to share a `sample_rate` constant (16 kHz). Conflating them was a category error.
- **Action:** Added `vad: Box<dyn VoiceActivityDetector>` to `DictationOrchestrator`. **Generalisation:** when composing multiple traits in an orchestrator, list them as bullet points in a docstring BEFORE writing the struct fields — forces clear thinking about which trait owns what.

## 2026-05-17 [phase-3-wave-4] Hard-coded timestamps in tests are guaranteed wrong

- **Context:** Wrote `assert_eq!(format_secs_as_iso(1_779_926_400), "2026-05-17T00:00:00Z")`. Test failed; my arithmetic was off by 11 days.
- **Finding:** Mental arithmetic on Unix epoch seconds is reliably wrong. Even careful pen-and-paper attempts miscount leap years (especially the 2000 case).
- **Action:** Corrected the value (1_778_976_000) by deriving from a known anchor (2024-01-01 = 1_704_067_200) plus precisely-counted intervening days. Added a second test using 2024-02-29 to catch leap-year regressions specifically. **Generalisation:** when a pure-math test fails, the test is the bug 90% of the time. Derive expected values from KNOWN anchors + arithmetic; never from "I think it's around…"

---


## 2026-05-17 [phase-3-wave-3] WH_KEYBOARD_LL thread-local discipline

- **Context:** Implementing `hotkey/windows.rs` real `SetWindowsHookEx(WH_KEYBOARD_LL, ...)` per ADR 0015. The callback is `unsafe extern "system" fn` — no captures, no closures, no `&self`. State has to live in a `static` somewhere.
- **Finding:** The natural temptation is `static SENDER: Mutex<Option<Sender>> = ...`. **Don't.** The callback fires on the hook-installing thread (the only thread that ever touches it), so a `Mutex` adds zero safety and risks the 300 ms hook-watchdog timeout if any other code on the same process ever locks it. The right tool is `thread_local!(static SENDER: RefCell<Option<Sender>> = RefCell::new(None))`. Uncontended by construction.
- **Action:** Used three `thread_local!` cells: `CALLBACK_TX` (the channel sender), `CALLBACK_VK` (configured VK for filtering), `CALLBACK_HHOOK` (owned hook handle for RAII unhook). All set on hook-thread entry, all cleared on exit. Drop order matters — unhook BEFORE dropping the sender (so a stray late callback can still no-op cleanly).

## 2026-05-17 [phase-3-wave-3] Pure-vs-OS split makes WH_KEYBOARD_LL testable

- **Context:** Without care, every test of `LowLevelKeyboardProc` would have to install a real hook + use real `SendInput`. Slow, flaky, and impossible on headless CI.
- **Finding:** The callback's actual work is "decide if this message + VK is interesting, and if so emit which `HotkeyEvent`". That's a pure function: `fn classify_keystroke(wparam, vk_code, configured_vk, at) -> Option<HotkeyEvent>`. The OS-side glue (read `KBDLLHOOKSTRUCT`, `try_send`, `CallNextHookEx`) is a 10-line shim with no decisions in it. **9 of the 11 unit tests in `hotkey/windows.rs` exercise the pure helper** with synthesised message/VK pairs; the 2 remaining are `#[ignore]` live SendInput round-trips.
- **Action:** Establish the pattern for the rest of Wave 3+: ANY new OS-bound module should split into a pure-helper file (or function) covered by fast unit tests, plus a thin OS shim that's `#[ignore]`-tested live. `hotkey/probe.rs` follows the same recipe (`probe_with` is pure, `probe_live` is the OS wrapper).

## 2026-05-17 [phase-3-wave-3] State-driver tick cadence is not "as fast as possible"

- **Context:** Designing `hotkey/driver.rs` — the loop that pulls from the listener channel + synthesises `HotkeyEvent::Tick` events when no real event arrives. First instinct: tick every 1 ms for sub-millisecond resolution.
- **Finding:** 1 ms ticks are pointless. The §6.1 thresholds we care about are 80 ms (hold), 300 s (max session), 30 s (cancel-threshold), 3 s (confirm-timeout). The TIGHTEST resolution we ever need is 80 ms / 4 = 20 ms. Lower cadence means: less CPU wake-up overhead on battery, less syscall churn through `recv_timeout`, and the LL-hook watchdog logger (every 250th tick = 5 min @ 20 ms) gets a clean integer count.
- **Action:** Default `DEFAULT_TICK_INTERVAL = Duration::from_millis(20)`. Tests in `driver.rs` use `Duration::from_millis(5)` for faster wall-clock test runs but the production constant stays 20 ms. **Generalisation:** when designing a tick loop, derive the cadence from the tightest deadline / (4 to 8), not from "feels precise."

---


## 2026-05-17 [phase-3-wave-2] windows-rs 0.56 HWND is isize, not pointer

- **Context:** Implementing `window_context/windows.rs` + `injection/secure_guard.rs`. First attempt used `.is_null()` and `*mut c_void` patterns that work in newer `windows-rs` releases.
- **Finding:** In `windows-rs 0.56` (our pinned version per ADR 0011 + Cargo.lock), `HWND` is `pub struct HWND(pub isize);` and `HANDLE` is `pub struct HANDLE(pub isize);`. Null check is `hwnd.0 == 0`, not `hwnd.0.is_null()`. Conversion to/from `ForegroundWindow.hwnd: isize` is the trivial `.0` access. **`windows-rs 0.61+` switched to `*mut c_void`-backed pointer types** for HWND/HANDLE — but we're not on 0.61+ and won't be in Phase 3.
- **Action:** Established pattern: pass `isize` across thread boundaries (in `ForegroundWindow.hwnd`), wrap as `HWND(isize_value)` at the OS boundary, compare against zero for null. If we ever upgrade `windows-rs`, this is one of the breaking points to watch.

## 2026-05-17 [phase-3-wave-2] GUI_SECUREINPUT is not a real Win32 constant

- **Context:** ADR 0017 specified three signals for `SecureInputGuard`, including `GetGUIThreadInfo(GUI_SECUREINPUT)`. Wave 2 implementor went to look up the constant value in `windows-rs 0.56` to use it.
- **Finding:** `GUI_SECUREINPUT` does not exist in `windows-rs` AND does not exist in the official Win32 SDK `winuser.h`. The full list of `GUITHREADINFO_FLAGS` is `GUI_CARETBLINKING (0x1)`, `GUI_INMOVESIZE (0x2)`, `GUI_INMENUMODE (0x4)`, `GUI_SYSTEMMENUMODE (0x8)`, `GUI_POPUPMENUMODE (0x10)`. The ADR author (me) conflated this with macOS's `IsSecureEventInputEnabled()` which IS a real API. The Windows reality is different: UAC consent prompts run on a separate **secure desktop** that our process can't enumerate; `GetForegroundWindow()` returns NULL during them, which trips the null-foreground guard in `window_context/windows.rs` BEFORE we ever reach the secure-input check.
- **Action:** Amended ADR 0017 with an "Update — 2026-05-17 (Wave 2)" section dropping signal 1. The remaining two signals (class-name allowlist + `ES_PASSWORD` on focused edit) are sufficient because: (a) UAC / Hello / BitLocker / Ctrl-Alt-Del trip the null-foreground guard, (b) Credential UI is caught by class name, (c) Win32 password edits are caught by `ES_PASSWORD`. WebView2 password fields remain documented as out-of-scope and are mitigated via per-app `Abort` overrides in ADR 0016.
- **Carry-forward:** When an ADR references an API, **the implementor MUST validate the API exists in our pinned dep version before sealing the ADR**. Add this to the planning-agent's "ADR review checklist".

## 2026-05-17 [phase-3-wave-2] State-machine precedence: hard stop wins

- **Context:** `HotkeyStateMachine::handle` for `(ConfirmingCancel, Tick)` originally checked `confirm_timeout` first, then `max_session`. A unit test (`max_session_overrides_confirm_cancel`) caught the latent bug.
- **Finding:** When two timeouts fire on the same tick, the precedence matters. The 300 s `max_session` is a HARD ceiling per PLAN §6.1 — without it a user who sits on the confirm-cancel toast past 300 s would have their recording grow indefinitely. The 3 s `confirm_timeout` is a SOFT revert. Hard stops must always win.
- **Action:** Reordered the branches; added a code comment explaining the precedence + a test that explicitly hits both timeouts simultaneously. **Generalisation:** state-machine code with multiple time-dependent transitions out of one state should always order the branches by "severity" — hard stops first, soft transitions after.

---


## 2026-05-17 [phase-3-wave-1] Build env, parallelism, and PowerShell stream/arg parsing

- **Context:** Phase 3 Wave 1 — five ADRs + module scaffolds + first cargo gate post-Phase-2. Hit six separate Windows toolchain papercuts before the gate went green.
- **Finding 1 — `scripts/cargo-with-cuda.ps1` is now the one-call wrapper for ALL cargo invocations in this project.** Imports MSVC env via `vcvars64.bat`, pins `CUDA_PATH` + `CUDA_PATH_V12_8` to v12.8 (ADR 0011), prepends cmake to PATH, caps `CMAKE_BUILD_PARALLEL_LEVEL=4`, then forwards args through `cmd.exe /c "cargo ... 2>&1"`. Replaces the prior "set env inline before every cargo call" pattern from Phase 2. Always invoke via `-File`, never `-Command` (see Finding 4).
- **Finding 2 — whisper-rs-sys CUDA compile OOMs at `--parallel 16` on a 16 GB machine.** Each `fattn-mma` template instance can use 2–4 GB resident RAM. With 16 cores, MSBuild fires 16 nvcc processes in parallel and one or more get killed silently, leaving 0-byte `.obj` files. The downstream `Lib.exe` then fails with `LNK1136: invalid or corrupt file`. **Fix:** export `CMAKE_BUILD_PARALLEL_LEVEL=4` (now baked into the wrapper script). Build time goes from ~5 min (when it works) to ~10 min, but the OOM is gone.
- **Finding 3 — Em-dashes in PowerShell scripts break `-File` invocations.** `powershell -File` reads the script as system code page (cp1252 on US Windows), not UTF-8. UTF-8 em-dashes (U+2014, three bytes `E2 80 94`) get split into bogus tokens (e.g. `'\libnvvp'`), parser fails downstream of the actual problem with a misleading "Unexpected token" error. **Fix:** stick to ASCII hyphens in `.ps1` files. Markdown / Rust source / ADRs can use em-dashes freely.
- **Finding 4 — `powershell -Command` eats the `--` argument delimiter; `-File` preserves it.** This kills `cargo clippy ... -- -D warnings`-style invocations. PowerShell's `-Command` parser silently swallows the `--`, sending `-D warnings` to cargo-clippy directly, which forwards it to `cargo check`, which errors out with `unexpected argument '-D'`. **Fix:** the wrapper script uses no `param` block and no `[CmdletBinding()]` — it grabs everything from `$args` — and all callers invoke via `-File`, not `-Command`.
- **Finding 5 — cmd.exe `%ERRORLEVEL%` expands at PARSE time, not run time, across `&` separators.** Writing `powershell ... & echo exit:%ERRORLEVEL%>exit.log` captures the previous-iteration exit code (typically 0), NOT the just-finished powershell's exit. **Fix:** use `call echo exit:%^ERRORLEVEL%` — the `^` escapes the `%` past parse-time, and `call` re-parses the line at run-time. Now exit codes propagate correctly.
- **Finding 6 — PowerShell pipelines treat native-command stderr as terminating errors under various `Tee-Object` / `*>&1` combinations.** Cargo writes "Compiling …" progress lines to stderr; under the wrong stream config, PowerShell promotes these to `NativeCommandError` and kills cargo mid-build. **Fix:** the wrapper invokes cargo via `& cmd.exe /c "cargo ARGS 2>&1"` — merging streams INSIDE cmd.exe means PowerShell only sees a unified text stream, no error promotion. `$ErrorActionPreference = 'Continue'` immediately before the cargo call adds belt-and-braces protection.
- **Action — all six findings now codified in `scripts/cargo-with-cuda.ps1`.** Future iterations should call `pwsh scripts/cargo-with-cuda.ps1 <cargo-args>` rather than reinventing the env-setup wheel.

---

## 2026-05-17 [phase-3-wave-1] Wave 1 retrospective

**Delivered:** 5 ADRs (0015–0019), 16 module scaffolds across `hotkey/`, `injection/`, `window_context/`, `AppError::Hotkey` + `AppError::Injection` variants, `phf` workspace dep, broader `windows-rs` feature set (UI_WindowsAndMessaging + UI_Input_KeyboardAndMouse + System_DataExchange + System_Memory + System_Threading + System_ProcessStatus), and a reusable `scripts/cargo-with-cuda.ps1` build wrapper.

**Surprised:** six separate PowerShell / cmd.exe / cmake / nvcc papercuts before the cargo gate went green. Each was one specific subtlety — ASCII-only scripts, parallelism cap, `-File` vs `-Command`, `%^ERRORLEVEL%`, cmd.exe stream merging, $ErrorActionPreference scope. Lost about 90 minutes here. None of these would have surfaced from "just write Rust code" planning — they only show up the first time a fresh shell tries to run the gate after Phase 2.

**Deferred:** none. All Wave 1 deliverables landed in one iteration.

**Carry-forward:**
- Wave 2 implementor (injection-author + code-puppy) MUST use `pwsh scripts/cargo-with-cuda.ps1` for every cargo call. No inline env setup; the script is the contract.
- Default for `WinKeyboardHook` is a non-derived impl that sets `vk = VK_RMENU`. Wave 3 must reconsider when the conflict probe (ADR 0019) resolves the binding — the constructor should accept a `vk` parameter.
- The `block-bare-paste` hook (`scripts/hooks/warn-bare-clipboard-set.py`) is shell-side only. Rust-side static enforcement of "only `injection/paste.rs` calls `SetClipboardData`" is deferred to a clippy lint or rust-analyzer rule in a later wave — YAGNI for Wave 1.

**Numbers:** 13 new tests (164 / 164 passing). 16 new files. ~600 net lines of code (counted in implementation files; ADRs are separate ~1100 lines). ADRs 0015–0019 sealed. bd tasks closed: 6 of 24 (mb-q1z, mb-3az, mb-anl, mb-jzm, mb-dlo, mb-rne).

---

## 2026-05-16 [phase-2] CUDA 12.8 install + GPU re-enable success story
- **Context:** Wave 4 punted CUDA because chocolatey only ships CUDA 13.2.1, which is too new (deprecated ggml archs + empty MSBuild `CudaToolkitDir`). Wave 5 finale: install CUDA 12.8 manually from developer.nvidia.com, side-by-side with the existing 13.2.
- **Finding 1 — Side-by-side works fine.** CUDA Toolkit installations live in version-suffixed dirs (`v12.8\`, `v13.2\`) under `C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\`. The installer asks about this politely (the "Environment Variable Check" dialog at the end is informational, not an action item) — pick custom install, uncheck Nsight / Documentation / Display Driver / HD Audio (the driver components would DOWNGRADE you from a newer driver), KEEP `Development` + `Runtime` + **`Visual Studio Integration`** under CUDA. Visual Studio Integration is the one that ships `.targets`/`.props` files into the VS BuildTools BuildCustomizations dir — without it cmake's VS generator can't build CUDA at all.
- **Finding 2 — MSBuild picks the LATEST `.targets` file alphabetically.** Even after CUDA 12.8 installs cleanly, cmake-rs's VS 2022 generator was still reading `CUDA 13.2.targets` (broken) instead of `CUDA 12.8.targets` (working) because MSBuild auto-imports all CUDA `.targets` files and tries the highest version. Fix: physically move (not delete — backup is reversible) the v13.2 `.props/.targets/.xml/.dll` files out of `C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\MSBuild\Microsoft\VC\v170\BuildCustomizations\` to a backup folder. Requires admin (UAC). See `scripts/disable-cuda13-msbuild.ps1`.
- **Finding 3 — `CMAKE_GENERATOR_TOOLSET=cuda=...` does NOT override cmake-rs.** Tried setting this env var to force cmake to use CUDA 12.8; cmake-rs is hard-coded to set `toolset=host=x64` and complained about a duplicate toolset spec. The MSBuild integration file route is the only viable path on Windows.
- **Finding 4 — Child PowerShell processes do NOT inherit User/Machine env from the registry.** When the agent spawns a fresh PowerShell, that shell inherits env from the agent's PARENT process (which started before the CUDA install). New `CUDA_PATH`, `CUDA_PATH_V12_8`, and `PATH` entries set via `[Environment]::SetEnvironmentVariable(..., 'User')` aren't visible in spawned shells until the parent process restarts. Workaround: explicitly assign `$env:CUDA_PATH = '...v12.8'` AND `$env:CUDA_PATH_V12_8 = '...v12.8'` at the top of every PowerShell invocation that calls cargo. Persisted env vars matter for future shells; the running session needs `$env:` assignments.
- **Finding 5 — `cargo clean -p whisper-rs-sys` doesn't always trigger a fresh CUDA rebuild.** Cargo computes a per-feature-set hash for each crate's build dir; two simultaneous hashes (`6bf2b1cc..` CPU-only and `9140b77c..` CUDA-on) coexisted in `target/release/build/`. The stale binary linked against the CPU artifact even though the CUDA artifact had built successfully — because the previous build's link step timed out before re-linking `stt_test.exe`. **Just run `cargo build` again after a timeout; cargo's incremental rebuild figures it out.**
- **Finding 6 — clippy uses the DEBUG profile by default, which requires a fresh debug cmake configure.** After cuda landed in `target/release/`, running plain `cargo clippy` triggered a brand-new debug build of whisper-rs-sys (~10 min, including all CUDA kernels recompiled in debug). Fix: `cargo clippy --release --all-targets`. Reuses the release artifacts; takes 60 s instead of 10+ min.
- **Finding 7 — Whisper hallucinates `"Thank you."` from pure silence.** Whisper's training data includes thousands of YouTube outro phrases; the model occasionally fabricates one from silence. **This is not a regression** — it's a documented Whisper artifact. The non-fabrication test should assert text length under ~50–100 chars, not text equality. Real VAD will trim silent buffers down to nothing before they reach Whisper in production, so this affects only `--no-vad` paths.
- **Action — Wave 5 final commit landed `phase-2-complete` tag.** 151/151 tests pass on GPU. Latency on silent.wav: 716 ms total (~600 ms is cold model load to GPU; subsequent transcribes will be sub-100 ms).

---

## 2026-05-16 [phase-2-retrospective] Phase 2 retrospective (Waves 1–5; ✅ SEALED)

**Delivered (5 waves):**
- Wave 1: 4 ADRs (0011 whisper-rs CUDA, 0012 ort runtime, 0013 cpal/ringbuf, 0014 model storage), `AudioCapture` + `VoiceActivityDetector` + `SpeechToText` traits, model-resolver + 224-token cap constant, scaffolds with `todo!()` bodies, model download script (BITS-resumable + SHA-256-verified).
- Wave 2: `CpalCapture` (cpal 0.15 + ringbuf 0.4, 16 kHz mono i16, 1 MB SPSC, 30 ms frames, start/stop idempotent), synthetic WAV fixture generator + 3 fixtures committed (silent / sine_440 / mixed), 8 integration tests.
- Wave 3: Silero VAD via ort `2.0.0-rc.10` with `load-dynamic + ndarray` features (sidesteps MSVC 2022 STL static-link demand), 512-sample frames + LSTM carry-through, `vad_trim` helper with lead-in/hangover/min-speech, 4 unit + 4 integration tests.
- Wave 4: `WhisperStt` (whisper-rs 0.16, CPU path), `prompt_builder` (recency × frequency × app-match, hand-rolled ISO-8601 parser, 224-token greedy pack), `stt_test` CLI (pretty + JSON), criterion bench skeleton, 4 whisper integration tests, 12 prompt_builder unit tests.
- Wave 5: 3 judge cards (`stt-correct`, `cuda-verified`, `perf-stt`) + 3 entries in `judges-template.json`, this retrospective. Initial Wave 5 commit held back the seal tag pending GPU verification. **Wave 5 finale (same day):** CUDA 12.8 installed side-by-side with CUDA 13.2, MSBuild integration sorted, `whisper-rs cuda` feature re-enabled, stt_test verified `gpu_used=true` on RTX 2060 — **`phase-2-complete` tag APPLIED.**

**Test count growth:** 101 (Phase 1) → 122 (W2) → 134 (W3) → 151 (W4). +50 tests over the phase, target was +40–50 — hit.

**What worked:**
- **The brief pattern keeps paying.** End-of-wave briefs nailed Wave 2/3/4 first-try compile-and-pass with one or two trivial clippy fixes. ~95% first-run pass rate.
- **Skipping gracefully > skipping silently.** Tests gated on runtime resources (`silero_runtime_available`, `whisper_model_present`) skip with a `eprintln!` — keeps CI green without `#[ignore]` hiding regressions.
- **ADRs upstream of dependency decisions.** ADR 0011 named the CUDA-fallback design BEFORE we hit the CUDA-13 chasm. When the chasm appeared the answer was "the architecture already handles this; ship CPU and re-enable later," not panic.

**What surprised us:**
- **Chocolatey's CUDA package is current-only.** No version pins available; you get whatever's latest. For tightly-coupled toolchains (CUDA 12 vs 13 is a different ABI generation), this is a footgun.
- **whisper-rs 0.13.2 ships internally inconsistent.** 71 build errors before we even touched CUDA — the wrapper accessed fields the `-sys` crate's bindgen explicitly hid as opaque. Newer crate version solved it; lesson is *don't blind-pin -sys-coupled crates*.
- **whisper-rs 0.16 API renames silently.** `full_n_segments()` → returns `i32` (was `Result`); `full_get_segment_text(i)` → `get_segment(i)` returning `Option<Segment>` with `.to_str_lossy()`. Caught at compile time but the brief was based on 0.13 shapes.
- **PowerShell parses em-dashes (U+2014) as multi-byte garbage in scripts.** Strip to ASCII before saving any `.ps1`. Already burnt this in Phase 1 — recurring.

**What we deferred:**
- **A real-speech 10s WAV fixture.** Wave 4 ships synthetic sine/silent only; the `stt-correct` + `perf-stt` judges need `hello.wav` (or similar) with an `.expected.txt` sidecar. Helios delegation candidate (Windows `System.Speech.Synthesis`).
- **Phase 3 will own the global hotkey + injection paths.** No keyboard hooks landed; the trait stubs for cross-app injection are NOT in Phase 2.

**Carry-forward to Phase 3 (cross-app injection):**
- The brief pattern: write `docs/phases/phase3-wave1-brief.md` at the start, treat it as binding.
- The `Provenance is total` invariant is about to get pressed harder — sessions table rows go from "in-memory test only" to "every hotkey press writes a row."
- Test density target: ~10 tests / 500 LoC. Phase 1 hit it, Phase 2 hit it (~50 tests / ~2,500 new lines).
- AppError-string variants generalize well; Phase 3 will likely add `Injection(String)` + `Hotkey(String)`.

**Phase 2 numbers:**
- Tests: 151 / 151 ✅ (was 101 at Phase 1 seal)
- LoC added: ~2,500 (audio + stt + vad + bin + tests + benches + scripts + ADRs)
- ADRs: 4 new (0011–0014) — all Status=Accepted
- LESSONS entries: +18 (now ~30 total)
- bd tasks: 27 of 27 Phase-2 tasks closed (including the GPU-verification seal-blocker `mb-ltq`)
- Phase tag: **`phase-2-complete` APPLIED 2026-05-16.**

---

## 2026-05-16 [phase-2] CUDA 13 + whisper-rs 0.16's bundled ggml = chasm
- **Context:** Wave 4 installed CUDA Toolkit 13.2.1 (latest, only version on choco) plus VS 2022 BT, cmake, LLVM. Tried to build whisper-rs with the `cuda` feature.
- **Finding 1:** ggml hard-codes CUDA architectures `52;61;70;75`. CUDA 13 dropped pre-Turing support — those archs no longer compile.
- **Finding 2:** MSBuild's `CUDA 13.2.targets` integration file reads `CudaToolkitDir` from somewhere that's coming up empty post-install. Either a registry timing issue or the installer not registering correctly when invoked through chocolatey.
- **Finding 3:** Chocolatey only publishes `cuda 13.2.1`. Older versions (12.x) are NOT available through the default repo — would require manual download from developer.nvidia.com (~3 GB).
- **Action:** Shipped Wave 4 CPU-only by dropping the `cuda` feature from whisper-rs in `Cargo.toml`. **This is NOT a shortcut** — ADR 0011's runtime CPU fallback was designed for exactly this scenario. `WhisperStt::new` still has GPU-first/CPU-fallback semantics; without the cuda feature the GPU attempt fails immediately and the CPU path runs. When CUDA 12.x is installed side-by-side from developer.nvidia.com (CUDA toolkits coexist in `v12.x` / `v13.x` subdirs), flip the feature back on and the GPU path activates.
- **Follow-up:** bd `mb-ltq` tracks the GPU re-enable task.

## 2026-05-16 [phase-2] whisper-rs 0.13.x ships incompatible with its own -sys crate
- **Context:** Wave 4 originally pinned `whisper-rs = "0.13"`. Build failed with 71 errors of the form `no field grammar_penalty on type whisper_full_params`.
- **Finding:** whisper-rs 0.13.2 (the high-level wrapper) and whisper-rs-sys 0.11.1 (the bindings) were published incompatible. The -sys crate's bindgen produced opaque structs (`pub _address: u8` with `size_of=264` assertion) — bindgen's signal for blocklisted types. The 0.13.2 wrapper tries to access fields the bindings explicitly hid.
- **Action:** Bumped to `whisper-rs = "0.16"`. The 0.16/sys-0.15.0 pair compiles cleanly and the field-access pattern works. **Lesson:** when a Rust crate has a sibling `-sys` crate, the high-level and low-level versions are coupled tightly; trust the crate author's pairing and use the LATEST stable rather than version-pinning blind.

## 2026-05-16 [phase-2] whisper-rs 0.16's segment API renamed methods
- **Context:** Brief specified `state.full_get_segment_text(i)` (0.13 API) and `state.full_n_segments()` returning `Result<i32>`.
- **Finding 1:** 0.16 changed `full_n_segments()` to return `i32` directly (no Result).
- **Finding 2:** 0.16 introduced a `Segment` accessor: `state.get_segment(i)` returns `Option<Segment>` (not Result); use `.to_str_lossy()` on the segment to get UTF-8-safe text.
- **Action:** Brief updated mid-execution. The 0.16 API is cleaner; document the shape in `stt/whisper.rs` comments so future bumps catch the next API drift.

## 2026-05-16 [phase-2] chocolatey package paths: cmake hides inside VS 2019 BT
- **Context:** Pre-install reconnaissance showed `where cmake` returned nothing — yet a recursive search of `C:\Program Files (x86)\Microsoft Visual Studio\2019\BuildTools` found `cmake.exe` already on disk.
- **Finding:** VS 2019/2022 BuildTools includes a cmake.exe inside `Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\`. It's NOT on PATH by default. Saved 50 MB of redundant install in option A's path; ultimately we installed standalone cmake anyway for explicit PATH access.
- **Lesson:** Before `choco install <tool>`, recursive-search the existing VS installs. Frequently the tool is already there.

## 2026-05-16 [phase-2] PowerShell em-dash bites in script files
- **Context:** `scripts/install-wave4-toolchain.ps1` failed to parse because I copy-pasted an em-dash ("—") inside a string literal. PowerShell's parser cascaded brace errors from the malformed string.
- **Finding:** PowerShell expects ASCII inside script files unless explicit BOM/UTF-8 encoding is declared. The em-dash (U+2014) and en-dash (U+2013) got mangled on save → garbled multi-byte sequences that broke string termination.
- **Action:** Strip both characters via `$c -replace [char]0x2014, '--' -replace [char]0x2013, '-'` before saving. Use ASCII hyphens in PowerShell scripts always.

## 2026-05-16 [phase-2] ort 2.0 is RC-only AND static-link demands MSVC 2022
- **Context:** Phase 2 Wave 3 added `ort` for Silero VAD. Compilation failed two ways.
- **Finding 1:** No stable ort 2.0 exists — only `2.0.0-rc.1` through `rc.12`. Plain `"2"` fails cargo ("prerelease must be specified explicitly"). Pin with `version = "=2.0.0-rc.10"`.
- **Finding 2:** With default features (`download-binaries`), ort-sys statically links a libonnxruntime.lib built with MSVC 2022 STL. On VS 2019 BuildTools, this triggers `LNK2001: unresolved external symbol __std_find_trivial_8` across dozens of `.obj` files.
- **Action 1:** Switched to `default-features = false, features = ["load-dynamic", "ndarray"]`. Skips the static lib; `dlopen`s `onnxruntime.dll` at runtime. **Version-locked:** rc.10 demands ONNX Runtime 1.22.x exactly. A 1.20.x DLL panics at startup with `expected GetVersionString to return '1.22.x', but got '1.20.1'`.
- **Action 2:** Wave 3 pinned `rc.10`, not `rc.12`. rc.12 has an internal compile bug under `load-dynamic` + default-features-off: `no field SessionOptionsAppendExecutionProvider_VitisAI on type &OrtApi`. Until that's fixed upstream, downgrade.
- **Action 3:** Added `scripts/download-onnxruntime.ps1` that fetches v1.22.0 specifically and tells you what to set `ORT_DYLIB_PATH` to. Production bundling (DLL alongside the .exe in Tauri's resources) is a Phase 4/5 concern.
- **Lesson:** When upstream is in RC, version-pin tightly and read release notes for every patch bump. ort's `load-dynamic` feature is a god-tier escape hatch when static link demands a newer toolchain than you have.

## 2026-05-16 [phase-2] Silero VAD model lives under `src/silero_vad/data/` now, not `files/`
- **Context:** Wave 1 manifest pointed at `https://github.com/snakers4/silero-vad/raw/master/files/silero_vad.onnx`. Download succeeded with HTTP 200 but the connection terminated mid-stream — the path is no longer canonical.
- **Finding:** snakers4 reorganized the repo around the Python `silero-vad` PyPI package. The ONNX file moved to `src/silero_vad/data/silero_vad.onnx`. The `raw.githubusercontent.com` host is more reliable than `github.com/.../raw/...` redirects.
- **Action:** Updated manifest URL + pinned the real SHA-256 (`1a153a22...`) and size (2,327,524 bytes). Documented the deprecation in the manifest `notes` field.

## 2026-05-16 [phase-2] `Box<dyn Trait>` brings trait methods into scope automatically
- **Context:** Wave 2 integration test imported both `make_default_capture` and `AudioCapture`. Clippy `-D warnings` flagged `AudioCapture` as unused even though I was calling `.start()`, `.drain()`, etc. on the returned `Box<dyn AudioCapture>`.
- **Finding:** When a value's type is `Box<dyn Trait>` (or `&dyn Trait`, etc.), the trait's methods are accessible **without** an explicit `use` for the trait. The trait is implicitly in scope via the type. Opposite of the usual "trait methods require trait in scope" rule for `impl Trait for ConcreteType` calls.
- **Action:** Drop redundant trait imports when working with `Box<dyn Trait>` return values. Rule of thumb: if you have a `Box<dyn Foo>` or `&dyn Foo`, you don't need `use crate::Foo;` to call Foo's methods.

## 2026-05-16 [phase-2] cpal::Stream is `!Send` on Windows
- **Context:** Wave 2 brief specified `AudioCapture: Send`. cpal's `Stream` on Windows is NOT `Send` (WASAPI handles are thread-bound).
- **Finding:** Wrapping a non-Send field in a struct that impls a `Send`-bounded trait fails with `*mut c_void` cannot be sent between threads safely`. The trait bound has to go.
- **Action:** Dropped `Send` from `AudioCapture`. Doc comment explains why; Phase 5 owns the recording thread. **Generalized lesson:** don't add `: Send` speculatively — add it when you need it, and be ready to discover an OS won't satisfy it.

## 2026-05-16 [phase-2] `cpal::Host` is `!Clone` — re-resolve in worker threads
- **Context:** Wave 2 device watcher needed Host access from a spawned thread. `let host = self.host.clone()` failed: `Host: !Clone`.
- **Finding:** cpal's `Host` is a per-platform singleton with no public clone path. Idiom: re-resolve via `cpal::default_host()` inside the thread (cheap; struct construction).
- **Action:** In `spawn_watcher`, call `let host = cpal::default_host();` inside the closure. No Host capture from outer scope.

## 2026-05-15 [bootstrap] bd (beads) lives next to STATUS.md, not instead of it
- **Context:** PLAN.md predates the decision to adopt `bd` for issue
  tracking. User asked during bootstrap if we were using beads.
- **Finding:** `bd` is the live, dependency-graph-aware task queue; STATUS.md
  is the human-readable phase snapshot the PLAN, judges, and hooks expect
  at iteration boundaries. They serve complementary roles — keep both in
  sync end-of-iteration.
- **Action:** Every iteration: `bd close <completed-ids>`, `bd create` for
  discovered work, AND update STATUS.md.

## 2026-05-15 [bootstrap] bd init is interactive and will timeout in a non-TTY
- **Context:** `bd init --prefix mb` ran for >60s because it prompted
  "Contributing to someone else's repo? [y/N]".
- **Finding:** The initial run *did* create `.beads/` before timing out;
  re-running fails because it sees the partial state.
- **Action:** Pipe `"N"` (PowerShell `'N' |`) to skip the prompt, or use a
  `--non-interactive` flag if/when bd adds one. If init is partial, just
  proceed with `bd status` — the partial state is usable.

## 2026-05-15 [bootstrap] PowerShell + native CLIs treat stderr as terminating
- **Context:** `bd create` emits a one-line warning to stderr
  ("beads.role not configured"); with `$ErrorActionPreference = "Stop"`
  PowerShell threw on the first call.
- **Finding:** PS 7's `$PSNativeCommandUseErrorActionPreference = $false`
  is the right escape hatch for native-command stderr noise.
- **Action:** In any PS script that wraps native CLIs, set
  `$PSNativeCommandUseErrorActionPreference = $false` at the top.

## 2026-05-15 [bootstrap] Hook scripts must decode subprocess stdout themselves on Windows
- **Context:** `session-start-briefing.py` crashed with cp1252 UnicodeDecodeError
  when reading `bd ready` output (em-dashes were not cp1252-encodable).
- **Finding:** `subprocess.run(..., text=True)` uses the locale codec on
  Windows (cp1252), not UTF-8.
- **Action:** Always pass `capture_output=True` without `text=True`, then
  decode `.stdout` as `utf-8, errors='replace'`. The shared
  `scripts/hooks/_lib.py` has examples.

## 2026-05-15 [phase-0] rust-toolchain.toml is a PIN, not an MSRV declaration
- **Context:** Added `rust-toolchain.toml` with `channel = "1.77"` thinking
  it would declare the project's MSRV. The next `rustc --version` call
  triggered rustup to install Rust 1.77 (a downgrade from the dev's
  installed 1.93), hanging the whole shell for ~40s.
- **Finding:** `rust-toolchain.toml` is a hard pin — rustup auto-installs
  the channel on *any* cargo/rustc invocation in that directory. MSRV
  (minimum supported) is a separate concept and belongs in
  `Cargo.toml`'s `[package] rust-version = "..."` field.
- **Action:** Do NOT commit `rust-toolchain.toml` unless you genuinely
  want every developer on the same exact Rust version. For "works on
  1.77+", use `rust-version = "1.77"` in `Cargo.toml` (added in Phase 1).
  Side lesson: `Get-Command rustc` in PowerShell will also block on the
  toolchain auto-install — diagnosis was confusing because the hang
  surfaces as a `Get-Command` hang, not a `rustup install` log.

## 2026-05-15 [bootstrap] Secret-scan hook needs a known-public-prefix allowlist
- **Context:** The Tauri updater public key in STATUS.md tripped the
  high-entropy heuristic in `block-secret-commit.py` (152-char base64 token).
- **Finding:** Public keys are *intended* to be in repos; scanning them as
  secrets is a false positive. Cleanest fix: an allowlist of well-known
  public-material prefixes (`dW50cnVzdGVkIGNvbW1lbnQ6` = Tauri/minisign,
  PEM `-----BEGIN PUBLIC KEY-----`, `ssh-rsa `, etc.) plus an inline pragma
  `pragma: allow-secret-scan` for human-vetted edge cases.
- **Action:** When adding a new "secrets you intentionally commit"
  category, extend `KNOWN_PUBLIC_PREFIXES` in `scripts/hooks/block-secret-commit.py`
  with a comment justifying the prefix. Never disable the high-entropy
  check wholesale.

## 2026-05-15 [bootstrap] PowerShell param defaults can't use $PSScriptRoot
- **Context:** `scripts/seed-judges.ps1` set a default param to
  `Join-Path $PSScriptRoot ...`; PS evaluates defaults before binding
  $PSScriptRoot, so the path was empty.
- **Finding:** Compute path defaults in the *body* of the script, not in
  the `param()` block. Also: `Join-Path` is two-arg only —
  `[IO.Path]::Combine(...)` is the n-arg version.
- **Action:** Pattern: `param([string]$X = "")` + `if (-not $X) { $X = ... }`.

## 2026-05-15 [phase-1] cargo fmt fights git autocrlf on Windows
- **Context:** Phase 1 Wave 1 first `cargo fmt --check` failed:
  `Incorrect newline style in src-tauri/src/lib.rs` even though the files
  were written with LF.
- **Finding:** Git's Windows default `core.autocrlf=true` converts LF to
  CRLF on checkout. rustfmt with `newline_style = "Unix"` then reads back
  CRLF and fails. The two settings fight each other.
- **Action:** Drop `newline_style` from `.rustfmt.toml` (default = Auto,
  accepts file as-is). Add `.gitattributes` with `*.rs text eol=lf` to
  pin LF cross-platform on next checkout. gitattributes is the single
  source of truth; rustfmt becomes ending-agnostic.

## 2026-05-15 [phase-1] rustup minimal toolchains do not include rustfmt or clippy
- **Context:** Fresh Rust install attempting `cargo fmt` produced
  `cargo-fmt.exe is not installed for the toolchain stable-x86_64-pc-windows-msvc`.
- **Finding:** rustup ships only the compiler by default; rustfmt and
  clippy are components, not bundled.
- **Action:** Always `rustup component add rustfmt clippy` as part of
  dev setup. Phase 1 Wave 5 task `p1-lefthook-verify` should add this
  to `setup-dev.ps1`.

## 2026-05-15 [phase-1] First cargo check with rusqlite-bundled takes ~4 minutes
- **Context:** First `cargo check --workspace` after Phase 1 Wave 1.
- **Finding:** 247 seconds (4m07s) cold-cache. `rusqlite` features=["bundled"]
  compiles SQLite from C source (~150k lines). One-time cost; incremental
  builds are seconds.
- **Action:** Budget the cold compile when planning iterations on a
  fresh checkout. CI should cache `target/` aggressively. Do NOT panic
  when cargo check appears to hang for 3-4 minutes on a fresh clone.

## 2026-05-15 [phase-1] Wave-specific briefs ship integration-test pass rates above 90% on first compile
- **Context:** Phase 1 Wave 2 — migrations 001-003 + runner + 7 integration tests.
  The wave was preceded by `docs/phases/phase1-wave2-brief.md` (~300 lines)
  written end-of-Wave-1 by code-puppy with fresh context, capturing every
  design decision PLAN §7 didn't pin down: audit-trigger SQL extrapolated
  to all 4 tables, runner file layout with function signatures,
  integration-test specs with exact assertion counts, PLAN bug flagged
  (`dictionary.OLD.enabled` doesn't exist).
- **Finding:** With the brief, migration-author delivered 4 files in one
  shot. Compile produced 9 trivial `From<rusqlite::Error>` errors (mechanical
  fix — add a variant to AppError). **Tests: 15/15 passed first run, including
  all 7 cross-crate integration tests.** Zero 5-attempt escalations. Zero
  surprise architectural decisions made under pressure.
- **Action:** **Pattern: at the end of every iteration, write a brief for
  the next wave** with full context. Briefs that work well: full SQL/code
  snippets (not just "do X"), exact assertion counts, flagged source-doc
  bugs, explicit deviations from canonical (PLAN) with reasons, visibility
  notes for cross-crate concerns. The cost (~one iteration of context to
  write) pays back ~3x in implementation efficiency. Adopt for Waves 3, 4,
  5 of Phase 1 and every multi-iteration phase going forward.

## 2026-05-15 [phase-1] `#[cfg(test)]` does NOT carry across crate boundaries
- **Context:** Wave 2 brief originally specified `#[cfg(test)]` on
  `Database::open_in_memory()`. migration-author flagged: integration tests
  in `src-tauri/tests/db_migrations.rs` are a **separate crate** from the
  `src-tauri` library crate, so `#[cfg(test)]` items in `src-tauri/src/`
  are invisible to them.
- **Finding:** `#[cfg(test)]` only enables items when the **current crate**
  is being compiled in test mode. Integration tests (`tests/*.rs`) build
  the library crate in **release mode** (not test mode), then link against
  it as a regular dependency. Items needed by integration tests must be
  `pub` (or `pub(crate)` if behind a shim).
- **Action:** For any helper that integration tests need (test-database
  fixtures, `open_in_memory`, etc.): make it plain `pub` with a doc
  comment marking it test-oriented. If you want to discourage production
  callers, gate behind a Cargo feature like `test-helpers` instead of
  `#[cfg(test)]`.

## 2026-05-15 [phase-1] AppError variants are added per-module as the modules come online
- **Context:** Wave 2 db module's first compile failed with 9 instances of
  `From<rusqlite::Error>` not implemented for AppError.
- **Finding:** I (code-puppy) preloaded AppError in Wave 1 with `Io` and
  `Tauri` variants only — the others get added when their source modules
  first compile. This is the right pattern (YAGNI: don't pre-declare error
  variants for modules that don't exist yet) and the fix is mechanical
  (add one `#[error("sqlite error: {0}")] Sqlite(#[from] rusqlite::Error)`
  variant).
- **Action:** When a new module fails to compile with `From<...>` errors,
  the fix is always: add a `#[from]` variant to `AppError` in `error.rs`.
  Don't refactor to module-local error types — the AppError aggregator is
  the explicit project-wide pattern (per `.code_puppy/AGENTS.md` Rust
  conventions). When in doubt, check `error.rs` first.



### Delivered (5 waves, 5 commits + 4 brief commits + seal)

- **Wave 1** (`8e70d7c`): scaffolding, error aggregator, ADR 0004, Cargo workspace, tauri.conf.json. 5 tests.
- **Wave 2** (`b1f39ff`): migrations 001-003 (4 files), runner with PRAGMA + integrity_check + foreign_key_check, prompt_loader with token substitution. **15/15** tests first run.
- **Wave 3** (`7dada9d`): 7 DB repository modules (transcripts, prompts, dictionary, examples, search, sessions, audit) + `tests/db_repos.rs`. **77/77** tests after 2 trivial test-only fixes (raw-string quote count, SQL UNIQUE+NULL gotcha).
- **Wave 4** (`c7d3faa`): logging (rolling appender + PII scrub), settings (typed facade + 8-key registry), tray (placeholder menu), commands (3 IPC handlers), app wire. **101/101** tests **first run** — zero fixes needed.
- **Wave 5** (this commit): docs/CONTRIBUTING.md, docs/SETTINGS.md (binding), 3 judge cards, `#![warn(missing_docs)]` re-enabled, retrospective, seal commit + `phase-1-complete` tag.

### Final test count

**101 tests** across the workspace, all green:
- 88 unit tests inside `src-tauri/src/`
- 7 integration tests in `tests/db_migrations.rs`
- 6 integration tests in `tests/db_repos.rs`

### What worked

1. **The brief pattern.** End-of-wave handoff briefs (`docs/phases/phase1-waveN-brief.md`) that specify types, function signatures, test specs, known risks, and explicit deviations from PLAN. Outcome: 3 consecutive ~100% first-run test pass rates. The pattern is now the documented default for any multi-iteration phase.
2. **AppError aggregator with `#[from]` variants.** New modules add a variant when they bring a new source error type. Mechanical, predictable, no abstraction debt.
3. **`Database::open_in_memory()` (plain `pub`, not `#[cfg(test)]`).** Bridged the cross-crate test boundary; integration tests get a fully-migrated DB in ~5ms.
4. **Typed registries.** `SettingKey` enum + `default_value` + `try_parse` + `all()` makes adding a setting a 4-step mechanical edit with no string-typing.
5. **`AuditedTable` enum gating dynamic SQL.** Zero SQL-injection surface in the audit/rollback path despite needing to UPDATE/INSERT/DELETE arbitrary tables.
6. **Provenance-is-total enforced at API layer, not schema.** `NewSession` requires `i64` (not `Option<i64>`) for FKs that SQL leaves nullable. The schema and API deliberately disagree.

### What surprised us

1. **`#[cfg(test)]` doesn't carry across crate boundaries.** Integration tests in `tests/*.rs` are a separate crate; `pub` is required for helpers they consume.
2. **SQL UNIQUE treats NULL as distinct.** Two rows with `app_context: None` both pass a `UNIQUE(term, app_context)` constraint. Fix: test with non-null values, or use a partial INDEX with COALESCE.
3. **SQLite `CURRENT_TIMESTAMP` has 1-second granularity.** Audit-rollback tests would race within the same second. Workaround: `pin_latest_at` test helper that overwrites the `at` column to a synthetic timestamp after the trigger fires.
4. **`#![warn(missing_docs)]` is hostile to repo modules with self-documenting fields.** 163 warnings for fields like `pub id: i64`. Resolution: keep the lint at the crate level, allow at the module level for repo modules, doc the small-API modules (commands, tray, logging) properly.
5. **Rolling 4-minute cold `cargo check`** because `rusqlite-bundled` compiles SQLite from C. One-time cost. Document so future contributors don't panic.
6. **PowerShell `Select-String` matches inside comments** when counting code patterns. Anchor with `^` or run via SQLite for ground truth.
7. **`tracing_subscriber::try_init` is once-per-process.** Test isolation matters; only call inside test code that's certain it's the first.

### What we deferred (intentional, captured in phase ownership)

- **Mockall trait abstractions** (Wave 3 brief — YAGNI; Wave 4 didn't need them either). Reintroduce only when a specific command/UI surface needs to mock a repo.
- **DBOS** (bootstrap step 3 — skipped per project owner).
- **Pack agents** (deprecated upstream — `no-pack-agents` judge enforces).
- **Operator-aware FTS5 query parsing** (Phase 6 history viewer brief). Phase 1 ships conservative phrase escaping.
- **Audio retention enforcement** (Phase 5).
- **Real example ranking + auto-selection** (Phase 8 learning loop).
- **`ClaudeApiKeyRef` actual Credential Manager lookup** (Phase 4).
- **Tray icon state transitions** (Phase 5 recording lifecycle).
- **Cross-app injection** (Phase 3 — requires human at keyboard).
- **Lefthook live-fire verification** — lefthook binary not on dev machine PATH this iteration. Config in `lefthook.yml` looks correct. Install (`scoop install lefthook` or equivalent) and run a real commit through pre-commit; append observations here.
- **`missing_docs` polish for repo modules** — applied `#[allow]` at module level rather than doc-ing every self-evident field. Phase-6 UI work may add field-level docs where they matter.

### Carry-forward for Phase 2+

- **Brief pattern is now the default.** Every multi-iteration wave gets `docs/phases/phaseN-waveM-brief.md` written end-of-current-iteration with full context.
- **LESSONS.md is institutional memory.** Append non-obvious findings as you hit them, not at retrospective time.
- **STATUS.md is the canonical handoff document.** Resume instructions, last-judge line, cost line, blocked-on section all live there.
- **AppError aggregator pattern** generalizes. Phase 2 will add `Stt`, `Audio` variants; Phase 3 will add `Injection`; Phase 4 will add `Claude`.
- **Provenance-is-total at the API layer** is a project-wide principle, not a Phase 1 quirk. Future repos honor it.
- **The `phase-N-complete` tag SEALS its migrations.** Phase 2 ships migration 004+; the previous numbers are now frozen forever.
- **Test-density target:** ~10 tests per ~500 lines of code. Phase 1 hit ~100 tests / ~5,000 lines.

### Numbers for posterity

- **Files created:** ~30 (modules) + ~10 (docs) + ~10 (judges/briefs).
- **Lines of code:** ~5,000 Rust + ~1,500 SQL + ~3,000 markdown.
- **bd tasks closed:** 25/25 Phase 1 tasks (plus 11 Phase 0 tasks).
- **Commits:** 9 (bootstrap + Phase 0 + Wave-1-brief + Wave 1 + Wave-2-brief + Wave 2 + Wave-3-brief + Wave 3 + Wave-4-brief + Wave 4 + Wave-5-brief + Wave 5 + seal).
- **Test pass rates per wave:** W1 5/5, W2 15/15, W3 75→77 (2 test fixes), W4 101/101 first run, W5 101/101 still.

---

## 2026-05-15 [phase-1] SQL UNIQUE treats NULL as distinct (`NULL != NULL`)
- **Context:** Wave 3 dictionary repo test `unique_term_app_context_is_enforced`
  inserted two rows with `term='Foo', app_context=NULL` expecting the UNIQUE
  constraint to fire. Both inserts succeeded.
- **Finding:** Standard SQL semantics: `NULL != NULL` for purposes of
  UNIQUE constraints. Two rows with NULL in the same UNIQUE column are
  considered distinct and both allowed. This is a famous SQLite gotcha
  (also true in Postgres, MySQL, etc).
- **Action:** For null-equal-null semantics, use a partial UNIQUE INDEX
  on `COALESCE(col, '')` or similar — that's a schema change requiring
  a future migration. For Phase 1 we test the constraint with a
  non-null value where UNIQUE actually fires. Phase 6 dictionary UI
  may want the null-equal-null behavior.

## 2026-05-15 [phase-1] SQLite `CURRENT_TIMESTAMP` has 1-second granularity
- **Context:** Wave 3 audit-rollback tests insert→update→rollback. Each
  audit-trigger fire timestamps with `CURRENT_TIMESTAMP` which only has
  per-second resolution. Two operations within the same second get
  identical `at` values, breaking the `state_at` algorithm's ordering.
- **Finding:** Sleeping ≥1s between ops works but makes tests slow.
  Cleaner: after each real operation, UPDATE the just-created history
  row's `at` field to a known synthetic timestamp. The audit table has
  no constraint preventing this — it's an internal-record-of-fact
  table, not a contract. Pattern (added as `pin_latest_at` helper):
  ```rust
  conn.execute("UPDATE _history_X SET at = ?1 WHERE id = (SELECT MAX(id) FROM _history_X)", [ts])?;
  ```
- **Action:** Use synthetic `at` values for any test that depends on
  temporal ordering. Keep this trick test-only — production code
  trusts `CURRENT_TIMESTAMP`.

## 2026-05-15 [phase-1] `#![warn(missing_docs)]` is hostile to repo modules with self-documenting fields
- **Context:** Wave 1 added `#![warn(missing_docs)]` at the top of
  `lib.rs`. Wave 3 added 7 repository modules with ~60 public structs/
  enums/fields where the field name IS the documentation (`pub id: i64`,
  `pub term: String`, etc.). Clippy spammed 60+ missing-doc warnings
  and `clippy -D warnings` refused to ship.
- **Finding:** Mandatory module-level docs are valuable. Mandatory
  field-level docs are noise when the field name is self-evident.
- **Action:** Demoted `missing_docs` from `warn` to nothing for now;
  Wave 5 polish task will (a) add doc comments to non-self-documenting
  public items, (b) re-enable the lint, (c) `#[allow(missing_docs)]`
  on the obvious cases like `pub id: i64`. Don't blanket-enable lints
  faster than you can comply with them.

## 2026-05-15 [phase-1] PowerShell Select-String matches inside comments — grep regexes need context
- **Context:** Sanity-checking the trigger count after Wave 2: I expected 14
  triggers (per the brief), but `Select-String -Pattern 'CREATE TRIGGER'`
  returned 15.
- **Finding:** One of those matches was inside a `--` SQL comment in
  `002_audit_triggers.sql` ("-- new migration that CREATE TRIGGER IF NOT
  EXISTS-replaces the offender"). Substring match doesn't distinguish
  code from comments.
- **Action:** For exact code counts, anchor the pattern: e.g.
  `Select-String -Pattern '^CREATE TRIGGER'` (line starts with) or
  `'^\s*CREATE TRIGGER'` (optional indent). Or use `sqlite3 :memory: < file.sql`
  followed by `SELECT COUNT(*) FROM sqlite_master WHERE type='trigger'`
  for the ground truth. The integration test asserts the ground truth
  (`trigger_count_is_14`) and that's the canonical check.
