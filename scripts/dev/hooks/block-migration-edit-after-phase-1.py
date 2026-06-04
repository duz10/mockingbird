"""Block edits to migrations 001/002/003 once Phase 1 has shipped.

PLAN.md Section 7: migrations are append-only after Phase 1 closes.
New schema changes go in new migration files, never by editing a
sealed one. The seal is the `phase-1-complete` git tag.
"""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from _lib import allow, block, read_payload, tool_args  # noqa: E402

SEALED_PATH = re.compile(
    r"src-tauri/src/db/migrations/00[123]_.*\.sql$", re.IGNORECASE
)


def phase_1_sealed() -> bool:
    try:
        result = subprocess.run(
            ["git", "tag", "--list", "phase-1-complete"],
            capture_output=True,
            timeout=3,
        )
    except (FileNotFoundError, subprocess.TimeoutExpired, OSError):
        return False
    return bool((result.stdout or b"").decode("utf-8", errors="replace").strip())


def main() -> None:
    payload = read_payload()
    args = tool_args(payload)
    path = (args.get("file_path") or args.get("path") or "").replace("\\", "/")
    if not SEALED_PATH.search(path):
        allow()
    if not phase_1_sealed():
        allow()
    block(
        f"Migrations 001/002/003 are frozen after Phase 1 (tag phase-1-complete exists). "
        f"Add a new migration file instead of editing {path}."
    )


if __name__ == "__main__":
    main()
