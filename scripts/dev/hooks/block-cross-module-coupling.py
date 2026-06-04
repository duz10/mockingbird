"""Block cross-module coupling between dictation / meeting / activity.

Phase MC binding (ADR 0026) defined the meeting subsystem as a SIBLING
to dictation. Phase 10 Wave 1B (ADR 0036) adds the activity subsystem
as a THIRD sibling. The same import-direction rules apply pairwise:
sibling subsystems share infrastructure (audio / stt / cleanup /
window_context / db / settings / command_center) and nothing else.

This hook is the generalization of the original
`block-cross-module-coupling-meeting-dictation.py` (kept around for
backwards-compat-by-name; the entry in settings.json now points here).

Rules:

  - Files under `src-tauri/src/meetings/` may NOT import:
      crate::hotkey::driver, crate::hotkey::windows, crate::hotkey::state,
      crate::dictation::*, crate::injection::*, crate::recording_window,
      crate::activity::*

  - Files under `src-tauri/src/activity/` may NOT import:
      crate::hotkey::*, crate::dictation::*, crate::injection::*,
      crate::recording_window, crate::meetings::*

  - Files under `src-tauri/src/{dictation,hotkey,injection}/` and the
    files `recording_window.rs`, `cleanup/provider.rs`, and
    `cleanup/llm_cleaner.rs` may NOT import:
      crate::meetings::*, crate::activity::*

Shared / allowed surface (NOT blocked):
  crate::audio::*, crate::stt::*, crate::cleanup::ollama::OllamaProvider,
  crate::window_context::*, crate::db::*, crate::settings::*,
  crate::command_center::* (the orchestrator entry point), crate::error::*.

Exit codes (via _lib):
  0  allow (no forbidden import found, OR the file isn't in scope)
  1  block (forbidden import found; payload printed)
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from _lib import allow, block, read_payload, tool_args  # noqa: E402

# --- Path classifiers --------------------------------------------------------

MEETINGS_PATH = re.compile(r"src-tauri/src/meetings(/|$)", re.IGNORECASE)
ACTIVITY_PATH = re.compile(r"src-tauri/src/activity(/|$)", re.IGNORECASE)

DICTATION_SURFACE_PATHS = [
    re.compile(r"src-tauri/src/dictation(/|$)", re.IGNORECASE),
    re.compile(r"src-tauri/src/hotkey(/|$)", re.IGNORECASE),
    re.compile(r"src-tauri/src/injection(/|$)", re.IGNORECASE),
    re.compile(r"src-tauri/src/recording_window\.rs$", re.IGNORECASE),
    re.compile(r"src-tauri/src/cleanup/provider\.rs$", re.IGNORECASE),
    re.compile(r"src-tauri/src/cleanup/llm_cleaner\.rs$", re.IGNORECASE),
]

# --- Forbidden imports -------------------------------------------------------

FORBIDDEN_FROM_MEETINGS = [
    re.compile(r"\bcrate\s*::\s*hotkey\s*::\s*(driver|windows|state)\b"),
    re.compile(r"\buse\s+crate\s*::\s*dictation\b"),
    re.compile(r"\bcrate\s*::\s*dictation\s*::"),
    re.compile(r"\buse\s+crate\s*::\s*injection\b"),
    re.compile(r"\bcrate\s*::\s*injection\s*::"),
    re.compile(r"\buse\s+crate\s*::\s*recording_window\b"),
    re.compile(r"\bcrate\s*::\s*recording_window\b"),
    re.compile(r"\buse\s+crate\s*::\s*activity\b"),
    re.compile(r"\bcrate\s*::\s*activity\s*::"),
]

FORBIDDEN_FROM_ACTIVITY = [
    re.compile(r"\buse\s+crate\s*::\s*hotkey\b"),
    re.compile(r"\bcrate\s*::\s*hotkey\s*::"),
    re.compile(r"\buse\s+crate\s*::\s*dictation\b"),
    re.compile(r"\bcrate\s*::\s*dictation\s*::"),
    re.compile(r"\buse\s+crate\s*::\s*injection\b"),
    re.compile(r"\bcrate\s*::\s*injection\s*::"),
    re.compile(r"\buse\s+crate\s*::\s*recording_window\b"),
    re.compile(r"\bcrate\s*::\s*recording_window\b"),
    re.compile(r"\buse\s+crate\s*::\s*meetings\b"),
    re.compile(r"\bcrate\s*::\s*meetings\s*::"),
]

FORBIDDEN_FROM_DICTATION = [
    re.compile(r"\buse\s+crate\s*::\s*meetings\b"),
    re.compile(r"\bcrate\s*::\s*meetings\s*::"),
    re.compile(r"\buse\s+crate\s*::\s*activity\b"),
    re.compile(r"\bcrate\s*::\s*activity\s*::"),
]


def path_matches(path: str, regexes: list[re.Pattern[str]]) -> bool:
    return any(rgx.search(path) for rgx in regexes)


def gather_new_content(args: dict) -> str:
    """Pull together everything the tool is about to write to disk."""
    chunks: list[str] = []
    direct = args.get("content") or args.get("new_str")
    if isinstance(direct, str):
        chunks.append(direct)
    replacements = args.get("replacements") or []
    for rep in replacements:
        ns = rep.get("new_str")
        if isinstance(ns, str):
            chunks.append(ns)
    return "\n".join(chunks)


def main() -> None:
    payload = read_payload()
    args = tool_args(payload)
    raw_path = args.get("file_path") or args.get("path") or ""
    path = str(raw_path).replace("\\", "/")

    # Only Rust source files are in scope.
    if not path.endswith(".rs"):
        allow()

    new_content = gather_new_content(args)
    if not new_content.strip():
        allow()

    if MEETINGS_PATH.search(path):
        for pat in FORBIDDEN_FROM_MEETINGS:
            m = pat.search(new_content)
            if m:
                block(
                    "Sibling-subsystem binding (ADR 0026 + ADR 0036): files under "
                    "src-tauri/src/meetings/ must not import the dictation surface "
                    "or the activity subsystem. Forbidden snippet: "
                    f"{m.group(0)!r} in {path}. Allowed shared surface: "
                    "crate::audio::*, crate::stt::*, crate::window_context::*, "
                    "crate::db::*, crate::settings::*, crate::cleanup::ollama::OllamaProvider."
                )
        allow()

    if ACTIVITY_PATH.search(path):
        for pat in FORBIDDEN_FROM_ACTIVITY:
            m = pat.search(new_content)
            if m:
                block(
                    "Sibling-subsystem binding (ADR 0036): files under "
                    "src-tauri/src/activity/ must not import the dictation or meeting "
                    f"surfaces. Forbidden snippet: {m.group(0)!r} in {path}. "
                    "Allowed shared surface: crate::audio::*, crate::window_context::*, "
                    "crate::db::*, crate::settings::*, crate::error::*, and "
                    "crate::command_center::* for the orchestrator hook."
                )
        allow()

    if path_matches(path, DICTATION_SURFACE_PATHS):
        for pat in FORBIDDEN_FROM_DICTATION:
            m = pat.search(new_content)
            if m:
                block(
                    "Sibling-subsystem binding (ADR 0026 + ADR 0036): the dictation "
                    "surface (hotkey/, dictation/, injection/, recording_window.rs, "
                    "cleanup/provider.rs, cleanup/llm_cleaner.rs) must not import "
                    "crate::meetings or crate::activity. Forbidden snippet: "
                    f"{m.group(0)!r} in {path}. Sibling features are consumed via "
                    "the Tauri command surface in src-tauri/src/commands/ (meetings.rs, "
                    "activity.rs), not by direct Rust import."
                )
        allow()

    # File not in scope — silent allow.
    allow()


if __name__ == "__main__":
    main()
