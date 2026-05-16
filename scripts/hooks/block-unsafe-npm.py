"""Veto `npm/pnpm/yarn install|ci|i` without `--ignore-scripts`.

PLAN.md Appendix D primary defense. `.npmrc` sets ignore-scripts=true
repo-wide, but the flag is enforced explicitly because (a) it's clearer
in command logs and (b) someone might run from a directory whose .npmrc
is shadowed.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from _lib import allow, block, read_payload, tool_args  # noqa: E402

# Match npm/pnpm/yarn install commands.
INSTALL_PATTERN = re.compile(
    r"\b(npm|pnpm|yarn)\s+(install|ci|i)(?![\w-])", re.IGNORECASE
)


def main() -> None:
    payload = read_payload()
    args = tool_args(payload)
    command = args.get("command") or ""
    if not INSTALL_PATTERN.search(command):
        allow()
    if "--ignore-scripts" in command:
        allow()
    block(
        "npm/pnpm/yarn install/ci must include --ignore-scripts "
        "(PLAN Appendix D). Add the flag or set .npmrc ignore-scripts=true "
        "and re-run with it explicit."
    )


if __name__ == "__main__":
    main()
