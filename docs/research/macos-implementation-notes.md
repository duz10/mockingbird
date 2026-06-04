# macOS Implementation Notes

**The maintainer does not pursue macOS distribution.** This document
exists to enable forkers who want Mac support without re-deriving the
deltas from scratch.

## TL;DR

Mockingbird's architecture is macOS-ready. Every platform-specific
surface (audio, hotkey, paste injection, secrets, secure-input
detection) is already isolated behind a `#[cfg(target_os)]` trait
implementation, and the rest of the codebase (Tauri, React frontend,
SQLite layer, cleanup pipeline, knowledge graph pipeline, Ollama
integration) is portable without changes.

A determined developer could ship a Mac build in roughly a week of
focused work by following the deltas in this document. The maintainer
has chosen a source-build-only path for macOS (no Apple Developer
subscription, no notarization pipeline, no signed installer) because it
eliminates the ~$99/yr recurring cost and the notarization round-trip
friction, and it lets forkers iterate without going through a
gatekeeper. Users on macOS build the app themselves from this
repository.

This is a tradeoff: Mac users pay a higher install cost (Rust toolchain
+ xcodebuild + 15 minutes of build time) in exchange for a Mac build
that exists at all. Forkers who want to ship a signed `.dmg` to a
broader audience are encouraged to take that step independently.

## What works as-is on macOS

| Subsystem | Status | Notes |
|-----------|--------|-------|
| Tauri 2 shell | Works | Tauri ships first-class macOS support. The window configuration in `src-tauri/tauri.conf.json` already includes a `bundle.icon` entry for `.icns`. |
| React + TypeScript frontend | Works | WKWebView replaces WebView2; all UI code is portable. |
| Tailwind v4 + design tokens | Works | No platform-specific styling. |
| SQLite storage layer | Works | `rusqlite` is cross-platform. Migrations run identically. |
| Cleanup pipeline (trait + Ollama provider) | Works | Ollama runs on macOS the same way it runs on Windows; the provider's HTTP client does not care. |
| Anthropic Claude provider | Works | HTTPS client is portable. |
| Knowledge graph pipeline | Works | Pure-Rust, no platform calls. |
| Vault projector | Works | Filesystem-only. |
| Reverse-watcher | Works | `notify` crate has a working macOS backend (`FSEvents`). |
| Inbox courier | Works | Same as vault projector. |
| Settings store | Works | Pure-Rust. |
| i18n | Works | Pure-TS. |

In total, ~80% of the codebase needs no changes for a macOS port.

## What needs platform-specific work

