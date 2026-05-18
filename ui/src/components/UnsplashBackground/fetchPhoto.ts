// Unsplash photo fetch + attribution-side-effects.
//
// Responsibilities:
//   1. Hit /photos/random with the right filters (orientation,
//      content_filter, optional query).
//   2. Trigger the "download" pixel per Unsplash API guidelines
//      (https://help.unsplash.com/en/articles/2511315) — every time
//      we display a photo we ping `links.download_location` so the
//      photographer gets stats credit. Fire-and-forget; does NOT
//      count against our rate limit.
//   3. Return a slim `Photo` value the rest of the app can render
//      without re-mapping the full Unsplash response.

const API_BASE = "https://api.unsplash.com";

/** Slim shape we actually use. The Unsplash response has ~40
 *  fields; we keep the ones that matter for display + attribution
 *  so the value flowing through React state stays small. */
export interface Photo {
  id: string;
  /** Full-res URL — Unsplash CDN serves these directly with no auth. */
  url: string;
  /** Page link (used by the attribution tooltip). */
  htmlLink: string;
  /** Average color from Unsplash — used as a placeholder bg while
   *  the bitmap is decoding so we don't flash blank. */
  color: string;
  /** Human-readable description (`description` falls back to
   *  `alt_description`). Empty string when neither is set. */
  description: string;
  photographerName: string;
  photographerUsername: string;
  photographerLink: string;
  /** Endpoint we GET to register a download event. Per Unsplash
   *  guidelines this MUST be hit when we display the photo. */
  downloadLocation: string;
}

export interface FetchOptions {
  /** Unsplash access key (Client-ID). Required; if blank the
   *  caller should not invoke this function in the first place. */
  accessKey: string;
  /** Optional search query — pass `undefined` for pure random. */
  query?: string;
  /** AbortSignal so the caller can cancel an in-flight request on
   *  unmount or toggle-off. */
  signal?: AbortSignal;
}

/** Hit /photos/random. Throws on non-2xx; caller decides whether to
 *  swallow (e.g. surface a tray toast vs. fall back to no background). */
export async function fetchRandomPhoto(opts: FetchOptions): Promise<Photo> {
  const params = new URLSearchParams({
    orientation: "landscape",
    content_filter: "high",
  });
  if (opts.query) params.set("query", opts.query);

  const res = await fetch(`${API_BASE}/photos/random?${params}`, {
    headers: {
      Authorization: `Client-ID ${opts.accessKey}`,
      "Accept-Version": "v1",
    },
    signal: opts.signal,
  });
  if (!res.ok) {
    throw new Error(`unsplash ${res.status}: ${await res.text()}`);
  }
  const raw = (await res.json()) as UnsplashRandomResponse;
  return normalise(raw);
}

/**
 * Fire-and-forget download-event ping. Called by the consumer after
 * the image actually loads into the DOM. We deliberately do NOT
 * await this — failures here are tracking-only, not user-visible,
 * and we must never block the visual on an analytics call.
 */
export function triggerDownload(photo: Photo, accessKey: string): void {
  if (!photo.downloadLocation || !accessKey) return;
  void fetch(photo.downloadLocation, {
    headers: { Authorization: `Client-ID ${accessKey}` },
  }).catch(() => {
    // Swallowed — see function doc. Attribution-tracking is a
    // best-effort courtesy, not correctness.
  });
}

/* ------------------------------------------------------------------ */
/* Internal — the raw API response shape we care about + normaliser.   */
/* ------------------------------------------------------------------ */

interface UnsplashRandomResponse {
  id: string;
  color?: string;
  description?: string | null;
  alt_description?: string | null;
  urls: { regular: string; full?: string };
  links: { html: string; download_location: string };
  user: {
    name: string;
    username: string;
    links: { html: string };
  };
}

function normalise(raw: UnsplashRandomResponse): Photo {
  return {
    id: raw.id,
    // `regular` is ~1080px wide — plenty for a background, much
    // smaller than `full`. Saves bandwidth + decode time.
    url: raw.urls.regular,
    htmlLink: raw.links.html,
    color: raw.color ?? "#1a1a1a",
    description: raw.description ?? raw.alt_description ?? "",
    photographerName: raw.user.name,
    photographerUsername: raw.user.username,
    photographerLink: raw.user.links.html,
    downloadLocation: raw.links.download_location,
  };
}

/**
 * Build the photographer / unsplash links with the UTM params
 * Unsplash's API guidelines require. Centralised so attribution
 * markup can't accidentally drop them.
 */
export function withUtm(url: string): string {
  const sep = url.includes("?") ? "&" : "?";
  return `${url}${sep}utm_source=mockingbird&utm_medium=referral`;
}

/**
 * Suggest a dark-scrim overlay opacity (0..1) for a photo based on
 * its perceived luminance. Bright photos (white walls, sky, snow,
 * book pages) need a scrim to keep cream-on-cream PageHeader text
 * legible; dark photos don't, and over-darkening them would muddy
 * the design.
 *
 * Caller composes with the user's manual overlay pref via Math.max
 * so the user's slider acts as a FLOOR — they can always add more
 * darkening, but the system guarantees a minimum based on photo
 * luminance.
 *
 * Input is the average colour Unsplash returns in `photo.color`
 * (e.g. "#E5E5E5"). Luminance is computed with the Rec. 709 / sRGB
 * coefficients. Below 0.35 luminance we add nothing; above 0.75 we
 * apply the cap; linear ramp in between. Numbers tuned empirically
 * against the qa-kitten Japanese-street / bookshelf / orangutan
 * fixtures — feel free to retune if the headers still wash on a
 * specific photo class.
 */
export function autoOverlayForColor(hexColor: string | undefined): number {
  if (!hexColor) return 0;
  const hex = hexColor.replace("#", "");
  if (hex.length !== 6) return 0;
  const r = parseInt(hex.slice(0, 2), 16);
  const g = parseInt(hex.slice(2, 4), 16);
  const b = parseInt(hex.slice(4, 6), 16);
  if (!Number.isFinite(r) || !Number.isFinite(g) || !Number.isFinite(b)) {
    return 0;
  }
  // Rec. 709 luminance, normalised 0..1.
  const luminance = (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255;

  const LOW = 0.35;   // below this → no auto-scrim
  const HIGH = 0.75;  // above this → full auto-scrim
  const CAP = 0.45;   // max auto-scrim opacity

  if (luminance <= LOW) return 0;
  if (luminance >= HIGH) return CAP;
  return ((luminance - LOW) / (HIGH - LOW)) * CAP;
}
