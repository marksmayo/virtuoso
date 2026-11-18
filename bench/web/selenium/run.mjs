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
import { phone } from '../phone.mjs';
import { androidSerial, deviceChromeVersion, quietChrome, reverseApp } from '../android.mjs';

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
// A phone (VIRTUOSO_BENCH_EMULATE): chromedriver's mobileEmulation, with the shared values.
const p = phone();
if (p) options.setMobileEmulation({ deviceMetrics: { width: p.width, height: p.height, pixelRatio: p.scale, touch: true }, userAgent: p.userAgent });
// On an Android device (VIRTUOSO_BENCH_ANDROID): chromedriver's Android mode drives the
// device's Chrome; its chromedriver the one for that Chrome's version (Selenium Manager finds
// it), the app's port reversed so the device reaches it.
const serial = androidSerial();
let driverOptions = options;
if (serial) {
  quietChrome(serial);
  reverseApp(serial);
  driverOptions = new chrome.Options().androidChrome().androidDeviceSerial(serial);
  driverOptions.setBrowserVersion(deviceChromeVersion(serial));
}
// A new session's profile is a fresh one: clearState (on a device, chromedriver clears Chrome's
// data as it starts it).
const driver = await new Builder().forBrowser('chrome').setChromeOptions(driverOptions).build();
let failed = false;
try {
  // On GitHub's Windows runners the first navigation of a new session now and then never starts
  // (the page stays Chrome's data:, start page): it is asked for again. Its time is Selenium's.
  const open = async (slug) => {
    const url = `${BASE}/e/${slug}`;
    await driver.get(url);
    if (!(await driver.getCurrentUrl()).startsWith(BASE)) await driver.get(url);
  };
  await scenario(driver, open);
  console.log(`${name}: passed`);
} catch (e) {
  failed = true;
  console.error(`${name}: FAILED\n${e?.message ?? e}`);
  // Where it was: the page's address, title and opening text.
  try {
    const url = await driver.getCurrentUrl();
    const title = await driver.getTitle();
    const body = await driver.executeScript('return document.body ? document.body.innerText.slice(0, 300) : "(no body)"');
    console.error(`at ${url} (${title}): ${JSON.stringify(body)}`);
  } catch (e2) {
    console.error(`page not readable: ${e2?.message ?? e2}`);
  }
} finally {
  await driver.quit();
}
process.exit(failed ? 1 : 0);