| Surface | Windows impl | macOS shape | Suggested crates / APIs |
|---------|--------------|-------------|-------------------------|
| Global hotkey | `SetWindowsHookExW(WH_KEYBOARD_LL, ...)` | `CGEventTap` registered with `kCGEventKeyDown` / `kCGEventKeyUp` | [`core-graphics`](https://crates.io/crates/core-graphics) crate, or the [`global-hotkey`](https://crates.io/crates/global-hotkey) crate as a higher-level alternative. Requires Input Monitoring permission in System Settings -> Privacy. |
| Microphone | `cpal` (CoreAudio backend) | `cpal` (CoreAudio backend) | Works out of the box. Triggers Microphone permission prompt on first use. |
| System audio loopback | WASAPI loopback on the default render device | **The big unknown.** Two options: ScreenCaptureKit (macOS 12.3+) or a virtual audio device like BlackHole. | See "The big macOS unknown" below. |
| STT acceleration | CUDA via the `whisper-rs` `cuda` feature | Metal via the `whisper-rs` `metal` feature | Swap the feature flags in `src-tauri/Cargo.toml`. The `whisper-rs` crate already supports both backends. |
| ONNX runtime (Silero VAD) | Bundled `onnxruntime.dll` for Windows x64 | Bundled `libonnxruntime.dylib` for macOS arm64 / x64 | Download from [ONNX Runtime releases](https://github.com/microsoft/onnxruntime/releases). The `ort` crate path-discovery code already handles both extensions; the launcher script needs a macOS variant. |
| Paste injection | Clipboard via `arboard` + `SendInput(VK_CONTROL, V)` | Clipboard via `arboard` + `CGEventCreateKeyboardEvent(... cmd+v)` posted to the HID event tap | The clipboard layer is portable; only the synthesized keypress changes. Requires Accessibility permission. |
| Secure-input detection | UI Automation `IsPassword` property on the focused element | AX API `kAXSecureTextFieldRole` / `IsSecure` attribute via [`accessibility-sys`](https://crates.io/crates/accessibility-sys) | Same logical check, different API. Requires Accessibility permission. |
| Secrets storage | DPAPI `CryptProtectData` | macOS Keychain via [`security-framework`](https://crates.io/crates/security-framework) | The trait already exists in `src-tauri/src/secrets/`. Implement a `KeychainSecretStore` and gate it behind `#[cfg(target_os = "macos")]`. |
| Installer / distribution | MSI via Tauri's WiX integration | `.app` bundle and optional `.dmg` via `tauri build` | Tauri does both. For source-build-only the user runs `tauri build` and opens the `.app` from `target/release/bundle/`. |
| Auto-launch on login | Tauri auto-start plugin | Tauri auto-start plugin | Same plugin works on macOS. No code change. |
| Foreground window context | Win32 `GetForegroundWindow` + UI Automation | `NSWorkspace.frontmostApplication` + AX API | Used by activity capture. Same logical shape, different API. |
| Background image fetch (Unsplash) | Portable | Portable | No change. |

Roughly 7-9 modules need attention. Each is small and well-isolated.

## The big macOS unknown: system audio loopback

This is the only subsystem without a clean drop-in solution. macOS has
no equivalent to WASAPI loopback for capturing system audio. There are
two real paths:

### Path A: ScreenCaptureKit (macOS 12.3+)

[ScreenCaptureKit](https://developer.apple.com/documentation/screencapturekit)
is Apple's modern capture framework. It supports audio-only capture
since macOS 13, and it does not require screen recording in the literal
sense: you can subscribe to the audio stream while passing an opaque
content filter that captures no video.

Tradeoffs:

- **Pros:** Apple-sanctioned, no kernel extension, no virtual device,
  no user setup, works on stock macOS.
- **Cons:** Requires a Screen Recording permission grant (it counts as
  screen capture from the user's perspective even though only audio is
  collected). 12.3+ minimum, which is fine for 2026 forks. There is no
  Rust crate that wraps it; you would write the Objective-C bridge
  yourself (the API is small, a few hundred lines including FFI).

### Path B: Virtual audio device (BlackHole)

[BlackHole](https://github.com/ExistentialAudio/BlackHole) is a free
open-source virtual audio cable that creates a loopback device users
can route system audio through.

Tradeoffs:

- **Pros:** No new code in Mockingbird; users install BlackHole, set it
  as their system output, and the app captures from it like any other
  mic. Already a popular solution for podcasters and screen recorders.
- **Cons:** Users have to install BlackHole themselves and reconfigure
  their audio routing. This is a meaningful friction step compared to
  the Windows zero-setup loopback experience. Audio also has to be
  routed back through a separate "Multi-Output Device" if the user
  still wants to hear it through their normal output.

### Recommended path for a fork

Ship the ScreenCaptureKit path. The user-facing friction is one
permission grant the first time they start a meeting capture, which is
strictly better than installing a virtual audio device. The Objective-C
FFI is the cost; it is a self-contained chunk that does not bleed into
the rest of the codebase.

A pragmatic minimum-viable-port can ship without meeting capture at all
on macOS, exposing only the dictation and knowledge graph capture
modes (which need only the microphone, which works out of the box).
That gets a Mac build out the door in days rather than weeks.

## Source-build-only path for users

For a forker willing to maintain a macOS build but not pay for an Apple
Developer account, the user-facing install instructions look like this:

1. Install Xcode Command Line Tools: `xcode-select --install`.
2. Install Rust 1.77+ via [rustup](https://rustup.rs/).
3. Install Node 20+ via [nvm](https://github.com/nvm-sh/nvm) or
   [Homebrew](https://brew.sh/) (`brew install node`).
4. Clone the fork's repo.
5. Optional: install [Ollama](https://ollama.com/download) and pull the
   recommended local model.
6. Build: `cargo tauri build` (no wrapper needed on macOS; CUDA env
   plumbing is Windows-specific).
7. Open the `.app` from `target/release/bundle/macos/`.

The first time the user opens the app, Gatekeeper will refuse to
launch it because it is not signed. The remedy is documented and
standard: right-click the app, choose Open from the context menu, then
confirm. macOS then allows the app to run going forward.

This is the same UX as any third-party open-source Mac app distributed
without notarization (Homebrew Cask installs work this way for plenty
of GUI apps). It is a one-time speed bump per user, not a recurring
cost.

## Permissions a macOS Mockingbird will request

Document these explicitly in the macOS README so users are not
surprised:

- **Microphone.** For dictation, meeting capture, and knowledge graph
  audio capture. Standard mic permission prompt.
- **Input Monitoring.** For the global hotkey hook. Granted in
  System Settings -> Privacy & Security -> Input Monitoring.
- **Accessibility.** For paste injection and secure-input detection.
  Granted in System Settings -> Privacy & Security -> Accessibility.
- **Screen Recording.** Only if meeting capture is implemented via
  ScreenCaptureKit. Granted in System Settings -> Privacy & Security ->
  Screen Recording. Audio-only capture still counts as screen recording
  for permission purposes; this is unavoidable.

All four are standard Mac permissions that any serious capture or
input-injection app needs. None require entitlements that are only
available to signed apps; unsigned source-built apps can request all
four. The user will see four separate permission prompts the first
time they exercise each feature.

## What is intentionally out of scope for this document

- **Notarization.** The maintainer's chosen path is source-build-only;
  a forker who wants a notarized signed build needs to set up their own
  Apple Developer account and run the [`tauri-action`](https://github.com/tauri-apps/tauri-action)
  signing flow. It is well-trodden and outside this document.
- **App Sandbox.** Mockingbird needs raw access to global keyboard
  events, accessibility APIs, system audio capture, and arbitrary
  filesystem paths (the user's Obsidian vault). Sandboxing is
  effectively incompatible with the feature set. A sandboxed fork
  would have to drop multiple features.
- **Mac App Store distribution.** Same reason as sandboxing: not
  realistic given the feature set.
- **iOS / iPadOS port.** Different problem. The current mobile capture
  path (an iOS Shortcut that drops audio into a synced Obsidian folder,
  picked up by the inbox courier) covers the practical use case without
  a native iOS app.

## Estimated effort

For a developer comfortable with both Rust and Objective-C FFI:

| Slice | Effort |
|-------|--------|
| Toolchain swap (CUDA -> Metal feature flags, ORT dylib path) | ~half a day |
| Hotkey via `CGEventTap` | ~1 day |
| Paste injection via `CGEventCreateKeyboardEvent` | ~half a day |
| Secure-input via AX API | ~half a day |
| Secrets via Keychain (`security-framework`) | ~half a day |
| Activity capture foreground polling via `NSWorkspace` | ~half a day |
| Launcher script equivalent (env vars for ORT) | ~quarter day |
| Meeting capture via ScreenCaptureKit (the big one) | ~3-5 days |
| Build pipeline, README, install docs | ~1 day |

Total: ~1 to 1.5 calendar weeks for a single developer at a steady
pace, or ~3-4 weeks if also developing meeting capture as a new
feature rather than porting an existing implementation. A
minimum-viable port that omits meeting capture cuts this in half.

## If you ship a working Mac fork

If you publish a Mockingbird fork that ships a Mac build, the
maintainer would genuinely like to know about it. Open a
[Discussion](../../discussions) on this repo with the fork URL. The
fork will not be merged into upstream (see CONTRIBUTING.md for why),
but it can be linked from the README so other Mac users find it.
