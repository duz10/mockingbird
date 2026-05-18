# ADR-0025: Optional remote ambient background — Unsplash provider

- **Status:** Accepted
- **Date:** 2026-05-19
- **Deciders:** Dustin (project lead), code-puppy (implementor)

## Context

Mockingbird is a **local-first** voice dictation app. AGENTS.md
binding principle #4 says "No telemetry. Crashes log locally. Never
phone home." Principle #5 says "Cross-platform from day one." ADR
0021 introduced the only network dependency to date (Ollama at
`http://localhost:11434`) — same machine, same trust boundary as the
app process itself.

A user request landed for an **ambient photo background** in the main
window — visual richness comparable to Wispr Flow's design without
giving up the privacy posture. Implementing this without a remote
photo source meant either:

- Bundling ~100 MB of photos in the app (size, taste-coupling),
- Asking the user to point at a local folder (UX friction, the
  point of "ambient" is variety the user doesn't curate), or
- Reaching out to a remote photo service.

The third option is the only one that delivers the experience but
introduces the first **non-localhost network call** in the app —
a real architectural shift worth recording, even though the surface
area is small. This ADR captures the constraints, the chosen
provider, the privacy contract, and the swap path.

## Decision

**We will offer an optional, opt-in, BYO-key Unsplash photo
background through a provider-abstracted component module at
`ui/src/components/UnsplashBackground/`.**

Concretely:

1. **Off by default.** First-run users see the warm-blob ambient
   bg from ADR 0023 / DL-W2. No network call happens until the
   user explicitly:
   1. Enters their own Unsplash access key in Settings.
   2. Toggles "Show background photo" on.
2. **Bring-your-own key.** The app **does not ship credentials**.
   The dev creds in `.env.local` are for local development only
   (gitignored). End users get a free Unsplash demo key at
   <https://unsplash.com/developers> — the cost on Mockingbird's
   side is "open Settings, paste key".
3. **Provider abstraction inside the module.** All Unsplash-
   specific code (request shape, response normalisation, UTM
   attribution, download trigger) lives in
   `UnsplashBackground/fetchPhoto.ts`. The component consumes a
   normalised `Photo` type. Swapping in Pexels, Lorem Picsum, a
   user-folder watcher, or a local `models/photos/` directory is
   a one-file replacement.
4. **Zero outbound traffic with the feature off.** The component
   short-circuits if `prefs.enabled === false || !prefs.apiKey`.
   No prefetch, no DNS lookups, no `<img>` tags.
5. **Compliance pre-cleared.** All six Unsplash API guideline
   items audited green pre-ship (hotlink, download trigger,
   no logo or similar name, visually distinct, accurate dev-
   portal metadata, photographer + Unsplash attribution with
   UTM). Audit log in `docs/cleanup/unsplash-background-ship.md`.
6. **CSP-fenced.** Tauri CSP in `tauri.conf.json` extended only
   to allow `api.unsplash.com` (connect-src) and
   `images.unsplash.com` (img-src). No other Unsplash subdomains,
   no third-party CDNs.

## Consequences

### Positive

- **Real visual richness without bundling photos** — every user
  gets fresh imagery, curated to their taste via category picker.
- **Provider abstraction means the feature isn't Unsplash-locked.**
  A future switch to Pexels (similar API), a local-folder watcher,
  or a fully-offline shipped-asset rotation requires changing one
  file (`fetchPhoto.ts`); the rest of the component is provider-
  agnostic.
- **Design-system override pattern documented in LESSONS**
  (2026-05-19 "glass token override beats per-component rewrites")
  generalises beyond this feature — any future "mode toggle" that
  needs to flip how the glass system reads should use the same
  `:root[data-*]` token-override scope, not per-component rewrites.
- **Privacy posture stays defensible.** Off by default, BYO key,
  no Mockingbird-side telemetry, no creds in the binary, audit
  log in tree.

### Negative

- **First remote-internet network dep in the app.** Even though
  it's opt-in, it expands the network attack surface from
  `localhost:11434` to include `*.unsplash.com`. Mitigated by:
  CSP fencing, no creds in binary, fire-and-forget download
  pings, no user PII sent.
- **End users must enter a key.** Friction relative to "just
  works". Accepted because shipping creds (even rotating ones)
  in a local app is irresponsible — the key would be extractable
  from the binary and inevitably abused. BYO-key keeps the trust
  boundary clear: the user's rate limit is the user's problem.
- **Rate-limit math depends on user's tier.** At demo tier (50
  req/hr) the 5-min rotation uses ~26% of the cap. If a user is
  also using their Unsplash key elsewhere, they share the budget.
  Documented in the Settings card help text and the ship doc.
- **Failure modes silently degrade.** If Unsplash is down,
  if the key is revoked, if the user is offline — the component
  logs and falls back to the no-photo state. No nag toasts,
  no retry storms. Tradeoff: less observability for the user;
  preferred over jank.

### Neutral

- **The `[data-photo-bg]` root attribute** is now a contract — any
  future visual feature that needs to know "is a photo bg
  currently active" can read it. Same pattern, single source of
  truth. Not Unsplash-specific.
- **Dev creds live in `.env.local`** (gitignored, verified). The
  app code never reads env vars for Unsplash credentials — that
  was a deliberate choice. The dev creds exist only so a
  contributor can hand-test without pasting into the UI on every
  launch. Production users always go through Settings.

## Alternatives considered

- **Bundle a local photo set.** Rejected: app size balloons,
  taste-coupled to whoever curated the set, no variety. The
  "ambient" experience is gone.
- **Watch a user folder.** Rejected as the primary UX (kept as a
  future provider option per the abstraction). Users would have
  to curate folders, manage size, deal with EXIF/orientation —
  not the ambient experience.
- **Ship with creds embedded.** Rejected outright: extractable
  from the binary, single shared rate limit gets DoS'd by any
  abuse, key rotation becomes a release-blocking concern. Worse
  in every way than BYO-key.
- **Proxy through a Mockingbird-hosted backend.** Rejected: this
  is a local-first app with no backend. Introducing one to mask
  Unsplash credentials would violate principles #1, #4, and #5.
  If we ever do have a backend, this can be revisited.
- **Pexels instead of Unsplash.** Equivalent API, similar terms.
  Unsplash chosen for breadth of editorial photography. The
  provider abstraction means we can swap if Unsplash terms ever
  become hostile to our use case.
- **Live wallpaper / video background.** Rejected: bandwidth, GPU
  cost, visual fatigue. Photos rotate every 5 min and pan slowly;
  the design intent is "ambient, not attention-grabbing".

## Cross-references

- **PLAN sections:** None — this is a lateral feature add, not
  phase work. Closest related principles are AGENTS.md binding
  principles #3 (Layers are replaceable), #4 (No telemetry), #5
  (Cross-platform).
- **Related ADRs:**
  - ADR 0021 (sync cleanup provider — Ollama / localhost network
    pattern, precedent for "optional network feature gated behind
    user opt-in").
  - ADR 0023 (design language v1 — defines the `--glass-tint-*`
    token system that this feature overrides via `[data-photo-
    bg]`).
- **Ship + compliance log:**
  `docs/cleanup/unsplash-background-ship.md`.
- **LESSONS:** 2026-05-19 "glass token override beats per-component
  rewrites" and "z-index:0 photo background trapped in-flow text".
- **bd:** mb-biy (ship issue, closed).
