<#
.SYNOPSIS
    One-shot installer for Phase 2 Wave 4 toolchain.
    MUST be run elevated (auto-elevates if launched non-elevated).

.DESCRIPTION
    Installs (idempotent):
      1. cmake                              (~50 MB)
      2. Visual Studio 2022 Build Tools     (~10 GB) -- C++ workload only
      3. CUDA Toolkit                       (~7 GB)

    Logs to $env:USERPROFILE\install-wave4-toolchain.log so the parent
    (non-elevated) session can poll progress.

.NOTES
    Phase 2 Wave 4 implementor task. Once this completes successfully,
    `cargo build --features whisper-rs/cuda` in src-tauri/ should
    succeed on Windows.
#>

[CmdletBinding()]
param(
    [string]$LogPath = (Join-Path $env:USERPROFILE 'install-wave4-toolchain.log')
)

$ErrorActionPreference = 'Stop'

# --- Self-elevation -------------------------------------------------
$currentPrincipal = New-Object Security.Principal.WindowsPrincipal(
    [Security.Principal.WindowsIdentity]::GetCurrent()
)
if (-not $currentPrincipal.IsInRole(
        [Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Write-Host "Re-launching elevated..." -ForegroundColor Yellow
    $argsLine = "-NoProfile -ExecutionPolicy Bypass -File `"$PSCommandPath`" -LogPath `"$LogPath`""
    Start-Process powershell -Verb RunAs -ArgumentList $argsLine -Wait
    exit $LASTEXITCODE
}

# --- Logging --------------------------------------------------------
function Log {
    param([string]$Message, [string]$Color = 'White')
    $ts = (Get-Date).ToString('HH:mm:ss')
    $line = "[$ts] $Message"
    Add-Content -Path $LogPath -Value $line
    Write-Host $line -ForegroundColor $Color
}

# Reset log on fresh run
Set-Content -Path $LogPath -Value "=== Wave 4 toolchain install -- $(Get-Date) ==="
Log "Starting elevated install. Log: $LogPath" 'Cyan'
Log "Current user: $env:USERNAME (elevated)"

# Refresh PATH from registry -- picks up tools installed earlier in this
# session, e.g. after cmake install completes nvcc may still not see it.
function Refresh-Path {
    $machinePath = [Environment]::GetEnvironmentVariable('PATH', 'Machine')
    $userPath = [Environment]::GetEnvironmentVariable('PATH', 'User')
    $env:PATH = "$machinePath;$userPath"
}

function Test-ToolPresent {
    param([string]$Name)
    $cmd = Get-Command $Name -ErrorAction SilentlyContinue
    return [bool]$cmd
}

# --- 1. cmake -------------------------------------------------------
Log "STEP 1/3: cmake" 'Yellow'
Refresh-Path
if (Test-ToolPresent 'cmake') {
    $existing = (Get-Command cmake).Source
    Log "  cmake already on PATH: $existing -- skipping" 'Green'
} else {
    Log "  Installing cmake via chocolatey..."
    & choco install cmake -y --no-progress --installargs 'ADD_CMAKE_TO_PATH=System' 2>&1 |
        ForEach-Object { Log "    $_" }
    Refresh-Path
    if (Test-ToolPresent 'cmake') {
        Log "  cmake installed: $((Get-Command cmake).Source)" 'Green'
    } else {
        Log "  cmake STILL not on PATH after install -- check log" 'Red'
    }
}

# --- 2. Visual Studio 2022 Build Tools (C++ workload) ---------------
Log "STEP 2/3: Visual Studio 2022 Build Tools (C++ workload)" 'Yellow'
$vsWhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$vs2022Present = $false
if (Test-Path $vsWhere) {
    $installations = & $vsWhere -version '[17.0,18.0)' -products 'Microsoft.VisualStudio.Product.BuildTools' -property installationPath 2>&1
    if ($installations) {
        $vs2022Present = $true
        Log "  VS 2022 BT detected at: $installations -- skipping" 'Green'
    }
}
if (-not $vs2022Present) {
    Log "  Installing visualstudio2022buildtools + vctools workload..."
    & choco install visualstudio2022buildtools -y --no-progress 2>&1 |
        ForEach-Object { Log "    $_" }
    & choco install visualstudio2022-workload-vctools -y --no-progress 2>&1 |
        ForEach-Object { Log "    $_" }
    Log "  VS 2022 BT install complete." 'Green'
}

# --- 3. CUDA Toolkit ------------------------------------------------
Log "STEP 3/3: CUDA Toolkit" 'Yellow'
Refresh-Path
if (Test-ToolPresent 'nvcc') {
    Log "  nvcc already on PATH: $((Get-Command nvcc).Source) -- skipping" 'Green'
} else {
    Log "  Installing CUDA Toolkit via chocolatey (this is the long one -- 3-7 GB)..."
    & choco install cuda -y --no-progress 2>&1 |
        ForEach-Object { Log "    $_" }
    Refresh-Path
    if (Test-ToolPresent 'nvcc') {
        Log "  nvcc installed: $((Get-Command nvcc).Source)" 'Green'
    } else {
        Log "  nvcc NOT on PATH yet -- may need new shell session to inherit" 'Yellow'
        # Try common install path
        $cudaPath = "${env:ProgramFiles}\NVIDIA GPU Computing Toolkit\CUDA"
        if (Test-Path $cudaPath) {
            $latest = Get-ChildItem $cudaPath -Directory | Sort-Object Name -Descending | Select-Object -First 1
            if ($latest) {
                Log "  CUDA installed at: $($latest.FullName)" 'Green'
            }
        }
    }
}

# --- Summary --------------------------------------------------------
Refresh-Path
Log ""
Log "=== Install summary ===" 'Cyan'
Log "  cmake:  $(if (Test-ToolPresent 'cmake') { (Get-Command cmake).Source } else { 'NOT FOUND' })"
Log "  nvcc:   $(if (Test-ToolPresent 'nvcc')  { (Get-Command nvcc).Source }  else { 'NOT FOUND (may need new shell)' })"
$vsBT = & $vsWhere -version '[17.0,18.0)' -products 'Microsoft.VisualStudio.Product.BuildTools' -property installationPath 2>&1
Log "  VS 2022 BT: $(if ($vsBT) { $vsBT } else { 'NOT FOUND' })"
Log ""
Log "DONE. Parent session may need to restart to pick up PATH changes." 'Green'
