// Rasterize an Open Graph image (1200x630) for the Mockingbird Pages site.
//
// Renders the canonical brand mark (assets/icons/mockingbird.svg) centered on
// a solid accent-color background, using Playwright's bundled Chromium for
// a deterministic SVG -> PNG path. Output: docs/site/og-image.png.
//
// Run from repo root:
//   node scripts/dev/build-og-image.mjs
//
// Requires ui/node_modules/playwright (already a repo dev dep).

import { chromium } from "../../ui/node_modules/playwright/index.mjs";
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(HERE, "..", "..");

const SVG_PATH = resolve(REPO_ROOT, "assets/icons/mockingbird.svg");
const OUT_PATH = resolve(REPO_ROOT, "docs/site/og-image.png");

const WIDTH = 1200;
const HEIGHT = 630;
const ACCENT = "#944730";
// Logo target height ~50% of canvas. SVG is 1:1 so width matches.
const LOGO_PX = 315;

const svgMarkup = readFileSync(SVG_PATH, "utf8");

// Strip the XML prolog so it can be inlined inside HTML.
const inlineSvg = svgMarkup.replace(/<\?xml[^>]*\?>\s*/u, "");

const html = `<!doctype html>
<html><head><meta charset="utf-8"><style>
  html, body { margin: 0; padding: 0; }
  body {
    width: ${WIDTH}px;
    height: ${HEIGHT}px;
    background: ${ACCENT};
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .mark { width: ${LOGO_PX}px; height: ${LOGO_PX}px; display: block; }
  .mark svg { width: 100%; height: 100%; display: block; }
</style></head>
<body><div class="mark">${inlineSvg}</div></body></html>`;

mkdirSync(dirname(OUT_PATH), { recursive: true });

const browser = await chromium.launch();
try {
  const ctx = await browser.newContext({
    viewport: { width: WIDTH, height: HEIGHT },
    deviceScaleFactor: 1,
  });
  const page = await ctx.newPage();
  await page.setContent(html, { waitUntil: "load" });
  const buf = await page.screenshot({
    type: "png",
    omitBackground: false,
    clip: { x: 0, y: 0, width: WIDTH, height: HEIGHT },
  });
  writeFileSync(OUT_PATH, buf);
  console.log(`Wrote ${OUT_PATH} (${buf.length} bytes, ${WIDTH}x${HEIGHT})`);
} finally {
  await browser.close();
}
