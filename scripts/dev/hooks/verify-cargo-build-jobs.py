"""Warn (exit 2) if `CARGO_BUILD_JOBS` is unset while Rust is being touched.

Wired as a git `pre-commit` hook (via lefthook). Best-effort safeguard:
LTO ICE / OOM on constrained-RAM Macs is the failure mode this prevents.
We only emit a warning when the staged set actually contains Rust files
(`*.rs`, `Cargo.toml`, `Cargo.lock`) — otherwise this is a no-op.

Exit codes:
  0  CARGO_BUILD_JOBS is set, or no Rust files staged.
  2  Rust files staged but env var unset. Lefthook treats this as warn:
     stderr surfaces but the commit proceeds.

The Phase 0 zshrc append (`export CARGO_BUILD_JOBS=2`) should keep this
silent on a fresh shell.
"""

from __future__ import annotations

import os
import subprocess
import sys

RUST_PATH_HINTS = ("Cargo.toml", "Cargo.lock")


def staged_rust_paths() -> list[str]:
    try:
        out = subprocess.run(
            ["git", "diff", "--cached", "--name-only", "--diff-filter=AM"],
            capture_output=True, text=True, timeout=5,
        ).stdout
    except (FileNotFoundError, subprocess.TimeoutExpired, OSError):
        return []
    paths = [ln.strip() for ln in out.splitlines() if ln.strip()]
    return [
        p for p in paths
        if p.endswith(".rs") or any(p.endswith(hint) or hint in p.split("/") for hint in RUST_PATH_HINTS)
    ]


def main() -> int:
    if os.environ.get("CARGO_BUILD_JOBS"):
        return 0
    paths = staged_rust_paths()
    if not paths:
        return 0
    sys.stderr.write(
        "[WARN] CARGO_BUILD_JOBS is unset and Rust files are staged.\n"
        "      Constrained-RAM Macs can ICE during LTO under default parallelism.\n"
        "      Add to ~/.zshrc:  export CARGO_BUILD_JOBS=2\n"
        f"      Staged Rust paths (first 5): {paths[:5]}\n"
    )
    return 2


if __name__ == "__main__":
    sys.exit(main())
