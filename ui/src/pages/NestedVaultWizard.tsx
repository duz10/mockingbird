// Nested-vault detection wizard (ADR 0046 Iter 4 / mb-3xww).
//
// Consumes the pre-positioned backend (`vault_check_path` +
// `vault_ensure_dir` IPCs). When the user picks a vault path inside
// an existing Obsidian vault, this dialog offers three options:
//
//   1. Use a sibling location instead (recommended)
//      Auto-suggests `<grandparent>/mockingbird-vault`, creates it,
//      persists that as VaultPath.
//   2. Pick a different location
//      Re-opens the native folder picker.
//   3. Use anyway
//      Persists the original (nested) path with a tracing-warning
//      trail. Out-of-scope for this dispatch: a separate "user
//      accepted nested-vault risk" persisted flag.
//
// Auto-migration of existing vault content is explicitly out of
// scope. The recommended branch creates an EMPTY sibling vault.
// Users with substantial existing content can manually copy after
// the fact -- this matches the kickoff's scope note.
//
// The wizard component itself is presentational; the parent owns
// `vault_settings_set`. That keeps this file free of the optimistic-
// update + rollback dance that's already done well in
// `SettingsMobileSyncTab.tsx::persist`.

import { useCallback, useState } from "react";

import { Button } from "../components/primitives";
import { t } from "../i18n";
import { api } from "../lib/tauri";

/** Mirror of the Rust enum's `nestedVault` variant — what the parent
 *  hands us after `vault_check_path` flagged the candidate. */
export interface NestedVaultInfo {
  /** The path the user originally picked (inside the parent vault). */
  candidatePath: string;
  /** The detected parent vault's root. */
  parentVault: string;
  /** Auto-suggested sibling location, if `suggest_sibling_vault`
   *  could produce one. Some edge-case paths (e.g. a vault at the
   *  filesystem root) won't have a suggestion. */
  suggestedSibling: string | null;
}

export interface NestedVaultWizardProps {
  info: NestedVaultInfo;
  /** Called with the path the user wants to persist + whether it was
   *  the "use anyway" branch (so the parent can log a tracing warning
   *  or surface a toast). The parent is responsible for actually
   *  calling `vault_settings_set`. */
  onAccept: (path: string, options: { acceptedNested: boolean }) => void;
  /** Called when the user picks "Pick a different location". The
   *  parent re-runs its folder-picker flow. */
  onPickDifferent: () => void;
  /** Called when the user dismisses the dialog without picking a
   *  branch (Escape / outside click). Equivalent to "cancel" — the
   *  parent should NOT persist anything. */
  onCancel: () => void;
}

/** Internal busy state while we run `vault_ensure_dir` in the
 *  recommended branch. */
type Busy = "idle" | "creatingSibling" | "error";

/**
 * Decide whether the recommended (sibling) branch can fire.
 *
 * Pulled out as a pure helper so the unit tests can sanity-check
 * edge cases (no suggestion, suggestion equal to candidate, ...)
 * without firing IPCs.
 */
export function siblingBranchAvailable(info: NestedVaultInfo): boolean {
  if (!info.suggestedSibling) return false;
  if (info.suggestedSibling === info.candidatePath) return false;
  return true;
}

