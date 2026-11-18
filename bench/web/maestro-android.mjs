// Maestro on Chrome on an Android device. Its web mode drives a desktop Chrome only; on a device
// a Maestro user tests a site through Chrome the app, which is what this runs: the flow with its
// `url:` header made `appId: com.android.chrome` and its launchApp followed by an openLink of the
// url (Chrome's data cleared by launchApp's clearState; its first-run screens stay off, the
// flags file being outside Chrome's data), then `maestro --device <serial> test` on it. Maestro
// reads the page through Chrome's accessibility tree.
//
//   node maestro-android.mjs <flow>.yaml    on the device VIRTUOSO_BENCH_ANDROID names
//   node maestro-android.mjs --version
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, join } from 'node:path';
import { androidSerial, quietChrome, reverseApp } from './android.mjs';

/** The flow, as an app flow of Chrome: its header's url opened after its launchApp. */
export function chromeAppFlow(text) {
  const m = text.match(/^url:\s*(\S+)\s*$/m);
  if (!m) throw new Error('the flow has no url: header');
  const url = m[1];
  let out = text.replace(m[0], 'appId: com.android.chrome');
  // The first launchApp (a line, or a block of indented lines under it) then an openLink.
  const lines = out.split('\n');
  const i = lines.findIndex((l) => /^- launchApp\b/.test(l));
  if (i < 0) throw new Error('the flow has no launchApp');
  let j = i + 1;
  while (j < lines.length && /^\s+\S/.test(lines[j])) j++;
  lines.splice(j, 0, `- openLink: ${url}`);
  return lines.join('\n');
}

const arg = process.argv[2] ?? '';
// Run as a script (imported, it only lends chromeAppFlow).
if (process.argv[1]?.endsWith('maestro-android.mjs')) {
  const maestro = process.env.MAESTRO_BIN;
  if (arg === '--version') {
    const r = spawnSync(maestro, ['--version'], { encoding: 'utf8', shell: process.platform === 'win32' });
    console.log(`maestro ${r.stdout.trim()}`);
    process.exit(0);
  }
  const serial = androidSerial();
  if (!serial) {
    console.error('VIRTUOSO_BENCH_ANDROID names the device');
    process.exit(2);
  }
  quietChrome(serial);
  reverseApp(serial);
  const dir = mkdtempSync(join(tmpdir(), 'maestro-android-'));
  const flow = join(dir, basename(arg));
  writeFileSync(flow, chromeAppFlow(readFileSync(arg, 'utf8')));
  const r = spawnSync(maestro, ['--device', serial, 'test', flow], { stdio: 'inherit', shell: process.platform === 'win32' });
  rmSync(dir, { recursive: true, force: true });
  process.exit(r.status ?? 1);
}
