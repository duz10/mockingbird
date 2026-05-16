"""Block writes that mutate raw transcript rows.

PLAN.md Section 12.3 and Section 7 invariant: rows in `transcripts`
where `stage='raw'` are IMMUTABLE. New facts about an utterance go
into a new row, never an UPDATE of the existing one.

This hook fires on edit_file / create_file / replace_in_file. It is
deliberately conservative — it blocks edits to `transcripts.rs` that
contain SQL UPDATE statements naming the raw stage. Phase 1 may need
to refine this if false positives surface; refine via PR with the
new heuristic, do not silently relax.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from _lib import allow, block, read_payload, tool_args  # noqa: E402

# Match SQL UPDATE statements that touch a column or row where stage='raw'
# or that update the transcripts table without an explicit stage filter
# (the latter is over-broad, but better safe than corrupt provenance).
RAW_UPDATE_PATTERNS = [
    re.compile(r"UPDATE\s+transcripts[^;]*stage\s*=\s*['\"]raw['\"]", re.IGNORECASE),
    re.compile(r"UPDATE\s+transcripts[^;]*WHERE[^;]*raw", re.IGNORECASE),
]


def main() -> None:
    payload = read_payload()
    args = tool_args(payload)
    path = (args.get("file_path") or args.get("path") or "").replace("\\", "/")

    # Only watch transcript-related Rust files.
    if not path.endswith("transcripts.rs") and "db/transcripts" not in path:
        allow()

    new_content = args.get("content") or args.get("new_str") or ""
    # replace_in_file passes an array of replacements
    replacements = args.get("replacements") or []
    for rep in replacements:
        new_content += "\n" + (rep.get("new_str") or "")

    for pat in RAW_UPDATE_PATTERNS:
        if pat.search(new_content):
            block(
                "Raw transcripts are immutable (PLAN Section 12.3). "
                "Write a new row instead of UPDATE-ing a stage='raw' row."
            )

    allow()


if __name__ == "__main__":
    main()
