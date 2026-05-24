// Unit tests for the import-progress overlay reducer.
//
// Lightweight by design (no React render) — the visual layer is
// tiny and the value is in the state-machine transitions. If the
// overlay grows behaviour (e.g. queueing multiple concurrent
// imports), promote this to a full RTL test.

import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";

import {
  reduceProgress,
  type IngestProgressPayload,
} from "./index";

function payload(
  partial: Partial<IngestProgressPayload> & { stage: IngestProgressPayload["stage"] },
): IngestProgressPayload {
  return {
    source: "desktop-import",
    originalFilename: "memo.m4a",
    ...partial,
  };
}

describe("reduceProgress", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-05-28T00:00:00Z"));
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("opens overlay on the first decoding event", () => {
    const next = reduceProgress(null, payload({ stage: "decoding" }));
    expect(next.payload.stage).toBe("decoding");
    expect(next.payload.originalFilename).toBe("memo.m4a");
  });

  it("transitions through decoding -> transcribing -> done", () => {
    let s = reduceProgress(null, payload({ stage: "decoding" }));
    s = reduceProgress(s, payload({ stage: "transcribing" }));
    expect(s.payload.stage).toBe("transcribing");
    s = reduceProgress(s, payload({ stage: "done", sessionId: 42 }));
    expect(s.payload.stage).toBe("done");
    expect(s.payload.sessionId).toBe(42);
  });

  it("bumps receivedAt on every fresh event", () => {
    const first = reduceProgress(null, payload({ stage: "decoding" }));
    vi.advanceTimersByTime(50);
    const second = reduceProgress(first, payload({ stage: "transcribing" }));
    expect(second.receivedAt).toBeGreaterThan(first.receivedAt);
  });

  it("preserves error payload on failed", () => {
    const next = reduceProgress(
      null,
      payload({ stage: "failed", error: "decode failed: bad bytes" }),
    );
    expect(next.payload.stage).toBe("failed");
    expect(next.payload.error).toBe("decode failed: bad bytes");
  });

  it("replaces prior in-flight payload with newer one (last writer wins)", () => {
    // Two concurrent imports would arrive interleaved; the simplest
    // contract (and what the Rust side does in serial-process mode)
    // is last-event-wins.
    let s = reduceProgress(null, payload({
      stage: "transcribing",
      originalFilename: "first.m4a",
    }));
    s = reduceProgress(s, payload({
      stage: "decoding",
      originalFilename: "second.m4a",
    }));
    expect(s.payload.originalFilename).toBe("second.m4a");
    expect(s.payload.stage).toBe("decoding");
  });

  it("carries source label through every transition", () => {
    let s = reduceProgress(null, payload({
      stage: "decoding",
      source: "mobile-inbox",
    }));
    expect(s.payload.source).toBe("mobile-inbox");
    s = reduceProgress(s, payload({
      stage: "done",
      source: "mobile-inbox",
      sessionId: 7,
    }));
    expect(s.payload.source).toBe("mobile-inbox");
  });
});
