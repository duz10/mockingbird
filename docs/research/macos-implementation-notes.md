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

## Recommended porting priority

Not every feature carries equal weight. A forker should port in this
order:

1. **Dictation (core).** The hotkey to speak to cleaned-text-pasted
   loop. This is the highest-value, lowest-risk slice and exercises
   most of the platform surface (hotkey, paste injection, secure-input,
   secrets, STT).
2. **Meeting capture (core).** Microphone plus system audio capture
   with speaker attribution. Previously the deepest unknown; the API
   path is now much clearer (see the ScreenCaptureKit section below).
   Still the deepest slice, but no longer open-ended.
3. **Knowledge-graph capture and activity (secondary).** The vault
   projection, entity extraction, and foreground-activity tagging
   layers. These are valuable but should come after the two core
   features are solid and stable on the Mac.

Framing: dictation and meeting capture are the core product. The
knowledge-graph and activity layers are secondary and can land after
the core is proven on macOS.

## GPU acceleration and quality parity on Apple Silicon

Two local models do the heavy lifting, and each is GPU-accelerated
independently on Apple Silicon:

- **Whisper (speech-to-text)** runs through the `whisper-rs` `metal`
  feature flag, swapped in place of the Windows `cuda` feature. This is
  the same whisper.cpp engine and the same GGUF model files as the
  Windows build, just Metal-accelerated instead of CUDA-accelerated.
  This is the parity-preserving STT path: identical engine, identical
  weights, identical output, different accelerator.
- **The cleanup / synthesis LLM (Ollama)** uses Metal automatically on
  Apple Silicon. There is nothing to do in the app for this; Ollama
  manages its own GPU. Running the same Ollama model as the Windows
  build yields the same cleanup quality.

Silero VAD runs on the CPU via the ONNX runtime (it needs the macOS
`libonnxruntime.dylib`). It is tiny and not a GPU concern.

**Parity goal:** keeping the same engine stack (whisper-rs plus Metal,
Silero via ONNX, the same Ollama models) is what guarantees that Mac
dictation and meeting quality match the Windows build. Apple-native
alternatives exist (see the alternative-engines section below) but they
diverge from cross-platform parity and would not produce
Windows-identical output.

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
| | System audio loopback | WASAPI loopback on the default render device | ScreenCaptureKit single-session capture (system audio plus microphone) on macOS 15+, with a BlackHole virtual-device fallback. | See "System audio loopback via ScreenCaptureKit" below. |
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

## System audio loopback via ScreenCaptureKit

macOS has no equivalent to WASAPI loopback for capturing system audio,
so this used to be the one open-ended unknown in the port. It is now a
concrete implementation path. **The specifics below are
researched-but-unverified: a developer working on macOS should confirm
them against the current Apple SDK before relying on them.**

### Single-session capture of both streams (macOS 15+)

