# Prerequisites

What you need to run Mockingbird, and what's optional but nice.

## Always required

### Operating system

- **Windows 10 build 19041 (May 2020 Update) or newer**, or **Windows 11** (any build).
- 64-bit only. There is no 32-bit build.

Older Windows 10 builds (pre-19041) lack the WebView2 APIs and modern audio capture surface Mockingbird relies on.

### Runtime libraries

- **Microsoft Edge WebView2 Runtime.** Preinstalled on Windows 11. On Windows 10 you may need to install it manually from [the WebView2 page](https://developer.microsoft.com/en-us/microsoft-edge/webview2/). The MSI installer will prompt you if it's missing.
- **Visual C++ Redistributable 2022 (x64).** The installer carries this. If you build from source, install it from [Microsoft](https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redists).

### Hardware

- **CPU:** Any 64-bit x86 CPU from the last 8 years. SSE4.2 and AVX2 are assumed.
- **RAM:** 8 GB minimum, 16 GB recommended (especially if you also run Ollama).
- **Disk:** 4 GB free for the Whisper model and SQLite database. More if you enable meeting recording retention.

## Optional but recommended

### NVIDIA GPU for fast Whisper

- **NVIDIA GPU** with CUDA Compute Capability 6.1 or higher (GTX 1060 era and newer).
- **CUDA Toolkit 12.x** installed. Mockingbird targets CUDA 12.8 in CI but other 12.x patch versions work in practice.
- The CPU fallback works fine for short utterances. It gets slow on multi-minute meeting transcripts.

If you do not have a GPU, the app still works. You will just notice longer latency on dictation and slower processing of meeting recordings.

### Ollama for local LLM cleanup

- [Ollama](https://ollama.com/) installed and running on `http://localhost:11434` (the default).
- A pulled chat model. The recommended default is `qwen2.5:7b-instruct-q4_K_M` (about 4.7 GB).
- The smaller `qwen2.5:3b` is documented as a degraded "tags only" mode for the knowledge graph features. It is not recommended for general cleanup.

Without Ollama (or a Claude API key, see below), dictation still produces a raw Whisper transcript. You just lose the punctuation and filler-word cleanup pass.

### Anthropic Claude API for cloud cleanup

- An [Anthropic API key](https://console.anthropic.com/).
- Network access to `api.anthropic.com`.
- Note that opting in to cloud cleanup sends your transcript text to Anthropic per their privacy policy. See [`PRIVACY.md`](./PRIVACY.md) for the full data flow guarantees.

This is mutually exclusive with Ollama at the per-mode level: each cleanup mode (casual, normal, formal) routes to one provider.

### Obsidian vault for knowledge graph capture

- An [Obsidian](https://obsidian.md/) vault folder somewhere on your disk. The vault can be empty; Mockingbird will create its own subtree inside it.
- Optional: [Obsidian Sync](https://obsidian.md/sync) or any folder-level sync (Dropbox, OneDrive, iCloud, Syncthing) if you want the vault available on multiple devices.

Mockingbird does not synchronize SQLite across machines. Cross-device knowledge sharing happens through the vault's Markdown files.

### Unsplash API key for ambient backgrounds

- A free [Unsplash developer account](https://unsplash.com/developers). The free tier is 50 requests per hour, more than enough for ambient backgrounds.
- Paste the key into Settings -> Appearance -> Unsplash. The key is encrypted via Windows DPAPI.

Backgrounds are purely cosmetic. The app works without this.

## For building from source

In addition to the runtime requirements above:

- **Rust 1.77 or newer** via [rustup](https://rustup.rs/).
- **Node 20 or newer** via [nvm-windows](https://github.com/coreybutler/nvm-windows) or the Node installer.
- **Git** (any recent version).
- **Visual Studio Build Tools 2022** with the "Desktop development with C++" workload. MSVC is required to compile the Rust native dependencies.
- **CMake 3.22 or newer**, usually picked up automatically if Visual Studio is installed.
- *(Optional)* **CUDA Toolkit 12.8** if you want CUDA-accelerated Whisper from your local build.

See [`INSTALL.md`](./INSTALL.md) Tier 3 for the actual build commands.
