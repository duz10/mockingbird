"""Shared helpers for Mockingbird hook scripts.

Hooks receive a JSON payload on stdin describing the tool call about to
run (PreToolUse), the result (PostToolUse), session metadata
(SessionStart), or exit context (Stop). They communicate back via:

  exit 0  - allow / no comment
  exit 1  - BLOCK (veto). stdout/stderr surfaces to the agent.
  exit 2  - WARN (allow but flag). stdout/stderr surfaces to the agent.

Keep these scripts tiny, fast (< 1s typical), and dependency-free
(stdlib only). They run on every tool call.
"""

from __future__ import annotations

import json
import sys
from typing import Any


def read_payload() -> dict[str, Any]:
    """Read and parse the JSON payload from stdin. Return {} on failure."""
    raw = sys.stdin.read()
    if not raw.strip():
        return {}
    try:
        return json.loads(raw)
    except json.JSONDecodeError:
        return {}


def block(msg: str) -> "None":
    """Exit 1 — veto the tool call with a message."""
    print(f"[BLOCKED] {msg}", file=sys.stderr)
    sys.exit(1)


def warn(msg: str) -> "None":
    """Exit 2 — allow but warn."""
    print(f"[WARN] {msg}", file=sys.stderr)
    sys.exit(2)


def allow() -> "None":
    """Exit 0 — silent allow."""
    sys.exit(0)


def tool_name(payload: dict[str, Any]) -> str:
    """Best-effort extraction of the tool name from a hook payload."""
    return (
        payload.get("tool_name")
        or payload.get("tool")
        or payload.get("name")
        or ""
    )


def tool_args(payload: dict[str, Any]) -> dict[str, Any]:
    """Best-effort extraction of tool arguments."""
    return (
        payload.get("tool_args")
        or payload.get("arguments")
        or payload.get("args")
        or payload.get("input")
        or {}
    )
