# watch-vault.ps1 -- observe filesystem activity in the Mockingbird vault.
#
# Built for ADR 0046 Iter 3 Wave 0 (mb-s8s2): the sync-layer observational
# spike. Logs every Created / Changed / Renamed / Deleted event under the
# vault (default: %USERPROFILE%\mockingbird-vault\) as one JSON object per
# line, with millisecond-precision ISO-8601 timestamps. Tees a human-friendly
# summary to stdout so the operator sees events live; the JSONL log is what
# downstream analysis (grep / Select-String / jq) consumes.
#
# WHY this script exists (vs. just Sysinternals procmon): procmon is heavier,
# has its own filtering UI, and produces a binary format that's annoying to
# grep. For a low-frequency observational spike (a human triggering a sync
# action and waiting ~30s) FileSystemWatcher in a small PS script is the
# cheapest tool that produces the right evidence shape.
#
# DESIGN NOTE: events are dequeued from PowerShell's $events queue in the
# main loop (no -Action handlers on Register-ObjectEvent). The -Action
# pattern dispatches each event into its own runspace, which (a) makes the
# $script:-scope WriteEvent helper invisible to the handler and (b) silently
# swallows handler exceptions. Polling Get-Event in one runspace keeps the
# data path debuggable and the script under 200 lines.
#
# KNOWN LIMITATION: System.IO.FileSystemWatcher can drop events under heavy
# bursts (its internal buffer is bounded; the kernel ReadDirectoryChangesW
# buffer behind it overflows under sustained load). For this spike's
# purposes (low-frequency, human-paced triggers) this is fine -- the
# production inbox watcher in the next implementation wave will pair
# notify-debouncer-full with a periodic reconciliation scan anyway, per
# ADR 0046 Section 6. If you see "Buffer overflow" warnings, the data is
# incomplete; bump -BufferKB and re-run.
#
# Usage (PowerShell 5.1 -- pwsh is NOT on the dev box; see LESSONS P1):
#
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts\watch-vault.ps1
#
# Common flags:
#
#   -VaultPath "<absolute>"        what to watch (default %USERPROFILE%\mockingbird-vault)
#   -LogPath "spike-round-1.log"   where JSONL lands (default vault-watch.log in cwd)
#   -IncludeMockingbird            don't filter .mockingbird/ (default: filtered out)
#   -ExcludeObsidian               filter .obsidian/ (default: INCLUDED - useful for spike)
#   -BufferKB 256                  FSW internal buffer in KB (default 64)

[CmdletBinding()]
param(
    [string]$VaultPath          = (Join-Path $env:USERPROFILE 'mockingbird-vault'),
    [string]$LogPath            = 'vault-watch.log',
    [switch]$IncludeMockingbird,
    [switch]$ExcludeObsidian,
    [int]   $BufferKB           = 64
)

$ErrorActionPreference = 'Stop'

# ---------------------------------------------------------------------------
# Resolve paths + sanity-check the vault.
# ---------------------------------------------------------------------------

if (-not (Test-Path -LiteralPath $VaultPath)) {
    Write-Error "Vault path does not exist: $VaultPath"
    exit 1
}

$VaultPathResolved = (Resolve-Path -LiteralPath $VaultPath).ProviderPath
$LogPathResolved   = if ([System.IO.Path]::IsPathRooted($LogPath)) {
    $LogPath
} else {
    Join-Path (Get-Location).ProviderPath $LogPath
}

# Truncate prior log so each round starts clean. For append semantics,
# pass a distinct -LogPath per round (the recommended pattern).
Set-Content -LiteralPath $LogPathResolved -Value '' -Encoding utf8

Write-Host ""
Write-Host "watch-vault.ps1 -- ADR 0046 Iter 3 Wave 0 (mb-s8s2)" -ForegroundColor Cyan
Write-Host "  vault   : $VaultPathResolved"
Write-Host "  log     : $LogPathResolved"
$mbState  = if ($IncludeMockingbird) { 'INCLUDED' } else { 'excluded' }
$obsState = if ($ExcludeObsidian)    { 'excluded' } else { 'INCLUDED' }
Write-Host "  filters : .mockingbird/ = $mbState, .obsidian/ = $obsState"
Write-Host "  buffer  : ${BufferKB} KB"
Write-Host "  stop    : Ctrl+C"
Write-Host ""

# ---------------------------------------------------------------------------
# FileSystemWatcher + StreamWriter (both disposed in the finally block).
# ---------------------------------------------------------------------------

$watcher = New-Object System.IO.FileSystemWatcher
$watcher.Path                  = $VaultPathResolved
$watcher.IncludeSubdirectories = $true
$watcher.EnableRaisingEvents   = $false
$watcher.InternalBufferSize    = $BufferKB * 1024
$watcher.NotifyFilter          = [System.IO.NotifyFilters]::FileName       `
                                -bor [System.IO.NotifyFilters]::DirectoryName  `
                                -bor [System.IO.NotifyFilters]::LastWrite      `
                                -bor [System.IO.NotifyFilters]::Size           `
                                -bor [System.IO.NotifyFilters]::CreationTime

