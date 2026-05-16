"""Print a short briefing on session start.

Surfaces:
  - Current phase per STATUS.md
  - Last 5 commits (`git log --oneline -5`)
  - Latest `phase-*-complete` tag
  - Last successful judge run line from STATUS.md
  - Any "Blocked / human input needed" entries from STATUS.md
  - `bd ready` top 5 (if bd is installed and a workspace exists)

Always exits 0 — informational only.
"""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
STATUS = REPO_ROOT / "STATUS.md"


def section(title: str) -> "None":
    print(f"\n-- {title} --", file=sys.stderr)


def safe_run(cmd: list[str], timeout: int = 4) -> str:
    try:
        result = subprocess.run(
            cmd,
            capture_output=True,
            timeout=timeout,
            cwd=REPO_ROOT,
        )
    except (FileNotFoundError, subprocess.TimeoutExpired, OSError):
        return ""
    raw = result.stdout or b""
    try:
        return raw.decode("utf-8", errors="replace").rstrip()
    except (AttributeError, UnicodeError):
        return ""


def main() -> None:
    if not STATUS.exists():
        section("Mockingbird briefing")
        print("  STATUS.md does not exist yet — likely pre-bootstrap.", file=sys.stderr)
    else:
        text = STATUS.read_text(encoding="utf-8", errors="replace")
        section("Mockingbird briefing")
        for label, pat in [
            ("Phase", r"\*\*Current phase:\*\*\s*(.+)"),
            ("Updated", r"\*\*Last updated:\*\*\s*(.+)"),
            ("Last judge", r"##\s*Last successful judge run\s*\n+\s*-?\s*(.+)"),
        ]:
            m = re.search(pat, text)
            if m:
                print(f"  {label}: {m.group(1).strip()}", file=sys.stderr)
        blocked = re.search(
            r"##\s*Blocked.*?\n(.*?)(?:\n##|\Z)", text, re.IGNORECASE | re.DOTALL
        )
        if blocked and blocked.group(1).strip().lstrip("- ").strip().lower() not in {
            "(none)",
            "none",
            "",
        }:
            print("  Blocked-on:", file=sys.stderr)
            for ln in blocked.group(1).splitlines():
                if ln.strip().startswith("-"):
                    print(f"    {ln.rstrip()}", file=sys.stderr)

    log = safe_run(["git", "log", "--oneline", "-5"])
    if log:
        section("Last 5 commits")
        for ln in log.splitlines():
            print(f"  {ln}", file=sys.stderr)

    tag = safe_run(["git", "tag", "--list", "phase-*-complete", "--sort=-creatordate"])
    if tag:
        section("Latest phase tag")
        print(f"  {tag.splitlines()[0]}", file=sys.stderr)

    ready = safe_run(["bd", "ready"], timeout=6)
    if ready and "no active blockers" in ready.lower():
        section("bd ready (top 5)")
        for ln in ready.splitlines()[:5]:
            if ln.strip().startswith(("○", "◐", "●")):
                print(f"  {ln.rstrip()}", file=sys.stderr)
    sys.exit(0)


if __name__ == "__main__":
    main()
