// UnsplashBackground — full-bleed photo background for the main app
// window, with a slow pan animation and clock-aligned rotation.
//
// Mounts ONCE at the app root (above <App>'s shell, behind every
// page). Position: fixed; inset: 0; z-index: 0; pointer-events: none.
// The app shell sits on top with its own stacking context, so the
// background is purely decorative — no click capture, no focus traps.
//
// ## Lifecycle
//   1. Read prefs from localStorage on mount.
//   2. If disabled OR no API key → render nothing.
//   3. Fetch a photo. While the fetch is in flight we render the
//      Unsplash-provided `color` as a solid backdrop so we never
//      flash blank.
//   4. Schedule the next swap to land on the next clock-aligned
//      5-minute boundary (`:00, :05, :10, …`). Each rotation:
//        a. ~5s before the boundary, prefetch the next photo so the
//           bitmap is decoded by swap time.
//        b. At the boundary, crossfade (2s opacity) from current →
//           next while both pan independently.
//
// ## Why two image layers and not background-image swaps
//   - `<img>` gives us `onLoad` for crossfade timing.
//   - object-fit: cover handles aspect ratio without math.
//   - We can run WAAPI animations per-element for the pan with
//     dynamic start/end offsets — easier than parametrising CSS
//     keyframes through CSS variables.
//
// ## Rate-limit math
//   - Demo Unsplash apps: 50 req/hr.
//   - 5-minute rotation = 12 req/hr, well under the cap. The
//     "trigger download" pings don't count against the limit.
//
// ## A note on z-index
//   The recording overlay is a SEPARATE Tauri window, so this
//   background has zero interaction with it. Inside the main window,
//   the app shell uses its own z-index stacking from the design
//   system — the background sits at z:0, app at z:1+.

import { useEffect, useRef, useState } from "react";

import { Attribution } from "./Attribution";
import {
  autoOverlayForColor,
  fetchRandomPhoto,
  triggerDownload,
  type Photo,
} from "./fetchPhoto";
import { pickRandomCategory } from "./categories";
import { getPrefs, PREFS_EVENT, type BackgroundPrefs } from "./prefs";

import styles from "./styles.module.css";

/** 5 minutes in ms — exported for tests. */
export const ROTATION_MS = 5 * 60 * 1000;
/** How long the crossfade between old and new photo takes. Matches
 *  the CSS `transition: opacity` duration on `.imgLayer`. */
const FADE_MS = 2000;
/** How early to start prefetching the next photo before the swap
 *  boundary. Just needs to cover network + decode for ~1080px JPEG. */
const PREFETCH_LEAD_MS = 8000;

