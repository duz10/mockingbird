# Build a complete release binary with the latest UI bundle embedded.
#
# Why this exists: `cargo build --release` (or our cargo-with-cuda.ps1
# wrapper) does NOT trigger Tauri's `beforeBuildCommand`. That hook only
# runs under `cargo tauri build`. So if you change UI sources and then
# `cargo build --release`, the release binary embeds the STALE dist/ —
# the bundled HTML/JS that was there at the previous build. Visible
# symptom: code-correct Rust + DB + IPC, but the on-screen UI behaves
# according to an older types.ts allowlist or render path. See LESSONS
# 2026-05-17 phase5-postship-9-followup for the full story.
#
# This script does the two-step every time:
#   1. `npm --prefix ui run build`         (regenerates ui/dist/)
#   2. Touch src-tauri/src/lib.rs           (forces cargo to re-link, since
#                                            tauri-build's rerun-if-changed
#                                            directives don't always pick up
#                                            frontendDist content changes)
#   3. cargo-with-cuda.ps1 build --release  (re-embeds the fresh dist/)
#
# Use this for any iteration where you've touched files under ui/.

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot

Write-Host "[1/3] npm run build  (ui/)" -ForegroundColor Cyan
Push-Location (Join-Path $root 'ui')
try {
    & npm run build
    if ($LASTEXITCODE -ne 0) { throw "npm run build exited $LASTEXITCODE" }
} finally {
    Pop-Location
}

Write-Host "[2/3] touch src-tauri/src/lib.rs  (force cargo re-link)" -ForegroundColor Cyan
$libRs = Join-Path $root 'src-tauri\src\lib.rs'
(Get-Item $libRs).LastWriteTime = Get-Date

Write-Host "[3/3] cargo build --release" -ForegroundColor Cyan
& (Join-Path $PSScriptRoot 'cargo-with-cuda.ps1') build --release
exit $LASTEXITCODE
