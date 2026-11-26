// Tester Army's web benchmark with Puppeteer, written the way a Puppeteer user writes it: the
// installed Chrome (headless, a 1280 x 800 window, as every tool here runs) and Puppeteer's
// locators, which wait for an element to be visible, stable and enabled before acting.
//
//   node run.mjs <flow>.yaml    scenarios.mjs's port of bench/web-flows/<flow>.yaml
//   node run.mjs --version
import puppeteer from 'puppeteer-core';
import { readFileSync, existsSync } from 'node:fs';
import { basename } from 'node:path';
import { SCENARIOS } from './scenarios.mjs';
import { phone } from '../phone.mjs';
import { androidChrome, androidSerial } from '../android.mjs';
import { puppeteerLaunch } from '../browser.mjs';

const arg = process.argv[2] ?? '';
if (arg === '--version') {
  const v = JSON.parse(readFileSync(new URL('./node_modules/puppeteer-core/package.json', import.meta.url), 'utf8')).version;
  console.log(`puppeteer ${v}`);
  process.exit(0);
}
const name = basename(arg).replace(/\.yaml$/, '');
const scenario = SCENARIOS[name];
if (!scenario) {
  console.error(`no Puppeteer test of ${name}`);
  process.exit(2);
}
const BASE = process.env.VIRTUOSO_BENCH_WEB_URL ?? 'http://127.0.0.1:4280';
// On an Android device (VIRTUOSO_BENCH_ANDROID): its Chrome, connected to through adb.
const serial = androidSerial();
const device = serial ? await androidChrome(serial) : null;
const browser = device
  ? await puppeteer.connect({ browserURL: device.http, defaultViewport: null })
  : // Chrome, or in a Firefox field (VIRTUOSO_BENCH_WEB_BROWSER) Firefox over WebDriver BiDi.
    await puppeteer.launch(puppeteerLaunch());
let failed = false;
try {
  // A fresh browser context is clearState: no cookies or storage from a run before. On a
  // device (Chrome there has no such contexts) a new tab, the site's data cleared first.
  const page = device ? await browser.newPage() : await (await browser.createBrowserContext()).newPage();
  if (device) {
    const cdp = await page.createCDPSession();
    await cdp.send('Storage.clearDataForOrigin', { origin: new URL(BASE).origin, storageTypes: 'all' });
  }
  // A phone (VIRTUOSO_BENCH_EMULATE): Puppeteer's page.emulate, with the shared values.
  const p = phone();
  if (p) await page.emulate({ viewport: { width: p.width, height: p.height, deviceScaleFactor: p.scale, isMobile: true, hasTouch: true }, userAgent: p.userAgent });
  page.setDefaultTimeout(17000);
  await scenario(page, (slug) => page.goto(`${BASE}/e/${slug}`));
  console.log(`${name}: passed`);
} catch (e) {
  failed = true;
  // The step that failed: its line in the scenarios.
  const at = String(e?.stack ?? '').split('\n').find((l) => l.includes('scenarios.mjs'))?.trim();
  console.error(`${name}: FAILED\n${e?.message ?? e}${at ? `\n${at}` : ''}`);
} finally {
  if (device) {
    await browser.disconnect();
    device.close();
  } else {
    await browser.close();
  }
}
process.exit(failed ? 1 : 0);
