import { describe, expect, it } from "vitest";

import {
  formatCount,
  formatDuration,
  formatStopwatch,
  numericWordCount,
  prettyAppName,
  truncate,
} from "./format";

describe("formatDuration", () => {
  it("formats seconds-only", () => {
    expect(formatDuration(47_000)).toBe("47s");
  });
  it("formats minutes and seconds", () => {
    expect(formatDuration(2 * 60_000 + 7_000)).toBe("2m 7s");
  });
  it("formats hours and minutes (drops seconds)", () => {
    expect(formatDuration(2 * 3600_000 + 14 * 60_000 + 30_000)).toBe("2h 14m");
  });
  it("guards garbage input", () => {
    expect(formatDuration(NaN)).toBe("—");
    expect(formatDuration(-1)).toBe("—");
  });
});

describe("formatStopwatch", () => {
  it("zero-pads seconds", () => {
    expect(formatStopwatch(5)).toBe("0:05");
    expect(formatStopwatch(60)).toBe("1:00");
    expect(formatStopwatch(754)).toBe("12:34");
  });
  it("guards bad input", () => {
    expect(formatStopwatch(NaN)).toBe("0:00");
  });
});

describe("formatCount", () => {
  it("applies thousands separator", () => {
    expect(formatCount(1842)).toMatch(/1[,. ]842/);
  });
  it("rounds floats", () => {
    expect(formatCount(3.7)).toBe("4");
  });
});

describe("prettyAppName", () => {
  it("strips .exe and title-cases", () => {
    expect(prettyAppName("slack.exe")).toBe("Slack");
    expect(prettyAppName("Code.exe")).toBe("Code");
    expect(prettyAppName(null)).toBe("Unknown");
    expect(prettyAppName(undefined)).toBe("Unknown");
  });
});

describe("truncate", () => {
  it("leaves short strings alone", () => {
    expect(truncate("hi", 10)).toBe("hi");
  });
  it("adds ellipsis when over the cap", () => {
    expect(truncate("hello world", 8)).toBe("hello w…");
  });
});

describe("numericWordCount", () => {
  it("returns 0 for empty + whitespace", () => {
    expect(numericWordCount("")).toBe(0);
    expect(numericWordCount("   ")).toBe(0);
  });
  it("counts simple words", () => {
    expect(numericWordCount("hello world")).toBe(2);
    expect(numericWordCount("  multiple   spaces   between  ")).toBe(3);
  });
});
