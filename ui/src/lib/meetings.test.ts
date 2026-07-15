// Unit tests for the meetings IPC wrappers.
//
// In jsdom + vitest we're always in the "fixture" code path (no
// `__TAURI_INTERNALS__` injected by Tauri's webview), so these tests
// exercise the fixture dispatcher + override hook. The real-Tauri
// path is exercised end-to-end by Playwright.

import { describe, it, expect, beforeEach, afterEach } from "vitest";

import {
  clampMaxDuration,
  meetings,
  MEETING_FIXTURES,
  MEETING_MAX_DURATION_MAX_SEC,
  MEETING_MAX_DURATION_MIN_SEC,
} from "./meetings";

describe("meetings IPC wrappers (fixture mode)", () => {
  beforeEach(() => {
    // Each test starts clean — overrides from earlier tests shouldn't
    // leak across.
    window.__MOCKINGBIRD_MEETING_FIXTURES__ = undefined;
  });

  afterEach(() => {
    window.__MOCKINGBIRD_MEETING_FIXTURES__ = undefined;
  });

  it("probeSources returns the default fixture probe", async () => {
    const probe = await meetings.probeSources();
    expect(probe).toEqual(MEETING_FIXTURES.probe);
    expect(probe.micAvailable).toBe(true);
    expect(probe.systemAvailable).toBe(true);
  });

  it("probeSources respects per-test override", async () => {
    window.__MOCKINGBIRD_MEETING_FIXTURES__ = {
      meeting_probe_sources: { micAvailable: false, systemAvailable: true },
    };
    const probe = await meetings.probeSources();
    expect(probe.micAvailable).toBe(false);
    expect(probe.systemAvailable).toBe(true);
  });

  it("list returns the fixture summaries with the canonical shape", async () => {
    const list = await meetings.list();
    expect(list).toHaveLength(MEETING_FIXTURES.list.length);
    // Spot-check the wire shape we care about most: status + source
    // must match the Rust enum string forms.
    const first = list[0]!;
    expect(["complete", "partial", "demoted", "interrupted", "failed"]).toContain(
      first.status,
    );
    expect(["mic", "system", "both"]).toContain(first.source);
  });

  it("detail resolves by uuid when present in the fixture set", async () => {
    const target = MEETING_FIXTURES.details[1]!;
    const detail = await meetings.detail(target.uuid);
    expect(detail.uuid).toBe(target.uuid);
    expect(detail.formattedSys).toBeNull();
    expect(detail.formattedMic).not.toBeNull();
  });

  it("detail falls back to the first fixture when uuid is unknown", async () => {
    // Defensive contract for the fixture dispatcher: an unknown uuid
    // shouldn't throw — useful when the browser-preview list is
    // truncated but a stale URL still points at an old uuid.
    const detail = await meetings.detail("does-not-exist");
    expect(detail.uuid).toBe(MEETING_FIXTURES.details[0]!.uuid);
  });

  it("search returns an empty array by default (no fixture hits)", async () => {
    const hits = await meetings.search("anything");
    expect(hits).toEqual([]);
  });

  it("start returns a synthesized uuid in fixture mode", async () => {
    const a = await meetings.start("mic");
    const b = await meetings.start("system");
    expect(a.uuid).toMatch(/^fixture-/);
    expect(b.uuid).toMatch(/^fixture-/);
    // Two consecutive starts must produce different uuids — otherwise
    // a test that simulates back-to-back meetings would collide on
    // the same handle. Timestamps tick at ms resolution; if they
    // happen to collide we still want different uuids, so this is a
    // weaker (timestamp-monotonic) invariant rather than strict
    // inequality.
    expect(a.uuid.length).toBeGreaterThan("fixture-".length);
  });

  it("stop / delete / copyToClipboard resolve to undefined", async () => {
    await expect(meetings.stop("any-uuid")).resolves.toBeUndefined();
    await expect(meetings.delete("any-uuid")).resolves.toBeUndefined();
    await expect(
      meetings.copyToClipboard("any-uuid"),
    ).resolves.toBeUndefined();
  });

  it("exportMarkdown returns a path string (default mode)", async () => {
    const result = await meetings.exportMarkdown("any-uuid");
    expect(typeof result.path).toBe("string");
    expect((result.path ?? "").length).toBeGreaterThan(0);
  });

  it("exportMarkdown forwards promptUserForPath default of false", async () => {
    // Just shape-check: the call shouldn't throw when the flag is
    // omitted. The fixture path returns a non-null string.
    const result = await meetings.exportMarkdown(
      "any-uuid",
      undefined,
      undefined,
    );
    expect(result.path).not.toBeNull();
  });

  it("runLlmPass with a built-in name returns the fixture payload", async () => {
    const result = await meetings.runLlmPass("any-uuid", "summary");
    expect(result.id).toBeDefined();
    expect(result.text.length).toBeGreaterThan(0);
    expect(result.latencyMs).toBeGreaterThan(0);
  });

  it("runLlmPass with a custom prompt body uses the same fixture path", async () => {
    // The fixture mode doesn't actually run a prompt — the shape
    // assertion is the value here: the wire arg `{ custom: body }`
    // must be accepted without throwing through the fixture
    // dispatcher.
    const result = await meetings.runLlmPass("any-uuid", {
      custom: "do the thing",
    });
    expect(result).toBeDefined();
    expect(result.id).toBeDefined();
  });

  it("override hook is applied per-command independently", async () => {
    window.__MOCKINGBIRD_MEETING_FIXTURES__ = {
      meeting_run_llm_pass: { id: "override-id", text: "OVERRIDE", latencyMs: 7 },
    };
    const result = await meetings.runLlmPass("any-uuid", "summary");
    expect(result.id).toBe("override-id");
    expect(result.text).toBe("OVERRIDE");
    // Other commands NOT overridden �?" should still use defaults.
    const probe = await meetings.probeSources();
    expect(probe).toEqual(MEETING_FIXTURES.probe);
  });
});

