# Smoke-test each hook with synthetic JSON. Each test reports PASS/FAIL.
# Hook exit codes: 0 = allow, 1 = block, 2 = warn.
$ErrorActionPreference = "Continue"
$PSNativeCommandUseErrorActionPreference = $false
$hookDir = $PSScriptRoot
$pass = 0
$fail = 0

function Check($name, $hook, $stdin, $want) {
    $script:total++
    $tmpIn  = [IO.Path]::GetTempFileName()
    [IO.File]::WriteAllText($tmpIn, $stdin)
    cmd /c "python `"$hookDir\$hook`" < `"$tmpIn`"" > $null 2>&1
    $got = $LASTEXITCODE
    Remove-Item $tmpIn -Force -ErrorAction SilentlyContinue
    if ($got -eq $want) {
        Write-Host "  PASS  $name  -> exit $got"
        $script:pass++
    } else {
        Write-Host "  FAIL  $name  -> exit $got (expected $want)" -ForegroundColor Red
        $script:fail++
    }
}

Write-Host "=== Hook smoke tests ==="

# block-raw-transcript-edit
Check "block-raw: allow non-transcript file" "block-raw-transcript-edit.py" '{"tool_name":"create_file","arguments":{"file_path":"src/foo.rs","content":"fn main(){}"}}' 0
Check "block-raw: allow transcripts.rs without bad SQL" "block-raw-transcript-edit.py" '{"tool_name":"create_file","arguments":{"file_path":"src-tauri/src/db/transcripts.rs","content":"INSERT INTO transcripts VALUES (?)"}}' 0
Check "block-raw: BLOCK UPDATE on raw stage" "block-raw-transcript-edit.py" '{"tool_name":"create_file","arguments":{"file_path":"src-tauri/src/db/transcripts.rs","content":"UPDATE transcripts SET text=? WHERE stage=''raw'' AND id=?"}}' 1

# block-migration-edit-after-phase-1  (no tag exists yet -> always allow)
Check "block-migration: allow when no phase-1 tag" "block-migration-edit-after-phase-1.py" '{"tool_name":"edit_file","arguments":{"file_path":"src-tauri/src/db/migrations/001_initial.sql"}}' 0

# block-tanstack
Check "block-tanstack: allow non-package.json" "block-tanstack.py" '{"tool_name":"edit_file","arguments":{"file_path":"src/foo.tsx","content":"@tanstack/react-table"}}' 0
Check "block-tanstack: BLOCK @tanstack in package.json" "block-tanstack.py" '{"tool_name":"edit_file","arguments":{"file_path":"package.json","content":"{\"dependencies\":{\"@tanstack/react-table\":\"8.0.0\"}}"}}' 1
Check "block-tanstack: allow clean package.json" "block-tanstack.py" '{"tool_name":"edit_file","arguments":{"file_path":"package.json","content":"{\"dependencies\":{\"react-window\":\"1.8.0\"}}"}}' 0

# block-unsafe-npm
Check "block-unsafe-npm: BLOCK bare npm ci" "block-unsafe-npm.py" '{"tool_name":"agent_run_shell_command","arguments":{"command":"npm ci"}}' 1
Check "block-unsafe-npm: allow npm ci --ignore-scripts" "block-unsafe-npm.py" '{"tool_name":"agent_run_shell_command","arguments":{"command":"npm ci --ignore-scripts"}}' 0
Check "block-unsafe-npm: allow npm run build" "block-unsafe-npm.py" '{"tool_name":"agent_run_shell_command","arguments":{"command":"npm run build"}}' 0
Check "block-unsafe-npm: BLOCK pnpm install" "block-unsafe-npm.py" '{"tool_name":"agent_run_shell_command","arguments":{"command":"pnpm install"}}' 1

# warn-bare-clipboard-set
Check "warn-clipboard: allow normal command" "warn-bare-clipboard-set.py" '{"tool_name":"agent_run_shell_command","arguments":{"command":"echo hi"}}' 0
Check "warn-clipboard: WARN on Set-Clipboard" "warn-bare-clipboard-set.py" '{"tool_name":"agent_run_shell_command","arguments":{"command":"Set-Clipboard -Value test"}}' 2

# block-secret-commit  (no staged diff -> always allow because the subprocess returns empty)
Check "block-secret: allow non-commit command" "block-secret-commit.py" '{"tool_name":"agent_run_shell_command","arguments":{"command":"ls"}}' 0

# post-commit-status-check  (not a commit -> allow)
Check "post-commit: allow non-commit" "post-commit-status-check.py" '{"tool_name":"agent_run_shell_command","arguments":{"command":"git status"}}' 0

# session-start-briefing  (always allow / informational)
Check "session-start: always exit 0" "session-start-briefing.py" '{}' 0

# stop-quality-gate  (no Cargo.toml yet -> allow)
Check "stop-gate: allow pre-cargo (no Cargo.toml yet)" "stop-quality-gate.py" '{}' 0

Write-Host ""
Write-Host "=== Result: $pass passed, $fail failed ==="
if ($fail -gt 0) { exit 1 } else { exit 0 }
