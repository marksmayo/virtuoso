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
const browser = await puppeteer.launch({
  ...(process.env.VIRTUOSO_CHROME ? { executablePath: process.env.VIRTUOSO_CHROME } : { channel: 'chrome' }),
  headless: true,
  defaultViewport: { width: 1280, height: 800 },
  args: ['--window-size=1280,800'],
});
let failed = false;
try {
  // A fresh browser context is clearState: no cookies or storage from a run before.
  const context = await browser.createBrowserContext();
  const page = await context.newPage();
  // A phone (VIRTUOSO_BENCH_EMULATE): Puppeteer's page.emulate, with the shared values.
  const p = phone();
  if (p) await page.emulate({ viewport: { width: p.width, height: p.height, deviceScaleFactor: p.scale, isMobile: true, hasTouch: true }, userAgent: p.userAgent });
  page.setDefaultTimeout(17000);
  await scenario(page, (slug) => page.goto(`${BASE}/e/${slug}`));
  console.log(`${name}: passed`);
} catch (e) {
  failed = true;
  console.error(`${name}: FAILED\n${e?.message ?? e}`);
} finally {
  await browser.close();
}
process.exit(failed ? 1 : 0);