[ScreenCaptureKit](https://developer.apple.com/documentation/screencapturekit)
is Apple's modern capture framework. The key finding: on macOS 15+ a
single `SCStream` session can capture **both** system audio and the
microphone at once.

- `capturesAudio` (system audio) is available since macOS 13.
- `captureMicrophone` (microphone in the same session) is macOS 15+.

**Recommended target: macOS 15+** for the clean single-session path.
Supporting macOS 13 to 14 requires a split approach: ScreenCaptureKit
for system audio plus `cpal` for the microphone, with the two streams
merged manually. Targeting 15+ avoids that extra plumbing.

### SCStreamConfiguration

Configure the stream for transcription-friendly audio:

- `capturesAudio = true`
- `captureMicrophone = true`
- `sampleRate = 16000` (ideal for Whisper)
- `channelCount = 1` (mono)

Audio capture is bound to a visual context, so even for audio-only
capture you must configure a dummy or off-screen capture filter. There
is no purely audio session; you ask for a screen content filter and
then ignore the video.

### Source demuxing equals speaker attribution at the capture layer

This is the elegant part. In the `SCStreamOutput` delegate each
`CMSampleBuffer` is tagged with its source. Check `CMGetAttachment`
for `SCStreamSampleBufferAttachment.microphoneStream`:

- If the attachment is present, the buffer is the **microphone** (the
  local user).
- If it is absent, the buffer is **system audio** (the remote
  participants).

That single check separates "me" from "them" at the capture layer, which
maps directly onto the separate-stream plus speaker-attribution meeting
model the app already uses on Windows.

### Permissions and entitlements

- Hardened Runtime with the Audio Input (Microphone) and Screen Capture
  entitlements.
- `Info.plist` keys `NSMicrophoneUsageDescription` and
  `NSScreenCaptureUsageDescription`.
- **Gotcha:** missing authorizations yield **silent audio buffers** (no
  error is raised). If capture produces empty audio, suspect a missing
  permission grant before anything else.

### Manual handling required

Working with raw `CMSampleBufferRef` means the developer is responsible
for synchronization, echo cancellation, and mixing. The recommended
approach is to transcribe each source stream **independently** rather
than mixing first. Keeping the two streams separate and attributed both
preserves speaker attribution and mitigates echo, since the local and
remote audio never get summed together.

### Rust / Tauri integration reality

No Rust crate wraps ScreenCaptureKit. This requires a small Swift or
Objective-C bridge compiled into the app and exposed to Rust over FFI.
The surface is small (a few hundred lines including the bridge and the
buffer-tagging delegate). It is a self-contained chunk that does not
bleed into the rest of the codebase.

### BlackHole fallback

[BlackHole](https://github.com/ExistentialAudio/BlackHole) is a free
open-source virtual audio cable that creates a loopback device users
can route system audio through. Keep it documented as a fallback for
forkers who want to avoid the Swift FFI or who need to support macOS
versions older than 15.

- **Pros:** no new native code in the app; users install BlackHole, set
  it as their system output, and the app captures from it like any
  other input device.
- **Cons:** users install and configure it themselves, and audio has to
  be routed back through a separate Multi-Output Device if they still
  want to hear it. This is meaningful friction compared to the
  single-session ScreenCaptureKit path, and it loses the automatic
  source tagging that gives free speaker attribution.

### Minimum-viable port

A pragmatic minimum-viable port can ship without meeting capture at all
on macOS, exposing only the dictation and knowledge-graph capture modes
(which need only the microphone, which works out of the box). That gets
a Mac build out the door in days rather than weeks, with meeting capture
following once the dictation core is solid.

## Alternative Apple-native engines (not used by the parity build)

Apple ships native engines that an Apple-first fork could use instead of
the cross-platform stack. They are presented here for completeness, but
all of them **diverge from Windows parity** and would not produce
Windows-identical output. The parity build does not use them.

- **Apple SpeechAnalyzer** (macOS-native on-device STT, Swift). Low
  latency and well optimized for Apple Silicon, but a different engine
  than whisper.cpp, so its transcripts would not match the Windows
  build.
- **MLX-Whisper** (Apple's Whisper port built on MLX, using Metal and
  unified memory). Can be faster than whisper.cpp on Apple Silicon in
  some cases, but it is a separate implementation, so output can drift
  from the Windows build.
- **CoreAudio HAL voice activity detection**
  (`kAudioDevicePropertyVoiceActivityDetectionState`). Hardware-level,
  echo-cancelled voice activity detection; an alternative to Silero,
  but it diverges from the shared VAD path.

Recommendation: the parity build uses whisper-rs with Metal plus Silero
over ONNX. The engines above are options for forkers who prioritize
Mac-native speed over cross-platform parity, and who accept that their
output will differ from the Windows build.

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
| Meeting capture via ScreenCaptureKit (the big one; de-risked from "unknown" to "known but non-trivial Swift / ObjC FFI work") | ~3-5 days |
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
