# Run `cargo` with the full Phase-2+ build environment set up.
#
# Solves LESSONS.md "Finding 4 -- child PowerShell processes do not inherit
# User/Machine env from the registry". Spawned shells start with the env
# that the agent's parent process had at launch -- which predates the
# CUDA 12.8 install + the cmake install. This script bootstraps the env
# from scratch every time, so the build env is identical regardless of
# who (human / agent / CI) invokes it.
#
# Usage examples:
#   pwsh scripts/dev/cargo-with-cuda.ps1 check
#   pwsh scripts/dev/cargo-with-cuda.ps1 test --release
#   pwsh scripts/dev/cargo-with-cuda.ps1 clippy --release --all-targets -- -D warnings
#   pwsh scripts/dev/cargo-with-cuda.ps1 fmt --check
#
# What this script does (in order):
#   1. Imports MSVC env from vcvars64.bat (cl.exe, link.exe, INCLUDE, LIB).
#   2. Pins CUDA_PATH and CUDA_PATH_V12_8 to v12.8 (see ADR 0011).
#   3. Prepends cmake.exe to PATH if not already resolvable.
#   4. Runs `cargo $args` and forwards the exit code.

# NOTE: no [CmdletBinding()] / no param() — we want ALL arguments to land
# in the automatic `$args` array verbatim. CmdletBinding sees `--release`
# and other cargo flags as candidate switches, which breaks the call.

$ErrorActionPreference = "Stop"

if ($args.Count -eq 0) {
    Write-Error "Usage: cargo-with-cuda.ps1 <cargo args...>  e.g. ... check  |  ... test --release"
    exit 1
}

function Resolve-FirstExisting {
    param([string[]]$Candidates)
    foreach ($c in $Candidates) {
        if (Test-Path -LiteralPath $c) { return $c }
    }
    return $null
}

# --- 1. MSVC env via vcvars64.bat -------------------------------------------
$vcvars = Resolve-FirstExisting @(
    'C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat',
    'C:\Program Files\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat',
    'C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat',
    'C:\Program Files\Microsoft Visual Studio\2022\Professional\VC\Auxiliary\Build\vcvars64.bat'
)
if (-not $vcvars) {
    Write-Error "vcvars64.bat not found. Install Visual Studio 2022 BuildTools (Desktop development with C++)."
    exit 2
}

# Spawn cmd.exe under vcvars64, dump the resulting env, and re-import into
# our PowerShell session. This is the documented MSVC pattern.
$envDump = & cmd.exe /c "`"$vcvars`" >nul 2>&1 && set"
foreach ($line in $envDump) {
    if ($line -match '^([^=]+)=(.*)$') {
        $name  = $matches[1]
        $value = $matches[2]
        # Skip a few entries that mess with PowerShell's own state.
        if ($name -notin @('PROMPT', '_', 'PSModulePath')) {
            Set-Item -Path "env:$name" -Value $value
        }
    }
}

# --- 2. CUDA env (ADR 0011) -------------------------------------------------
$cudaRoot = 'C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.8'
if (-not (Test-Path -LiteralPath $cudaRoot)) {
    Write-Error "CUDA v12.8 not found at $cudaRoot -- see ADR 0011 + scripts/install-wave4-toolchain.ps1."
    exit 3
}
$env:CUDA_PATH        = $cudaRoot
$env:CUDA_PATH_V12_8  = $cudaRoot
$cudaBin     = Join-Path $cudaRoot 'bin'
$cudaLibnvvp = Join-Path $cudaRoot 'libnvvp'
$currentPath = $env:PATH
if ($currentPath -notlike ('*' + $cudaBin + '*')) {
    $env:PATH = $cudaBin + ';' + $cudaLibnvvp + ';' + $currentPath
}

# --- 3. cmake on PATH -------------------------------------------------------
if (-not (Get-Command cmake -ErrorAction SilentlyContinue)) {
    $cmakeBin = Resolve-FirstExisting @(
        'C:\Program Files\CMake\bin',
        'C:\Program Files (x86)\CMake\bin',
        'C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin',
        'C:\Program Files\Microsoft Visual Studio\2022\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin'
    )
    if (-not $cmakeBin) {
        Write-Error "cmake not found. Install via https://cmake.org/download/ or VS2022 BuildTools."
        exit 4
    }
    $env:PATH = $cmakeBin + ';' + $env:PATH
}

# --- 4. Parallelism cap (LESSONS 2026-05-17) --------------------------------
# whisper-rs-sys's CUDA compile spawns ~150 nvcc processes. Each fattn-mma
# instance can consume 2-4 GB of RAM. On a 16 GB machine with 6 GB free at
# steady state, --parallel 16 OOMs nvcc, leaving 0-byte .obj files that
# Lib.exe then rejects with LNK1136. Cap at 4 unless the caller explicitly
# overrides via CMAKE_BUILD_PARALLEL_LEVEL.
if (-not $env:CMAKE_BUILD_PARALLEL_LEVEL) {
    $env:CMAKE_BUILD_PARALLEL_LEVEL = '4'
}

# --- 5. Invoke cargo --------------------------------------------------------
# Cargo writes diagnostics to stderr in the normal course of business.
# PowerShell's pipeline treats native-command stderr as non-terminating
# errors that, under various $ErrorActionPreference / Tee-Object / redirect
# combinations, kill the run mid-build (see LESSONS 2026-05-17).
# Routing through cmd.exe with shell-level `2>&1` flattens the streams
# *outside* PowerShell, so the parent shell sees a single text stream
# regardless of how stdout/stderr are mixed.
$ErrorActionPreference = 'Continue'
$argString = ($args | ForEach-Object {
    if ($_ -match '\s') { '"' + $_ + '"' } else { $_ }
}) -join ' '
& cmd.exe /c "cargo $argString 2>&1"
exit $LASTEXITCODE
