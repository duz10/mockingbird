"""Block any `@tanstack/*` dep from sneaking into package.json.

PLAN.md Appendix D: the May 2026 Mini Shai-Hulud supply-chain
compromise hit 84 versions across 42 `@tanstack/*` packages. We
avoid the namespace entirely. Use `react-window` for virtualization
or hand-roll the component.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from _lib import allow, block, read_payload, tool_args  # noqa: E402

TANSTACK_PATTERN = re.compile(r'"@tanstack/[^"]+"')


def main() -> None:
    payload = read_payload()
    args = tool_args(payload)
    path = (args.get("file_path") or args.get("path") or "").replace("\\", "/")
    if not path.endswith("package.json"):
        allow()

    new_content = args.get("content") or args.get("new_str") or ""
    replacements = args.get("replacements") or []
    for rep in replacements:
        new_content += "\n" + (rep.get("new_str") or "")

    if TANSTACK_PATTERN.search(new_content):
        block(
            "@tanstack/* is banned per PLAN Appendix D (Mini Shai-Hulud). "
            "Use react-window or hand-roll the component."
        )

    allow()


if __name__ == "__main__":
    main()
