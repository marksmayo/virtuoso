// Runs one web benchmark flow's e2e test: `node run.mjs <flow>.yaml` runs tests/<flow>.e2e.ts on
// the installed Chrome, started here (e2e's web engine attaches to it over CDP). `--version`
// prints e2e's version.
import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = dirname(fileURLToPath(import.meta.url));
const v = (p) => JSON.parse(readFileSync(join(root, 'node_modules', p, 'package.json'), 'utf8')).version;
const arg = process.argv[2] ?? '';
if (arg === '--version') {
  console.log(`e2e ${v('e2e')} (@e2e-dev/web ${v('@e2e-dev/web')})`);
  process.exit(0);
}
const name = basename(arg).replace(/\.yaml$/, '');
const test = `tests/${name}.e2e.ts`;
if (!existsSync(join(root, test))) {
  console.error(`no e2e test of ${name}`);
  process.exit(2);
}
const chrome = process.env.VIRTUOSO_CHROME || [
  String.raw`C:\Program Files\Google\Chrome\Application\chrome.exe`,
  '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  '/usr/bin/google-chrome',
  '/usr/bin/google-chrome-stable',
].find(existsSync);
const profile = mkdtempSync(join(tmpdir(), 'bench-e2e-'));
const browser = spawn(chrome, ['--headless=new', '--remote-debugging-port=0', `--user-data-dir=${profile}`, '--window-size=1280,800', '--no-first-run', '--no-default-browser-check', 'about:blank'], { stdio: 'ignore' });
let ws = '';
for (let i = 0; i < 300 && !ws; i++) {
  try {
    const [port, path] = readFileSync(join(profile, 'DevToolsActivePort'), 'utf8').split('\n');
    if (port && path) ws = `ws://127.0.0.1:${port}${path}`;
  } catch {}
  if (!ws) await new Promise((r) => setTimeout(r, 50));
}
const cli = join(root, 'node_modules', 'e2e', 'dist', 'cli', 'bin.js');
const r = ws
  ? spawnSync(process.execPath, [cli, 'run', test, '--target', 'web'], { cwd: root, stdio: 'inherit', env: { ...process.env, VIRTUOSO_BENCH_CDP: ws } })
  : { status: 1 };
if (!ws) console.error('Chrome did not start');
browser.kill();
await new Promise((r) => browser.once('exit', r).once('error', r));
try { rmSync(profile, { recursive: true, force: true, maxRetries: 5 }); } catch {}
process.exit(r.status ?? 1);
