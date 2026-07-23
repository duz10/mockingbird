# Homebrew Cask for Mockingbird (macOS, Apple Silicon).
#
# STATUS: SCAFFOLD. `version` and `sha256` are PLACEHOLDERS -- they can
# only be finalized once the first .dmg release exists (the CI macOS lane
# in .github/workflows/release.yml produces it on a `v*.*.*` tag). After
# the first release:
#   1. Set `version` to match the release tag WITHOUT the leading `v`
#      (and match src-tauri/tauri.conf.json `version`, which is what
#      tauri stamps into the .dmg filename).
#   2. Replace the `sha256` with the real hash:
#        shasum -a 256 Mockingbird_<version>_aarch64.dmg
#      (or `brew fetch --cask mockingbird` then read the cached hash).
#
# The `url` matches tauri v2's dmg naming: <productName>_<version>_<arch>.dmg
# -> Mockingbird_#{version}_aarch64.dmg (productName "Mockingbird",
# arch "aarch64" for Apple Silicon). If the release tag and the tauri
# config `version` ever diverge, the filename follows the CONFIG version,
# not the tag -- keep them in sync at release time.
#
# Ad-hoc-signed bundle: no Apple Developer ID / notarization (no paid
# account). The .app IS validly codesigned (bundle.macOS.signingIdentity
# "-" in tauri.macos.conf.json), so it does NOT read as "damaged and
# should be uninstalled" -- that error is caused by an INVALID/unsealed
# signature, which the ad-hoc sign fixes.
#
# NOTE: Homebrew (6.x) still ADDS com.apple.quarantine on cask install --
# it does NOT strip it. So on first launch Gatekeeper shows the ordinary
# "unidentified developer" prompt (Apple can't notarization-check an
# ad-hoc app). Approve once via right-click > Open, or System Settings >
# Privacy & Security > "Open Anyway". To skip the prompt entirely, install
# with:  brew install --cask --no-quarantine duz10/mockingbird/mockingbird
# (Homebrew removed the per-cask `quarantine false` opt-out, so the cask
# cannot suppress it on the user's behalf.) Without a $99/yr Developer ID
# + notarization, that one-time approval is inherent to this distribution.
cask "mockingbird" do
  version "0.3.0-beta.2"
  sha256 "90eae61bf8ee3ec403b41a9e310d622eb26224e689b2a96202c8a57a2bc7ba9e"

  url "https://github.com/duz10/mockingbird/releases/download/v#{version}/Mockingbird_#{version}_aarch64.dmg"
  name "Mockingbird"
  desc "Local-first, zero-telemetry voice dictation and meeting capture"
  homepage "https://github.com/duz10/mockingbird"

  # Apple Silicon + macOS 15 (Sequoia) floor -- ScreenCaptureKit unified
  # single-session system-audio capture requires 15+, matching
  # bundle.macOS.minimumSystemVersion in tauri.conf.json.
  depends_on arch: :arm64
  depends_on macos: :sequoia

  app "Mockingbird.app"

  # Local-only app data (zero telemetry): clear on `brew uninstall --zap`.
  zap trash: [
    "~/Library/Application Support/com.dustin.mockingbird",
    "~/Library/Caches/com.dustin.mockingbird",
    "~/Library/Preferences/com.dustin.mockingbird.plist",
    "~/Library/Saved Application State/com.dustin.mockingbird.savedState",
  ]
end
