"""Refuse any staged change to a `*windows.rs` file while on `macos-port`.

Wired as a git `pre-commit` hook (via lefthook). The hook reads the
current branch via `git rev-parse --abbrev-ref HEAD` and the staged
file list via `git diff --cached --name-only --diff-filter=AM`. If the
branch is `macos-port` and any staged path matches `*windows.rs`
(typically `src-tauri/src/<module>/windows.rs`), it exits 1 (BLOCK).

Anti-drift rationale: the macos-port branch must not touch Windows-cfg
modules. Edits to those belong on a Windows host. The branch-gate makes
this hook a no-op on every other branch.

Bypass (emergency, rarely): `git commit --no-verify`. Don't normalize that.

For testability, all I/O is funneled through `git_state()` and the pure
classifier `should_block()` is exercised directly by `_test_macport_hooks.py`.
"""

from __future__ import annotations

import fnmatch
import subprocess
import sys

WINDOWS_RS_GLOB = "*windows.rs"
PROTECTED_BRANCH = "macos-port"


def git_state() -> tuple[str, list[str]]:
    """Return (current_branch, staged_paths). Empty defaults on git error.

    Prefer `git symbolic-ref --short HEAD` so unborn branches (no commits
    yet) still report a real branch name rather than the literal "HEAD"
    that `rev-parse --abbrev-ref` falls back to.
    """
    try:
        branch = subprocess.run(
            ["git", "symbolic-ref", "--short", "HEAD"],
            capture_output=True, text=True, timeout=5,
        ).stdout.strip()
        if not branch:
            branch = subprocess.run(
                ["git", "rev-parse", "--abbrev-ref", "HEAD"],
                capture_output=True, text=True, timeout=5,
            ).stdout.strip()
    except (FileNotFoundError, subprocess.TimeoutExpired, OSError):
        return ("", [])
    try:
        out = subprocess.run(
            ["git", "diff", "--cached", "--name-only", "--diff-filter=AM"],
            capture_output=True, text=True, timeout=5,
        ).stdout
    except (FileNotFoundError, subprocess.TimeoutExpired, OSError):
        return (branch, [])
    paths = [ln.strip() for ln in out.splitlines() if ln.strip()]
    return (branch, paths)


def should_block(branch: str, staged_paths: list[str]) -> list[str]:
    """Return the list of offending staged paths (empty == allow)."""
    if branch != PROTECTED_BRANCH:
        return []
    return [p for p in staged_paths if fnmatch.fnmatch(p, WINDOWS_RS_GLOB)]


def main() -> int:
    branch, paths = git_state()
    bad = should_block(branch, paths)
    if bad:
        sys.stderr.write(
            "[BLOCKED] cannot edit *windows.rs files from the macos-port branch.\n"
            "Offending staged paths:\n  - "
            + "\n  - ".join(bad)
            + "\nWindows-cfg modules belong on a Windows host; do that edit\n"
            "on the Windows branch, then merge here via PR.\n"
        )
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
