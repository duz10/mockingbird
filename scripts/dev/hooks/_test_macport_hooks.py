"""Dry-run test cases for the four macOS-port wiggum hooks.

Mirrors the harness style of `_test_meeting_coupling.py`. Pure-function
helpers (`parse_push_lines`, `find_protected_pushes`, `should_block`,
`should_block_diff`) are imported and exercised directly — no git plumbing,
no temp repos. The main() functions that actually call git are validated
in `main_smoke` via `subprocess.run` with synthetic inputs where possible.

Run:
    python scripts/dev/hooks/_test_macport_hooks.py

Exit code = number of failed cases.
"""

from __future__ import annotations

import importlib.util
import subprocess
import sys
from pathlib import Path

HOOK_DIR = Path(__file__).parent


def _load(mod_filename: str):
    spec = importlib.util.spec_from_file_location(
        mod_filename.replace("-", "_").replace(".py", ""),
        HOOK_DIR / mod_filename,
    )
    mod = importlib.util.module_from_spec(spec)  # type: ignore[arg-type]
    spec.loader.exec_module(mod)  # type: ignore[union-attr]
    return mod


push_mod = _load("block-push-to-main.py")
wrs_mod = _load("block-windows-rs-edit-on-macport.py")
stt_mod = _load("block-stt-swap.py")


PUSH_CASES = [
    (
        "push only to feature branch (ALLOW)",
        "refs/heads/macos-port abc123 refs/heads/macos-port def456\n",
        [],
    ),
    (
        "push only to feature branch via -f (ALLOW)",
        "refs/heads/macos-port abc refs/heads/feature/x def\n",
        [],
    ),
    (
        "push to main (BLOCK)",
        "refs/heads/macos-port abc refs/heads/main def\n",
        ["refs/heads/main"],
    ),
    (
        "push to master (BLOCK)",
        "refs/heads/macos-port abc refs/heads/master def\n",
        ["refs/heads/master"],
    ),
    (
        "mixed: feature + main (BLOCK)",
        "refs/heads/macos-port a refs/heads/macos-port b\n"
        "refs/heads/main c refs/heads/main d\n",
        ["refs/heads/main"],
    ),
    (
        "empty stdin (ALLOW)",
        "",
        [],
    ),
    (
        "malformed line (ALLOW — no 4-tuple parsed)",
        "garbage\n",
        [],
    ),
]


WINDOWS_RS_CASES = [
    ("on macos-port, edits windows.rs (BLOCK)",
     "macos-port",
     ["src-tauri/src/secrets/windows.rs", "ui/x.ts"],
     ["src-tauri/src/secrets/windows.rs"]),
    ("on macos-port, no windows.rs (ALLOW)",
     "macos-port",
     ["src-tauri/src/secrets/macos.rs", "ui/x.ts"],
     []),
    ("on main, edits windows.rs (ALLOW — branch gate)",
     "main",
     ["src-tauri/src/secrets/windows.rs"],
     []),
    ("on macos-port, edits multiple windows.rs (BLOCK both)",
     "macos-port",
     ["src-tauri/src/hotkey/windows.rs", "src-tauri/src/injection/windows.rs"],
     ["src-tauri/src/hotkey/windows.rs", "src-tauri/src/injection/windows.rs"]),
    ("on macos-port, file path that *contains* windows but not windows.rs (ALLOW)",
     "macos-port",
     ["src-tauri/src/window_context/macos.rs"],
     []),
    ("empty branch (degenerate) — git unreachable, ALLOW",
     "",
     ["src-tauri/src/secrets/windows.rs"],
     []),
]


STT_DIFF_BLOCK = """\
diff --git a/src-tauri/Cargo.toml b/src-tauri/Cargo.toml
index abc..def 100644
--- a/src-tauri/Cargo.toml
+++ b/src-tauri/Cargo.toml
@@ -1,3 +1,3 @@
-whisper-rs = { version = "0.13", features = ["metal"] }
+whisper-rs = { version = "0.13", features = [] }
+mlx-whisper = "0.1"
"""

# Realistic version bump: BOTH a - and + line for whisper-rs, both with metal.
STT_DIFF_ALLOW_BUMP = """\
diff --git a/src-tauri/Cargo.toml b/src-tauri/Cargo.toml
index abc..def 100644
--- a/src-tauri/Cargo.toml
+++ b/src-tauri/Cargo.toml
@@ -1,3 +1,3 @@
-whisper-rs = { version = "0.13", features = ["metal"] }
+whisper-rs = { version = "0.13.1", features = ["metal"] }
"""

STT_DIFF_FEATURE_ADD = """\
diff --git a/src-tauri/Cargo.toml b/src-tauri/Cargo.toml
@@ -1,3 +1,4 @@
+metal = ["whisper-rs/metal"]
"""

# Sneaky: replaces with same dep but no metal feature.
STT_DIFF_DROP_METAL = """\
diff --git a/src-tauri/Cargo.toml b/src-tauri/Cargo.toml
@@ -1,3 +1,3 @@
-whisper-rs = { version = "0.13", features = ["metal"] }
+whisper-rs = { version = "0.13", features = [] }
"""

# Outright removal (no + line).
STT_DIFF_REMOVE = """\
diff --git a/src-tauri/Cargo.toml b/src-tauri/Cargo.toml
@@ -1,3 +1,2 @@
-whisper-rs = { version = "0.13", features = ["metal"] }
"""

