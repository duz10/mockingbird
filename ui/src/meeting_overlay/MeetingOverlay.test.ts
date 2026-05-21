// Pure-helper tests for MeetingOverlay.
//
// ADR 0032 / mb-nig: the dBFS-to-fill mapping is the only piece of
// the VU bar that has logic worth pinning. The component itself is
// presentational + driven by Tauri events that the integration tests
// (Playwright, future) cover end-to-end.

import { describe, it, expect } from "vitest";

import { dbfsToFill } from "./MeetingOverlay";

describe("dbfsToFill (ADR 0032 / mb-nig)", () => {
  it("collapses the no-data sentinel (0 exact) to a flat bar", () => {
    expect(dbfsToFill(0)).toBe(0);
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

  it("treats 0 specially — it's the sentinel, NOT '0 dBFS clipping'", () => {
    // This is the design choice that lets the UI distinguish "no
    // data yet" from "signal at clipping". A reading of exactly 0
    // collapses to 0; a reading of +0.0001 (anomaly) snaps to 1.
    expect(dbfsToFill(0)).toBe(0);
    expect(dbfsToFill(0.0001)).toBe(1);
  });
});
