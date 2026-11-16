// Runs one web benchmark flow's Cypress spec: `node run.mjs <flow>.yaml` runs
// cypress/e2e/<flow>.cy.js with `cypress run` in the installed Chrome, headless.
// `--version` prints Cypress's version.
import { spawnSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { basename, dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

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
const r = spawnSync(process.execPath, [cli, 'run', '--browser', browser, '--headless', '--spec', spec], {
  cwd: root,
  stdio: 'inherit',
  env: { ...process.env, CYPRESS_CRASH_REPORTS: '0', CYPRESS_COMMERCIAL_RECOMMENDATIONS: '0', NO_COLOR: '1' },
});
process.exit(r.status ?? 1);
