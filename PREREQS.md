# Prerequisites

What you need to run Mockingbird, and what's optional but nice.

## Always required

### Operating system

- **Windows 10 build 19041 (May 2020 Update) or newer**, or **Windows 11** (any build). 64-bit only; there is no 32-bit build.
- **macOS 15 (Sequoia) or newer, Apple Silicon** (M1 or later). Install with the Homebrew cask (primary path) or the downloadable `.dmg`; building from source is also supported. Nothing on macOS is Apple-signed, so the first launch needs a one-time Gatekeeper approval. The floor is macOS 15 because meeting capture uses ScreenCaptureKit's single-session system-audio API, which is 15+. See [`INSTALL.md`](./INSTALL.md#macos-apple-silicon) for all three paths.

Older Windows 10 builds (pre-19041) lack the WebView2 APIs and modern audio capture surface Mockingbird relies on. Intel Macs are not supported (no Metal-tuned build); Linux has no build yet.

> **What's on macOS today:** voice dictation and meeting capture, both with local LLM cleanup, at Windows parity. **Windows-only for now:** Activity capture, the Knowledge Graph pipeline, and Mobile Sync — these show as "coming soon" in the Mac build.

### Runtime libraries

- **Microsoft Edge WebView2 Runtime.** Preinstalled on Windows 11. On Windows 10 you may need to install it manually from [the WebView2 page](https://developer.microsoft.com/en-us/microsoft-edge/webview2/). The MSI installer will prompt you if it's missing.
- **Visual C++ Redistributable 2022 (x64).** The installer carries this. If you build from source, install it from [Microsoft](https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redists).

### Hardware

- **CPU:** Any 64-bit x86 CPU from the last 8 years. SSE4.2 and AVX2 are assumed.
- **RAM:** 8 GB minimum, 16 GB recommended (especially if you also run Ollama).
- **Disk:** 4 GB free for the Whisper model and SQLite database. More if you enable meeting recording retention.

## Optional but recommended

### GPU acceleration (NVIDIA)

Mockingbird ships in two installer variants on the [Releases](../../releases) page. Pick the one that matches your hardware.

- **`Mockingbird_x.y.z_x64_en-US.msi`** (about 9 MB download). CPU-only Whisper. Runs on any 64-bit Windows machine that meets the rest of the prerequisites. Transcription is slower than GPU mode but works everywhere.
- **`Mockingbird-CUDA_x.y.z_x64_en-US.msi`** (about 580 MB download). NVIDIA GPU Whisper via cuBLAS. The MSI bundles the NVIDIA CUDA runtime libraries (cudart, cublas, cublasLt) so you do NOT need to install the CUDA Toolkit separately. The only user-side prereq is an NVIDIA driver, which ships with every NVIDIA GPU and auto-updates via Windows Update or GeForce Experience.

The CUDA variant requires an NVIDIA GPU. On a machine with an AMD GPU, Intel integrated graphics, or no GPU, install the CPU variant instead. Installing the CUDA variant on a non-NVIDIA machine will produce a launch failure with a missing-DLL error.

The two variants install side by side under distinct Add/Remove Programs entries (`Mockingbird` and `Mockingbird-CUDA`) so switching between them does not require uninstalling the other first.

If you are building from source and want to compile WITH CUDA support, see [`INSTALL.md`](./INSTALL.md) Tier 3. CUDA Toolkit 12.8 is required at build time only; runtime requirements are unchanged.

### Ollama for local LLM cleanup

- [Ollama](https://ollama.com/) installed and running on `http://localhost:11434` (the default). Available for both Windows and macOS.
- A pulled chat model. The recommended default is `qwen2.5:7b-instruct-q4_K_M` (about 4.7 GB).
- The smaller `qwen2.5:3b` is documented as a degraded "tags only" mode for the knowledge graph features. It is not recommended for general cleanup on Windows.
- **macOS is RAM-aware (ADR 0064).** On Apple Silicon, Mockingbird picks the cleanup model from unified memory: **16 GB or more → the parity 7B**; **less than 16 GB (e.g. an 8 GB Mac) → auto-downshift to `qwen2.5:3b-instruct-q4_K_M`** (about 1.9 GB) so it coexists with Whisper-Metal. Pull whichever your Mac will use. See [`INSTALL.md`](./INSTALL.md#local-cleanup-ollama-on-macos).

Without Ollama (or a Claude API key, see below), dictation still produces a raw Whisper transcript. You just lose the punctuation and filler-word cleanup pass.

The current cleanup provider speaks Ollama's native API. Support for generic OpenAI-compatible local LLM servers (LM Studio, llama.cpp server, Jan, vLLM, Ollama's own OpenAI-compat endpoint, etc.) is planned for a future release.

### Anthropic Claude API for cloud cleanup

- An [Anthropic API key](https://console.anthropic.com/).
- Network access to `api.anthropic.com`.
- Note that opting in to cloud cleanup sends your transcript text to Anthropic per their privacy policy. See [`PRIVACY.md`](./PRIVACY.md) for the full data flow guarantees.

This is mutually exclusive with Ollama at the per-mode level: each cleanup mode (casual, normal, formal) routes to one provider.

### Obsidian vault for knowledge graph capture

- An [Obsidian](https://obsidian.md/) vault folder somewhere on your disk. The vault can be empty; Mockingbird will create its own subtree inside it.
- Optional: [Obsidian Sync](https://obsidian.md/sync) or any folder-level sync (Dropbox, OneDrive, iCloud, Syncthing) if you want the vault available on multiple devices.

Mockingbird does not synchronize SQLite across machines. Cross-device knowledge sharing happens through the vault's Markdown files.

### Mobile sync for iPhone capture

- A sync provider that mirrors a folder between your iPhone and your PC. iCloud Drive, OneDrive, Dropbox, and Google Drive all work. You probably already have one set up.
- An iOS Shortcut configured to drop captures into that synced folder. See [`docs/mobile/`](./docs/mobile/) for the recipes (one for general dictation capture, one for knowledge graph capture).

Mockingbird's vault watcher picks up files as soon as your sync provider mirrors them to disk. Latency end to end depends on your provider; iCloud Drive is typically a few seconds for small files.

### Unsplash API key for ambient backgrounds

- A free [Unsplash developer account](https://unsplash.com/developers). The free tier is 50 requests per hour, more than enough for ambient backgrounds.
- Paste the key into Settings -> Appearance -> Unsplash. The key is encrypted via Windows DPAPI.

Backgrounds are purely cosmetic. The app works without this.

## For building from source

### Windows

In addition to the runtime requirements above:

- **Rust 1.77 or newer** via [rustup](https://rustup.rs/).
- **Node 20 or newer** via [nvm-windows](https://github.com/coreybutler/nvm-windows) or the Node installer.
- **Git** (any recent version).
- **Visual Studio Build Tools 2022** with the "Desktop development with C++" workload. MSVC is required to compile the Rust native dependencies.
- **CMake 3.22 or newer**, usually picked up automatically if Visual Studio is installed.
- *(Optional)* **CUDA Toolkit 12.8** if you want CUDA-accelerated Whisper from your local build.

See [`INSTALL.md`](./INSTALL.md) Tier 3 for the actual build commands.

### Building from source on macOS (Apple Silicon)

Only needed if you build the `.app` yourself instead of using the Homebrew cask or the `.dmg`. You need:

- **macOS 15 (Sequoia) or newer** on **Apple Silicon** (M1+).
- **Xcode Command Line Tools** — `xcode-select --install` (provides the clang compiler + git).
- **CMake** — `brew install cmake`. This is a **hard prerequisite**: `whisper-rs-sys`'s `build.rs` shells out to CMake to compile the bundled `whisper.cpp`. Unlike Windows (where Visual Studio ships CMake), the macOS Command Line Tools do **not** include it.
- **jq** — `brew install jq`. Used by `scripts/download-models.sh` to read the model manifest.
- **Rust 1.77 or newer** via [rustup](https://rustup.rs/).
- **Node 20 or newer** (e.g. `brew install node`).
- **[Ollama for macOS](https://ollama.com/download)** if you want cleanup (optional; see the RAM-aware note above).

Whisper runs on the **Metal** GPU backend — no CUDA on macOS. The build
bundles the Whisper + Silero models into the `.app`, so macOS users never
fetch models separately (unlike Windows, where the first launch downloads
a Whisper model of roughly 500 MB to 2 GB). This holds for the Homebrew
cask and the `.dmg` too. See
[`INSTALL.md`](./INSTALL.md#macos-apple-silicon) for the
exact build commands, permissions, and Gatekeeper steps.
