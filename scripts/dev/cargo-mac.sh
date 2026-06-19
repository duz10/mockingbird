#!/usr/bin/env bash
#
# cargo-mac.sh -- run `cargo` with the macOS (Apple Silicon) build +
# runtime environment set up. The Mac analogue of
# scripts/dev/cargo-with-cuda.ps1.
#
# What it does, in order:
#   1. Sources ~/.cargo/env so cargo is on PATH even in minimal shells
#      (lefthook git hooks spawn a shell WITHOUT cargo on PATH -- see
#      the kennel gotcha; this wrapper is safe to call from anywhere).
#   2. Caps CARGO_BUILD_JOBS at 2 (constrained-RAM default; whisper-rs +
#      Metal shader compile is memory-hungry).
#   3. Exports MODEL_PATH (<repo>/models) so the STT/VAD loaders find
#      the GGUF + silero models, and ORT_DYLIB_PATH so `ort`'s
#      load-dynamic discovery finds libonnxruntime.dylib -- the Mac
#      equivalent of how cargo-with-cuda.ps1 wires the ORT env.
#   4. For build-y cargo verbs (build/check/run/test/clippy/bench/tauri/
#      doc/rustc/rustdoc), auto-injects `--features mockingbird/metal`
#      so Whisper runs on the GPU. Skipped if the user already passed
#      --features, or if MOCKINGBIRD_NO_METAL=1.
#
# Usage:
#   scripts/dev/cargo-mac.sh build --release
#   scripts/dev/cargo-mac.sh clippy --all-targets -- -D warnings
#   scripts/dev/cargo-mac.sh test --release
#   MOCKINGBIRD_NO_METAL=1 scripts/dev/cargo-mac.sh check   # CPU-only
#
set -euo pipefail

if [[ $# -eq 0 ]]; then
    echo "Usage: cargo-mac.sh <cargo args...>  e.g. ... build --release" >&2
    exit 1
fi

# --- 1. cargo on PATH (lefthook-hook-safe) ----------------------------------
# shellcheck disable=SC1090
[[ -f "${HOME}/.cargo/env" ]] && source "${HOME}/.cargo/env"
export PATH="${HOME}/.cargo/bin:${PATH}"

# --- 2. Parallelism cap -----------------------------------------------------
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"

# --- 3. Model + ORT runtime env ---------------------------------------------
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
MODELS_DIR="${MODELS_DIR:-${REPO_ROOT}/models}"

export MODEL_PATH="${MODEL_PATH:-${MODELS_DIR}}"

# ort uses load-dynamic; point it at the dylib fetched by
# scripts/download-onnxruntime.sh. Caller-set ORT_DYLIB_PATH wins.
if [[ -z "${ORT_DYLIB_PATH:-}" ]]; then
    ort_dylib="${MODELS_DIR}/libonnxruntime.dylib"
    if [[ -e "${ort_dylib}" ]]; then
        export ORT_DYLIB_PATH="${ort_dylib}"
    fi
fi

# --- 4. Auto-inject `--features mockingbird/metal` for build-y verbs ---------
feature_capable=" build check clippy test run bench doc rustc rustdoc tree tauri "
cargo_verb=""
for a in "$@"; do
    case "$a" in
        -*) ;;            # skip leading flags
        *) cargo_verb="$a"; break ;;
    esac
done

user_passed_features=0
for a in "$@"; do
    case "$a" in
        --features|--features=*) user_passed_features=1; break ;;
    esac
done

inject_metal=0
if [[ -n "${cargo_verb}" ]] \
    && [[ "${feature_capable}" == *" ${cargo_verb} "* ]] \
    && [[ "${user_passed_features}" -eq 0 ]] \
    && [[ "${MOCKINGBIRD_NO_METAL:-0}" != "1" ]]; then
    inject_metal=1
fi

# Build the effective arg list, inserting the feature flag right after
# the verb so it lands inside cargo's flag set (before any `--`
# passthrough boundary for test/clippy).
args=()
if [[ "${inject_metal}" -eq 1 ]]; then
    inserted=0
    for a in "$@"; do
        args+=("$a")
        if [[ "${inserted}" -eq 0 && "$a" == "${cargo_verb}" ]]; then
            args+=("--features" "mockingbird/metal")
            inserted=1
        fi
    done
else
    args=("$@")
fi

# --- 5. Invoke cargo --------------------------------------------------------
exec cargo "${args[@]}"
