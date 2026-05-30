// Phase 1C Wave 1C.1 + 1C.2 — IPC-contract tests for the KG-settings
// tab.
//
// Mirrors the SettingsMeetingTab.test.ts shape: we don't ship
// @testing-library/react, so the component's JSX rendering is
// covered by Playwright (qa-kitten). What we CAN test here is the
// `api.kg_settings_get_all()` / `api.kg_settings_set()` contract —
// the snapshot shape the component leans on and the write payload
// it issues. If either drifts, the component breaks.
//
// Wave 1C.2 (`mb-9ufg`) adds contract tests for the three new IPCs
// the failed-filings UX depends on:
//   * `kg_list_failed_filings(limit?)` — default fixture is empty
//     so the "all good!" state renders without an override.
//   * `kg_queue_status()` — 4-field snapshot, `lastDoneIso`
//     nullable.
//   * `kg_requeue_failed(queueId)` — idempotent void return.
//
// Tests run in fixture mode (no Tauri shell) so they exercise the
// fixture path in `ui/src/lib/tauri.ts`.

import { afterEach, describe, expect, it } from "vitest";

import { api } from "../lib/tauri";
import type { FailedFiling, QueueStatus } from "../lib/types";

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

/* ------------------------------------------------------------------ */
/* Wave 1C.2 — failed-filings + queue-status + requeue IPCs.         */
/* ------------------------------------------------------------------ */

// Per-test fixture overrides are read off window. Restore between
// tests so a custom row list doesn't leak into the "empty default"
// assertions below.
afterEach(() => {
  if (typeof window !== "undefined") {
    window.__MOCKINGBIRD_FIXTURES__ = undefined;
  }
});

describe("api.kg_list_failed_filings (fixture mode)", () => {
  it("defaults to an empty list so the 'all good!' state renders", async () => {
    const rows = await api.kg_list_failed_filings();
    expect(Array.isArray(rows)).toBe(true);
    expect(rows.length).toBe(0);
  });

  it("honours window.__MOCKINGBIRD_FIXTURES__ overrides for non-empty cases", async () => {
    const override: FailedFiling[] = [
      {
        queueId: 11,
        entryId: 42,
        attemptCount: 3,
        lastError: "ollama: connection refused",
        enqueuedIso: "2026-05-30T10:00:00Z",
        failedIso: "2026-05-30T10:05:00Z",
      },
    ];
    window.__MOCKINGBIRD_FIXTURES__ = {
      kg_list_failed_filings: override,
    };
    const rows = await api.kg_list_failed_filings();
    expect(rows).toEqual(override);
    expect(rows.length).toBe(1);
    // Every wire field the UI reads is present and typed correctly
    // — a sanity belt around the FailedFiling DTO contract.
    const row = rows[0];
    if (!row) throw new Error("unreachable: length asserted above");
    expect(typeof row.queueId).toBe("number");
    expect(typeof row.entryId).toBe("number");
    expect(typeof row.attemptCount).toBe("number");
    expect(typeof row.lastError).toBe("string");
    expect(typeof row.enqueuedIso).toBe("string");
    expect(typeof row.failedIso).toBe("string");
  });

  it("accepts an explicit limit argument without throwing", async () => {
    // The Rust side caps unbounded calls at 50; passing a smaller
    // cap is forward-compat for a future paginated UX.
    await expect(api.kg_list_failed_filings(10)).resolves.toEqual([]);
  });
});

describe("api.kg_queue_status (fixture mode)", () => {
  it("returns a fully-populated QueueStatus snapshot", async () => {
    const s = await api.kg_queue_status();
    expect(typeof s.pending).toBe("number");
    expect(typeof s.processing).toBe("number");
    expect(typeof s.failed).toBe("number");
    // lastDoneIso is null in the default fixture (no successful
    // filings yet); guard the type union explicitly so a drift to
    // `undefined` breaks the contract test instead of silently
    // rendering "never" forever.
    expect(s.lastDoneIso === null || typeof s.lastDoneIso === "string").toBe(
      true,
    );
  });

  it("default fixture matches the kgGraphEnabled=false invariant (all zero)", async () => {
    const s = await api.kg_queue_status();
    expect(s.pending).toBe(0);
    expect(s.processing).toBe(0);
    expect(s.failed).toBe(0);
    expect(s.lastDoneIso).toBeNull();
  });

  it("honours window.__MOCKINGBIRD_FIXTURES__ overrides for non-empty cases", async () => {
    const override: QueueStatus = {
      pending: 2,
      processing: 1,
      failed: 3,
      lastDoneIso: "2026-05-30T09:00:00Z",
    };
    window.__MOCKINGBIRD_FIXTURES__ = { kg_queue_status: override };
    const s = await api.kg_queue_status();
    expect(s).toEqual(override);
  });
});

describe("api.kg_requeue_failed (fixture mode)", () => {
  it("resolves to void without throwing (idempotent contract)", async () => {
    await expect(api.kg_requeue_failed(1)).resolves.toBeUndefined();
    // Calling on the same id twice is the Wave 1C.5 J3 invariant
    // (idempotent on already-pending rows). The fixture mode
    // doesn't model state, but exercising the call twice guards
    // against any future fixture that throws on a duplicate.
    await expect(api.kg_requeue_failed(1)).resolves.toBeUndefined();
  });
});
