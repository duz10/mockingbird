// App boot — hydrate cross-route state from the Rust IPC surface.
//
// `mb-v7pd` (v0.2.0-beta.1 smoke fix Bugs 1 + 2) — the original boot
// effect awaited `Promise.all([list_modes, get_settings,
// get_active_mode, kg_settings_get_all])` and then ran ALL setters
// after the await. If ANY single IPC rejected (e.g. `get_active_mode`
// returns no row on a clean install, or `kg_settings_get_all` races a
// late-registered command on cold start), Promise.all short-circuited
// to the catch block and NONE of the setters fired. The visible
// symptom was:
//   * Bug 1: sidebar "Active mode" label absent on launch until the
//     user visited /modes (whose per-page effect re-hydrates
//     activeModeSlug + modes into the same store).
//   * Bug 2: sidebar Knowledge Graph nav item absent on launch even
//     with KgGraphEnabled=true, until the user visited
//     Settings -> Knowledge Graph (whose per-page effect re-hydrates
//     kgGraphEnabled into the same store).
//
// Pattern fix: use `Promise.allSettled` and dispatch each setter
// independently on its corresponding result's `fulfilled` status.
// A late-registered or no-row IPC can no longer suppress the others.
//
// Extracted into a pure helper so the independence property is
// directly testable without mounting a React tree (the codebase
// doesn't ship @testing-library/react; tests target data/logic).

import { fetchAppVersion } from "./appVersion";
import type {
  ActiveMode,
  KgSettings,
  ModeRow,
  SettingsSnapshot,
  ThemeChoice,
} from "./types";

/** Minimal IPC surface bootApp depends on. Narrowed from `api` so
 *  tests can pass a hand-rolled mock without satisfying the full
 *  Tauri shim. */
export interface BootApi {
  list_modes: () => Promise<ModeRow[]>;
  get_settings: () => Promise<SettingsSnapshot>;
  get_active_mode: () => Promise<ActiveMode>;
  kg_settings_get_all: () => Promise<KgSettings>;
}

/** Setters bootApp invokes. Each is fired independently on its
 *  corresponding IPC's `fulfilled` status. */
export interface BootSetters {
  setModes: (m: ModeRow[]) => void;
  setSettings: (s: SettingsSnapshot | null) => void;
  setActiveModeSlug: (slug: string | null) => void;
  setKgGraphEnabled: (on: boolean | null) => void;
  setAppVersion: (v: string | null) => void;
  applyTheme: (theme: ThemeChoice) => void;
}

/** Result of a single boot run. Useful for tests + debug. */
export interface BootResult {
  modes: "fulfilled" | "rejected";
  settings: "fulfilled" | "rejected";
  activeMode: "fulfilled" | "rejected";
  kgSettings: "fulfilled" | "rejected";
  appVersion: "fulfilled" | "rejected";
}

interface BootOptions {
  /** Override the version fetcher (used by tests + non-Tauri shells). */
  fetchVersion?: () => Promise<string>;
  /** Logger for individual IPC failures. Defaults to `console.warn`.
   *  Tests pass `() => {}` to keep output clean. */
  log?: (msg: string, err: unknown) => void;
}

/**
 * Hydrate the cross-route app store from IPC. Each setter fires
 * independently — a single rejected IPC does NOT suppress the
 * others (this is the Bug 1 / Bug 2 pattern fix).
 *
 * Returns a per-call status map for tests / debugging.
 */
export async function bootApp(
  api: BootApi,
  setters: BootSetters,
  options: BootOptions = {},
): Promise<BootResult> {
  const log = options.log ?? ((msg, err) => {
    // eslint-disable-next-line no-console
    console.warn(msg, err);
  });
  const fetchVersion = options.fetchVersion ?? fetchAppVersion;

  const [modesRes, settingsRes, activeModeRes, kgSettingsRes, versionRes] =
    await Promise.allSettled([
      api.list_modes(),
      api.get_settings(),
      api.get_active_mode(),
      api.kg_settings_get_all(),
      fetchVersion(),
    ]);

  if (modesRes.status === "fulfilled") {
    setters.setModes(modesRes.value);
  } else {
    log("App boot: list_modes failed", modesRes.reason);
  }

  if (settingsRes.status === "fulfilled") {
    setters.setSettings(settingsRes.value);
    setters.applyTheme(settingsRes.value.theme);
  } else {
    log("App boot: get_settings failed", settingsRes.reason);
  }

  if (activeModeRes.status === "fulfilled") {
    setters.setActiveModeSlug(activeModeRes.value.slug);
  } else {
    log("App boot: get_active_mode failed", activeModeRes.reason);
  }

  if (kgSettingsRes.status === "fulfilled") {
    setters.setKgGraphEnabled(kgSettingsRes.value.kgGraphEnabled);
  } else {
    log("App boot: kg_settings_get_all failed", kgSettingsRes.reason);
  }

  if (versionRes.status === "fulfilled") {
    setters.setAppVersion(versionRes.value);
  } else {
    log("App boot: fetchAppVersion failed", versionRes.reason);
  }

  return {
    modes: modesRes.status,
    settings: settingsRes.status,
    activeMode: activeModeRes.status,
    kgSettings: kgSettingsRes.status,
    appVersion: versionRes.status,
  };
}
