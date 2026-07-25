// macOS permissions onboarding tab (mb-mac-v1.4.6 / ADR 0061).
//
// Surfaces the four macOS privacy grants Mockingbird needs, their live
// status, and a one-click deep-link to the right System Settings pane.
//
// This tab is only mounted on macOS — `Settings.tsx` gates it on
// `host_os() === "macos"`. The status payload reflects REAL TCC state
// (the backend does silent preflight reads), so we re-poll whenever the
// window regains focus: the user grants a permission in System Settings,
// tabs back to Mockingbird, and the badges update without a manual
// refresh.
//
// State source-of-truth is the Rust side; the snapshot in component
// state is a cache refreshed on mount + focus. No optimistic writes —
// you can't grant a permission from here, only jump the user to where
// they can.

import { useCallback, useEffect, useState } from "react";

import { Button, Card, Pill, Spinner } from "../components/primitives";
import { t } from "../i18n";
import { api } from "../lib/tauri";
import type {
  PermissionKey,
  PermissionState,
  PermissionStatuses,
} from "../lib/types";

import styles from "./Settings.module.css";

// The four permissions in onboarding order: the three dictation grants
// first (Mic → Input Monitoring → Accessibility), then Screen Recording
// (Phase 4 meetings, surfaced now for completeness).
const PERMISSIONS: readonly PermissionKey[] = [
  "microphone",
  "inputMonitoring",
  "accessibility",
  "screenRecording",
] as const;

// Map a grant state to a status-token tone for the badge.
function toneFor(state: PermissionState): string {
  switch (state) {
    case "granted":
      return "status-ok";
    case "denied":
    case "restricted":
      return "status-error";
    case "notDetermined":
    case "unsupported":
    default:
      return "status-idle";
  }
}

export function SettingsPermissionsTab() {
  const [statuses, setStatuses] = useState<PermissionStatuses | null>(null);
  const [error, setError] = useState<string | null>(null);
  // True while the mic TCC prompt is up (the request blocks until the
  // user answers), so we can disable the button and show progress.
  const [requestingMic, setRequestingMic] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const next = await api.mac_permission_statuses();
      setStatuses(next);
      setError(null);
    } catch (err) {
      setError(String(err));
    }
  }, []);

  // Poll on mount, then again every time the window regains focus so the
  // panel reflects a grant the user just made in System Settings.
  useEffect(() => {
    void refresh();
    const onFocus = () => void refresh();
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [refresh]);

  const openPane = useCallback(async (permission: PermissionKey) => {
    try {
      await api.mac_open_settings_pane(permission);
    } catch (err) {
      setError(String(err));
    }
  }, []);

  // Microphone is special: macOS won't list the app in the Microphone
  // pane until it *requests* access, so "Open Settings" alone is a dead
  // end. This pops the real TCC prompt. If the user has already denied
  // (macOS won't re-prompt), fall back to opening the pane — the app is
  // now listed there and can be toggled on. mb-qz3.
  const requestMic = useCallback(async () => {
    setRequestingMic(true);
    try {
      const result = await api.request_microphone_access();
      if (result === "denied" || result === "restricted") {
        await api.mac_open_settings_pane("microphone");
      }
      await refresh();
      setError(null);
    } catch (err) {
      setError(String(err));
    } finally {
      setRequestingMic(false);
    }
  }, [refresh]);

  if (!statuses) {
    return error ? (
      <div className={styles.errorBanner} role="alert">
        {error}
      </div>
    ) : (
      <Spinner />
    );
  }

  return (
    <div className={styles.stack}>
      {error ? (
        <div className={styles.errorBanner} role="alert">
          {error}
        </div>
      ) : null}

      <Card title={t("settings.permissions.title")}>
        <p
          style={{
            color: "var(--on-surf-muted)",
            margin: 0,
            font: "var(--type-sm)",
          }}
        >
          {t("settings.permissions.help")}
        </p>

        {PERMISSIONS.map((perm) => {
          const state = statuses[perm];
          // Mic in the NotDetermined state gets a "Request access" action
          // that pops the TCC prompt; every other case (other perms, or a
          // mic that's already granted/denied) keeps "Open Settings".
          const onRequest =
            perm === "microphone" && state === "notDetermined"
              ? requestMic
              : undefined;
          return (
            <PermissionRow
              key={perm}
              permission={perm}
              state={state}
              busy={perm === "microphone" && requestingMic}
              onOpen={() => void openPane(perm)}
              onRequest={onRequest ? () => void onRequest() : undefined}
            />
          );
        })}

        {/* mb-icp — dev-build TCC hint. In a `cargo tauri dev` build the
            grant attaches to the launching TERMINAL, not "Mockingbird";
            the built .app grants Mockingbird directly. Harmless for
            .app users (it also explains their case). isMac-gated
            already (this whole tab is macOS-only). */}
        <p
          style={{
            color: "var(--on-surf-faint)",
            margin: 0,
            font: "var(--type-xs)",
          }}
        >
          {t("settings.permissions.devHint")}
        </p>
      </Card>
    </div>
  );
}

function PermissionRow({
  permission,
  state,
  busy,
  onOpen,
  onRequest,
}: {
  permission: PermissionKey;
  state: PermissionState;
  busy: boolean;
  onOpen: () => void;
  /** Present only for the microphone row when a TCC request is possible
   *  (NotDetermined). When set, the button pops the prompt instead of
   *  opening System Settings. */
  onRequest?: () => void;
}) {
  const granted = state === "granted";
  const requestable = onRequest !== undefined;
  const label = requestable
    ? t("settings.permissions.requestAccess")
    : t("settings.permissions.openSettings");
  return (
    <div className={styles.row}>
      <div className={styles.rowMain}>
        <span className={styles.rowLabel}>
          {t(`settings.permissions.${permission}.name`)}
        </span>
        <span className={styles.rowHelp}>
          {t(`settings.permissions.${permission}.why`)}
        </span>
      </div>
      <div
        className={styles.rowControl}
        style={{ display: "flex", gap: "var(--s-2)", alignItems: "center" }}
      >
        <Pill tone={toneFor(state)}>
          {t(`settings.permissions.state.${state}`)}
        </Pill>
        <Button
          variant={granted ? "ghost" : "primary"}
          onClick={requestable ? onRequest : onOpen}
          disabled={busy}
          ariaLabel={label}
        >
          {busy ? t("settings.permissions.requesting") : label}
        </Button>
      </div>
    </div>
  );
}
