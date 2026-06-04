"""Refuse session exit if fmt / clippy / tests are red.

PLAN.md Section 11 + AGENTS.md end-of-iteration rule: every iteration
ends green. This hook is the mechanical enforcement of that rule.

Runs in sequence:
  1. cargo fmt --check
  2. cargo clippy -- -D warnings
  3. cargo test --quiet

The first failure short-circuits and reports its last 20 lines. We
deliberately skip npm-side checks here — Tauri's `npm run build` is
slow; npm hygiene is a phase-specific judge concern.

If `Cargo.toml` doesn't exist yet (pre-Phase-1), every check is a
no-op success.
"""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
CARGO_TOML = REPO_ROOT / "Cargo.toml"


def run_or_block(cmd: list[str], label: str) -> "None":
    try:
        result = subprocess.run(
            cmd, capture_output=True, text=True, timeout=110, cwd=REPO_ROOT
        )
    except FileNotFoundError:
        print(
            f"[BLOCKED] Quality gate: `{cmd[0]}` not on PATH. Install it or "
            "remove the check from settings.json with justification.",
            file=sys.stderr,
        )
        sys.exit(1)
    except subprocess.TimeoutExpired:
        print(f"[BLOCKED] Quality gate: `{label}` timed out (>110s).", file=sys.stderr)
        sys.exit(1)
    if result.returncode == 0:
        return
    tail = "\n".join((result.stdout + "\n" + result.stderr).splitlines()[-20:])
    print(
        f"[BLOCKED] Quality gate failed at `{label}`. Last 20 lines:\n{tail}\n"
        "Fix locally, then exit again.",
        file=sys.stderr,
    )
    sys.exit(1)


def main() -> None:
    if not CARGO_TOML.exists():
        # Pre-Phase-1: no Rust crate yet; the gate is a no-op.
        sys.exit(0)
    run_or_block(["cargo", "fmt", "--check"], "cargo fmt --check")
    run_or_block(
        ["cargo", "clippy", "--", "-D", "warnings"], "cargo clippy -- -D warnings"
    )
    run_or_block(["cargo", "test", "--quiet"], "cargo test --quiet")
    sys.exit(0)


if __name__ == "__main__":
    main()
