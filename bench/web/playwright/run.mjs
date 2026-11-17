// Tester Army's web benchmark with Playwright, written the way a Playwright user writes it: the
// installed Chrome (headless, a 1280 x 800 window, as every tool here runs), locators with
// Playwright's auto-waiting, and expect-style checks done with locator waits (playwright-core
// has no test runner: each run is one scenario in this process, as the other tools' runs are).
//
//   node run.mjs <flow>.yaml    scenarios.mjs's port of bench/web-flows/<flow>.yaml
//   node run.mjs --version
import { chromium } from 'playwright-core';
import { readFileSync } from 'node:fs';
import { basename } from 'node:path';
import { SCENARIOS } from './scenarios.mjs';
import { phone } from '../phone.mjs';

const arg = process.argv[2] ?? '';
if (arg === '--version') {
  const v = JSON.parse(readFileSync(new URL('./node_modules/playwright-core/package.json', import.meta.url), 'utf8')).version;
  console.log(`playwright ${v}`);
  process.exit(0);
}
const name = basename(arg).replace(/\.yaml$/, '');
const scenario = SCENARIOS[name];
if (!scenario) {
  console.error(`no Playwright test of ${name}`);
  process.exit(2);
}
const BASE = process.env.VIRTUOSO_BENCH_WEB_URL ?? 'http://127.0.0.1:4280';
const browser = await chromium.launch({
  channel: process.env.VIRTUOSO_CHROME ? undefined : 'chrome',
  executablePath: process.env.VIRTUOSO_CHROME || undefined,
  headless: true,
});
let failed = false;
try {
  // A fresh context is clearState: no cookies or storage from a run before.
  // A phone (VIRTUOSO_BENCH_EMULATE): Playwright's device emulation, with the shared values.
  const p = phone();
  const context = p
    ? await browser.newContext({ viewport: { width: p.width, height: p.height }, deviceScaleFactor: p.scale, isMobile: true, hasTouch: true, userAgent: p.userAgent })
    : await browser.newContext({ viewport: null });
  const page = await context.newPage();
  if (!p) await page.setViewportSize({ width: 1280, height: 800 });
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
