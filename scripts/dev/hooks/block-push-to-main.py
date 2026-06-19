"""Refuse any `git push` whose remote ref is `refs/heads/main` while on macOS.

Wired as a git `pre-push` hook (via lefthook). Git invokes pre-push with
no CLI args and feeds it stdin lines of the form:

    <local-ref> <local-sha1> <remote-ref> <remote-sha1>\\n

per the contract at https://git-scm.com/docs/githooks#_pre_push. We exit 1
(BLOCK) if any line targets `refs/heads/main`. Otherwise exit 0.

Anti-drift rationale: the macos-port branch must never land on main from
this Mac. Merges to main happen via PR on the host that owns Windows.

Bypass: `git push --no-verify` (lefthook honors it). Don't normalize that.
"""

from __future__ import annotations

import sys

MAIN_REF = "refs/heads/main"
PROTECTED_REFS = {MAIN_REF, "refs/heads/master"}


def parse_push_lines(text: str) -> list[tuple[str, str, str, str]]:
    """Parse git's pre-push stdin into (local_ref, local_sha, remote_ref, remote_sha) tuples."""
    out: list[tuple[str, str, str, str]] = []
    for raw in text.splitlines():
        parts = raw.strip().split()
        if len(parts) == 4:
            out.append((parts[0], parts[1], parts[2], parts[3]))
    return out


def find_protected_pushes(rows: list[tuple[str, str, str, str]]) -> list[str]:
    """Return the offending remote refs (if any)."""
    return [remote_ref for (_l, _ls, remote_ref, _rs) in rows if remote_ref in PROTECTED_REFS]


def main() -> int:
    rows = parse_push_lines(sys.stdin.read())
    bad = find_protected_pushes(rows)
    if bad:
        sys.stderr.write(
            "[BLOCKED] refusing push to protected ref(s) from this Mac: "
            + ", ".join(bad)
            + "\nThis is the macos-port branch. Push to origin/macos-port only;\n"
            "merge to main via PR on a host that owns the Windows build.\n"
        )
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
