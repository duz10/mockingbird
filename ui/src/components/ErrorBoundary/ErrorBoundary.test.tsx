// mb-e3z — ErrorBoundary behaviour.
//
// The project doesn't ship `@testing-library/react`, so we mount with
// `react-dom/client` + `flushSync` (forces a synchronous commit, which
// is when React runs the error-boundary catch) and assert against the
// jsdom DOM directly. This is enough to prove the two contracts that
// matter: healthy children pass through, and a render throw swaps in
// the fallback panel (rather than an empty tree = blank screen).

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createRoot, type Root } from "react-dom/client";
import { flushSync } from "react-dom";

import { ErrorBoundary } from "./ErrorBoundary";

function Boom(): never {
  throw new Error("kaboom");
}

describe("ErrorBoundary", () => {
  let container: HTMLDivElement;
  let root: Root;
  // React logs caught errors via console.error; silence + capture so
  // the suite output stays clean and we can assert we logged locally.
  let errorSpy: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
    errorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
  });

  afterEach(() => {
    flushSync(() => root.unmount());
    container.remove();
    errorSpy.mockRestore();
  });

  it("renders children unchanged when nothing throws", () => {
    flushSync(() => {
      root.render(
        <ErrorBoundary>
          <p>all good</p>
        </ErrorBoundary>,
      );
    });
    expect(container.textContent).toContain("all good");
    expect(container.querySelector('[role="alert"]')).toBeNull();
  });

  it("shows the fallback panel + reload affordance on a render throw", () => {
    flushSync(() => {
      root.render(
        <ErrorBoundary>
          <Boom />
        </ErrorBoundary>,
      );
    });
    const alert = container.querySelector('[role="alert"]');
    expect(alert).not.toBeNull();
    expect(container.textContent).toContain("Something went wrong");
    // Reload affordance present + labelled.
    const reload = container.querySelector('button[aria-label="Reload"]');
    expect(reload).not.toBeNull();
    // We logged the crash locally (no telemetry) via componentDidCatch.
    expect(errorSpy).toHaveBeenCalled();
  });

  it("getDerivedStateFromError maps the error into fallback state", () => {
    const err = new Error("boom");
    expect(ErrorBoundary.getDerivedStateFromError(err)).toEqual({ error: err });
  });
});
