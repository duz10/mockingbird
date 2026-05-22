// Smoke tests for the Command Center component + its pure helpers.
//
// Phase 10 Wave 1A. The state machine itself is tested exhaustively
// on the Rust side; this file exercises the React glue + the
// `formatElapsed` pure helper.

import { describe, expect, it } from "vitest";

import { formatElapsed } from "./CommandCenter";

describe("formatElapsed", () => {
  it("renders 0 ms as 0:00", () => {
    expect(formatElapsed(0)).toBe("0:00");
  });

  it("clamps negative input to 0:00", () => {
    expect(formatElapsed(-5_000)).toBe("0:00");
  });

  it("clamps NaN to 0:00", () => {
    expect(formatElapsed(Number.NaN)).toBe("0:00");
  });

  it("renders < 1 minute as 0:SS", () => {
    expect(formatElapsed(7_500)).toBe("0:07");
    expect(formatElapsed(59_999)).toBe("0:59");
  });

  it("pads single-digit seconds", () => {
    expect(formatElapsed(61_000)).toBe("1:01");
  });

  it("renders minutes up to 59 without an hour segment", () => {
    expect(formatElapsed(59 * 60 * 1000 + 33_000)).toBe("59:33");
  });

  it("renders past the hour as h:MM:SS", () => {
    // 1h 02m 03s.
    expect(formatElapsed((1 * 3600 + 2 * 60 + 3) * 1000)).toBe("1:02:03");
  });

  it("pads sub-10 minute counts inside hour format", () => {
    // 1h 05m 00s — leading zero on the minute segment because we're
    // in h:MM:SS land.
    expect(formatElapsed((1 * 3600 + 5 * 60) * 1000)).toBe("1:05:00");
  });
});
