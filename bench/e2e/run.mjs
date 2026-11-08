// Runs one benchmark scenario's e2e test: `node run.mjs <scenario>.e2e.ts` from its suite's
// folder (settings, app or tam), on that suite's target. `--version` prints e2e's version.
import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { basename, dirname, join, relative, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = dirname(fileURLToPath(import.meta.url));
const cli = join(root, 'node_modules', 'e2e', 'dist', 'cli', 'bin.js');
const arg = process.argv[2];
if (arg === '--version') {
  const v = (p) => JSON.parse(readFileSync(join(root, 'node_modules', p, 'package.json'), 'utf8')).version;
  console.log(`e2e ${v('e2e')} (@e2e-dev/mobile ${v('@e2e-dev/mobile')})`);
  process.exit(0);
}
// The suite's target: its -ios twin on iOS (VIRTUOSO_BENCH_PLATFORM=ios).
const folder = basename(process.cwd()).replace(/-ios$/, '');
const suite = process.env.VIRTUOSO_BENCH_PLATFORM === 'ios' ? `${folder}-ios` : folder;
const file = relative(root, join(process.cwd(), arg)).split(sep).join('/');
const r = spawnSync(process.execPath, [cli, 'run', file, '--target', suite], { cwd: root, stdio: 'inherit' });
process.exit(r.status ?? 1);
