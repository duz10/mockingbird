// Curated category list for the "Curated" background mode.
//
// These are human-readable labels mapped to Unsplash `query=` terms.
// Deliberately small (12 picks) — exhaustive lists turn into
// a checkbox wall and decision paralysis. If the user wants something
// not in the list, they can switch to Random mode. We can grow the
// list later if real usage shows gaps.
//
// Selection criteria:
//   - Visually background-friendly (texture-y, depth-y, not portrait-
//     centric — landscapes filter helps too).
//   - Distinct from each other so the user gets variety from picking
//     2–3 categories rather than overlapping concepts.
//   - Common-sense Unsplash search terms with large pools, so any
//     random fetch lands on a quality photo.

export interface Category {
  /** Stable slug stored in localStorage. Don't rename without a migration. */
  slug: string;
  /** Human-readable label shown in the Settings checkbox grid. */
  label: string;
  /** Query string sent to Unsplash's `/photos/random?query=…` endpoint. */
  query: string;
}

export const CATEGORIES: readonly Category[] = [
  { slug: "nature",       label: "Nature",       query: "nature" },
  { slug: "mountains",    label: "Mountains",    query: "mountains" },
  { slug: "ocean",        label: "Ocean",        query: "ocean" },
  { slug: "forest",       label: "Forest",       query: "forest" },
  { slug: "sky",          label: "Sky",          query: "sky clouds" },
  { slug: "minimal",      label: "Minimal",      query: "minimal" },
  { slug: "abstract",     label: "Abstract",     query: "abstract" },
  { slug: "architecture", label: "Architecture", query: "architecture" },
  { slug: "cityscape",    label: "Cityscape",    query: "cityscape" },
  { slug: "space",        label: "Space",        query: "space stars" },
  { slug: "texture",      label: "Texture",      query: "texture" },
  { slug: "art",          label: "Art",          query: "art" },
] as const;

/** Quick lookup by slug. Used by the orchestrator to translate a
 *  user-saved slug into the actual Unsplash query at fetch time. */
const BY_SLUG = new Map(CATEGORIES.map((c) => [c.slug, c]));

export function categoryBySlug(slug: string): Category | undefined {
  return BY_SLUG.get(slug);
}

/**
 * Pick a random category from the user's selection. Returns
 * `undefined` if the selection is empty (caller should fall back to
 * Random mode rather than silently picking).
 */
export function pickRandomCategory(
  selectedSlugs: readonly string[],
): Category | undefined {
  const valid = selectedSlugs
    .map((s) => BY_SLUG.get(s))
    .filter((c): c is Category => c !== undefined);
  if (valid.length === 0) return undefined;
  return valid[Math.floor(Math.random() * valid.length)];
}
