import type { E2EConfig } from 'e2e';
import { mobile } from '@e2e-dev/mobile';

/**
 * The benchmark's scenarios as Tester Army e2e tests: the mobile engine (agent-device), locators
 * only, no model. One target per app; run.mjs picks it from the test's folder. The device is the
 * one the benchmark was given (VIRTUOSO_BENCH_DEVICE).
 */
const android = mobile({ platform: 'android', device: process.env.VIRTUOSO_BENCH_DEVICE, session: 'virtuoso-bench' });

export default {
  projectId: 'dev.virtuoso.bench',
  tests: '**/*.e2e.ts',
  targets: [
    { name: 'settings', engine: android, app: { bundleId: 'com.android.settings' } },
    { name: 'app', engine: android, app: { bundleId: 'dev.speedrun.bench' } },
    { name: 'tam', engine: android, app: { bundleId: 'dev.e2e.benchmark' } },
  ],
  workers: 1,
  // A test's whole budget: the longest flows (twelve animation scenes, a 512-row list) take
  // minutes, and a Maestro flow has no such limit.
  timeout: 600_000,
  reporters: ['list'],
} satisfies E2EConfig;
