# Unsplash photo background — ship notes

**Shipped:** 2026-05-19
**Tracking issue:** `mb-biy` (closed)
**Related LESSONS:** 2026-05-19 (×2 — token override + stacking trap)

Optional ambient photo background for the main app window. Off by
default; users opt in by pasting their own Unsplash access key into
Settings → Background. Nothing ships with the app; no creds in the
binary.

---

## Unsplash API compliance audit

Audit performed 2026-05-19 against the six-item Unsplash dev
checklist. All items green; flagged items noted for future review.

| # | Requirement | Status | Evidence |
|---|---|---|---|
| 1 | **Hotlink photos** | ✅ | `fetchPhoto.ts` normalises to `raw.urls.regular` (the `images.unsplash.com` CDN URL). `tauri.conf.json` CSP allows only `img-src ... https://images.unsplash.com`. No proxying, no local caching. |
| 2 | **Trigger downloads on use** | ✅ | `triggerDownload()` pings `links.download_location` with Client-ID auth, fire-and-forget. Called on first photo + every rotation in `index.tsx`. Does NOT count against rate limit per Unsplash docs. |
| 3 | **No Unsplash logo / similar name** | ✅ | App is "Mockingbird" — nothing close to "Unsplash". Only Unsplash glyph rendered is the 14px mark INSIDE the attribution pill (correct usage: attribution, not branding). |
| 4 | **Visually distinct from Unsplash** | ✅ | No layout, palette, or chrome resemblance. Editorial M3 + Liquid Glass language; Unsplash is masonry-grid white. |
| 5 | **Accurate name + description in dev portal** | ⚠️ MANUAL | Verified by Dustin at https://unsplash.com/oauth/applications/954653 — name="Mockingbird", description accurately reflects "local-first voice dictation app; uses random photos as ambient background". Re-verify if either changes. |
| 6 | **Attribute photographer + Unsplash** | ✅ | `Attribution.tsx` renders "Photo by [Name] on Unsplash" with both as `target="_blank"` links, plus UTM params (`utm_source=mockingbird&utm_medium=referral`) via `withUtm()`. Centralised so attribution markup can't drop them. |

### Rate-limit math

Demo Unsplash tier: **50 req/hr**. Our 5-minute rotation = **12 req/hr**
+ 1 initial fetch on enable = **~13 req/hr** (≈26% of cap). Headroom
to ~3 toggle-driven manual refetches per hour. `triggerDownload`
pings to `links.download_location` are free per Unsplash docs.

If we apply for production tier (5000 req/hr) the headroom would be
~99.7%. Not required at current rotation cadence.

### Compliance gotcha — the secret key

Unsplash issues both an access key (Client-ID) and a secret key. The
**secret key is only used for OAuth user-authorization flows** (code
→ token exchange). We use Client-ID auth against public endpoints
only, so the secret key is **never referenced by application code**.
It lives in `.env.local` for completeness only; if a future feature
ever adds OAuth user flows (we have no plan to), a charter ADR comes
first.

---

## Developer credentials

**Location:** `.env.local` (gitignored — verified 2026-05-19 with
`git check-ignore -v .env.local` → matched by `.env.*` rule on
`.gitignore:21`). Doesn't even appear in `git status`.

**Discovery convention:** `.env.example` carries commented
placeholders so the convention is grep-able:

```bash
# UNSPLASH_APPLICATION_ID=
# UNSPLASH_ACCESS_KEY=
# UNSPLASH_SECRET_KEY=
```

**End-user UX:** the app does NOT read these env vars. End users
paste their own access key into Settings → Background → API key. The
key persists in `localStorage` (preview build) or DPAPI (release
build — see "Known follow-ups" below).

---

## Architecture quick reference

| File | Responsibility |
|---|---|
| `index.tsx` | Component lifecycle, clock-aligned 5-min rotation, crossfade orchestration, `[data-photo-bg]` attribute management, adaptive overlay composition |
| `fetchPhoto.ts` | Typed Unsplash API shim. Photo normaliser. `triggerDownload`, `withUtm`, `autoOverlayForColor` helpers |
| `Attribution.tsx` | Bottom-right hover-revealed credit pill. Photographer + Unsplash links with UTM. Rendered as a sibling of `.root` (NOT a child) so its popover floats above app cards |
| `prefs.ts` | localStorage seam. Reactive via custom `mockingbird:unsplash-prefs-changed` event. Migrate to DPAPI later |
| `categories.ts` | Curation slug list for the "curated" mode |
| `styles.module.css` | Component CSS only. Design-system overrides live in `materials-v2.css` |
| `materials-v2.css` | `:root[data-photo-bg]` token override scope — flips four `--glass-tint-*` tokens from cream-alpha to dark-alpha. ONE place, every glass surface adapts |
| `App.module.css` | `.shell { position: relative; z-index: 1 }` — stacking-context fix (see LESSONS 2026-05-19) |
| `primitives.module.css` | EmptyState gets glass under `[data-photo-bg]`; PageHeader title + subtitle get text-shadow halo |
| `pages/History.module.css` | `.leftPane` gets glass under `[data-photo-bg]` |
| `pages/Dictionary.module.css` | `.shell` gets glass under `[data-photo-bg]` |

