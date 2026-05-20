"""Block cross-module coupling between meeting capture and dictation.

Phase MC binding (ADR 0026): the meeting subsystem is a SIBLING to the
dictation subsystem. They share `audio::AudioCapture`, the extended
`stt::SpeechToText` trait, and `cleanup::OllamaProvider`'s existing
arg-less `new()` — and nothing else.

This hook fires on edit_file / create_file / replace_in_file. It
inspects the new content for forbidden imports:

  - Files under `src-tauri/src/meetings/`   may NOT import:
      crate::hotkey::driver, crate::hotkey::windows, crate::hotkey::state,
      crate::dictation::*, crate::injection::*, crate::recording_window
      (the shared `cleanup::OllamaProvider` + `crate::audio` +
      `crate::stt` are explicitly allowed.)

  - Files under `src-tauri/src/{dictation,hotkey,injection}/` and the
    files `recording_window.rs`, `cleanup/provider.rs`, and
    `cleanup/llm_cleaner.rs` may NOT import:
      crate::meetings::*

The binding-list also forbids EDITING those dictation files at all
during Phase MC; this hook does NOT enforce edit-bans (those are a
plan-level rule), only import-direction discipline. The edit-ban
catches the rest at PR-review time + via the `mc-dictation-untouched`
judge in Wave 6.

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

DICTATION_SURFACE_PATHS = [
    re.compile(r"src-tauri/src/dictation(/|$)", re.IGNORECASE),
    re.compile(r"src-tauri/src/hotkey(/|$)", re.IGNORECASE),
    re.compile(r"src-tauri/src/injection(/|$)", re.IGNORECASE),
    re.compile(r"src-tauri/src/recording_window\.rs$", re.IGNORECASE),
    re.compile(r"src-tauri/src/cleanup/provider\.rs$", re.IGNORECASE),
    re.compile(r"src-tauri/src/cleanup/llm_cleaner\.rs$", re.IGNORECASE),
]

# --- Forbidden imports ------------------------------------------------------

# From inside meetings/, do NOT import:
#   - hotkey internals (driver/windows/state are the sealed sibling-
#     hook surface; meeting capture installs its OWN parallel hook)
#   - dictation orchestrator pieces
#   - injection (meetings never inject text)
#   - recording_window (the dictation overlay; meetings have their own)
FORBIDDEN_FROM_MEETINGS = [
    re.compile(r"\bcrate\s*::\s*hotkey\s*::\s*(driver|windows|state)\b"),
    re.compile(r"\buse\s+crate\s*::\s*dictation\b"),
    re.compile(r"\bcrate\s*::\s*dictation\s*::"),
    re.compile(r"\buse\s+crate\s*::\s*injection\b"),
    re.compile(r"\bcrate\s*::\s*injection\s*::"),
    re.compile(r"\buse\s+crate\s*::\s*recording_window\b"),
    re.compile(r"\bcrate\s*::\s*recording_window\b"),
]

# From inside the dictation surface, do NOT import the meetings module.
FORBIDDEN_FROM_DICTATION = [
    re.compile(r"\buse\s+crate\s*::\s*meetings\b"),
    re.compile(r"\bcrate\s*::\s*meetings\s*::"),
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
                    "Phase MC binding (ADR 0026): files under src-tauri/src/meetings/ "
                    f"must not import the dictation surface. Forbidden snippet: {m.group(0)!r} "
                    f"in {path}. Allowed shared surface: crate::audio::*, crate::stt::* "
                    "(including the additive transcribe_segments method), and "
                    "crate::cleanup::ollama::OllamaProvider's existing public new()."
                )
        allow()

    if path_matches(path, DICTATION_SURFACE_PATHS):
        for pat in FORBIDDEN_FROM_DICTATION:
            m = pat.search(new_content)
            if m:
                block(
                    "Phase MC binding (ADR 0026): the dictation surface "
                    "(hotkey/, dictation/, injection/, recording_window.rs, "
                    "cleanup/provider.rs, cleanup/llm_cleaner.rs) must not import "
                    f"crate::meetings. Forbidden snippet: {m.group(0)!r} in {path}. "
                    "Meeting features are consumed via the Tauri command surface in "
                    "src-tauri/src/commands/meetings.rs (Wave 4), not by direct Rust import."
                )
        allow()

    # File not in scope — silent allow.
    allow()


if __name__ == "__main__":
    main()