STT_CASES = [
    ("removes metal + adds mlx-whisper (BLOCK)",        STT_DIFF_BLOCK, 2),
    ("version bump, keeps metal (ALLOW)",               STT_DIFF_ALLOW_BUMP, 0),
    ("adds metal feature gate (ALLOW)",                 STT_DIFF_FEATURE_ADD, 0),
    ("drops metal but keeps dep (BLOCK)",               STT_DIFF_DROP_METAL, 1),
    ("outright removes whisper-rs+metal (BLOCK)",       STT_DIFF_REMOVE, 1),
    ("empty diff (ALLOW)",                              "", 0),
]


def run_push_cases() -> int:
    fail = 0
    print("--- block-push-to-main ---")
    for name, stdin_text, want in PUSH_CASES:
        rows = push_mod.parse_push_lines(stdin_text)
        got = push_mod.find_protected_pushes(rows)
        ok = (sorted(got) == sorted(want))
        status = "OK" if ok else "FAIL"
        if not ok:
            fail += 1
            print(f"  [{status}] {name}\n      want={want}\n      got={got}")
        else:
            print(f"  [{status}] {name}")
    return fail


def run_windows_rs_cases() -> int:
    fail = 0
    print("--- block-windows-rs-edit-on-macport ---")
    for name, branch, paths, want in WINDOWS_RS_CASES:
        got = wrs_mod.should_block(branch, paths)
        ok = (sorted(got) == sorted(want))
        status = "OK" if ok else "FAIL"
        if not ok:
            fail += 1
            print(f"  [{status}] {name}\n      want={want}\n      got={got}")
        else:
            print(f"  [{status}] {name}")
    return fail


def run_stt_cases() -> int:
    fail = 0
    print("--- block-stt-swap ---")
    for name, diff_text, want_findings_count in STT_CASES:
        findings = stt_mod.should_block_diff(diff_text)
        ok = (len(findings) == want_findings_count)
        status = "OK" if ok else "FAIL"
        if not ok:
            fail += 1
            print(f"  [{status}] {name}  want_count={want_findings_count} got={len(findings)}")
            for f in findings:
                print(f"         finding: {f}")
        else:
            print(f"  [{status}] {name}  (findings={len(findings)})")
    return fail


def run_main_smoke() -> int:
    """End-to-end smoke: invoke main scripts via subprocess.

    block-push-to-main: feed stdin matching git's pre-push contract.
    The other two depend on the live repo state — we just confirm they exit 0
    (since nothing offending should be staged right now).
    """
    fail = 0
    print("--- main(): subprocess smoke ---")

    # block-push-to-main: BLOCK
    bad_stdin = b"refs/heads/macos-port abc refs/heads/main def\n"
    r = subprocess.run(
        [sys.executable, str(HOOK_DIR / "block-push-to-main.py")],
        input=bad_stdin, capture_output=True,
    )
    if r.returncode == 1:
        print("  [OK] block-push-to-main: exit 1 on refs/heads/main")
    else:
        fail += 1
        print(f"  [FAIL] block-push-to-main: want exit 1, got {r.returncode}\n{r.stderr.decode()}")

    # block-push-to-main: ALLOW
    r = subprocess.run(
        [sys.executable, str(HOOK_DIR / "block-push-to-main.py")],
        input=b"refs/heads/macos-port a refs/heads/macos-port b\n", capture_output=True,
    )
    if r.returncode == 0:
        print("  [OK] block-push-to-main: exit 0 on refs/heads/macos-port")
    else:
        fail += 1
        print(f"  [FAIL] block-push-to-main: want exit 0, got {r.returncode}")

    # block-windows-rs-edit-on-macport: with empty staged set (live git),
    # should exit 0.
    r = subprocess.run(
        [sys.executable, str(HOOK_DIR / "block-windows-rs-edit-on-macport.py")],
        capture_output=True,
    )
    if r.returncode == 0:
        print("  [OK] block-windows-rs-edit-on-macport: exit 0 on clean state")
    else:
        fail += 1
        print(f"  [FAIL] block-windows-rs-edit-on-macport: exit {r.returncode}\n{r.stderr.decode()}")

    # block-stt-swap: clean state, exit 0.
    r = subprocess.run(
        [sys.executable, str(HOOK_DIR / "block-stt-swap.py")],
        capture_output=True,
    )
    if r.returncode == 0:
        print("  [OK] block-stt-swap: exit 0 on clean state")
    else:
        fail += 1
        print(f"  [FAIL] block-stt-swap: exit {r.returncode}\n{r.stderr.decode()}")

    # verify-cargo-build-jobs: CARGO_BUILD_JOBS unset + no staged rust = exit 0
    import os
    env = os.environ.copy()
    env.pop("CARGO_BUILD_JOBS", None)
    r = subprocess.run(
        [sys.executable, str(HOOK_DIR / "verify-cargo-build-jobs.py")],
        capture_output=True, env=env,
    )
    if r.returncode == 0:
        print("  [OK] verify-cargo-build-jobs: exit 0 when no Rust staged")
    else:
        fail += 1
        print(f"  [FAIL] verify-cargo-build-jobs: exit {r.returncode}")

    return fail


def main() -> int:
    fail = 0
    fail += run_push_cases()
    fail += run_windows_rs_cases()
    fail += run_stt_cases()
    fail += run_main_smoke()
    print("---")
    print("FAILED" if fail else "ALL PASS")
    return fail


if __name__ == "__main__":
    sys.exit(main())
