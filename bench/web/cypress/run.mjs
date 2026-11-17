// Runs one web benchmark flow's Cypress spec: `node run.mjs <flow>.yaml` runs
// cypress/e2e/<flow>.cy.js with `cypress run` in the installed Chrome, headless.
// `--version` prints Cypress's version.
import { spawnSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { basename, dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { phone } from '../phone.mjs';

const root = dirname(fileURLToPath(import.meta.url));
const arg = process.argv[2] ?? '';
if (arg === '--version') {
  const v = JSON.parse(readFileSync(join(root, 'node_modules', 'cypress', 'package.json'), 'utf8')).version;
  console.log(`cypress ${v}`);
  process.exit(0);
}
const name = basename(arg).replace(/\.yaml$/, '');
const spec = `cypress/e2e/${name}.cy.js`;
if (!existsSync(join(root, spec))) {
  console.error(`no Cypress test of ${name}`);
  process.exit(2);
}
const cli = join(root, 'node_modules', 'cypress', 'bin', 'cypress');
const browser = process.env.VIRTUOSO_CHROME || 'chrome';
// A phone (VIRTUOSO_BENCH_EMULATE): Cypress's viewport and user agent, with the shared values
// (Cypress has no touch emulation).
const p = phone();
// (As CYPRESS_* variables: --config cannot parse a user agent's commas.)
const config = p ? { CYPRESS_viewportWidth: String(p.width), CYPRESS_viewportHeight: String(p.height), CYPRESS_userAgent: p.userAgent } : {};
const r = spawnSync(process.execPath, [cli, 'run', '--browser', browser, '--headless', '--spec', spec], {
  cwd: root,
  stdio: 'inherit',
  env: { ...process.env, ...config, CYPRESS_CRASH_REPORTS: '0', CYPRESS_COMMERCIAL_RECOMMENDATIONS: '0', NO_COLOR: '1' },
});
process.exit(r.status ?? 1);
