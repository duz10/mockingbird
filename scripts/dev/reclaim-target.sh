#!/usr/bin/env bash
#
# reclaim-target.sh — practical anti-bloat guardrail for target/
# -----------------------------------------------------------------------------
# WHY THIS EXISTS
#   cargo never garbage-collects stale fingerprints. Every feature-flag
#   permutation compiled during a long-running epic (e.g. the macos-port:
#   metal / no-metal / CUDA on-off) leaves its own whisper-rs-sys-<hash>
#   build dir behind FOREVER. On this box that quietly grew target/ to 34G,
#   with ~30G of it dead debug cache. This script prunes ONLY the stale
#   crud while keeping the current build warm — so we never pay the
#   expensive whisper CUDA/Metal recompile just to reclaim disk.
#
# CADENCE (read this before you run it)
#   * Weekly, or whenever disk is tight:   ./scripts/dev/reclaim-target.sh
#       -> stale-prune only (cargo-sweep --time 15). Cheap. Keeps recent
#          artifacts, so your next build is still incremental.
#   * ONLY at phase boundaries (phase-{N}-complete):
#       ./scripts/dev/reclaim-target.sh --full
#       -> cargo clean. Nukes ALL of target/. Next build pays the full
#          ~10-min whisper ggml recompile. Do NOT do this mid-work.
#
# SAFETY
#   * target/ is gitignored and 100% regenerable — nothing here is precious.
#   * Refuses to run if a cargo/rustc build is live (would corrupt it).
#   * Never touches models/, .git/, or any source.
#   * Idempotent: running twice in a row on a clean tree is a no-op.
# -----------------------------------------------------------------------------
set -euo pipefail

# --- locate repo root (script lives in scripts/dev/) -------------------------
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
cd "$REPO_ROOT"

STALE_DAYS=15          # cargo-sweep prunes artifacts unused > this many days
FULL=0

# --- arg parsing -------------------------------------------------------------
for arg in "$@"; do
  case "$arg" in
    --full) FULL=1 ;;
    -h|--help)
      sed -n '2,40p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      echo "reclaim-target.sh: unknown argument '$arg' (try --help)" >&2
      exit 2
      ;;
  esac
done

# --- guard: never run over a live build -------------------------------------
# Match cargo/rustc build processes, but exclude THIS script and the pgrep
# itself so we don't false-positive on our own name.
if pgrep -f 'cargo|rustc' 2>/dev/null \
     | grep -vxq "$$" \
   && pgrep -fl 'cargo|rustc' 2>/dev/null \
     | grep -viE 'reclaim-target|pgrep' >/dev/null; then
  echo "reclaim-target.sh: a cargo/rustc build appears to be running." >&2
  echo "  Refusing to prune target/ — could corrupt the live build." >&2
  echo "  Running processes:" >&2
  pgrep -fl 'cargo|rustc' 2>/dev/null | grep -viE 'reclaim-target|pgrep' >&2 || true
  exit 3
fi

if [[ ! -d target ]]; then
  echo "reclaim-target.sh: no target/ directory — nothing to reclaim."
  exit 0
fi

# --- helpers -----------------------------------------------------------------
size_bytes() { du -sk "$1" 2>/dev/null | awk '{print $1 * 1024}'; }
human()      { du -sh "$1" 2>/dev/null | awk '{print $1}'; }

BEFORE_H="$(human target)"
BEFORE_B="$(size_bytes target)"
echo "target/ before: ${BEFORE_H}"

# --- do the work -------------------------------------------------------------
if [[ "$FULL" -eq 1 ]]; then
  echo "mode: --full  (cargo clean — phase-boundary only)"
  cargo clean
elif command -v cargo-sweep >/dev/null 2>&1 || cargo sweep --version >/dev/null 2>&1; then
  echo "mode: stale-prune  (cargo sweep --time ${STALE_DAYS})"
  # --time N removes artifacts not accessed in the last N days, leaving
  # current builds warm.
  cargo sweep --time "$STALE_DAYS"
else
  echo "mode: fallback  (cargo-sweep not installed)"
  echo "  Install the surgical pruner with:"
  echo "      cargo install cargo-sweep"
  echo "  Falling back to removing stale debug cache (target/debug/)."
  echo "  Your --release artifacts stay untouched."
  rm -rf target/debug
fi

# --- report ------------------------------------------------------------------
AFTER_H="$(human target)"
AFTER_B="$(size_bytes target)"
RECLAIMED_B=$(( BEFORE_B - AFTER_B ))
if [[ "$RECLAIMED_B" -lt 0 ]]; then RECLAIMED_B=0; fi
RECLAIMED_H="$(awk -v b="$RECLAIMED_B" 'BEGIN{
  split("B KB MB GB TB", u, " ");
  i=1; while (b>=1024 && i<5){b/=1024; i++}
  printf "%.1f%s", b, u[i]
}')"

echo "target/ after:  ${AFTER_H}"
echo "reclaimed:      ${RECLAIMED_H}  (${RECLAIMED_B} bytes)"
