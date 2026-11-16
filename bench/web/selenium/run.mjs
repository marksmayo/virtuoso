// Tester Army's web benchmark with Selenium WebDriver, written the way a Selenium user writes it:
// the installed Chrome (headless, a 1280 x 800 window, as every tool here runs) through
// chromedriver (Selenium Manager finds the one for that Chrome), explicit waits on each element.
//
//   node run.mjs <flow>.yaml    scenarios.mjs's port of bench/web-flows/<flow>.yaml
//   node run.mjs --version
import { Builder } from 'selenium-webdriver';
import chrome from 'selenium-webdriver/chrome.js';
import { readFileSync } from 'node:fs';
import { basename } from 'node:path';
import { SCENARIOS } from './scenarios.mjs';

const arg = process.argv[2] ?? '';
if (arg === '--version') {
  const v = JSON.parse(readFileSync(new URL('./node_modules/selenium-webdriver/package.json', import.meta.url), 'utf8')).version;
  console.log(`selenium ${v}`);
  process.exit(0);
}
const name = basename(arg).replace(/\.yaml$/, '');
const scenario = SCENARIOS[name];
if (!scenario) {
  console.error(`no Selenium test of ${name}`);
  process.exit(2);
}
const BASE = process.env.VIRTUOSO_BENCH_WEB_URL ?? 'http://127.0.0.1:4280';
const options = new chrome.Options().addArguments('--headless=new', '--window-size=1280,800');
if (process.env.VIRTUOSO_CHROME) options.setChromeBinaryPath(process.env.VIRTUOSO_CHROME);
// A new session's profile is a fresh one: clearState.
const driver = await new Builder().forBrowser('chrome').setChromeOptions(options).build();
let failed = false;
try {
  await scenario(driver, (slug) => driver.get(`${BASE}/e/${slug}`));
  console.log(`${name}: passed`);
} catch (e) {
  failed = true;
  console.error(`${name}: FAILED\n${e?.message ?? e}`);
} finally {
  await driver.quit();
}
process.exit(failed ? 1 : 0);
