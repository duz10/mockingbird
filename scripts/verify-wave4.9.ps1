# Wave 4.9 QA verification — runs all the SQL probes for Bug A and Bug B
# and tells you what to do for Bug C (clipboard) and the focus-change
# check, because those need a human at the keyboard.
#
# Usage:
#   pwsh scripts/verify-wave4.9.ps1            # one-shot dump
#   pwsh scripts/verify-wave4.9.ps1 -Watch     # repeats every 3 seconds
#
# Prereqs: do at least one dictation FIRST (RightAlt + speak + release)
# so there's a session row to inspect.

param(
    [switch]$Watch
)

$ErrorActionPreference = 'Stop'

$db = Join-Path $env:APPDATA 'com.dustin.mockingbird\mockingbird.db'
if (-not (Test-Path $db)) {
    Write-Host "❌ DB not found at $db — has mockingbird ever started?" -ForegroundColor Red
    exit 1
}

# Find sqlite3.exe. Try PATH first, then a couple of common spots.
$sqlite = Get-Command sqlite3.exe -ErrorAction SilentlyContinue
if (-not $sqlite) {
    $candidates = @(
        'C:\Program Files\sqlite\sqlite3.exe',
        "$env:USERPROFILE\scoop\apps\sqlite\current\sqlite3.exe",
        "$env:USERPROFILE\mockingbird_models\sqlite3.exe"
    )
    foreach ($c in $candidates) {
        if (Test-Path $c) { $sqlite = @{ Source = $c }; break }
    }
}

# Fallback: use rusqlite via cargo if sqlite3.exe isn't installed.
# Easier path — emit a one-shot Rust probe via `cargo run --example`
# but that needs an example; instead we just demand sqlite3.exe.
if (-not $sqlite) {
    Write-Host "❌ sqlite3.exe not found on PATH or in common locations." -ForegroundColor Red
    Write-Host "   Install via: winget install SQLite.SQLite" -ForegroundColor Yellow
    Write-Host "   Or download from: https://www.sqlite.org/download.html (precompiled tools)" -ForegroundColor Yellow
    Write-Host ""
    Write-Host "   Alternatively, you can copy-paste each query manually:" -ForegroundColor Cyan
    Write-Host "     sqlite3 `"$db`"" -ForegroundColor Cyan
    Write-Host "     SELECT stage, substr(text,1,40), model_used FROM transcripts ORDER BY id DESC LIMIT 6;" -ForegroundColor Cyan
    exit 2
}

$sqliteExe = $sqlite.Source

function Invoke-Sql {
    param([string]$Query, [string]$Header)
    Write-Host ""
    Write-Host ("=== " + $Header + " ===") -ForegroundColor Cyan
    & $sqliteExe -header -column $db $Query
}

function Show-Report {
    Clear-Host
    Write-Host "🐶 Wave 4.9 verification — DB at $db" -ForegroundColor Green
    Write-Host "    sqlite3: $sqliteExe"

    # --- Bug A: transcripts populated ---
    Invoke-Sql @"
SELECT s.id AS session_id,
       s.injection_status,
       COUNT(t.id) AS transcript_rows,
       GROUP_CONCAT(t.stage) AS stages
FROM sessions s
LEFT JOIN transcripts t ON t.session_id = s.id
GROUP BY s.id
ORDER BY s.id DESC
LIMIT 5;
"@ "BUG A — Last 5 sessions: transcript counts + stages (expect 3 stages for 'ok', 2 for 'aborted_*')"

    Invoke-Sql @"
SELECT id, session_id, stage,
       substr(text, 1, 40) AS text_preview,
       model_used,
       created_at
FROM transcripts
ORDER BY id DESC
LIMIT 6;
"@ "BUG A — Last 6 transcript rows (raw should have NULL model_used; cleaned + final should have 'passthrough')"

    # --- Bug B: foreground_app populated ---
    Invoke-Sql @"
SELECT foreground_app,
       COUNT(*) AS sessions,
       MAX(id) AS last_id
FROM sessions
GROUP BY foreground_app
ORDER BY sessions DESC;
"@ "BUG B — foreground_app distribution (NO empty strings should appear; expect 'notepad.exe', 'Code.exe', etc.)"

    # --- Sessions overview ---
    Invoke-Sql @"
SELECT id, foreground_app, injection_status, substr(recording_ended_at, 12, 8) AS time_utc
FROM sessions
ORDER BY id DESC
LIMIT 5;
"@ "OVERVIEW — Last 5 session rows"

    # --- Focus-change permissive check ---
    Invoke-Sql @"
SELECT injection_status, COUNT(*) AS cnt
FROM sessions
GROUP BY injection_status
ORDER BY cnt DESC;
"@ "ADR 0020 — injection_status distribution (Wave 4.9 should NOT produce 'aborted_focus_changed' on new sessions)"

    Write-Host ""
    Write-Host "--- HUMAN CHECKS (no SQL can verify these) ---" -ForegroundColor Yellow
    Write-Host ""
    Write-Host "BUG C — Clipboard restore:" -ForegroundColor Yellow
    Write-Host "  1. Copy the text OLD-CLIP-MARKER to your clipboard (highlight + Ctrl+C)."
    Write-Host "  2. Focus a Notepad window."
    Write-Host "  3. Hold RightAlt, say something like 'hello world test', release."
    Write-Host "  4. Notepad should show 'Hello world test.' (or similar)."
    Write-Host "  5. Click elsewhere (NOT Notepad), Ctrl+V."
    Write-Host "  6. PASS: you see 'OLD-CLIP-MARKER'."
    Write-Host "     FAIL: you see 'Hello world test.' — that means the restore is still busted."
    Write-Host ""
    Write-Host "PERMISSIVE FOCUS — ADR 0020:" -ForegroundColor Yellow
    Write-Host "  1. Focus Notepad."
    Write-Host "  2. Hold RightAlt."
    Write-Host "  3. While still holding, Alt+Tab to Chrome (any address bar / search box)."
    Write-Host "  4. Say something, release RightAlt."
    Write-Host "  5. PASS: text appears in Chrome. Re-run this script — the last session"
    Write-Host "     should have foreground_app='chrome.exe', injection_status='ok'."
    Write-Host "     FAIL: text goes nowhere AND last session is injection_status='aborted_focus_changed'."
    Write-Host ""
    Write-Host "Logs: $env:APPDATA\com.dustin.mockingbird\logs\" -ForegroundColor DarkGray
    Write-Host "Look for: 'focus changed during dictation; proceeding into key-up app'"
    Write-Host "Look for: 'clipboard sequence diverged' (should be RARE, not every paste)"
}

if ($Watch) {
    while ($true) {
        Show-Report
        Write-Host ""
        Write-Host "(refreshing in 3s — Ctrl+C to stop)" -ForegroundColor DarkGray
        Start-Sleep -Seconds 3
    }
} else {
    Show-Report
}
