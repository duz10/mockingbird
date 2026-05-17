// Single-key translation lookup. v1 ships English-only; the seam is
// here so a future contribution can drop in `fr.json` + a language
// picker in Settings without touching call sites.

import en from "./en.json";

type Strings = typeof en;
type Key = keyof Strings;

const STRINGS: Record<string, string> = en as Record<string, string>;

/** Lookup a translated string by key. Returns the key itself on miss
 *  (visible in dev so missing keys surface fast). */
export function t(key: Key | (string & {})): string {
  return STRINGS[key as string] ?? key;
}

/** Format a count + label, plural-aware in the simplest possible way. */
export function plural(
  n: number,
  singular: string,
  pluralWord = `${singular}s`,
): string {
  return n === 1 ? `1 ${singular}` : `${n} ${pluralWord}`;
}
