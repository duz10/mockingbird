// Vitest setup — runs once before the suite. Keeps the bar low:
// just enough globals so component tests can render without a
// browser.

import { vi } from "vitest";

// jsdom doesn't ship `matchMedia` — components reading prefers-* will
// crash without this shim.
if (typeof window !== "undefined" && !window.matchMedia) {
  Object.defineProperty(window, "matchMedia", {
    writable: true,
    value: vi.fn().mockImplementation((query: string) => ({
      matches: false,
      media: query,
      onchange: null,
      addListener: vi.fn(),
      removeListener: vi.fn(),
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      dispatchEvent: vi.fn(),
    })),
  });
}
