# One-shot helper: move CUDA 13.2's MSBuild integration files out of
# VS 2022 BuildTools so that cmake's "Visual Studio 17 2022" generator
# stops trying to use them (CUDA 13's targets file is broken on this
# install -- empty CudaToolkitDir property).
#
# Result: cmake picks the CUDA 12.8.targets file instead, which works.
# Reversible: backed-up files live under
# %USERPROFILE%\mockingbird_models\cuda13_buildcustomizations_backup\.
#
# Idempotent: subsequent runs are no-ops once the files have moved.

$ErrorActionPreference = 'Stop'

$bc = 'C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\MSBuild\Microsoft\VC\v170\BuildCustomizations'
$backup = Join-Path $env:USERPROFILE 'mockingbird_models\cuda13_buildcustomizations_backup'

if (-not (Test-Path -LiteralPath $bc)) {
    Write-Error "BuildCustomizations dir not found at $bc"
    exit 1
}

New-Item -ItemType Directory -Force -Path $backup | Out-Null

$files = Get-ChildItem -LiteralPath $bc | Where-Object { $_.Name -match '13\.2' }
foreach ($f in $files) {
    $dest = Join-Path $backup $f.Name
    Move-Item -LiteralPath $f.FullName -Destination $dest -Force
    Write-Host "moved $($f.Name) -> $dest"
}

Write-Host '--- Remaining CUDA files ---'
Get-ChildItem -LiteralPath $bc | Where-Object { $_.Name -match 'CUDA|Cuda' } | Format-Table Name
