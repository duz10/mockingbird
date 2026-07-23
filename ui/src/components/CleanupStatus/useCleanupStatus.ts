// Shared cleanup-engine status hook (DRY source of truth for the
// isMac-gated signposting on Settings / Dictations / Modes).
//
// Fetches the `cleanup_status` command and re-checks on window focus —
// mirroring the permissions panel — so the moment a user starts Ollama
// (or pulls a model) and tabs back, every surface updates without a
// restart. The derived `state` collapses the raw payload into the three
// display states each surface renders.

import { useCallback, useEffect, useState } from "react";

import { api } from "../../lib/tauri";
import type { CleanupStatus } from "../../lib/types";

/**
 * The three human-facing cleanup states (plus `unknown` before the first
 * fetch resolves). Derived from the raw payload so every surface tells
 * the same story:
 *   - `active`     — Ollama up + a usable model → AI-cleaned text.
 *   - `ollamaDown` — Ollama not running → raw passthrough.
 *   - `noModel`    — Ollama up but no suitable cleanup model → passthrough.
 */
export type CleanupDisplayState = "active" | "ollamaDown" | "noModel" | "unknown";

/** Pure: collapse a raw status payload into a display state. */
export function cleanupDisplayState(
  status: CleanupStatus | null,
): CleanupDisplayState {
  if (!status) return "unknown";
  if (status.cleanupActive) return "active";
  if (!status.ollamaReachable) return "ollamaDown";
  // Reachable, but no installed model resolved → passthrough.
  return "noModel";
}

export interface UseCleanupStatus {
  /** Raw payload, or `null` before the first fetch / on error. */
  status: CleanupStatus | null;
  /** Derived display state (see {@link cleanupDisplayState}). */
  state: CleanupDisplayState;
  /** `true` until the first fetch resolves (or errors). */
  loading: boolean;
  /** Last fetch error message, if any. */
  error: string | null;
  /** Manually re-fetch (also fires on mount + window focus). */
  refresh: () => Promise<void>;
}

/**
 * Fetch cleanup status on mount + every window-focus. The payload is a
 * cache of the Rust-side truth; there are no optimistic writes (you
 * can't enable Ollama from here, only be told how).
 */
export function useCleanupStatus(): UseCleanupStatus {
  const [status, setStatus] = useState<CleanupStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const next = await api.cleanup_status();
      setStatus(next);
      setError(null);
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
    const onFocus = () => void refresh();
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [refresh]);

  return { status, state: cleanupDisplayState(status), loading, error, refresh };
}
