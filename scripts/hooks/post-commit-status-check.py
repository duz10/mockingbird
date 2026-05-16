"""After `git commit`, warn if STATUS.md wasn't part of the commit.

PLAN.md AGENTS.md end-of-iteration rule: STATUS.md must be updated each
iteration so the next agent (post context-clear) can pick up the trail.
This is a warn (exit 2), not a block — there are legitimate `chore:`
commits where STATUS.md need not change.
"""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from _lib import allow, read_payload, tool_args, warn  # noqa: E402

COMMIT_PATTERN = re.compile(r"^\s*git\s+commit\b")


def main() -> None:
    payload = read_payload()
    args = tool_args(payload)
    command = args.get("command") or ""
    if not COMMIT_PATTERN.search(command):
        allow()

    try:
        proc = subprocess.run(
            ["git", "diff", "--name-only", "HEAD~1", "HEAD"],
            capture_output=True,
            timeout=5,
        )
    except (FileNotFoundError, subprocess.TimeoutExpired, OSError):
        allow()
    files = (proc.stdout or b"").decode("utf-8", errors="replace")

    if "STATUS.md" in files.splitlines():
        allow()
    warn(
        "STATUS.md was not updated in this commit. If this commit ends an "
        "iteration, the next agent (post context-clear) may lose track of "
        "progress. Amend if needed."
    )


if __name__ == "__main__":
    main()
