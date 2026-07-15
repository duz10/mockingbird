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

# --- 2b. macOS deployment target (whisper.cpp native-build floor) ------------
# whisper-rs-sys compiles ggml/whisper.cpp via the `cmake`/`cc` crates. clang's
# effective -mmacosx-version-min decides two things on a from-scratch RELEASE
# native compile:
#   1. COMPILE: below 10.15, libc++ marks std::filesystem::path unavailable, so
#      ggml-backend-reg.cpp fails with "'path' is unavailable: introduced in
#      macOS 10.15" (20 errors, cmake exits 2).
#   2. LINK: below macOS 15, ggml-metal's `@available(macOS 15, *)` guards for
#      Metal residency sets (MTLResidencySet, 15.0-only) become RUNTIME checks
#      that call the compiler-rt builtin `___isPlatformVersionAtLeast`, which
#      rustc's linker does not pull in -> undefined-symbol link failure.
# 15.0 is also the app's real floor -- ScreenCaptureKit unified single-session
# audio capture requires macOS 15+ (see AGENTS-MACBUILD.md).
#
# IMPORTANT -- which layer actually governs the target:
#   * For PLAIN cargo verbs (build/check/test/clippy without tauri) this export
#     is authoritative: without it `cc` defaults to 11.0 for aarch64, and this
#     pins those paths to 15.0 for parity with the shipped .app.
#   * For `cargo tauri {build,dev}` this export is IGNORED. tauri-cli
#     unconditionally set_var("MACOSX_DEPLOYMENT_TARGET", <minimumSystemVersion>)
#     (build.rs), whose default is "10.13" -- so the deployment target for the
#     release .app is owned by `bundle.macOS.minimumSystemVersion` in
#     src-tauri/tauri.conf.json (set to "15.0"). Keep the two in sync there;
#     changing only this line will NOT fix a `tauri build`. (mb-d6i)
# Caller override wins.
export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-15.0}"

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
#
# Placement matters. For plain cargo verbs (build/check/test/...) the
# feature flag goes right after the verb. For `cargo tauri dev|build`,
# cargo-tauri rejects flags placed BEFORE its subcommand, so the flag
# must land AFTER the tauri subcommand:
#     cargo tauri dev --features mockingbird/metal     (correct)
#     cargo tauri --features mockingbird/metal dev     (WRONG -- rejected:
#                                                       "unexpected argument")
feature_capable=" build check clippy test run bench doc rustc rustdoc tree tauri "
tauri_feature_subcommands=" dev build "

# Find the cargo verb (first non-flag token) and its 0-based index.
cargo_verb=""
verb_index=-1
idx=0
for a in "$@"; do
    case "$a" in
        -*) ;;            # skip leading flags
        *) cargo_verb="$a"; verb_index="${idx}"; break ;;
    esac
    idx=$((idx + 1))
done

user_passed_features=0
for a in "$@"; do
    case "$a" in
        --features|--features=*) user_passed_features=1; break ;;
    esac
done

# Decide the token index AFTER which we splice the feature flag.
#   plain verb -> right after the verb.
#   tauri verb -> right after the tauri subcommand (dev|build only; other
#                 tauri subcommands don't take --features, so skip them).
insert_after_index=-1
if [[ -n "${cargo_verb}" ]] \
    && [[ "${feature_capable}" == *" ${cargo_verb} "* ]] \
    && [[ "${user_passed_features}" -eq 0 ]] \
    && [[ "${MOCKINGBIRD_NO_METAL:-0}" != "1" ]]; then
    if [[ "${cargo_verb}" == "tauri" ]]; then
        # Locate the tauri subcommand: first non-flag token after the verb.
        sub=""
        sub_index=-1
        idx=0
        for a in "$@"; do
            if [[ "${idx}" -gt "${verb_index}" ]]; then
                case "$a" in
                    -*) ;;
                    *) sub="$a"; sub_index="${idx}"; break ;;
                esac
            fi
            idx=$((idx + 1))
        done
        if [[ -n "${sub}" ]] \
            && [[ "${tauri_feature_subcommands}" == *" ${sub} "* ]]; then
            insert_after_index="${sub_index}"
        fi
    else
        insert_after_index="${verb_index}"
    fi
fi

# Build the effective arg list, splicing the feature flag after the chosen
# token (verb, or tauri subcommand). For plain build-y verbs this lands
# inside cargo's flag set, before any `--` passthrough boundary.
args=()
if [[ "${insert_after_index}" -ge 0 ]]; then
    idx=0
    for a in "$@"; do
        args+=("$a")
        if [[ "${idx}" -eq "${insert_after_index}" ]]; then
            args+=("--features" "mockingbird/metal")
        fi
        idx=$((idx + 1))
    done
else
    args=("$@")
fi

# --- 5. Invoke cargo --------------------------------------------------------
# MOCKINGBIRD_DRY_RUN=1 echoes the constructed command instead of running
# it -- handy for verifying feature-flag placement without launching a
# long-running verb like `tauri dev`.
if [[ "${MOCKINGBIRD_DRY_RUN:-0}" == "1" ]]; then
    printf 'cargo'
    printf ' %q' "${args[@]}"
    printf '\n'
    exit 0
fi

exec cargo "${args[@]}"
