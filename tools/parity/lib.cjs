// Playwright driver for the local SHAG probe server (the browser-probe skill's
// cdp.mjs, on Linux Chromium with SwiftShader).
const { chromium } = require('playwright');
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
async function open({ width = 1280, height = 720 } = {}) {
  const browser = await chromium.launch({ args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader', '--ignore-gpu-blocklist', '--enable-webgl'] });
  const page = await browser.newPage({ viewport: { width, height } });
  const logs = [];
  page.on('console', (m) => { if (m.type() === 'error' || m.type() === 'warning') logs.push(`${m.type()}: ${m.text().slice(0, 300)}`); });
  page.on('pageerror', (e) => logs.push('EXCEPTION: ' + e.message.slice(0, 300)));
  const ev = (fn, arg) => page.evaluate(fn, arg);
  const command = (cmd) => page.evaluate(async (c) => {
    const { CommandRegistry } = await import('/src/engine/console/CommandRegistry.ts');
    try { return JSON.stringify(await CommandRegistry.getInstance().executeChecked(c)); } catch (e) { return 'error: ' + e.message; }
  }, cmd);
  return { browser, page, logs, ev, command, close: () => browser.close() };
}
async function boot(b, url = 'http://127.0.0.1:3100/?renderer=webgl2') {
  await b.page.goto(url, { waitUntil: 'domcontentloaded', timeout: 120000 });
  for (let i = 0; i < 240; i++) {
    await sleep(500);
    if (await b.ev(() => !!document.querySelector('.ui-loading-screen--ready'))) break;
  }
  const start = await b.page.$('.boot__start');
  if (start) await start.click({ force: true }).catch(() => {});
  else await b.ev(async () => (await import('/src/engine/ui/LoadingScreen.ts')).LoadingScreen.forceDismiss());
  await sleep(3000);
  await b.ev(() => document.querySelector('.welcome button[type=submit]')?.click());
  for (let i = 0; i < 120; i++) {
    if (await b.ev(() => !!window.scene)) return;
    await sleep(500);
  }
  await b.page.screenshot({ path: 'noscene.png' }); console.log(b.logs.slice(-15).join('\n'));
  throw new Error('no window.scene');
}
async function waitForMap(b, seconds = 240) {
  for (let i = 0; i < seconds; i++) {
    await sleep(1000);
    if (await b.ev(() => !!window.scene?.meshes?.some((m) => m.name.startsWith('bsp_tex')) && !document.querySelector('.ui-map-loading-overlay'))) return;
  }
  throw new Error('map did not load');
}
module.exports = { open, boot, waitForMap, sleep };
