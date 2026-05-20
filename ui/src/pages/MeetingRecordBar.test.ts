// Unit tests for `summarizeProgress` — the pure helper that drives
// the meeting record bar's "transcribing N/M" chip.
//
// We don't yet have @testing-library/react, so we test the math
// directly rather than mounting the component. The component-level
// wiring (which-phase-shows-the-chip + onChange handlers) is covered
// by Playwright in Wave 6.

import { describe, it, expect } from "vitest";

import { summarizeProgress } from "./MeetingRecordBar";

describe("summarizeProgress", () => {
  it("returns null when no channel has reported", () => {
    expect(summarizeProgress({})).toBeNull();
  });

  it("returns null when both channels are explicitly undefined", () => {
    expect(
      summarizeProgress({ mic: undefined, system: undefined }),
    ).toBeNull();
  });

  it("renders single-channel mic progress with concrete total", () => {
    const out = summarizeProgress({ mic: { done: 4, total: 12 } });
    expect(out).toEqual({ done: 4, total: "12" });
  });

  it("renders single-channel system progress with concrete total", () => {
    const out = summarizeProgress({ system: { done: 7, total: 20 } });
    expect(out).toEqual({ done: 7, total: "20" });
  });

  it("sums across both channels when both totals are known", () => {
    const out = summarizeProgress({
      mic: { done: 3, total: 10 },
      system: { done: 5, total: 10 },
    });
    expect(out).toEqual({ done: 8, total: "20" });
  });

  it("renders unknown total as '?' when any channel's total is null", () => {
    // mic total known; system total still open
    const out = summarizeProgress({
      mic: { done: 3, total: 10 },
      system: { done: 1, total: null },
    });
    // Done counter sums; total degrades to "?" rather than misleading
    // the user with a partial sum.
    expect(out).toEqual({ done: 4, total: "?" });
  });

  it("renders unknown total as '?' when only channel reports null total", () => {
    const out = summarizeProgress({ mic: { done: 2, total: null } });
    expect(out).toEqual({ done: 2, total: "?" });
  });

  it("treats zero-done as valid (not the same as 'no channel reported')", () => {
    // A channel can legitimately report `done: 0, total: 12` right
    // after the driver registers it but before the first chunk lands.
    // summarizeProgress must return non-null here so the chip can
    // render "0/12" rather than disappearing.
    const out = summarizeProgress({ mic: { done: 0, total: 12 } });
    expect(out).toEqual({ done: 0, total: "12" });
  });
});
