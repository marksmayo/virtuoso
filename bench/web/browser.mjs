// The browser a web field drives, and how each code tool launches it: Chrome (the default) or
// Firefox, by VIRTUOSO_BENCH_WEB_BROWSER (the harness sets it for a Firefox field). Chrome's
// launches are the ones each run.mjs had before Firefox; FIREFOX.md says why Firefox's are these.
// Each function takes the environment, so browser.test.mjs checks them without a browser.
//
//   VIRTUOSO_CHROME     the Chrome every tool drives (Chrome for Testing, pinned in CI)
//   VIRTUOSO_FIREFOX    the Firefox every tool that can drives (a pinned release in CI)
//   VIRTUOSO_GECKODRIVER  Selenium's geckodriver (pinned in CI; else Selenium Manager finds one)
//   VIRTUOSO_BENCH_PLAYWRIGHT_FIREFOX=stock  Playwright on VIRTUOSO_FIREFOX over WebDriver BiDi
//     (its experimental moz-firefox channel) in place of its own patched Firefox

const SIZE = { width: 1280, height: 800 };

export function browserName(env = process.env) {
  const b = (env.VIRTUOSO_BENCH_WEB_BROWSER || 'chrome').toLowerCase();
  if (b !== 'chrome' && b !== 'firefox') throw new Error(`VIRTUOSO_BENCH_WEB_BROWSER is ${b}: chrome or firefox`);
  return b;
}

export const isFirefox = (env = process.env) => browserName(env) === 'firefox';

// Playwright: the BrowserType to launch (chromium or firefox) and its launch options. Firefox is
// Playwright's own build (`playwright-core install firefox`): stock Firefox lacks the Juggler
// protocol it drives Firefox with.
export function playwrightLaunch(env = process.env) {
  if (!isFirefox(env)) {
    return { type: 'chromium', options: { channel: env.VIRTUOSO_CHROME ? undefined : 'chrome', executablePath: env.VIRTUOSO_CHROME || undefined, headless: true } };
  }
  if (env.VIRTUOSO_BENCH_PLAYWRIGHT_FIREFOX === 'stock') {
    return { type: 'firefox', options: { channel: 'moz-firefox', executablePath: env.VIRTUOSO_FIREFOX || undefined, headless: true } };
  }
  return { type: 'firefox', options: { headless: true } };
}

// Puppeteer: its launch options. Firefox over WebDriver BiDi, the stock release.
export function puppeteerLaunch(env = process.env) {
  if (!isFirefox(env)) {
    return {
      ...(env.VIRTUOSO_CHROME ? { executablePath: env.VIRTUOSO_CHROME } : { channel: 'chrome' }),
      headless: true,
      defaultViewport: SIZE,
      args: [`--window-size=${SIZE.width},${SIZE.height}`],
    };
  }
  return {
    browser: 'firefox',
    ...(env.VIRTUOSO_FIREFOX ? { executablePath: env.VIRTUOSO_FIREFOX } : {}),
    headless: true,
    defaultViewport: SIZE,
    args: [`--width=${SIZE.width}`, `--height=${SIZE.height}`],
  };
}

// Selenium in Firefox (geckodriver): the browser's arguments and preferences, its binary, and the
// viewport to set over BiDi once the session is open: headless Firefox's 1280 x 800 window has a
// 1280 x 715 page, so the page is made 1280 x 800 as every tool's is, or a phone's with its
// scale (geckodriver has no mobile emulation). A phone's user agent is Firefox's own override
// preference.
export function seleniumFirefox(phone, env = process.env) {
  return {
    args: ['-headless', `--width=${SIZE.width}`, `--height=${SIZE.height}`],
    prefs: phone ? { 'general.useragent.override': phone.userAgent } : {},
    binary: env.VIRTUOSO_FIREFOX || null,
    geckodriver: env.VIRTUOSO_GECKODRIVER || null,
    viewport: phone ? { width: phone.width, height: phone.height, devicePixelRatio: phone.scale } : { ...SIZE, devicePixelRatio: undefined },
  };
}

// Cypress: its --browser (a name, or the path of the binary to drive).
export function cypressBrowser(env = process.env) {
  return isFirefox(env) ? env.VIRTUOSO_FIREFOX || 'firefox' : env.VIRTUOSO_CHROME || 'chrome';
}

// e2e's web engine: in Chrome it attaches over CDP to the Chrome run.mjs starts; in Firefox
// (which has no CDP) it launches Playwright's Firefox itself (`browser: 'firefox'`).
export function e2eAttaches(env = process.env) {
  return !isFirefox(env);
}
