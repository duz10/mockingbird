// Tests for the `bootApp` boot-fetch orchestrator.
//
// These guard the `mb-v7pd` smoke fix (v0.2.0-beta.1 Bugs 1 + 2):
// the pre-fix code used `Promise.all` and a single rejected IPC
// suppressed ALL setters, leaving the sidebar showing no active
// mode label (Bug 1) and no Knowledge Graph nav item (Bug 2) until
// the user navigated to a route that re-hydrated those stores
// individually.
//
// The contract these tests lock in: each setter fires independently
// based on the per-call `Promise.allSettled` result, NOT a single
// all-or-nothing await.

import { afterEach, describe, expect, it, vi } from "vitest";

import { bootApp, type BootApi, type BootSetters } from "./bootApp";
import type {
  ActiveMode,
  KgSettings,
  ModeRow,
  SettingsSnapshot,
} from "./types";

/* ------------------------------------------------------------------ */
/* Fixture builders.                                                  */
/* ------------------------------------------------------------------ */

function makeApi(overrides: Partial<BootApi> = {}): BootApi {
  const modes: ModeRow[] = [
    {
      slug: "normal",
      label: "Normal",
      description: "",
      system_prompt: "",
      example_set_id: null,
      seeded: true,
      builtin: true,
    } as unknown as ModeRow,
  ];
  const settings: SettingsSnapshot = {
    theme: "system",
  } as unknown as SettingsSnapshot;
  const activeMode: ActiveMode = { slug: "normal" } as unknown as ActiveMode;
  const kgSettings: KgSettings = {
    kgGraphEnabled: true,
  } as unknown as KgSettings;
  return {
    list_modes: () => Promise.resolve(modes),
    get_settings: () => Promise.resolve(settings),
    get_active_mode: () => Promise.resolve(activeMode),
    kg_settings_get_all: () => Promise.resolve(kgSettings),
    ...overrides,
  };
}

function makeSetters(): BootSetters & {
  // Re-typed call arrays so individual assertions stay terse.
  setModes: ReturnType<typeof vi.fn>;
  setSettings: ReturnType<typeof vi.fn>;
  setActiveModeSlug: ReturnType<typeof vi.fn>;
  setKgGraphEnabled: ReturnType<typeof vi.fn>;
  setAppVersion: ReturnType<typeof vi.fn>;
  applyTheme: ReturnType<typeof vi.fn>;
} {
  return {
    setModes: vi.fn(),
    setSettings: vi.fn(),
    setActiveModeSlug: vi.fn(),
    setKgGraphEnabled: vi.fn(),
    setAppVersion: vi.fn(),
    applyTheme: vi.fn(),
  };
}

const SILENT_LOG = () => {};

afterEach(() => {
  vi.restoreAllMocks();
});

/* ------------------------------------------------------------------ */
/* Happy path.                                                        */
/* ------------------------------------------------------------------ */

describe("bootApp — happy path", () => {
  it("fires every setter when every IPC fulfills", async () => {
    const api = makeApi();
    const setters = makeSetters();

    const result = await bootApp(api, setters, {
      fetchVersion: () => Promise.resolve("0.2.0-beta.1"),
      log: SILENT_LOG,
    });

    expect(result).toEqual({
      modes: "fulfilled",
      settings: "fulfilled",
      activeMode: "fulfilled",
      kgSettings: "fulfilled",
      appVersion: "fulfilled",
    });
    expect(setters.setModes).toHaveBeenCalledTimes(1);
    expect(setters.setSettings).toHaveBeenCalledTimes(1);
    expect(setters.setActiveModeSlug).toHaveBeenCalledWith("normal");
    expect(setters.setKgGraphEnabled).toHaveBeenCalledWith(true);
    expect(setters.setAppVersion).toHaveBeenCalledWith("0.2.0-beta.1");
    expect(setters.applyTheme).toHaveBeenCalledWith("system");
  });
});

/* ------------------------------------------------------------------ */
/* Bug 1 + Bug 2 pattern-fix regressions.                             */
/*                                                                    */
/* These are the tests that would have caught the v0.2.0-beta.1       */
/* smoke incident. Each one rejects a SINGLE IPC and asserts that     */
/* the OTHER setters fire normally. With the old Promise.all          */
/* code, every one of these would fail: the catch block swallowed    */
/* the rejection and no setter ran.                                   */
/* ------------------------------------------------------------------ */

