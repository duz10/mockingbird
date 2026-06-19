#!/usr/bin/env bash
#
# download-onnxruntime.sh — macOS (Apple Silicon) equivalent of
# scripts/download-onnxruntime.ps1.
#
# Fetches the ONNX Runtime build required by `ort = "=2.0.0-rc.10"`.
#
# WHY 1.22.0: the `ort` 2.0.0-rc.10 crate links libonnxruntime
# 1.22.0. Evidence: scripts/download-onnxruntime.ps1 (the Windows
# sibling) pins `-Version 1.22.0` for rc.10 and its header documents
# "matching the version ort expects (1.22.x for rc.10)". This script
# tracks that pin; bump both together when the `ort` dep bumps.
#
# The project uses ort's `load-dynamic` feature, so the runtime needs
# `libonnxruntime.dylib` discoverable via `ORT_DYLIB_PATH` (wired in
# scripts/dev/cargo-mac.sh). This script drops the dylib (+ versioned
# symlink) into the gitignored models dir the whisper/VAD loaders use.
#
# Usage:
#   scripts/download-onnxruntime.sh
#   OUTPUT_DIR=/custom/path scripts/download-onnxruntime.sh
#   ORT_VERSION=1.22.0 scripts/download-onnxruntime.sh
#
set -euo pipefail

ORT_VERSION="${ORT_VERSION:-1.22.0}"

# --- Resolve the target dir (mirror the PS1 + models_dir() order) -----------
# Mac dev convention: <repo>/models/ (gitignored — see .gitignore line
# "models/"). Honor MODEL_PATH / OUTPUT_DIR overrides like the PS1 does.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
OUTPUT_DIR="${OUTPUT_DIR:-${MODEL_PATH:-${REPO_ROOT}/models}}"

mkdir -p "${OUTPUT_DIR}"

DYLIB_PATH="${OUTPUT_DIR}/libonnxruntime.dylib"
VERSIONED_DYLIB="${OUTPUT_DIR}/libonnxruntime.${ORT_VERSION}.dylib"

# --- Idempotency: skip if the versioned dylib is already present ------------
if [[ -f "${VERSIONED_DYLIB}" && -e "${DYLIB_PATH}" ]]; then
    size="$(stat -f%z "${VERSIONED_DYLIB}" 2>/dev/null || echo "?")"
    echo " ONNX Runtime ${ORT_VERSION} already present:"
    echo "    ${VERSIONED_DYLIB} (${size} bytes)"
    echo "    ${DYLIB_PATH} -> $(readlink "${DYLIB_PATH}" 2>/dev/null || echo "${DYLIB_PATH}")"
    echo ""
    echo "Set: export ORT_DYLIB_PATH='${DYLIB_PATH}'"
    exit 0
fi

TARBALL_NAME="onnxruntime-osx-arm64-${ORT_VERSION}.tgz"
URL="https://github.com/microsoft/onnxruntime/releases/download/v${ORT_VERSION}/${TARBALL_NAME}"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "${TMP_DIR}"' EXIT

echo "Downloading ONNX Runtime ${ORT_VERSION} (osx-arm64) ..."
echo "  ${URL}"
curl -fsSL --retry 3 -o "${TMP_DIR}/${TARBALL_NAME}" "${URL}"
echo "  downloaded $(stat -f%z "${TMP_DIR}/${TARBALL_NAME}") bytes"

echo "Extracting ..."
tar -xzf "${TMP_DIR}/${TARBALL_NAME}" -C "${TMP_DIR}"

# Locate the real (versioned) dylib inside the extracted lib/ dir.
# NOTE: the archive also ships a `.dSYM` debug bundle containing a
# same-named Mach-O *companion* file — that is NOT a loadable dylib and
# dlopen rejects it. Exclude any `.dSYM` path and prefer the one under
# `lib/`.
SRC_VERSIONED="$(find "${TMP_DIR}" -type f -path "*/lib/libonnxruntime.${ORT_VERSION}.dylib" -not -path "*.dSYM*" | head -1)"
if [[ -z "${SRC_VERSIONED}" ]]; then
    # Fallback: any libonnxruntime*.dylib that's a real file, never a dSYM.
    SRC_VERSIONED="$(find "${TMP_DIR}" -type f -name "libonnxruntime*.dylib" -not -path "*.dSYM*" | head -1)"
fi
if [[ -z "${SRC_VERSIONED}" ]]; then
    echo "ERROR: no libonnxruntime dylib found in the ${ORT_VERSION} archive" >&2
    exit 1
fi

cp "${SRC_VERSIONED}" "${VERSIONED_DYLIB}"
# Recreate the unversioned symlink the loader resolves against.
ln -sf "$(basename "${VERSIONED_DYLIB}")" "${DYLIB_PATH}"

size="$(stat -f%z "${VERSIONED_DYLIB}")"
echo ""
echo " ONNX Runtime ${ORT_VERSION} ready:"
echo "    ${VERSIONED_DYLIB} (${size} bytes)"
echo "    ${DYLIB_PATH} -> $(readlink "${DYLIB_PATH}")"
echo ""
echo "cargo-mac.sh exports this automatically. To set it manually:"
echo "  export ORT_DYLIB_PATH='${DYLIB_PATH}'"
