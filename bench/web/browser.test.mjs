// browser.mjs's launches, without a browser: node --test bench/web/browser.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { browserName, cypressBrowser, e2eAttaches, playwrightLaunch, puppeteerLaunch, seleniumFirefox } from './browser.mjs';

const chrome = { VIRTUOSO_CHROME: '/cft/chrome' };
const firefox = { VIRTUOSO_BENCH_WEB_BROWSER: 'firefox', VIRTUOSO_FIREFOX: '/ff/firefox', VIRTUOSO_GECKODRIVER: '/gd/geckodriver' };
const pixel7 = { width: 412, height: 839, scale: 2.625, userAgent: 'UA' };

test('Chrome unless Firefox is named, and nothing else', () => {
  assert.equal(browserName({}), 'chrome');
  assert.equal(browserName({ VIRTUOSO_BENCH_WEB_BROWSER: 'Firefox' }), 'firefox');
  assert.throws(() => browserName({ VIRTUOSO_BENCH_WEB_BROWSER: 'safari' }), /chrome or firefox/);
});

test("Chrome's launches are the ones there were", () => {
  assert.deepEqual(playwrightLaunch(chrome), { type: 'chromium', options: { channel: undefined, executablePath: '/cft/chrome', headless: true } });
  assert.deepEqual(playwrightLaunch({}), { type: 'chromium', options: { channel: 'chrome', executablePath: undefined, headless: true } });
  assert.deepEqual(puppeteerLaunch(chrome), { executablePath: '/cft/chrome', headless: true, defaultViewport: { width: 1280, height: 800 }, args: ['--window-size=1280,800'] });
  assert.deepEqual(puppeteerLaunch({}).channel, 'chrome');
  assert.equal(cypressBrowser(chrome), '/cft/chrome');
  assert.equal(cypressBrowser({}), 'chrome');
  assert.equal(e2eAttaches(chrome), true);
});

test("Firefox's: the pinned release, Playwright's (and e2e's) own build", () => {
  assert.deepEqual(playwrightLaunch(firefox), { type: 'firefox', options: { headless: true } });
  assert.deepEqual(playwrightLaunch({ ...firefox, VIRTUOSO_BENCH_PLAYWRIGHT_FIREFOX: 'stock' }), {
    type: 'firefox',
    options: { channel: 'moz-firefox', executablePath: '/ff/firefox', headless: true },
  });
  assert.deepEqual(puppeteerLaunch(firefox), {
    browser: 'firefox',
    executablePath: '/ff/firefox',
    headless: true,
    defaultViewport: { width: 1280, height: 800 },
    args: ['--width=1280', '--height=800'],
  });
  assert.equal(cypressBrowser(firefox), '/ff/firefox');
  assert.equal(cypressBrowser({ VIRTUOSO_BENCH_WEB_BROWSER: 'firefox' }), 'firefox');
  assert.equal(e2eAttaches(firefox), false);
});

test("Selenium's Firefox: a desktop window, or a phone's viewport, scale and user agent", () => {
  assert.deepEqual(seleniumFirefox(null, firefox), {
    args: ['-headless', '--width=1280', '--height=800'],
    prefs: {},
    binary: '/ff/firefox',
    geckodriver: '/gd/geckodriver',
    viewport: { width: 1280, height: 800, devicePixelRatio: undefined },
  });
  const p = seleniumFirefox(pixel7, { VIRTUOSO_BENCH_WEB_BROWSER: 'firefox' });
  assert.deepEqual(p.prefs, { 'general.useragent.override': 'UA' });
  assert.deepEqual(p.viewport, { width: 412, height: 839, devicePixelRatio: 2.625 });
  assert.equal(p.binary, null);
  assert.equal(p.geckodriver, null);
});
