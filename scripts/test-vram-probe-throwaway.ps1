# Throwaway-crate live-test rig for cleanup/vram_probe.rs.
# LESSONS PINNED P2: cargo test --release on this box is broken
# (STATUS_ENTRYPOINT_NOT_FOUND). Pure-Rust modules with no whisper-rs /
# ort / cuda deps can be tested live by copying source into a temp
# crate and running vanilla `cargo test` there.

$ErrorActionPreference = 'Stop'

$tmp = Join-Path $env:TEMP 'vram_probe_tests'
if (Test-Path $tmp) { Remove-Item -Recurse -Force $tmp }
New-Item -ItemType Directory -Path $tmp | Out-Null
New-Item -ItemType Directory -Path (Join-Path $tmp 'src') | Out-Null

$repoRoot = Split-Path -Parent $PSScriptRoot
$src = Join-Path $repoRoot 'src-tauri\src\cleanup\vram_probe.rs'
Copy-Item $src (Join-Path $tmp 'src\lib.rs')

$cargoToml = @'
[package]
name = "vram_probe_tests"
version = "0.0.0"
edition = "2021"

[lib]
path = "src/lib.rs"
'@
Set-Content -Path (Join-Path $tmp 'Cargo.toml') -Value $cargoToml

Write-Host '--- Cargo.toml ---'
Get-Content (Join-Path $tmp 'Cargo.toml')

Write-Host '--- running cargo test ---'
Push-Location $tmp
try {
    cargo test
} finally {
    Pop-Location
}
