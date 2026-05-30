// IPC-contract tests for the Phase 1C Wave 1C.3 KG retrieval
// helpers (`mb-5ly5`). Mirrors the SettingsKgTab.test.ts shape: we
// don't ship @testing-library/react so component JSX is verified
// via Playwright (qa-kitten). What we CAN test in vitest is the
// fixture wire contract -- if the typed wrapper signature OR the
// fixture-mode default shape drifts, the Dictations page will
// regress and these tests will catch it before Playwright does.

import { afterEach, describe, expect, it } from "vitest";

import { api } from "../lib/tauri";
import type {
  EntitySuggestion,
  EntrySummary,
  SearchFilter,
  TagSuggestion,
} from "../lib/types";

afterEach(() => {
  if (typeof window !== "undefined") {
    window.__MOCKINGBIRD_FIXTURES__ = undefined;
  }
});

describe("api.kg_search_entries (fixture mode)", () => {
  it("returns an empty number[] for the default fixture", async () => {
    const filter: SearchFilter = { entities: [], tags: [] };
    const ids = await api.kg_search_entries(filter);
    expect(Array.isArray(ids)).toBe(true);
    expect(ids.length).toBe(0);
  });

  it("accepts the full SearchFilter shape (entities + tags + query)", async () => {
    // Type-level guard: the wire payload mirrors SearchFilterArg on
    // the Rust side; if any field disappears here the call won't
    // typecheck at the call site in Dictations.tsx.
    const filter: SearchFilter = {
      entities: [1, 2, 3],
      tags: ["calculus", "weekend"],
      query: "rivers",
    };
    await expect(api.kg_search_entries(filter)).resolves.toEqual([]);
  });

  it("honours window.__MOCKINGBIRD_FIXTURES__ overrides for non-empty cases", async () => {
    window.__MOCKINGBIRD_FIXTURES__ = {
      kg_search_entries: [10, 11, 12],
    };
    const filter: SearchFilter = { entities: [42], tags: [] };
    const ids = await api.kg_search_entries(filter);
    expect(ids).toEqual([10, 11, 12]);
  });
});

describe("api.kg_list_entities (fixture mode)", () => {
  it("defaults to an empty list (no entities indexed in fixture)", async () => {
    const rows = await api.kg_list_entities();
    expect(rows).toEqual([]);
  });

  it("accepts prefix + limit args without throwing", async () => {
    await expect(api.kg_list_entities("ma", 10)).resolves.toEqual([]);
    await expect(api.kg_list_entities(undefined, 50)).resolves.toEqual([]);
  });

  it("honours overrides for non-empty cases (mention_count ordering)", async () => {
    const override: EntitySuggestion[] = [
      {
        entityId: 1,
        canonicalName: "Mrs. Chen",
        entityType: "person",
        mentionCount: 12,
      },
      {
        entityId: 2,
        canonicalName: "Home Depot",
        entityType: "place",
        mentionCount: 7,
      },
    ];
    window.__MOCKINGBIRD_FIXTURES__ = { kg_list_entities: override };
    const rows = await api.kg_list_entities();
    expect(rows).toEqual(override);
    // The Rust side orders DESC by mention_count; assert the wire
    // contract by sampling the head.
    const head = rows[0];
    if (!head) throw new Error("unreachable: asserted length above");
    expect(head.mentionCount).toBeGreaterThanOrEqual(rows[1]?.mentionCount ?? 0);
  });
});

describe("api.kg_list_tags (fixture mode)", () => {
  it("defaults to an empty list", async () => {
    const rows = await api.kg_list_tags();
    expect(rows).toEqual([]);
  });

  it("honours overrides (tag_slug is the identifier in 1B)", async () => {
    const override: TagSuggestion[] = [
      { tagSlug: "calculus", mentionCount: 5 },
      { tagSlug: "weekend-plans", mentionCount: 2 },
    ];
    window.__MOCKINGBIRD_FIXTURES__ = { kg_list_tags: override };
    const rows = await api.kg_list_tags("c", 10);
    expect(rows).toEqual(override);
    // Every wire field used by the UI must be present + typed.
    const head = rows[0];
    if (!head) throw new Error("unreachable: asserted length above");
    expect(typeof head.tagSlug).toBe("string");
    expect(typeof head.mentionCount).toBe("number");
  });
});

describe("api.kg_entries_summary (fixture mode)", () => {
  it("defaults to an empty record so 'all silent' rendering works", async () => {
    const map = await api.kg_entries_summary([1, 2, 3]);
    expect(map).toEqual({});
  });

  it("returns the full EntrySummary shape under override", async () => {
    const override: Record<string, EntrySummary> = {
      "1": {
        entities: [
          { entityId: 7, canonicalName: "Mrs. Chen", entityType: "person" },
        ],
        tags: [{ tagSlug: "calculus" }],
        filingState: "done",
      },
      "2": {
        entities: [],
        tags: [],
        filingState: "failed",
      },
    };
    window.__MOCKINGBIRD_FIXTURES__ = { kg_entries_summary: override };
    const map = await api.kg_entries_summary([1, 2]);
    expect(map).toEqual(override);
    // Spot-check the wire contract for failed-state rows -- the
    // pill rendering depends on the snake_case wire form
    // (`"failed"`, not `"Failed"`).
    expect(map["2"]?.filingState).toBe("failed");
  });

  it("keys are stringified -- the wire form of a numeric Rust HashMap<i64, _>", async () => {
    // JSON object keys are strings even when the Rust side keyed
    // the map by i64. Asserting the documented shape here guards
    // against a future migration to a Vec<(i64, EntrySummary)>
    // that would silently flip the indexing pattern in
    // DictationsList.tsx.
    const override: Record<string, EntrySummary> = {
      "42": {
        entities: [],
        tags: [],
        filingState: "pending",
      },
    };
    window.__MOCKINGBIRD_FIXTURES__ = { kg_entries_summary: override };
    const map = await api.kg_entries_summary([42]);
    expect(Object.keys(map)).toEqual(["42"]);
    expect(typeof Object.keys(map)[0]).toBe("string");
  });
});
