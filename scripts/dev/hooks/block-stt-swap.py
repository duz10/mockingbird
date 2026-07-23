"""Refuse staged changes that swap the STT engine away from whisper-rs/metal.

Wired as a git `pre-commit` hook (via lefthook). Scans the staged diff of
`src-tauri/Cargo.toml` for any of:

  * Removal lines (`-` in the unified diff) that mention `whisper-rs` and
    drop the `metal` feature.
  * Addition lines (`+` in the unified diff) that add a banned alternative
    STT dep — currently `mlx-whisper`, `speech-analyzer`, `whisperkit`.

The intent is hard-block on toolchain drift. If you legitimately need a
parallel STT engine for evaluation, do it on a research branch — never on
macos-port.

Bypass (emergency, rarely): `git commit --no-verify`.

For testability, the parser/classifier `should_block_diff()` is pure and
exercised directly from `_test_macport_hooks.py`. Main does the git plumbing.
"""

from __future__ import annotations

import re
import subprocess
import sys

TARGET_FILE = "src-tauri/Cargo.toml"

BANNED_NEW_DEPS = (
    "mlx-whisper",
    "speech-analyzer",
    "whisperkit",
)


WHISPER_LINE_RE = re.compile(r"\bwhisper-rs\b")
METAL_FEATURE_RE = re.compile(r'"metal"|metal\s*=')


def cached_diff(target: str) -> str:
    try:
        return subprocess.run(
            ["git", "diff", "--cached", "--no-color", "--unified=0", "--", target],
            capture_output=True, text=True, timeout=5,
        ).stdout
    except (FileNotFoundError, subprocess.TimeoutExpired, OSError):
        return ""


def should_block_diff(diff_text: str) -> list[str]:
    """Return list of human-readable findings (empty == allow).

    Rule semantics:
      * The *post-state* of every staged `whisper-rs` line must contain
        `metal`. A version bump that keeps the `metal` feature is fine.
      * Any `-whisper-rs ... metal` line *not* matched by a corresponding
        `+whisper-rs ... metal` line means metal was dropped → block.
      * Any `+` line introducing a banned alternative STT dep → block.
    """
    findings: list[str] = []
    plus_whisper_lines: list[tuple[int, str]] = []
    minus_whisper_with_metal_lines: list[tuple[int, str]] = []

    for ln_no, line in enumerate(diff_text.splitlines(), start=1):
        # Skip diff headers.
        if line.startswith(("+++", "---", "@@", "diff ", "index ")):
            continue

        if line.startswith("+"):
            body = line[1:]
            if WHISPER_LINE_RE.search(body):
                plus_whisper_lines.append((ln_no, body))
            for banned in BANNED_NEW_DEPS:
                if banned in body:
                    findings.append(
                        f"L{ln_no}: adds banned STT dep `{banned}` — "
                        f"{body.strip()[:120]}"
                    )
                    break
            continue

        if line.startswith("-"):
            body = line[1:]
            if WHISPER_LINE_RE.search(body) and METAL_FEATURE_RE.search(body):
                minus_whisper_with_metal_lines.append((ln_no, body))

    # Any `+whisper-rs` line that is MISSING metal → drift detected directly.
    for ln_no, body in plus_whisper_lines:
        if not METAL_FEATURE_RE.search(body):
            findings.append(
                f"L{ln_no}: whisper-rs line lacks `metal` feature — "
                f"{body.strip()[:120]}"
            )

    # True outright removal: -whisper-rs+metal with NO matching + line at all.
    # When there IS a + line (even one without metal), the previous loop has
    # already surfaced the drift — don't double-flag the same change.
    if minus_whisper_with_metal_lines and not plus_whisper_lines:
        for ln_no, body in minus_whisper_with_metal_lines:
            findings.append(
                f"L{ln_no}: removes whisper-rs+metal with no replacement — "
                f"{body.strip()[:120]}"
            )

    return findings


def main() -> int:
    diff = cached_diff(TARGET_FILE)
    if not diff:
        return 0
    findings = should_block_diff(diff)
    if findings:
        sys.stderr.write(
            "[BLOCKED] STT toolchain drift detected in src-tauri/Cargo.toml.\n"
            "Findings:\n  - "
            + "\n  - ".join(findings[:10])
            + "\nThe macos-port stays on whisper-rs + metal feature. If you need\n"
            "another engine for research, use a separate branch.\n"
        )
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