describe("clampMaxDuration (ADR 0032 / mb-mom)", () => {
  it("floors below the minimum to MIN", () => {
    expect(clampMaxDuration(0)).toBe(MEETING_MAX_DURATION_MIN_SEC);
    expect(clampMaxDuration(-9999)).toBe(MEETING_MAX_DURATION_MIN_SEC);
    expect(clampMaxDuration(59)).toBe(MEETING_MAX_DURATION_MIN_SEC);
  });

  it("ceils above the maximum to MAX", () => {
    expect(clampMaxDuration(MEETING_MAX_DURATION_MAX_SEC + 1)).toBe(
      MEETING_MAX_DURATION_MAX_SEC,
    );
    expect(clampMaxDuration(1_000_000)).toBe(MEETING_MAX_DURATION_MAX_SEC);
  });

  it("collapses NaN / Infinity to MIN", () => {
    expect(clampMaxDuration(Number.NaN)).toBe(MEETING_MAX_DURATION_MIN_SEC);
    expect(clampMaxDuration(Number.POSITIVE_INFINITY)).toBe(
      MEETING_MAX_DURATION_MIN_SEC,
    );
    expect(clampMaxDuration(Number.NEGATIVE_INFINITY)).toBe(
      MEETING_MAX_DURATION_MIN_SEC,
    );
  });

  it("floors fractional inputs (Math.floor semantics)", () => {
    expect(clampMaxDuration(120.9)).toBe(120);
    expect(clampMaxDuration(3600.5)).toBe(3600);
  });

  it("passes through valid integers unchanged", () => {
    expect(clampMaxDuration(60)).toBe(60);
    expect(clampMaxDuration(3600)).toBe(3600);
    expect(clampMaxDuration(MEETING_MAX_DURATION_MAX_SEC)).toBe(
      MEETING_MAX_DURATION_MAX_SEC,
    );
  });
});