$writer = [System.IO.StreamWriter]::new(
    $LogPathResolved, $true, [System.Text.UTF8Encoding]::new($false))
$writer.AutoFlush = $true

# ---------------------------------------------------------------------------
# Filter + emit helpers. Same runspace as the main loop -- visible because
# we're dequeueing from $events ourselves, not registering -Action handlers.
# ---------------------------------------------------------------------------

function Should-Filter([string]$fullPath) {
    if (-not $fullPath) { return $true }
    $rel = $fullPath
    if ($fullPath.StartsWith($VaultPathResolved, [StringComparison]::OrdinalIgnoreCase)) {
        $rel = $fullPath.Substring($VaultPathResolved.Length).TrimStart('\','/')
    }
    if (-not $IncludeMockingbird -and $rel -match '^\.mockingbird([\\/]|$)') { return $true }
    if ($ExcludeObsidian           -and $rel -match '^\.obsidian([\\/]|$)')    { return $true }
    return $false
}

$script:Counter = 0

function Emit-Event([string]$kind, [string]$path, [string]$oldPath) {
    if (Should-Filter $path) { return }

    # File size -- best-effort; on Deleted / Renamed-source the file is gone.
    $size = $null
    try {
        if ((Test-Path -LiteralPath $path -PathType Leaf)) {
            $size = (Get-Item -LiteralPath $path -ErrorAction Stop).Length
        }
    } catch {
        # Race against the syncing writer is expected; leave size null.
    }

    $obj = [ordered]@{
        ts    = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ss.fffZ')
        event = $kind
        path  = $path
    }
    if ($null -ne $size)                       { $obj.size    = $size }
    if (-not [string]::IsNullOrEmpty($oldPath)) { $obj.oldPath = $oldPath }

    $json = ($obj | ConvertTo-Json -Compress -Depth 3)
    $writer.WriteLine($json)

    $sizeStr  = if ($null -ne $size) { " (${size}B)" } else { '' }
    $extraStr = if ($oldPath)        { " <- $oldPath" } else { '' }
    Write-Host ("{0} {1,-9} {2}{3}{4}" -f $obj.ts, $kind, $path, $extraStr, $sizeStr)
    $script:Counter++
}

# ---------------------------------------------------------------------------
# Subscribe without -Action: each event becomes a PSEvent in the queue,
# dequeued by Get-Event in the main loop below.
# ---------------------------------------------------------------------------

$sourceId = "vaultwatch_$([guid]::NewGuid().ToString('N'))"
$createdSub = Register-ObjectEvent -InputObject $watcher -EventName Created -SourceIdentifier "$sourceId`_C"
$changedSub = Register-ObjectEvent -InputObject $watcher -EventName Changed -SourceIdentifier "$sourceId`_M"
$deletedSub = Register-ObjectEvent -InputObject $watcher -EventName Deleted -SourceIdentifier "$sourceId`_D"
$renamedSub = Register-ObjectEvent -InputObject $watcher -EventName Renamed -SourceIdentifier "$sourceId`_R"
$errorSub   = Register-ObjectEvent -InputObject $watcher -EventName Error   -SourceIdentifier "$sourceId`_E"

$watcher.EnableRaisingEvents = $true

# ---------------------------------------------------------------------------
# Main loop: dequeue PSEvent objects until Ctrl+C. Wait-Event with a short
# timeout blocks on the event queue without burning CPU, so an idle vault
# costs ~0% CPU.
# ---------------------------------------------------------------------------

try {
    while ($true) {
        $evt = Wait-Event -Timeout 1
        if ($null -eq $evt) { continue }

        switch -Wildcard ($evt.SourceIdentifier) {
            "${sourceId}_C" { Emit-Event 'Created' $evt.SourceEventArgs.FullPath $null }
            "${sourceId}_M" { Emit-Event 'Changed' $evt.SourceEventArgs.FullPath $null }
            "${sourceId}_D" { Emit-Event 'Deleted' $evt.SourceEventArgs.FullPath $null }
            "${sourceId}_R" { Emit-Event 'Renamed' $evt.SourceEventArgs.FullPath $evt.SourceEventArgs.OldFullPath }
            "${sourceId}_E" {
                $msg = $evt.SourceEventArgs.GetException().Message
                Write-Warning "FileSystemWatcher error: $msg (possible internal buffer overflow; bump -BufferKB)"
            }
        }
        Remove-Event -EventIdentifier $evt.EventIdentifier
    }
} finally {
    Write-Host ""
    Write-Host "Shutting down watcher..." -ForegroundColor Yellow
    try { $watcher.EnableRaisingEvents = $false } catch {}
    foreach ($sub in @($createdSub, $changedSub, $deletedSub, $renamedSub, $errorSub)) {
        if ($sub) {
            Unregister-Event -SourceIdentifier $sub.Name -ErrorAction SilentlyContinue
        }
    }
    try { $watcher.Dispose() } catch {}
    if ($writer) { try { $writer.Flush(); $writer.Dispose() } catch {} }
    Write-Host ("logged {0} events to {1}" -f $script:Counter, $LogPathResolved) -ForegroundColor Green
}
