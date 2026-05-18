// Attribution pill — required by Unsplash API guidelines.
//
// Fixed bottom-right of the viewport. Default appearance is small +
// dim so it doesn't compete with app content; on hover it expands
// and reveals the full credit (photographer name + Unsplash brand,
// both linking out with the mandatory UTM params).
//
// Why a custom tooltip and not the native `title` attribute:
// - Native tooltips are slow (700ms delay), styled by the OS, and
//   can't render links inside. We need clickable links to satisfy
//   the attribution policy.

import type { Photo } from "./fetchPhoto";
import { withUtm } from "./fetchPhoto";

import styles from "./styles.module.css";

interface AttributionProps {
  photo: Photo;
}

export function Attribution({ photo }: AttributionProps) {
  // Truncate to keep the pill compact at rest. The full description
  // is in the hover tooltip if curiosity strikes.
  const titleShort =
    photo.description.length > 38
      ? `${photo.description.slice(0, 36)}…`
      : photo.description || "Untitled";

  return (
    <div className={styles.attribution} aria-label="Photo credit">
      <UnsplashGlyph />
      <span className={styles.attributionTitle}>{titleShort}</span>

      {/*
        Hover popover with the full credit. CSS handles the
        show/hide via `:hover` on `.attribution` — no JS state,
        no flicker, accessible via keyboard focus too (the inner
        links become focusable, which keeps the popover open).
      */}
      <div className={styles.attributionPop} role="tooltip">
        {photo.description ? (
          <p className={styles.attributionPopTitle}>{photo.description}</p>
        ) : null}
        <p className={styles.attributionPopCredit}>
          Photo by{" "}
          <a
            href={withUtm(photo.photographerLink)}
            target="_blank"
            rel="noreferrer noopener"
          >
            {photo.photographerName}
          </a>{" "}
          on{" "}
          <a
            href={withUtm("https://unsplash.com")}
            target="_blank"
            rel="noreferrer noopener"
          >
            Unsplash
          </a>
        </p>
        <p className={styles.attributionPopHint}>
          <a
            href={withUtm(photo.htmlLink)}
            target="_blank"
            rel="noreferrer noopener"
          >
            View this photo →
          </a>
        </p>
      </div>
    </div>
  );
}

/**
 * Tiny Unsplash wordmark. Inlined as SVG so we don't ship a network
 * dependency for the brand asset. Sized off the parent's font.
 */
function UnsplashGlyph() {
  return (
    <svg
      className={styles.attributionGlyph}
      width="14"
      height="14"
      viewBox="0 0 32 32"
      aria-hidden="true"
      focusable="false"
    >
      {/* Unsplash camera-shutter mark, simplified — two stacked
          rectangles, the lower one slightly wider. Matches the
          official mark closely enough for a 14px footer glyph. */}
      <path
        d="M10 9h12v7H10V9zm-3 9h18v8H7v-8z"
        fill="currentColor"
      />
    </svg>
  );
}
