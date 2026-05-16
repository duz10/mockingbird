"""Veto `git commit` if staged changes look like they contain secrets.

PLAN.md AGENTS.md "Never do" list: never commit .env, *.key, *.pfx,
or plausible secret strings. Scans `git diff --cached` for:
  - filenames matching .env, *.key, *.pfx, *.pem
  - lines that look like api_key=..., secret=..., password=..., token=...
  - high-entropy strings (rough heuristic — base64-ish 40+ chars)
"""

from __future__ import annotations

import math
import re
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from _lib import allow, block, read_payload, tool_args  # noqa: E402

COMMIT_PATTERN = re.compile(r"^\s*git\s+commit\b")
SENSITIVE_FILENAME = re.compile(r"\.(env|key|pfx|pem)(\b|$)", re.IGNORECASE)
SECRET_ASSIGNMENT = re.compile(
    r"(api[_-]?key|secret|password|passwd|token|access[_-]?key)\s*[:=]\s*[\"']?([A-Za-z0-9+/_=\-]{16,})",
    re.IGNORECASE,
)
HIGH_ENTROPY_TOKEN = re.compile(r"[A-Za-z0-9+/]{40,}={0,2}")

# Known-public material that legitimately ships in a repo. A line containing
# any of these substrings (after base64-decode where applicable) is exempt
# from the high-entropy scan. Lines stay subject to SECRET_ASSIGNMENT.
KNOWN_PUBLIC_PREFIXES = [
    # Tauri / minisign public key header. The base64 of
    # b"untrusted comment: minisign public key" begins with this prefix.
    "dW50cnVzdGVkIGNvbW1lbnQ6",
    # PEM public keys
    "-----BEGIN PUBLIC KEY-----",
    "-----BEGIN RSA PUBLIC KEY-----",
    "-----BEGIN OPENSSH PUBLIC KEY-----",
    # SSH authorized_keys format
    "ssh-rsa ",
    "ssh-ed25519 ",
    "ecdsa-sha2-",
    # GPG public key block
    "-----BEGIN PGP PUBLIC KEY BLOCK-----",
]
# Inline pragma to exempt a line the human has reviewed.
ALLOWLIST_PRAGMA = "pragma: allow-secret-scan"


def is_known_public_line(body: str) -> bool:
    return ALLOWLIST_PRAGMA in body or any(p in body for p in KNOWN_PUBLIC_PREFIXES)


def shannon_entropy(s: str) -> float:
    if not s:
        return 0.0
    freq: dict[str, int] = {}
    for ch in s:
        freq[ch] = freq.get(ch, 0) + 1
    n = len(s)
    return -sum((c / n) * math.log2(c / n) for c in freq.values())


def main() -> None:
    payload = read_payload()
    args = tool_args(payload)
    command = args.get("command") or ""
    if not COMMIT_PATTERN.search(command):
        allow()

    try:
        proc = subprocess.run(
            ["git", "diff", "--cached", "--no-color"],
            capture_output=True,
            timeout=8,
        )
    except (FileNotFoundError, subprocess.TimeoutExpired, OSError):
        allow()
    diff = (proc.stdout or b"").decode("utf-8", errors="replace")

    findings: list[str] = []
    for ln_no, line in enumerate(diff.splitlines(), start=1):
        # File header lines like `+++ b/path/to/.env`
        if line.startswith("+++ ") and SENSITIVE_FILENAME.search(line):
            findings.append(f"L{ln_no}: sensitive filename — {line.strip()}")
            continue
        if not line.startswith("+") or line.startswith("+++"):
            continue
        body = line[1:]
        m = SECRET_ASSIGNMENT.search(body)
        if m:
            findings.append(f"L{ln_no}: secret-like assignment — {m.group(1)}=…")
            continue
        if is_known_public_line(body):
            continue
        for tok in HIGH_ENTROPY_TOKEN.findall(body):
            if shannon_entropy(tok) >= 4.0:
                findings.append(f"L{ln_no}: high-entropy token ({len(tok)} chars)")
                break

    if findings:
        block(
            "Possible secret in staged changes. Findings:\n  - "
            + "\n  - ".join(findings[:10])
            + "\nUnstage with `git restore --staged <file>` or move secrets "
            "to DPAPI / env vars before committing."
        )
    allow()


if __name__ == "__main__":
    main()