describe("bootApp — independence (regression for mb-v7pd Bugs 1+2)", () => {
  it("still fires modes/settings/activeMode/version when kg_settings_get_all rejects (Bug 2)", async () => {
    const api = makeApi({
      kg_settings_get_all: () => Promise.reject(new Error("kg IPC race")),
    });
    const setters = makeSetters();

    const result = await bootApp(api, setters, {
      fetchVersion: () => Promise.resolve("0.2.0-beta.1"),
      log: SILENT_LOG,
    });

    expect(result.kgSettings).toBe("rejected");
    expect(setters.setKgGraphEnabled).not.toHaveBeenCalled();

    // The point of the pattern fix: everything else still fired.
    expect(setters.setModes).toHaveBeenCalledTimes(1);
    expect(setters.setSettings).toHaveBeenCalledTimes(1);
    expect(setters.setActiveModeSlug).toHaveBeenCalledWith("normal");
    expect(setters.setAppVersion).toHaveBeenCalledWith("0.2.0-beta.1");
    expect(setters.applyTheme).toHaveBeenCalledWith("system");
  });

  it("still fires modes/settings/kg/version when get_active_mode rejects (Bug 1)", async () => {
    const api = makeApi({
      get_active_mode: () =>
        Promise.reject(new Error("no active mode row yet")),
    });
    const setters = makeSetters();

    const result = await bootApp(api, setters, {
      fetchVersion: () => Promise.resolve("0.2.0-beta.1"),
      log: SILENT_LOG,
    });

    expect(result.activeMode).toBe("rejected");
    expect(setters.setActiveModeSlug).not.toHaveBeenCalled();

    expect(setters.setModes).toHaveBeenCalledTimes(1);
    expect(setters.setSettings).toHaveBeenCalledTimes(1);
    expect(setters.setKgGraphEnabled).toHaveBeenCalledWith(true);
    expect(setters.setAppVersion).toHaveBeenCalledWith("0.2.0-beta.1");
    expect(setters.applyTheme).toHaveBeenCalledWith("system");
  });

  it("still fires kg + active mode + version when list_modes rejects", async () => {
    const api = makeApi({
      list_modes: () => Promise.reject(new Error("modes table empty")),
    });
    const setters = makeSetters();

    const result = await bootApp(api, setters, {
      fetchVersion: () => Promise.resolve("0.2.0-beta.1"),
      log: SILENT_LOG,
    });

    expect(result.modes).toBe("rejected");
    expect(setters.setModes).not.toHaveBeenCalled();

    expect(setters.setSettings).toHaveBeenCalledTimes(1);
    expect(setters.setActiveModeSlug).toHaveBeenCalledWith("normal");
    expect(setters.setKgGraphEnabled).toHaveBeenCalledWith(true);
    expect(setters.setAppVersion).toHaveBeenCalledWith("0.2.0-beta.1");
  });

  it("still fires the other setters when fetchAppVersion rejects (Bug 3 fallback)", async () => {
    const api = makeApi();
    const setters = makeSetters();

    const result = await bootApp(api, setters, {
      fetchVersion: () => Promise.reject(new Error("version IPC unavailable")),
      log: SILENT_LOG,
    });

    expect(result.appVersion).toBe("rejected");
    expect(setters.setAppVersion).not.toHaveBeenCalled();

    expect(setters.setModes).toHaveBeenCalledTimes(1);
    expect(setters.setSettings).toHaveBeenCalledTimes(1);
    expect(setters.setActiveModeSlug).toHaveBeenCalledWith("normal");
    expect(setters.setKgGraphEnabled).toHaveBeenCalledWith(true);
  });

  it("only applies theme when get_settings fulfills (defensive)", async () => {
    const api = makeApi({
      get_settings: () => Promise.reject(new Error("settings IPC down")),
    });
    const setters = makeSetters();

    await bootApp(api, setters, {
      fetchVersion: () => Promise.resolve("0.2.0-beta.1"),
      log: SILENT_LOG,
    });

    expect(setters.setSettings).not.toHaveBeenCalled();
    expect(setters.applyTheme).not.toHaveBeenCalled();
    // Sister setters still fire — independence holds.
    expect(setters.setModes).toHaveBeenCalledTimes(1);
    expect(setters.setKgGraphEnabled).toHaveBeenCalledWith(true);
    expect(setters.setAppVersion).toHaveBeenCalledWith("0.2.0-beta.1");
  });

  it("logs each individual failure via the injected logger", async () => {
    const api = makeApi({
      list_modes: () => Promise.reject(new Error("a")),
      kg_settings_get_all: () => Promise.reject(new Error("b")),
    });
    const setters = makeSetters();
    const log = vi.fn();

    await bootApp(api, setters, {
      fetchVersion: () => Promise.resolve("0.2.0-beta.1"),
      log,
    });

    // Two failures => two log calls. Other three IPCs succeeded.
    expect(log).toHaveBeenCalledTimes(2);
  });
});
