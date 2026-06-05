# Stage NVIDIA CUDA 12.x runtime DLLs next to the Tauri bundle resources
# so the CUDA MSI variant ships a self-contained GPU acceleration runtime.
#
# Background (LESSONS PINNED P15 / docs/audits/cublasLt-omission-verification.md):
#
# whisper-rs-sys with the cuda feature links cublas64_12.dll and
# cudart64_12.dll statically into mockingbird.exe. The Windows loader
# walks the entire PE static-import closure before main() runs, so the
# binary cannot launch unless every transitively imported DLL resolves.
# cublas64_12.dll itself has cublasLt64_12.dll as a static PE import in
# its own header. Source-level analysis of whisper.cpp shows it never
# calls a cuBLAS-Lt API, but that is irrelevant: the OS rejects the
# process before main() if cublasLt is unreachable. Therefore the CUDA
# MSI must bundle ALL THREE DLLs, not just cudart + cublas.
#
# nvcuda.dll is intentionally NOT staged here. It is the driver-side
# user-mode CUDA library and ships with every NVIDIA driver via Windows
# Update / GeForce Experience. Bundling our own copy would conflict
# with the installed driver and is not allowed by NVIDIA's redist
# license.
#
# Usage:
#   powershell -File scripts\dev\stage-cuda-runtime.ps1
#   powershell -File scripts\dev\stage-cuda-runtime.ps1 -DestDir custom\path
#   powershell -File scripts\dev\stage-cuda-runtime.ps1 -CudaBinDir C:\CUDA\v12.8\bin
#
# Exit codes:
#   0  success
#   2  CUDA bin dir not found
#   3  required DLL missing from CUDA install
#   4  copy failed
#   5  size verification failed (truncated copy)

[CmdletBinding()]
param(
    # Source CUDA bin directory. Defaults to CUDA_PATH\bin (set by
    # cargo-with-cuda.ps1 to v12.8) or the v12.8 install path as a
    # last-resort fallback so CI runners (where Jimver/cuda-toolkit
    # sets CUDA_PATH) and local dev both work without overrides.
    [string]$CudaBinDir = "",

    # Destination dir where DLLs are copied. Tauri picks these up via
    # bundle.resources in src-tauri/tauri.cuda.conf.json. Path is
    # relative to the repo root so the script works regardless of cwd.
    [string]$DestDir = "src-tauri\cuda-runtime"
)

$ErrorActionPreference = "Stop"

# Resolve repo root so the script works from any cwd. The script lives
# in scripts\dev\ so the repo root is two parents up.
$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$DestDirAbs = Join-Path $repoRoot $DestDir

# --- Resolve source CUDA bin dir -------------------------------------------
if (-not $CudaBinDir) {
    if ($env:CUDA_PATH -and (Test-Path -LiteralPath (Join-Path $env:CUDA_PATH "bin"))) {
        $CudaBinDir = Join-Path $env:CUDA_PATH "bin"
    } elseif (Test-Path -LiteralPath "C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.8\bin") {
        $CudaBinDir = "C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.8\bin"
    }
}

if (-not $CudaBinDir -or -not (Test-Path -LiteralPath $CudaBinDir)) {
    Write-Error "CUDA bin dir not found. Tried CUDA_PATH\bin and v12.8 default. Pass -CudaBinDir explicitly or set CUDA_PATH."
    exit 2
}

Write-Host "Staging CUDA runtime DLLs"
Write-Host "  source: $CudaBinDir"
Write-Host "  dest:   $DestDirAbs"

# --- Verify all three DLLs are present in the source -----------------------
# CUDA 12.x ships these as MAJOR_VERSION-suffixed filenames. The exact
# minor (12.0, 12.4, 12.6, 12.8, ...) only matters for choosing a
# Toolkit install; the filenames stay 64_12 across the whole 12.x line.
$requiredDlls = @(
    @{ name = "cudart64_12.dll";    approxMB = 0.55  },
    @{ name = "cublas64_12.dll";    approxMB = 108.0 },
    @{ name = "cublasLt64_12.dll";  approxMB = 660.0 }
)

foreach ($dll in $requiredDlls) {
    $src = Join-Path $CudaBinDir $dll.name
    if (-not (Test-Path -LiteralPath $src)) {
        Write-Error "Required DLL missing from CUDA install: $($dll.name) at $src"
        exit 3
    }
}

# --- Ensure dest dir exists, then copy + verify ----------------------------
if (-not (Test-Path -LiteralPath $DestDirAbs)) {
    New-Item -ItemType Directory -Path $DestDirAbs -Force | Out-Null
}

foreach ($dll in $requiredDlls) {
    $src = Join-Path $CudaBinDir $dll.name
    $dst = Join-Path $DestDirAbs $dll.name

    try {
        Copy-Item -LiteralPath $src -Destination $dst -Force
    } catch {
        Write-Error "Copy failed for $($dll.name): $_"
        exit 4
    }

    # Size sanity check. Truncated copies past the network boundary on
    # CI runners would otherwise produce an MSI that fails at install
    # time with no useful diagnostics. Compare to a generous lower
    # bound (50 percent of expected) since exact bytes vary by CUDA
    # patch release.
    $srcSize = (Get-Item -LiteralPath $src).Length
    $dstSize = (Get-Item -LiteralPath $dst).Length
    if ($dstSize -ne $srcSize) {
        Write-Error "Size mismatch for $($dll.name): source $srcSize bytes vs dest $dstSize bytes (truncated copy)"
        exit 5
    }

    $sizeMB = [math]::Round($dstSize / 1MB, 2)
    Write-Host "  staged $($dll.name) ($sizeMB MB)"
}

# --- Closure total for sanity ---------------------------------------------
$totalBytes = 0
foreach ($dll in $requiredDlls) {
    $totalBytes += (Get-Item -LiteralPath (Join-Path $DestDirAbs $dll.name)).Length
}
$totalMB = [math]::Round($totalBytes / 1MB, 1)
Write-Host "CUDA runtime closure staged: $totalMB MB total across 3 DLLs (cudart + cublas + cublasLt)"
Write-Host "Ready for tauri-action --config src-tauri/tauri.cuda.conf.json"
