// Puts the benchmark's receipt photo in the emulator's gallery, the newest picture there; on a
// Mac with a simulator booted (the iOS benchmark's, the one Argent runs on: it hands a script
// neither the device nor the platform), in that simulator's photo library.
import { execFileSync } from 'node:child_process';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const photo = join(dirname(fileURLToPath(import.meta.url)), '..', '..', 'tam-flows', 'media', 'receipt.png');
// A simulator booted on a Mac: the iOS benchmark's.
const simulator = process.platform === 'darwin' && /\(Booted\)/.test(execFileSync('xcrun', ['simctl', 'list', 'devices', 'booted'], { encoding: 'utf8' }));
if (simulator) {
  execFileSync('xcrun', ['simctl', 'addmedia', 'booted', photo]);
} else {
  const devices = execFileSync('adb', ['devices'], { encoding: 'utf8' });
  const serial = process.env.VIRTUOSO_BENCH_DEVICE || (devices.match(/^(emulator-\d+)\tdevice/m) ?? [])[1];
  const adb = (...args) => execFileSync('adb', serial ? ['-s', serial, ...args] : args);
  const name = `receipt-${Date.now()}.png`;
  adb('push', photo, `/sdcard/Pictures/${name}`);
  adb('shell', 'am', 'broadcast', '-a', 'android.intent.action.MEDIA_SCANNER_SCAN_FILE', '-d', `file:///sdcard/Pictures/${name}`);
}
