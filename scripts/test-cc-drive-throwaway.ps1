# Throwaway-crate test runner for the command_center::drive module.
# Per LESSONS P2 / 2026-05-17: cargo test --release fails to launch
# binaries on this Windows box. Pure-Rust modules with no whisper-rs
# / ort / cuda deps run cleanly in a side-crate. This script wires
# up exactly that for drive.rs + state.rs.

$ErrorActionPreference = "Stop"
$repoRoot = Resolve-Path "$PSScriptRoot\.."
$root = Join-Path $env:TEMP "cc_drive_tests"
if (Test-Path $root) { Remove-Item -Recurse -Force $root }
New-Item -ItemType Directory -Path "$root\src" | Out-Null

$cargoToml = @"
[package]
name = "cc_drive_tests"
version = "0.0.0"
edition = "2021"

[dependencies]
tracing = "0.1"
"@
Set-Content -Path "$root\Cargo.toml" -Value $cargoToml -Encoding UTF8

$libRs = @"
#![allow(dead_code, clippy::module_name_repetitions)]
pub mod state;
pub mod drive;
"@
Set-Content -Path "$root\src\lib.rs" -Value $libRs -Encoding UTF8

Copy-Item "$repoRoot\src-tauri\src\command_center\state.rs" "$root\src\state.rs"
Copy-Item "$repoRoot\src-tauri\src\command_center\drive.rs" "$root\src\drive.rs"

Write-Host "Throwaway crate scaffolded at $root" -ForegroundColor Cyan
Push-Location $root
try {
    cargo test --quiet 2>&1 | Write-Host
    $code = $LASTEXITCODE
    if ($code -ne 0) { throw "tests failed with exit $code" }
    Write-Host "ALL GREEN" -ForegroundColor Green
} finally {
    Pop-Location
}
