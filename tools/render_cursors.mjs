// Renders the gold Washoi cursors (wash-fps `client/styles/cursors/*-46.svg`,
// copied to `assets/cursors/*.svg`) to PNGs at each display scale the native
// client picks from (`src/client/cursors.rs`). Run from the repository root
// with Playwright's Chromium: `node tools/render_cursors.mjs`.
import { createRequire } from "node:module";
import { readFileSync } from "node:fs";

// Playwright as installed globally or beside the script.
const { chromium } = createRequire(import.meta.url)("playwright");

const NAMES = ["arrow", "hand", "disabled", "text"];
const SCALES = [1, 1.25, 1.5, 1.75, 2, 2.5, 3];
const CSS_SIZE = 46;

const browser = await chromium.launch();
for (const scale of SCALES) {
  const page = await browser.newPage({ viewport: { width: CSS_SIZE, height: CSS_SIZE }, deviceScaleFactor: scale });
  for (const name of NAMES) {
    const svg = readFileSync(`assets/cursors/${name}.svg`, "utf8");
    await page.setContent(
      `<style>html,body{margin:0;background:transparent}</style>` +
        `<img src="data:image/svg+xml;base64,${Buffer.from(svg).toString("base64")}" width="${CSS_SIZE}" height="${CSS_SIZE}">`,
    );
    await page.waitForFunction(() => document.images[0].complete);
    const tag = String(Math.round(scale * 100));
    await page.screenshot({
      path: `assets/cursors/${name}-${tag}.png`,
      omitBackground: true,
      clip: { x: 0, y: 0, width: CSS_SIZE, height: CSS_SIZE },
    });
  }
  await page.close();
}
await browser.close();
