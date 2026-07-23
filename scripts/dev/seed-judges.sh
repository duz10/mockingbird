#!/usr/bin/env bash
#
# seed-judges.sh — macOS/POSIX equivalent of seed-judges.ps1.
#
# Merges a judges template (default .code_puppy/judges-template.json) into
# ~/.code_puppy/judges.json. Idempotent: judges are keyed by `id`; an
# existing entry is preserved unless --force is passed, in which case the
# template entry wins. Reports Added / Updated / Skipped, mirroring the
# PowerShell version's semantics exactly.
#
# Usage (from repo root):
#   scripts/dev/seed-judges.sh
#   scripts/dev/seed-judges.sh --force
#   scripts/dev/seed-judges.sh --template .code_puppy/judges-template-macos.json
#   scripts/dev/seed-judges.sh --template <path> --target <path> --force
#
# Requires: jq. (We verify and STOP if missing — we do NOT install it.)

set -euo pipefail

# --- locate repo-relative defaults --------------------------------------
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" >/dev/null 2>&1 && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/../.." >/dev/null 2>&1 && pwd)"

TEMPLATE_PATH="${REPO_ROOT}/.code_puppy/judges-template.json"
TARGET_PATH="${HOME}/.code_puppy/judges.json"
FORCE=false

# --- arg parsing ---------------------------------------------------------
while [ "$#" -gt 0 ]; do
  case "$1" in
    --force) FORCE=true; shift ;;
    --template) TEMPLATE_PATH="$2"; shift 2 ;;
    --target)   TARGET_PATH="$2";   shift 2 ;;
    -h|--help)
      sed -n '2,21p' "$0"
      exit 0
      ;;
    *)
      echo "seed-judges.sh: unknown argument: $1" >&2
      exit 2
      ;;
  esac
done

# --- preconditions -------------------------------------------------------
if ! command -v jq >/dev/null 2>&1; then
  echo "seed-judges.sh: STOP — jq is required but not found on PATH." >&2
  echo "  Install jq, then re-run. (This script will not install it for you.)" >&2
  exit 1
fi

if [ ! -f "$TEMPLATE_PATH" ]; then
  echo "seed-judges.sh: STOP — template not found: $TEMPLATE_PATH" >&2
  exit 1
fi

# --- normalise inputs (strip a possible UTF-8 BOM; jq is BOM-intolerant) -
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

TEMPLATE_CLEAN="${TMP_DIR}/template.json"
TARGET_CLEAN="${TMP_DIR}/target.json"

# sed 1s strips a leading BOM if present; harmless otherwise.
sed '1s/^\xEF\xBB\xBF//' "$TEMPLATE_PATH" > "$TEMPLATE_CLEAN"

if [ -f "$TARGET_PATH" ]; then
  sed '1s/^\xEF\xBB\xBF//' "$TARGET_PATH" > "$TARGET_CLEAN"
else
  mkdir -p "$(dirname -- "$TARGET_PATH")"
  echo '{ "version": 1, "judges": [] }' > "$TARGET_CLEAN"
fi

# Validate both parse as JSON before mutating anything.
if ! jq empty "$TEMPLATE_CLEAN" 2>/dev/null; then
  echo "seed-judges.sh: STOP — template is not valid JSON: $TEMPLATE_PATH" >&2
  exit 1
fi
if ! jq empty "$TARGET_CLEAN" 2>/dev/null; then
  echo "seed-judges.sh: STOP — existing target is not valid JSON: $TARGET_PATH" >&2
  exit 1
fi

# --- merge (pure jq; preserves target's top-level keys) ------------------
RESULT="$(jq -n \
  --argjson force "$FORCE" \
  --slurpfile target   "$TARGET_CLEAN" \
  --slurpfile template "$TEMPLATE_CLEAN" \
  '
    ($target[0])                       as $tgt
  | ($template[0].judges // [])        as $incoming
  | ($tgt.judges // [])                as $existing
  | ([ $existing[].id ])               as $existing_ids
  | reduce $incoming[] as $j (
      { judges: $existing, added: 0, updated: 0, skipped: 0 };
      if ($existing_ids | index($j.id)) != null then
        if $force then
          .judges  = ((.judges | map(select(.id != $j.id))) + [$j])
        | .updated += 1
        else
          .skipped += 1
        end
      else
        .judges += [$j] | .added += 1
      end
    )
  | { merged:  ($tgt + { judges: .judges }),
      added:   .added,
      updated: .updated,
      skipped: .skipped }
  ')"

ADDED="$(printf '%s' "$RESULT"   | jq -r '.added')"
UPDATED="$(printf '%s' "$RESULT" | jq -r '.updated')"
SKIPPED="$(printf '%s' "$RESULT" | jq -r '.skipped')"

# Write merged result back atomically.
printf '%s' "$RESULT" | jq '.merged' > "${TMP_DIR}/merged.json"
mv "${TMP_DIR}/merged.json" "$TARGET_PATH"

# --- report --------------------------------------------------------------
echo "Merged judges template into ${TARGET_PATH}"
echo "  Template: ${TEMPLATE_PATH}"
echo "  Added:    ${ADDED}"
echo "  Updated:  ${UPDATED}"
echo "  Skipped (use --force to overwrite): ${SKIPPED}"
