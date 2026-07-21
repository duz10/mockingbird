// Pure-logic tests for the shared cleanup-status helpers. The React
// rendering is exercised by the isMac-gated surfaces + Playwright; here
// we lock down the two pure functions every surface depends on.

import { describe, expect, it } from "vitest";

import type { CleanupStatus } from "../../lib/types";

import { cleanupDisplayState } from "./useCleanupStatus";
import { isPassthroughModel } from "./CleanupStatus";

function status(over: Partial<CleanupStatus>): CleanupStatus {
  return {
    ollamaReachable: true,
    cleanupActive: true,
    effectiveModel: "qwen2.5:3b-instruct-q4_K_M",
    installedModels: ["qwen2.5:3b-instruct-q4_K_M"],
    recommendedPull: "qwen2.5:3b",
    ramTier: "small",
    ...over,
  };
}

describe("cleanupDisplayState", () => {
  it("is unknown before the first fetch", () => {
    expect(cleanupDisplayState(null)).toBe("unknown");
  });

  it("is active when Ollama is up and a model resolves", () => {
    expect(cleanupDisplayState(status({ cleanupActive: true }))).toBe("active");
  });

  it("is ollamaDown when the service is unreachable", () => {
    expect(
      cleanupDisplayState(
        status({ ollamaReachable: false, cleanupActive: false, effectiveModel: null }),
      ),
    ).toBe("ollamaDown");
  });

  it("is noModel when reachable but nothing usable is installed", () => {
    expect(
      cleanupDisplayState(
        status({
          ollamaReachable: true,
          cleanupActive: false,
          effectiveModel: null,
          installedModels: [],
        }),
      ),
    ).toBe("noModel");
  });
});

describe("isPassthroughModel", () => {
  it("treats null / empty / 'passthrough' as raw", () => {
    expect(isPassthroughModel(null)).toBe(true);
    expect(isPassthroughModel(undefined)).toBe(true);
    expect(isPassthroughModel("")).toBe(true);
    expect(isPassthroughModel("  ")).toBe(true);
    expect(isPassthroughModel("passthrough")).toBe(true);
    expect(isPassthroughModel("Passthrough")).toBe(true);
  });

  it("treats a real model tag as cleaned", () => {
    expect(isPassthroughModel("qwen2.5:3b-instruct-q4_K_M")).toBe(false);
    expect(isPassthroughModel("qwen2.5:7b-instruct-q4_K_M")).toBe(false);
  });
});
