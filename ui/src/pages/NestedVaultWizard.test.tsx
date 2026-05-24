// Unit tests for the nested-vault wizard.
//
// The project doesn't ship `@testing-library/react`, so this file
// tests the pure exported helpers + the wizard's prop contract by
// shape-checking the callback signatures. Full UI verification of
// the dialog rendering + click routing lives in the Iter 4
// Playwright sweep (qa-kitten).

import { describe, expect, it } from "vitest";

import {
  siblingBranchAvailable,
  type NestedVaultInfo,
  type NestedVaultWizardProps,
} from "./NestedVaultWizard";

const baseInfo: NestedVaultInfo = {
  candidatePath: "C:\\Users\\you\\ObsidianVault\\mockingbird-vault",
  parentVault: "C:\\Users\\you\\ObsidianVault",
  suggestedSibling: "C:\\Users\\you\\mockingbird-vault",
};

describe("siblingBranchAvailable", () => {
  it("returns true when a distinct sibling is suggested", () => {
    expect(siblingBranchAvailable(baseInfo)).toBe(true);
  });

  it("returns false when no sibling could be suggested", () => {
    expect(
      siblingBranchAvailable({ ...baseInfo, suggestedSibling: null }),
    ).toBe(false);
  });

  it("returns false when the suggestion equals the candidate (defensive)", () => {
    expect(
      siblingBranchAvailable({
        ...baseInfo,
        suggestedSibling: baseInfo.candidatePath,
      }),
    ).toBe(false);
  });

  it("treats an empty suggestion as missing", () => {
    expect(
      siblingBranchAvailable({ ...baseInfo, suggestedSibling: "" }),
    ).toBe(false);
  });
});

describe("NestedVaultWizardProps shape", () => {
  // Compile-time + runtime sanity check that the wizard exposes the
  // three branch-handlers the kickoff mandates. If a future
  // refactor renames `onPickDifferent` to `onChange`, this test
  // will fail at type-check time — the runtime assertions are
  // belt-and-suspenders for the JSON-only review path.
  it("requires onAccept / onPickDifferent / onCancel handlers", () => {
    const props: NestedVaultWizardProps = {
      info: baseInfo,
      onAccept: () => {},
      onPickDifferent: () => {},
      onCancel: () => {},
    };
    expect(typeof props.onAccept).toBe("function");
    expect(typeof props.onPickDifferent).toBe("function");
    expect(typeof props.onCancel).toBe("function");
  });

  it("onAccept signature carries the acceptedNested boolean", () => {
    // The parent uses this flag to decide between the recommended
    // and use-anyway branches' post-save toast. If it were dropped
    // accidentally, the warning would silently disappear.
    let captured: { acceptedNested: boolean } | null = null;
    const props: NestedVaultWizardProps = {
      info: baseInfo,
      onAccept: (_p, opts) => {
        captured = opts;
      },
      onPickDifferent: () => {},
      onCancel: () => {},
    };
    props.onAccept("/some/path", { acceptedNested: true });
    expect(captured).toEqual({ acceptedNested: true });
  });
});