---

## Release-build smoketest

The Tauri release build pipeline runs `npm --prefix ../ui run build`
via the `beforeBuildCommand` in `tauri.conf.json` — **but only under
`cargo tauri build`**. Plain `cargo build --release` does NOT trigger
a UI rebuild (this is the trap from LESSONS 2026-05-17). For this
ship we want the fresh UI bundle in the release binary, so use the
full Tauri build.

```pwsh
# From repo root
cargo tauri build --release
```

### Verification checklist

1. **Build artefact present + UI bundle fresh.** Check the bundled
   `index.html` references a CSS asset with a recent hash. The
   working bundle as of ship: `main-Ckt336ix.css` /
   `main-9NbkO0yY.js`. New builds will have new hashes — what
   matters is the bundle is from THIS build, not a stale one.
2. **CSP allows Unsplash.** Open Insights / any page, devtools
   console — no CSP violations for `api.unsplash.com` or
   `images.unsplash.com`.
3. **Settings → Background card renders.** Title "Background photo",
   help text mentions Unsplash, API-key input visible, enable toggle
   present (disabled until key entered — gated via
   `bg.enabled.needsKey` i18n string).
4. **Toggle on with a real key.** First photo loads ≤2s; attribution
   pill appears bottom-right; navigating between pages doesn't
   re-fetch (the component is mounted at App root).
5. **Attribution hover popover floats above app cards.** Mouse over
   the bottom-right pill on the History page (which has cards over
   the photo area) — popover should overlay cards, not be clipped
   beneath them.
6. **Page h1s visible.** Walk Insights / History / Dictionary / Modes
   / Settings / About — every page title and subtitle clearly legible
   regardless of photo content.
7. **Sidebar inactive nav readable.** Every nav item (not just the
   active one) is readable against the photo.
8. **Bright-photo behaviour.** Wait for or force a bright photo
   (white walls, sky). The auto-overlay should mute it noticeably
   without you touching the slider. Verify by setting overlay slider
   to 0 manually — system still darkens bright photos.
9. **Dark-photo behaviour.** Wait for or force a dark photo
   (cityscape night, forest). The auto-overlay should be near zero;
   the photo should shine through unaffected.
10. **Toggle off.** Switch Enabled off — photo disappears, all
    surfaces revert to cream-tint-over-warm-ambient design.
    `<html>` no longer has `data-photo-bg` attribute (devtools
    Elements panel).

### Rate-limit safety check

Watch the browser console for `[unsplash] fetch failed` warnings.
Sustained `403` responses mean we've blown the demo cap (50 req/hr).
At normal 5-min rotation this should never happen; if it does,
either the user has a downstream caching proxy stripping our
Client-ID or someone is hammering the toggle in dev.

---

## Known follow-ups (not blocking ship)

These are explicitly NON-BLOCKING — the feature ships in its current
state. Capture as `bd` issues if/when they become priorities.

1. **DPAPI migration for the access key.** `prefs.ts` currently
   uses `localStorage` for both preview and release. The
   `prefs.ts` module-level doc spells out the migration path: when
   release wiring lands, the API key moves to DPAPI (same pattern
   as the Claude key); the enabled/mode/categories/overlay flags
   move to the SQLite settings table. Callers don't need to change
   — they all go through `getPrefs()` / `setPref()`.

2. **Unsplash glyph in attribution pill.** `Attribution.tsx`'s
   `UnsplashGlyph()` is a simplified camera-shutter mark inlined
   as SVG. Within Unsplash brand guidelines for attribution
   contexts. If a future reviewer side-eyes it, swap for a generic
   camera icon and keep just the "on Unsplash" text link. Trivial
   change; no design system impact.

3. **ESLint v9 config migration.** Pre-existing, unrelated. `npm
   run lint` errors out before reading our files due to ESLint v9
   requiring `eslint.config.js` instead of `.eslintrc.*`. Type
   safety covered by `tsc --noEmit` in the meantime.

4. **Photo prefetch on settings change.** Currently a settings
   change (category, mode) reuses the next clock-aligned boundary
   for the swap. A user changing categories might wait up to 5
   minutes to see a category-relevant photo. If users complain,
   the easy fix is to fire an immediate refetch on prefs change
   that affects the query (apiKey, mode, categories) — the effect
   dep array in `index.tsx` already keys on these.

5. **Recording overlay is unaffected.** The recording window is a
   separate Tauri webview with its own document; `<html data-photo-
   bg>` is per-document, so the overlay never gets the override
   even when the main window has a photo on. Intentional and
   correct, but worth knowing if someone adds shared CSS later.
