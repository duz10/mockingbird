"""Warn (do not block) when something writes to the clipboard outside `paste.rs`.

PLAN.md Section 12.17: clipboard save/restore is mandatory around every
paste. Bare `set_clipboard` calls in code paths other than `paste.rs`
likely violate that rule. This is a warn-only hook because there are
legitimate paths (e.g. user-initiated copy from history view) where
direct clipboard writes are fine — the human reviews.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from _lib import allow, read_payload, tool_args, warn  # noqa: E402

# Detects `git add` / shell edits that introduce clipboard writes outside paste.rs
# Pre-tool fires on shell commands; we look for risky shell-side `clipboard`
# manipulation patterns. (The narrower static-analysis check is implemented
# in a Rust lint at Phase 3; this hook is the shell-side belt to that braces.)
CLIPBOARD_WRITE_PATTERN = re.compile(
    r"(set-clipboard|Set-Clipboard|clip\.exe|pbcopy)", re.IGNORECASE
)


def main() -> None:
    payload = read_payload()
    args = tool_args(payload)
    command = args.get("command") or ""
    if not CLIPBOARD_WRITE_PATTERN.search(command):
        allow()
    warn(
        "Direct clipboard write detected. If this is part of dictation injection, "
        "route through paste.rs (save+restore is mandatory — PLAN Section 12.17)."
    )


if __name__ == "__main__":
    main()
