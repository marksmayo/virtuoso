// Tester Army's web benchmark with Appium, on Chrome on an Android device: the way an Appium user
// tests a mobile site, a UiAutomator2 session with browserName Chrome (Appium drives the
// device's Chrome through a chromedriver it downloads for that Chrome), and the W3C WebDriver
// calls of a client library, here selenium-webdriver's, running the Selenium port's tests
// (bench/web/selenium/scenarios.mjs): mobile web through Appium is WebDriver too. The Appium
// server (the mobile benchmark's install, bench/appium) runs in this process and starts and stops
// with each run, as there.
//
//   node run.mjs <flow>.yaml    on the device VIRTUOSO_BENCH_ANDROID names
//   node run.mjs --version
import { createRequire } from 'node:module';
import { readFileSync } from 'node:fs';
import { basename } from 'node:path';
import { pathToFileURL } from 'node:url';
import { SCENARIOS } from '../selenium/scenarios.mjs';
import { androidSerial, quietChrome, reverseApp } from '../android.mjs';

const fromAppium = createRequire(new URL('../../appium/package.json', import.meta.url));
const fromSelenium = createRequire(new URL('../selenium/package.json', import.meta.url));
const PORT = 4723;

const arg = process.argv[2] ?? '';
if (arg === '--version') {
  const v = (req, p) => JSON.parse(readFileSync(req.resolve(`${p}/package.json`), 'utf8')).version;
  console.log(`appium ${v(fromAppium, 'appium')} (selenium-webdriver ${v(fromSelenium, 'selenium-webdriver')})`);
  process.exit(0);
}
const name = basename(arg).replace(/\.yaml$/, '');
const scenario = SCENARIOS[name];
if (!scenario) {
  console.error(`no Appium test of ${name}`);
  process.exit(2);
}
const serial = androidSerial();
if (!serial) {
  console.error('Appium drives Chrome on an Android device: VIRTUOSO_BENCH_ANDROID names it');
  process.exit(2);
}
const BASE = process.env.VIRTUOSO_BENCH_WEB_URL ?? 'http://127.0.0.1:4280';
quietChrome(serial);
reverseApp(serial);
const { main: appium } = await import(pathToFileURL(fromAppium.resolve('appium')).href);
const { Builder } = await import(pathToFileURL(fromSelenium.resolve('selenium-webdriver')).href);
const { Command } = await import(pathToFileURL(fromSelenium.resolve('selenium-webdriver/lib/command.js')).href);
const server = await appium({ port: PORT, address: '127.0.0.1', loglevel: 'error', allowInsecure: ['uiautomator2:chromedriver_autodownload'] });
let failed = false;
let driver;
try {
  driver = await new Builder()
    .usingServer(`http://127.0.0.1:${PORT}`)
    .withCapabilities({
      platformName: 'Android',
      browserName: 'Chrome',
      'appium:automationName': 'UiAutomator2',
      'appium:udid': serial,
      'appium:newCommandTimeout': 120,
    })
    .build();
  // Chrome's DevTools, as Selenium's Chrome driver reaches them (chromedriver's goog/cdp/execute,
  // which Appium hands on to the chromedriver it drives Chrome with): the scenarios' closed
  // shadow roots are opened through it.
  driver.getExecutor().defineCommand('sendDevToolsCommand', 'POST', '/session/:sessionId/goog/cdp/execute');
  driver.sendDevToolsCommand = (cmd, params = {}) =>
    driver.execute(new Command('sendDevToolsCommand').setParameter('cmd', cmd).setParameter('params', params));
  await scenario(driver, (slug) => driver.get(`${BASE}/e/${slug}`));
  console.log(`${name}: passed`);
} catch (e) {
  failed = true;
  console.error(`${name}: FAILED\n${e?.message ?? e}`);
} finally {
  if (driver) await driver.quit().catch(() => {});
  await server.close();
}
process.exit(failed ? 1 : 0);
