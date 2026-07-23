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
# Unsigned bundle: no Apple Developer account is used. Homebrew's default
# cask install STRIPS the com.apple.quarantine attribute, so there is NO
# Gatekeeper wall via this path -- that is the whole point of shipping a
# cask alongside the raw .dmg.
cask "mockingbird" do
  version "0.0.0"
  sha256 "0000000000000000000000000000000000000000000000000000000000000000"

  url "https://github.com/duz10/mockingbird/releases/download/v#{version}/Mockingbird_#{version}_aarch64.dmg"
  name "Mockingbird"
  desc "Local-first, zero-telemetry voice dictation and meeting capture"
  homepage "https://github.com/duz10/mockingbird"

  # Apple Silicon + macOS 15 (Sequoia) floor -- ScreenCaptureKit unified
  # single-session system-audio capture requires 15+, matching
  # bundle.macOS.minimumSystemVersion in tauri.conf.json.
  depends_on arch: :arm64
  depends_on macos: ">= :sequoia"

  app "Mockingbird.app"

  # Local-only app data (zero telemetry): clear on `brew uninstall --zap`.
  zap trash: [
    "~/Library/Application Support/com.dustin.mockingbird",
    "~/Library/Caches/com.dustin.mockingbird",
    "~/Library/Preferences/com.dustin.mockingbird.plist",
    "~/Library/Saved Application State/com.dustin.mockingbird.savedState",
  ]
end
