import type { E2EConfig } from 'e2e';
import { web } from '@e2e-dev/web';
import { phone } from '../phone.mjs';
import { e2eAttaches } from '../browser.mjs';

/**
 * The web benchmark's flows as Tester Army e2e tests: the web engine, locators only, no model.
 * Its own launch is Playwright's bundled headless shell, so run.mjs starts the installed Chrome
 * (headless, a 1280 x 800 window, as every tool here runs) and the engine attaches to it over
 * CDP (VIRTUOSO_BENCH_CDP); each attempt is a fresh context, so no state from a run before.
 */
// A phone (VIRTUOSO_BENCH_EMULATE): the web engine's viewport and user agent, with the shared
// values (it has no touch or mobile-viewport setting).
const p = phone();
// On an Android device (VIRTUOSO_BENCH_ANDROID) Chrome has no new browser contexts: the engine's
// persistent mode (its reconnect endpoint), in Chrome's own context, at the phone's own size.
const onDevice = !!process.env.VIRTUOSO_BENCH_ANDROID;
const endpoint = async () => process.env.VIRTUOSO_BENCH_CDP ?? '';
// In a Firefox field (VIRTUOSO_BENCH_WEB_BROWSER), no CDP: the engine launches Playwright's own
// Firefox (headless), with the same viewport and user agent.
const viewport = p ? { width: p.width, height: p.height } : { width: 1280, height: 800 };
const engine = onDevice
  ? web({ connect: { cdpEndpoint: endpoint, reconnectEndpoint: endpoint }, viewport: null })
  : !e2eAttaches()
  ? web({ browser: 'firefox', viewport, ...(p ? { userAgent: p.userAgent } : {}) })
  : web({
      connect: { cdpEndpoint: endpoint },
      viewport: p ? { width: p.width, height: p.height } : { width: 1280, height: 800 },
      ...(p ? { userAgent: p.userAgent } : {}),
    });

export default {
  projectId: 'dev.virtuoso.bench.web',
  tests: 'tests/**/*.e2e.ts',
  targets: [{ name: 'web', engine, app: { url: process.env.VIRTUOSO_BENCH_WEB_URL ?? 'http://127.0.0.1:4280' } }],
  workers: 1,
  timeout: 600_000,
  reporters: ['list'],
} satisfies E2EConfig;
