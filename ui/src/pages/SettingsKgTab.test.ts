// Phase 1C Wave 1C.1 — IPC-contract tests for the KG-settings tab.
//
// Mirrors the SettingsMeetingTab.test.ts shape: we don't ship
// @testing-library/react, so the component's JSX rendering is
// covered by Playwright (qa-kitten). What we CAN test here is the
// `api.kg_settings_get_all()` / `api.kg_settings_set()` contract —
// the snapshot shape the component leans on and the write payload
// it issues. If either drifts, the component breaks.
//
// Tests run in fixture mode (no Tauri shell) so they exercise the
// fixture path in `ui/src/lib/tauri.ts`.

import { describe, it, expect } from "vitest";

import { api } from "../lib/tauri";

describe("api.kg_settings_get_all (fixture mode)", () => {
  it("returns a typed KgSettings snapshot with kgGraphEnabled defined", async () => {
    const snap = await api.kg_settings_get_all();
    expect(snap.kgGraphEnabled).not.toBeUndefined();
    expect(typeof snap.kgGraphEnabled).toBe("boolean");
  });

  it("defaults to graph-off (ADR 0049 §Sandbox isolation)", async () => {
    // The default-off invariant is also enforced server-side; this
    // assertion guards the fixture from drifting away from it.
    const snap = await api.kg_settings_get_all();
    expect(snap.kgGraphEnabled).toBe(false);
  });
});

describe("api.kg_settings_set (fixture mode)", () => {
  it("accepts the kg_graph_enabled key + boolean value without throwing", async () => {
    // This is the only key on the Rust allowlist today
    // (is_kg_setting_allowed_for_ui in commands/kg.rs).
    await expect(
      api.kg_settings_set("kg_graph_enabled", true),
    ).resolves.toBeUndefined();
    await expect(
      api.kg_settings_set("kg_graph_enabled", false),
    ).resolves.toBeUndefined();
  });
});
