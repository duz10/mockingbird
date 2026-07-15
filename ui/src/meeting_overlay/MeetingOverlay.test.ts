// Pure-helper tests for MeetingOverlay.
//
// ADR 0032 / mb-nig: the dBFS-to-fill mapping is the only piece of
// the VU bar that has logic worth pinning. The component itself is
// presentational + driven by Tauri events that the integration tests
// (Playwright, future) cover end-to-end.

import { describe, it, expect } from "vitest";

import { dbfsToFill } from "./MeetingOverlay";

describe("dbfsToFill (ADR 0032 / mb-nig)", () => {
  it("collapses the no-data value (null) to a flat bar", () => {
    // mb-x1d: "no data yet" is now `null`, distinct from a real reading.
    expect(dbfsToFill(null)).toBe(0);
  });

  it("clamps DBFS_FLOOR (-100) and below to 0", () => {
    expect(dbfsToFill(-100)).toBe(0);
    expect(dbfsToFill(-150)).toBe(0);
  });

  it("clamps positive readings (e.g. clipped digital signal) to 1", () => {
    expect(dbfsToFill(0.5)).toBe(1);
    expect(dbfsToFill(12)).toBe(1);
  });

  it("maps midpoints linearly in dB across the [-100, 0] range", () => {
    // -50 dBFS → halfway → 0.5
    expect(dbfsToFill(-50)).toBeCloseTo(0.5, 6);
    // -25 dBFS → 75%
    expect(dbfsToFill(-25)).toBeCloseTo(0.75, 6);
    // -75 dBFS → 25%
    expect(dbfsToFill(-75)).toBeCloseTo(0.25, 6);
  });

  it("treats 0 as a REAL full-scale reading, not 'no data' (mb-x1d)", () => {
    // mb-x1d fix: a full-scale 0 dBFS (clipping) reading fills the bar;
    // only `null` means "no data yet". Previously 0 was the sentinel and
    // wrongly collapsed to a flat bar.
    expect(dbfsToFill(0)).toBe(1);
    expect(dbfsToFill(null)).toBe(0);
  });
});