export function NestedVaultWizard({
  info,
  onAccept,
  onPickDifferent,
  onCancel,
}: NestedVaultWizardProps) {
  const [busy, setBusy] = useState<Busy>("idle");
  const [error, setError] = useState<string | null>(null);

  const useSibling = useCallback(async () => {
    if (!info.suggestedSibling) return;
    setBusy("creatingSibling");
    setError(null);
    try {
      await api.vault_ensure_dir(info.suggestedSibling);
      onAccept(info.suggestedSibling, { acceptedNested: false });
    } catch (e) {
      setBusy("error");
      setError(String(e));
    }
  }, [info.suggestedSibling, onAccept]);

  const useAnyway = useCallback(() => {
    onAccept(info.candidatePath, { acceptedNested: true });
  }, [info.candidatePath, onAccept]);

  const siblingAvailable = siblingBranchAvailable(info);

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby="nested-vault-title"
      aria-describedby="nested-vault-body"
      data-testid="nested-vault-wizard"
      // Inline-styled because we don't have a Modal primitive yet
      // and pulling one in just for this wizard would be premature
      // abstraction (YAGNI). If a second non-trivial dialog lands,
      // promote this layout into `components/primitives.tsx`.
      style={{
        position: "fixed",
        inset: 0,
        zIndex: 1000,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        background: "color-mix(in oklab, black 40%, transparent)",
        backdropFilter: "blur(2px)",
      }}
      onClick={(e) => {
        // Click on the dim-out backdrop = cancel. Clicks inside the
        // panel below stopPropagation, so this only fires for the
        // backdrop itself.
        if (e.target === e.currentTarget) onCancel();
      }}
      onKeyDown={(e) => {
        if (e.key === "Escape") onCancel();
      }}
    >
      <div
        style={{
          maxWidth: 520,
          width: "90%",
          padding: 24,
          background: "var(--surface-2)",
          color: "var(--on-surface)",
          border: "1px solid var(--border-subtle)",
          borderRadius: 12,
          boxShadow: "0 12px 32px color-mix(in oklab, black 28%, transparent)",
          display: "flex",
          flexDirection: "column",
          gap: 16,
        }}
        onClick={(e) => e.stopPropagation()}
      >
        <h2
          id="nested-vault-title"
          style={{ margin: 0, font: "var(--type-lg)" }}
        >
          {t("nestedVault.title")}
        </h2>

        <div
          id="nested-vault-body"
          style={{
            font: "var(--type-sm)",
            color: "var(--on-surf-muted)",
            lineHeight: 1.45,
            display: "flex",
            flexDirection: "column",
            gap: 8,
          }}
        >
          <p style={{ margin: 0 }}>{t("nestedVault.body.intro")}</p>
          <code
            style={{
              padding: "6px 8px",
              background: "var(--surface-1)",
              borderRadius: 4,
              font: "var(--type-xs)",
              wordBreak: "break-all",
            }}
            data-testid="nested-vault-parent"
          >
            {info.parentVault}
          </code>
          <p style={{ margin: 0 }}>{t("nestedVault.body.risk")}</p>
        </div>

        {/* Primary action — sibling auto-create. Disabled when
            the backend couldn't suggest a sensible sibling. */}
        {siblingAvailable && info.suggestedSibling && (
          <div
            style={{
              display: "flex",
              flexDirection: "column",
              gap: 6,
              padding: 12,
              border: "1px solid var(--border-subtle)",
              borderRadius: 8,
              background: "var(--surface-1)",
            }}
          >
            <div style={{ font: "var(--type-sm)", fontWeight: 600 }}>
              {t("nestedVault.sibling.title")}
            </div>
            <code
              data-testid="nested-vault-suggested"
              style={{
                padding: "4px 6px",
                background: "var(--surface-2)",
                borderRadius: 4,
                font: "var(--type-xs)",
                wordBreak: "break-all",
              }}
            >
              {info.suggestedSibling}
            </code>
            <div style={{ display: "flex", gap: 8, marginTop: 4 }}>
              <Button
                variant="primary"
                onClick={() => void useSibling()}
                disabled={busy === "creatingSibling"}
                data-testid="nested-vault-use-sibling"
              >
                {busy === "creatingSibling"
                  ? t("common.loading")
                  : t("nestedVault.sibling.action")}
              </Button>
              <Button
                variant="ghost"
                onClick={onPickDifferent}
                disabled={busy === "creatingSibling"}
                data-testid="nested-vault-pick-different"
              >
                {t("nestedVault.pickDifferent")}
              </Button>
            </div>
          </div>
        )}

        {!siblingAvailable && (
          // Edge case: the backend couldn't produce a sibling
          // suggestion (e.g. parent vault sits at filesystem root).
          // Fall back to just "pick a different location" + "use
          // anyway" with no primary recommended branch.
          <Button
            variant="primary"
            onClick={onPickDifferent}
            data-testid="nested-vault-pick-different"
          >
            {t("nestedVault.pickDifferent")}
          </Button>
        )}

        {error && (
          <div
            role="alert"
            style={{ color: "var(--status-error)", font: "var(--type-sm)" }}
          >
            {error}
          </div>
        )}

        <div
          style={{
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center",
            marginTop: 4,
          }}
        >
          {/* Less prominent — link-styled "Use anyway" so it's
              clearly the last-resort path, not the recommendation. */}
          <button
            type="button"
            onClick={useAnyway}
            data-testid="nested-vault-use-anyway"
            style={{
              background: "transparent",
              border: 0,
              padding: 0,
              color: "var(--on-surf-muted)",
              font: "var(--type-xs)",
              textDecoration: "underline",
              cursor: "pointer",
            }}
          >
            {t("nestedVault.useAnyway")}
          </button>
          <Button variant="ghost" onClick={onCancel}>
            {t("common.cancel")}
          </Button>
        </div>
      </div>
    </div>
  );
}
