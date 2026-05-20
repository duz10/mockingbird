"""Dry-run test cases for block-cross-module-coupling-meeting-dictation.

NOT a wired test (the hook scripts have no pytest layer); just a
runnable harness so the Wave 1 author can confirm the hook does what
it says. Delete once Wave 2 adds a real pytest layer to scripts/hooks/.
"""

from __future__ import annotations

import json
import subprocess
import sys

CASES = [
    {
        "name": "meetings imports audio (ALLOW)",
        "path": "src-tauri/src/meetings/runtime.rs",
        "content": "use crate::audio::AudioCapture;\nuse crate::stt::SpeechToText;",
        "want": 0,
    },
    {
        "name": "meetings imports OllamaProvider (ALLOW)",
        "path": "src-tauri/src/meetings/llm_pass.rs",
        "content": "use crate::cleanup::ollama::OllamaProvider;",
        "want": 0,
    },
    {
        "name": "meetings imports dictation::runtime (BLOCK)",
        "path": "src-tauri/src/meetings/x.rs",
        "content": "use crate::dictation::runtime::Foo;",
        "want": 1,
    },
    {
        "name": "meetings imports hotkey::driver (BLOCK)",
        "path": "src-tauri/src/meetings/x.rs",
        "content": "use crate::hotkey::driver::HotkeyDriver;",
        "want": 1,
    },
    {
        "name": "dictation imports meetings (BLOCK)",
        "path": "src-tauri/src/dictation/runtime.rs",
        "content": "use crate::meetings::activation::Foo;",
        "want": 1,
    },
    {
        "name": "meetings imports injection (BLOCK)",
        "path": "src-tauri/src/meetings/q.rs",
        "content": "use crate::injection::paste::do_thing;",
        "want": 1,
    },
    {
        "name": "unrelated md file (ALLOW; not .rs)",
        "path": "docs/notes.md",
        "content": "use crate::dictation::Foo;",
        "want": 0,
    },
    {
        "name": "commands/meetings.rs imports crate::meetings (ALLOW)",
        "path": "src-tauri/src/commands/meetings.rs",
        "content": "use crate::meetings::runtime::MeetingCaptureRuntime;",
        "want": 0,
    },
    {
        "name": "recording_window.rs imports meetings (BLOCK)",
        "path": "src-tauri/src/recording_window.rs",
        "content": "use crate::meetings::overlay::MeetingOverlay;",
        "want": 1,
    },
]


def main() -> int:
    fail = 0
    for c in CASES:
        payload = {
            "tool_name": "create_file",
            "input": {"file_path": c["path"], "content": c["content"]},
        }
        r = subprocess.run(
            [
                "python",
                "scripts/hooks/block-cross-module-coupling-meeting-dictation.py",
            ],
            input=json.dumps(payload).encode(),
            capture_output=True,
        )
        status = "OK" if r.returncode == c["want"] else "FAIL"
        if status == "FAIL":
            fail += 1
        print(f"[{status}] want={c['want']} got={r.returncode}  {c['name']}")
    print("---")
    print("FAILED" if fail else "ALL PASS")
    return fail


if __name__ == "__main__":
    sys.exit(main())
