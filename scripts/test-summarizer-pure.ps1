# Throwaway-test runner for the Wave-3 pure pipeline modules
# (segmenter / blocker / assembler). Per LESSONS P2.
#
# These three modules use `use super::...` to reach a sibling module
# (segmenter -> persist for ActivityEventRow; blocker -> segmenter for
# NormalizedEvent; assembler -> blocker for Block + segmenter for
# NormalizedEvent in tests). The base `throwaway-test.ps1` drops a
# single source as the crate root `src/lib.rs`, which breaks the
# `super::` resolution.
#
# This wrapper builds a proper temp crate with:
#   src/lib.rs                    -- declares persist (stub) + the three modules
#   src/persist.rs                -- stub ActivityEventRow
#   src/segmenter.rs              -- copied verbatim from src-tauri/...
#   src/blocker.rs                -- copied verbatim
#   src/assembler.rs              -- copied verbatim
# and runs `cargo test --release` in the temp dir.

$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path $PSScriptRoot -Parent
$srcDir   = Join-Path $repoRoot 'src-tauri\src\activity'

$dir = Join-Path $env:TEMP 'mb_summarizer_tests'
if (Test-Path $dir) { Remove-Item -Recurse -Force $dir }
New-Item -ItemType Directory -Path $dir | Out-Null
New-Item -ItemType Directory -Path (Join-Path $dir 'src') | Out-Null

# Cargo.toml
@"
[package]
name = "mb_summarizer_tests"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
"@ | Out-File -Encoding utf8 (Join-Path $dir 'Cargo.toml')

# Stub persist module that provides ActivityEventRow with the same
# field shape as src-tauri/src/activity/persist.rs.
@'
pub mod persist {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ActivityEventRow {
        pub id: String,
        pub session_id: String,
        pub ts: i64,
        pub kind: String,
        pub app_name: Option<String>,
        pub window_title: Option<String>,
        pub snapshot_json: Option<String>,
        pub created_at: i64,
    }
}

pub mod segmenter;
pub mod blocker;
pub mod assembler;
'@ | Out-File -Encoding utf8 (Join-Path $dir 'src\lib.rs')

# Copy the three modules verbatim.
Copy-Item (Join-Path $srcDir 'segmenter.rs')  (Join-Path $dir 'src\segmenter.rs')
Copy-Item (Join-Path $srcDir 'blocker.rs')    (Join-Path $dir 'src\blocker.rs')
Copy-Item (Join-Path $srcDir 'assembler.rs')  (Join-Path $dir 'src\assembler.rs')

# The pure modules reference `crate::activity::segmenter::NormalizedEvent`
# only inside a #[cfg(test)] block in assembler.rs. Rewrite that
# specific path to the throwaway-crate's flat layout.
$asm = Get-Content -Raw (Join-Path $dir 'src\assembler.rs')
$asm = $asm -replace 'use crate::activity::segmenter::NormalizedEvent;', 'use crate::segmenter::NormalizedEvent;'
$asm | Out-File -Encoding utf8 (Join-Path $dir 'src\assembler.rs')

Push-Location $dir
try {
    cargo test --release
    $exit = $LASTEXITCODE
} finally {
    Pop-Location
}

if ($exit -ne 0) {
    Write-Error "Throwaway summarizer tests failed (exit $exit)"
}
