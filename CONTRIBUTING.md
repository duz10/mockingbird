# Build Your Own Mockingbird

This is a reference implementation. The maintainer is solo and builds
Mockingbird primarily for personal daily use. Pull requests are
welcomed but are not prioritized: most outside PRs will be closed
without merge. This is not personal; it is a capacity statement. The
project is MIT licensed and fork-friendly, and the fruitful path for
outside developers is to fork it and ship the version of Mockingbird
you actually want.

This document is the guide for doing that successfully.

## What to read first

In this order, before touching code:

1. [`ARCHITECTURE.md`](./ARCHITECTURE.md). The subsystem map. One page,
   ~200 lines, covers every directory.
2. [`src-tauri/src/`](./src-tauri/src/). The Rust backend. Each
   subsystem is its own module with a `mod.rs` that documents the
   intent.
3. [`ui/src/`](./ui/src/). The React frontend. Pages, components,
   design tokens, i18n.
4. [`PRIVACY.md`](./PRIVACY.md) and [`SECURITY.md`](./SECURITY.md).
   The non-negotiable user guarantees. A fork that breaks them should
   probably not call itself Mockingbird.

## Common fork ideas

The architecture is intentionally pluggable in a few specific places.
If you want to do one of these, you are in good shape: the seam is
already there.

- **Swap the cleanup LLM.** `src-tauri/src/cleanup/` defines a
  `CleanupProvider` trait. Implement it for OpenAI, Gemini,
  llama.cpp, an in-process model, whatever. Add a provider enum
  variant and a settings UI tab. No other subsystem needs to know.
- **Swap the STT engine.** `src-tauri/src/stt/` is where whisper-rs is
  wrapped. The trait surface is small: take audio in, emit transcript
  events out. A fork could target Groq, Deepgram, faster-whisper, or
  a remote Whisper server.
- **Add a capture surface.** Dictation, meeting capture, and knowledge
  graph capture are the three current modes. Add a fourth by following
  the dictation orchestrator pattern: hotkey trigger, capture session,
  STT, optional cleanup, sink (clipboard paste, file, database row,
  vault projection, whatever).
- **Port to macOS.** See [`docs/research/macos-implementation-notes.md`](./docs/research/macos-implementation-notes.md).
  This is the most-developed alternate-platform path; the document is
  written explicitly to enable a forker.
- **Port to Linux.** No research doc yet, but the `#[cfg(target_os)]`
  pattern is already in place in `audio/`, `hotkey/`, `injection/`,
  and `secrets/`. The shape is similar to the macOS port: replace the
  Windows-specific implementations behind each trait.

## Local development setup

These steps assume a clean Windows machine. For full system
requirements see [`PREREQS.md`](./PREREQS.md).

1. **Install Rust 1.77 or newer** via [rustup](https://rustup.rs/).
2. **Install Node 20 or newer** via [nvm-windows](https://github.com/coreybutler/nvm-windows)
   or the [Node installer](https://nodejs.org/).
3. **Clone the repo** and `cd` in.

   ```pwsh
   git clone https://github.com/duz10/mockingbird.git
   cd mockingbird
   ```

4. **Run the dev setup script.** This verifies your toolchain,
   installs the npm dependencies with `--ignore-scripts` (defending
   against postinstall supply-chain attacks), and prints any missing
   prerequisites.

   ```pwsh
   powershell -File scripts\setup-dev.ps1
   ```

5. **Build the release binary.** All cargo invocations on Windows go
   through the project wrapper, which imports the MSVC + CUDA
   environment before invoking cargo. Plain `cargo build` will compile
   but may produce a binary that cannot find its CUDA or ONNX runtime
   DLLs at launch.

   ```pwsh
   powershell -File scripts\dev\cargo-with-cuda.ps1 build --release
   ```

   First build pulls down the CUDA Whisper artefacts and takes 15-30
   minutes. Warm-cache rebuilds are ~3 minutes.

6. **Fetch the runtime models.**

   ```pwsh
   powershell -File scripts\download-onnxruntime.ps1
   powershell -File scripts\download-models.ps1
   ```

7. **Launch.** Use the launcher script, not the exe directly. The
   launcher sets `ORT_DYLIB_PATH` and prepends the CUDA bin directory
   to PATH so the binary can find `onnxruntime.dll` and `cudart64_12.dll`
   at process start.

   ```pwsh
   powershell -File scripts\run-mockingbird.ps1
   ```

## Running tests

```pwsh
# Rust: link-check + clippy + format
powershell -File scripts\dev\cargo-with-cuda.ps1 fmt --check
powershell -File scripts\dev\cargo-with-cuda.ps1 clippy --release -- -D warnings
powershell -File scripts\dev\cargo-with-cuda.ps1 test --release --no-run

# UI: build, type-check, unit tests
cd ui
npm run build
npx tsc --noEmit
npm test
cd ..
```

There is a known issue on the maintainer's primary Windows dev box
where `cargo test --release` (without `--no-run`) exits with
`STATUS_ENTRYPOINT_NOT_FOUND` (0xc0000139) during the test runner load
sequence. The test binaries themselves link clean, which validates
type system, trait surface, and link correctness. On Linux and macOS
the runner executes normally. The shipping app binary is unaffected.

If you want to actually execute the test suite on Windows, the
documented workaround is to copy a pure-Rust module's sources into a
throwaway crate without the whisper-rs / ort / cuda dependencies and
run `cargo test` there. Less neat than running the full suite in place,
but it works for narrow regression checks. The Rust + Tauri test
binaries also run cleanly on a Linux developer VM.

## Code style

- **Rust:** edition 2021, MSRV 1.77. `cargo fmt` is law.
  `cargo clippy --release -- -D warnings` must pass (the `--release`
  flag is required to reuse the whisper-rs CUDA build artefacts;
  without it clippy will rebuild from scratch and OOM the linker).
  `Result<T, E>` everywhere. `unwrap()` only in tests. `thiserror` for
  module error types. `tracing` for logs, never `println!`. Files cap
  at 600 lines; split into submodules when they grow past that.
- **TypeScript:** strict mode. No `any` without a `// SAFETY:` comment
  explaining the cast. React 19 conventions, no class components.
  Tailwind v4 only; design tokens from `ui/src/design/tokens.css`. No
  `@tanstack/*` packages.
- **Tests:** every non-trivial function gets one. Test files mirror
  source layout. `rstest` for parameterized cases, `proptest` for
  property checks, `mockall` at trait boundaries.
- **No telemetry. No analytics. No phone-home. Ever.** A PR that adds
  any of these will be rejected without further review.

## Submitting a PR anyway

If you have read everything above and still want to land a PR upstream
rather than fork:

1. Open a [Discussion](../../discussions) describing the change before
   you write code. The maintainer will tell you frankly whether it has
   a chance.
2. Match the scope to one logical change. PRs that conflate unrelated
   changes will be closed.
3. Keep the diff small. Big speculative refactors will be closed.
4. Include tests. Untested changes will be closed.
5. Sign off in the commit message that the change adds no telemetry,
   no analytics, and no phone-home behaviour.

Even with all of that, most PRs will be closed without merge. Fork
freely. That is what the MIT license is for.

## Security disclosures

For security issues specifically, do **not** open a public issue or
Discussion. Use [GitHub Security Advisories](../../security/advisories/new)
on this repository. Full guidance in [`SECURITY.md`](./SECURITY.md).
