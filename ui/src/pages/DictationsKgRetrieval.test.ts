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
  EntityDetail,
  EntitySuggestion,
  EntrySummary,
  SearchFilter,
  TagDetail,
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

describe("api.kg_entity_detail (fixture mode, Wave 1C.4)", () => {
  it("defaults to an empty entity payload (loading-state shape)", async () => {
    const d = await api.kg_entity_detail(0);
    expect(d.entityId).toBe(0);
    expect(d.canonicalName).toBe("");
    expect(d.aliases).toEqual([]);
    expect(d.mentionCount).toBe(0);
    expect(d.totalEntries).toBe(0);
    expect(d.recentEntries).toEqual([]);
  });

  it("accepts recentLimit override without throwing", async () => {
    await expect(api.kg_entity_detail(42, 10)).resolves.toBeDefined();
    await expect(api.kg_entity_detail(42, undefined)).resolves.toBeDefined();
  });

  it("surfaces the full EntityDetail shape under override", async () => {
    const override: EntityDetail = {
      entityId: 7,
      canonicalName: "Mrs. Chen",
      entityType: "person",
      aliases: ["Chen", "Mrs Chen"],
      mentionCount: 12,
      totalEntries: 4,
      recentEntries: [
        {
          entryId: 100,
          title: "Talked with Mrs. Chen about the calculus assignment",
          capturedIso: "2026-05-30T10:00:00Z",
          category: null,
        },
      ],
    };
    window.__MOCKINGBIRD_FIXTURES__ = { kg_entity_detail: override };
    const d = await api.kg_entity_detail(7);
    expect(d).toEqual(override);
    // `category` is reserved null per the 1C.4 wire contract
    // (mb-oji5 parking lot). Assert here so a future un-reserve
    // (entity-side category badge) flags this test for review.
    expect(d.recentEntries[0]?.category).toBeNull();
  });
});

describe("api.kg_tag_detail (fixture mode, Wave 1C.4)", () => {
  it("defaults to an empty tag payload (open-vocab unknown-slug shape)", async () => {
    // Open-vocab semantics: an unknown slug is NOT an error; it
    // resolves to zero counts + empty recentEntries. The default
    // fixture mirrors that.
    const d = await api.kg_tag_detail("never-seen-slug");
    expect(d.tagSlug).toBe("");
    expect(d.mentionCount).toBe(0);
    expect(d.totalEntries).toBe(0);
    expect(d.recentEntries).toEqual([]);
  });

  it("keyed by tagSlug (string), NOT a synthetic tagId -- ADR 0051 deviation", async () => {
    // The kickoff prescribed `tag_id: i64` but the Rust side ships
    // `tag_slug: String`. This test pins the wire shape so a
    // future refactor toward synthetic ids fails loudly.
    const override: TagDetail = {
      tagSlug: "calculus",
      mentionCount: 5,
      totalEntries: 2,
      recentEntries: [
        {
          entryId: 50,
          title: "Calculus homework due Tuesday",
          capturedIso: "2026-05-28T09:15:00Z",
          category: null,
        },
      ],
    };
    window.__MOCKINGBIRD_FIXTURES__ = { kg_tag_detail: override };
    const d = await api.kg_tag_detail("calculus", 50);
    expect(d).toEqual(override);
    expect(typeof d.tagSlug).toBe("string");
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
