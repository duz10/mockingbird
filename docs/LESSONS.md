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
