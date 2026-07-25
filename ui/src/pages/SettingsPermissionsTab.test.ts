// mb-mac-v1.4.6 — IPC-contract tests for the macOS permissions tab.
//
// We don't ship @testing-library/react, so the live panel rendering +
// deep-link opening fold into the human e2e (`mac-p3f-permissions-
// onboarding-renders`, a MANUAL judge). What we CAN test here is the
// shape of the IPC the component leans on: the four-field status
// payload, the host_os gate string, and that the deep-link command
// accepts the permission keys without throwing. If those drift, the
// component breaks.
//
// Tests run in fixture mode (no Tauri shell) — they exercise the
// fixture path in `ui/src/lib/tauri.ts`.

import { describe, it, expect } from "vitest";

import { api } from "../lib/tauri";
import type { PermissionKey, PermissionState } from "../lib/types";

const PERMISSION_KEYS: PermissionKey[] = [
  "microphone",
  "inputMonitoring",
  "accessibility",
  "screenRecording",
];

const VALID_STATES: PermissionState[] = [
  "granted",
  "denied",
  "notDetermined",
  "restricted",
  "unsupported",
];

describe("api.host_os (fixture mode)", () => {
  it("returns a non-empty platform string", async () => {
    const os = await api.host_os();
    expect(typeof os).toBe("string");
    expect(os.length).toBeGreaterThan(0);
  });
});

describe("api.mac_permission_statuses (fixture mode)", () => {
  it("returns all four permission fields, each a valid state", async () => {
    const s = await api.mac_permission_statuses();
    for (const key of PERMISSION_KEYS) {
      expect(s[key]).not.toBeUndefined();
      expect(VALID_STATES).toContain(s[key]);
    }
  });
});

describe("api.mac_open_settings_pane (fixture mode)", () => {
  it("accepts every permission key without throwing", async () => {
    // Void command — the fixture path (these tests never touch a real
    // Tauri shell) resolves to `null`; the point is that no key rejects.
    for (const key of PERMISSION_KEYS) {
      await expect(api.mac_open_settings_pane(key)).resolves.toBeNull();
    }
  });
});

describe("api.request_microphone_access (fixture mode)", () => {
  it("resolves to a valid permission state", async () => {
    // mb-qz3 — this is the command that pops the macOS TCC prompt (mic
    // can't be added to System Settings manually). The panel calls it
    // from the Microphone "Request access" button and branches on the
    // returned state (granted -> done; denied/restricted -> open pane).
    const state = await api.request_microphone_access();
    expect(VALID_STATES).toContain(state);
  });
});
