# Throwaway-crate test runner for pure-Rust activity-capture modules.
# Per LESSONS P2 — `cargo test --release` on this box exits with
# STATUS_ENTRYPOINT_NOT_FOUND from the workspace-binary test runner,
# so pure modules get exercised in an isolated temp crate.
#
# Usage:
#   powershell -File scripts\throwaway-test.ps1 <module-name> <source-path>
#
# Example:
#   powershell -File scripts\throwaway-test.ps1 uia_payload `
#       src-tauri\src\activity\uia\payload.rs

param(
    [Parameter(Mandatory = $true)][string]$ModuleName,
    [Parameter(Mandatory = $true)][string]$SourcePath,
    [string[]]$Dependencies = @('serde = { version = "1", features = ["derive"] }', 'serde_json = "1"'),
    # Optional Rust source prepended to the throwaway lib.rs — use this to
    # stub out `crate::error` and similar workspace-only paths so a single-
    # file pure module can compile in isolation.
    [string]$Preamble = ''
)

$ErrorActionPreference = 'Stop'

$dir = Join-Path $env:TEMP "mb_${ModuleName}_tests"
if (Test-Path $dir) {
    Remove-Item -Recurse -Force $dir
}
New-Item -ItemType Directory -Path $dir | Out-Null
New-Item -ItemType Directory -Path (Join-Path $dir 'src') | Out-Null

$cargoToml = @"
[package]
name = "mb_${ModuleName}_tests"
version = "0.1.0"
edition = "2021"

[dependencies]
$($Dependencies -join "`n")
"@

$cargoToml | Out-File -Encoding utf8 (Join-Path $dir 'Cargo.toml')

if ([string]::IsNullOrEmpty($Preamble)) {
    Copy-Item $SourcePath (Join-Path $dir 'src\lib.rs')
} else {
    # Append the preamble at the END of the file so the source's own
    # inner attributes (`#![allow(...)]`) and module doc comments stay
    # at position 1. The preamble is a module declaration; ordering
    # is irrelevant for resolution.
    $body = Get-Content -Raw -Path $SourcePath
    ($body + "`n" + $Preamble) | Out-File -Encoding utf8 (Join-Path $dir 'src\lib.rs')
}

Push-Location $dir
try {
    cargo test --release
    $exit = $LASTEXITCODE
}
finally {
    Pop-Location
}

if ($exit -ne 0) {
    Write-Error "Throwaway tests failed (exit $exit)"
}
