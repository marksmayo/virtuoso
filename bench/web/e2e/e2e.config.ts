import type { E2EConfig } from 'e2e';
import { web } from '@e2e-dev/web';

/**
 * The web benchmark's flows as Tester Army e2e tests: the web engine, locators only, no model.
 * Its own launch is Playwright's bundled headless shell, so run.mjs starts the installed Chrome
 * (headless, a 1280 x 800 window, as every tool here runs) and the engine attaches to it over
 * CDP (VIRTUOSO_BENCH_CDP); each attempt is a fresh context, so no state from a run before.
 */
const engine = web({
  connect: { cdpEndpoint: async () => process.env.VIRTUOSO_BENCH_CDP ?? '' },
  viewport: { width: 1280, height: 800 },
});

export default {
  projectId: 'dev.virtuoso.bench.web',
  tests: 'tests/**/*.e2e.ts',
  targets: [{ name: 'web', engine, app: { url: process.env.VIRTUOSO_BENCH_WEB_URL ?? 'http://127.0.0.1:4280' } }],
  workers: 1,
  timeout: 600_000,
  reporters: ['list'],
} satisfies E2EConfig;
