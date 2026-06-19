#!/usr/bin/env bash
#
# download-models.sh — macOS equivalent of scripts/download-models.ps1.
#
# Downloads the ML models declared in scripts/model-manifest.json.
# GGUF/GGML whisper models are architecture-agnostic, so these are the
# exact same files the Windows script fetches — only the fetch tooling
# (curl + shasum) and the target-dir resolution differ.
#
# Per ADR 0014, the target dir resolves in this order:
#   1. OUTPUT_DIR env (explicit override)
#   2. MODEL_PATH env (dev override)
#   3. <repo>/models/  (Mac dev default; gitignored)
#
# Idempotent: a model whose SHA-256 already matches the manifest is
# skipped. A manifest hash of "tbd-pin-when-downloaded" (case-insensitive)
# is treated as unpinned — the file is downloaded and its observed hash
# printed for pinning, but not failed on.
#
# Usage:
#   scripts/download-models.sh
#   OUTPUT_DIR=/custom/path scripts/download-models.sh
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
MANIFEST="${MANIFEST:-${SCRIPT_DIR}/model-manifest.json}"
TARGET_DIR="${OUTPUT_DIR:-${MODEL_PATH:-${REPO_ROOT}/models}}"

if [[ ! -f "${MANIFEST}" ]]; then
    echo "ERROR: manifest not found: ${MANIFEST}" >&2
    exit 1
fi
if ! command -v jq >/dev/null 2>&1; then
    echo "ERROR: jq is required (brew install jq)" >&2
    exit 1
fi

mkdir -p "${TARGET_DIR}"
echo "Target directory: ${TARGET_DIR}"

sha256_of() {
    # macOS ships `shasum`; fall back to `sha256sum` if present.
    if command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{print tolower($1)}'
    else
        sha256sum "$1" | awk '{print tolower($1)}'
    fi
}

count="$(jq '.models | length' "${MANIFEST}")"
failed=()
total_bytes=0

for i in $(seq 0 $((count - 1))); do
    name="$(jq -r ".models[$i].name" "${MANIFEST}")"
    filename="$(jq -r ".models[$i].filename" "${MANIFEST}")"
    url="$(jq -r ".models[$i].url" "${MANIFEST}")"
    expected="$(jq -r ".models[$i].sha256" "${MANIFEST}" | tr '[:upper:]' '[:lower:]')"
    size_bytes="$(jq -r ".models[$i].size_bytes" "${MANIFEST}")"
    dest="${TARGET_DIR}/${filename}"

    size_mb="$(echo "${size_bytes}" | awk '{printf "%.1f", $1/1048576}')"
    echo ""
    echo "[${name}] ${filename} -- ~${size_mb} MB"

    unpinned=0
    if [[ "${expected}" == "tbd-pin-when-downloaded" ]]; then
        unpinned=1
        echo "  note: SHA-256 not yet pinned in manifest."
    fi

    # Idempotency: skip if present + matching.
    if [[ -f "${dest}" ]]; then
        actual="$(sha256_of "${dest}")"
        if [[ "${unpinned}" -eq 0 && "${actual}" == "${expected}" ]]; then
            echo "  ok: already present with matching SHA-256."
            total_bytes=$((total_bytes + $(stat -f%z "${dest}")))
            continue
        elif [[ "${unpinned}" -eq 1 ]]; then
            echo "  present (unpinned); observed SHA-256: ${actual}"
            total_bytes=$((total_bytes + $(stat -f%z "${dest}")))
            continue
        else
            echo "  hash mismatch on disk; re-downloading."
        fi
    fi

    echo "  downloading from ${url}"
    if ! curl -fSL --retry 3 -o "${dest}" "${url}"; then
        echo "  ERROR: download failed for ${name}" >&2
        failed+=("${name}")
        continue
    fi

    actual="$(sha256_of "${dest}")"
    if [[ "${unpinned}" -eq 1 ]]; then
        echo "  observed SHA-256: ${actual}"
        echo "  (pin this in model-manifest.json to enforce integrity)"
    elif [[ "${actual}" == "${expected}" ]]; then
        echo "  ok: SHA-256 verified."
    else
        echo "  ERROR: SHA-256 mismatch! expected ${expected}, got ${actual}" >&2
        failed+=("${name}")
        continue
    fi
    total_bytes=$((total_bytes + $(stat -f%z "${dest}")))
done

echo ""
total_mb="$(echo "${total_bytes}" | awk '{printf "%.1f", $1/1048576}')"
echo "Total on disk: ${total_mb} MB across ${count} model(s)."

if [[ ${#failed[@]} -gt 0 ]]; then
    echo "FAILED: ${failed[*]}" >&2
    exit 1
fi
echo "All models present."