export function UnsplashBackground() {
  // Prefs are reactive: we re-read on the `PREFS_EVENT` custom event
  // (fired from `setPref()` whenever Settings UI changes anything).
  // This avoids a global store dependency for what is otherwise an
  // isolated component.
  const [prefs, setPrefs] = useState<BackgroundPrefs>(() => getPrefs());

  useEffect(() => {
    const onChange = () => setPrefs(getPrefs());
    window.addEventListener(PREFS_EVENT, onChange);
    // Also listen to cross-tab `storage` events for completeness —
    // not strictly necessary in a single-window Tauri app, but
    // costs nothing and helps the preview build (multiple tabs).
    window.addEventListener("storage", onChange);
    return () => {
      window.removeEventListener(PREFS_EVENT, onChange);
      window.removeEventListener("storage", onChange);
    };
  }, []);

  const active = prefs.enabled && prefs.apiKey.length > 0;

  // Flag the document root while a photo is on screen. The design
  // system's glass tokens (--glass-tint-*) are tuned to refract
  // against the warm ambient blob; against a real photo they wash
  // out. The `[data-photo-bg]` scope in materials-v2.css swaps the
  // tints to dark-opaque values so every glass surface (sidebar,
  // cards, empty states, …) stays readable WITHOUT any per-component
  // CSS change. Single source of truth for the photo-mode adjustment.
  useEffect(() => {
    const root = document.documentElement;
    if (active) {
      root.dataset.photoBg = "active";
    } else {
      delete root.dataset.photoBg;
    }
    return () => {
      delete root.dataset.photoBg;
    };
  }, [active]);

  // The visible photo + the next one being preloaded for crossfade.
  // `current` is what's on screen now; `incoming` is decoded + ready
  // to crossfade in. We swap by promoting incoming → current.
  const [current, setCurrent] = useState<Photo | null>(null);
  const [incoming, setIncoming] = useState<Photo | null>(null);
  const [crossfade, setCrossfade] = useState(false);

  // Refs for cleanup. AbortController so an in-flight fetch can be
  // cancelled on unmount / toggle-off / prefs change.
  const abortRef = useRef<AbortController | null>(null);
  const timerRef = useRef<number | null>(null);

  // Main effect: when active toggles on, kick off the cycle. When
  // it toggles off, tear everything down. Re-runs on prefs changes
  // so changing categories takes effect on the next rotation.
  useEffect(() => {
    if (!active) {
      cleanup();
      setCurrent(null);
      setIncoming(null);
      return;
    }

    let cancelled = false;

    // Helper that does one fetch using the current prefs.
    const fetchOne = async (): Promise<Photo | null> => {
      const ctrl = new AbortController();
      abortRef.current = ctrl;
      const query = resolveQuery(prefs);
      try {
        const photo = await fetchRandomPhoto({
          accessKey: prefs.apiKey,
          query,
          signal: ctrl.signal,
        });
        return photo;
      } catch (err) {
        if (!(err instanceof DOMException && err.name === "AbortError")) {
          // eslint-disable-next-line no-console
          console.warn("[unsplash] fetch failed", err);
        }
        return null;
      }
    };

    // Schedule the next rotation. We use setTimeout (not setInterval)
    // because the first tick is offset to land on the next clock
    // boundary; subsequent ticks chain themselves.
    const scheduleNext = () => {
      const now = Date.now();
      const msUntilBoundary = ROTATION_MS - (now % ROTATION_MS);
      // Prefetch fires BEFORE the boundary. If we're already inside
      // the prefetch window for THIS boundary (i.e. the rotation
      // gap is shorter than the lead), fire prefetch immediately.
      const prefetchAt = Math.max(0, msUntilBoundary - PREFETCH_LEAD_MS);
      const swapAt = msUntilBoundary;

      timerRef.current = window.setTimeout(async () => {
        if (cancelled) return;
        const next = await fetchOne();
        if (cancelled || !next) {
          // If prefetch failed, try again on the next boundary
          // rather than spinning. Avoids hammering Unsplash on
          // sustained failures (e.g. user typo'd their API key).
          scheduleNext();
          return;
        }
        setIncoming(next);
        triggerDownload(next, prefs.apiKey);

        // Wait the remaining time to the actual boundary, then
        // crossfade.
        const remaining = swapAt - prefetchAt;
        timerRef.current = window.setTimeout(() => {
          if (cancelled) return;
          setCrossfade(true);
          // After the fade completes, promote incoming → current
          // and reset the crossfade state for the next cycle.
          timerRef.current = window.setTimeout(() => {
            if (cancelled) return;
            setCurrent(next);
            setIncoming(null);
            setCrossfade(false);
            scheduleNext();
          }, FADE_MS);
        }, remaining);
      }, prefetchAt);
    };

    // Initial fetch — render the first photo as fast as possible,
    // then start the rotation loop.
    void (async () => {
      const first = await fetchOne();
      if (cancelled || !first) return;
      setCurrent(first);
      triggerDownload(first, prefs.apiKey);
      scheduleNext();
    })();

    return () => {
      cancelled = true;
      cleanup();
    };

    function cleanup() {
      abortRef.current?.abort();
      abortRef.current = null;
      if (timerRef.current !== null) {
        window.clearTimeout(timerRef.current);
        timerRef.current = null;
      }
    }
    // We deliberately key on `active` + the prefs values that
    // influence fetching. apiKey and mode trigger a full reset;
    // overlay alone doesn't (it's a pure visual layer).
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active, prefs.apiKey, prefs.mode, prefs.categories.join(",")]);

  if (!active) return null;

  // ⚠️ Attribution is rendered as a SIBLING of `.root`, not a child.
  // `.root` sets `z-index: 0` which forms its own stacking context;
  // anything nested inside (including `position: fixed` children) is
  // pinned beneath the app shell. The attribution pill needs to
  // float OVER the app so its hover popover doesn't get clipped by
  // cards. The pill itself uses `position: fixed` + high z-index so
  // it sits in the top-level stacking context.
  return (
    <>
      <div
        className={styles.root}
        aria-hidden="true"
        style={{
          backgroundColor: current?.color ?? "#1a1a1a",
          // CSS var consumed by `.overlay` — see styles.module.css.
          // Effective overlay = max(user setting, auto-scrim derived
          // from photo luminance). The user's slider is a FLOOR; if
          // the photo is bright enough that PageHeader text would
          // wash, the system adds more darkening on top automatically.
          // `as React.CSSProperties` keeps TS from complaining about
          // the custom property name.
          ["--mb-photo-overlay" as string]: Math.max(
            prefs.overlay,
            autoOverlayForColor(current?.color),
          ),
        }}
      >
        {current ? <PhotoLayer photo={current} fadeOut={crossfade} /> : null}
        {incoming ? <PhotoLayer photo={incoming} fadeIn={crossfade} /> : null}

        {/* Dark overlay — opacity from user pref (default 0). */}
        <div className={styles.overlay} />
      </div>

      {current ? <Attribution photo={current} /> : null}
    </>
  );
}

