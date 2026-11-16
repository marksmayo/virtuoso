import { defineConfig } from 'cypress';

// The web benchmark in Cypress: the installed Chrome, headless, a 1280 x 800 viewport (as every
// tool here runs); a spec per flow (cypress/e2e/<flow>.cy.js). testIsolation clears the site's
// state before each test: clearState.
export default defineConfig({
  e2e: {
    baseUrl: process.env.VIRTUOSO_BENCH_WEB_URL ?? 'http://127.0.0.1:4280',
    supportFile: false,
    specPattern: 'cypress/e2e/**/*.cy.js',
    testIsolation: true,
    defaultCommandTimeout: 17000,
    viewportWidth: 1280,
    viewportHeight: 800,
    video: false,
    screenshotOnRunFailure: false,
  },
});