/**
 * One `<img>` layer with a randomised slow pan animation. We use the
 * Web Animations API (vs. CSS keyframes) so the start/end translate
 * values can be picked at runtime — keeps each photo feeling
 * distinct without 8 keyframe variants in CSS.
 */
function PhotoLayer({
  photo,
  fadeOut,
  fadeIn,
}: {
  photo: Photo;
  fadeOut?: boolean;
  fadeIn?: boolean;
}) {
  const imgRef = useRef<HTMLImageElement | null>(null);

  useEffect(() => {
    const el = imgRef.current;
    if (!el) return;
    // Pick one of four pan directions. The 6%/4% reach is small
    // enough that the image never reveals its edges (we oversize
    // by 12% via CSS width/height), but large enough to feel like
    // genuine movement over 5 minutes.
    const directions = [
      { fromX: -6, fromY: -4, toX: 6, toY: 4 },
      { fromX: 6, fromY: -4, toX: -6, toY: 4 },
      { fromX: -6, fromY: 4, toX: 6, toY: -4 },
      { fromX: 6, fromY: 4, toX: -6, toY: -4 },
    ];
    const pan = directions[Math.floor(Math.random() * directions.length)]!;

    const anim = el.animate(
      [
        { transform: `translate(${pan.fromX}%, ${pan.fromY}%)` },
        { transform: `translate(${pan.toX}%, ${pan.toY}%)` },
      ],
      {
        duration: ROTATION_MS,
        easing: "linear",
        fill: "forwards",
      },
    );
    return () => {
      // Cancel rather than finish — we don't want the last frame
      // jumping to the end position if the layer is being removed.
      try {
        anim.cancel();
      } catch {
        // Some browsers throw on cancel of a finished animation;
        // safe to ignore.
      }
    };
  }, [photo.id]);

  // Class assembly: base layer + optional fade direction.
  const klass = [
    styles.imgLayer,
    fadeOut ? styles.fadeOut : "",
    fadeIn ? styles.fadeIn : "",
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <img
      ref={imgRef}
      className={klass}
      src={photo.url}
      alt=""
      // `decoding=async` lets the browser decode off the main
      // thread; the crossfade orchestrator already gave it 8s of
      // prefetch lead so by swap time the bitmap is ready.
      decoding="async"
      loading="eager"
    />
  );
}

/**
 * Translate prefs into the `query` string for `/photos/random`.
 * Pure helper — no side effects, easy to unit-test if we add a
 * test file later.
 */
function resolveQuery(prefs: BackgroundPrefs): string | undefined {
  if (prefs.mode === "random") return undefined;
  const pick = pickRandomCategory(prefs.categories);
  return pick?.query;
}
