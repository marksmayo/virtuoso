// The emulator at the route's end.
import { execFileSync } from 'node:child_process';

const devices = execFileSync('adb', ['devices'], { encoding: 'utf8' });
const serial = process.env.VIRTUOSO_BENCH_DEVICE || (devices.match(/^(emulator-\d+)\tdevice/m) ?? [])[1];
const adb = (...args) => execFileSync('adb', serial ? ['-s', serial, ...args] : args, { encoding: 'utf8' });
adb('shell', 'appops', 'set', 'com.android.shell', 'android:mock_location', 'allow');
// The GPS and network test providers, as the Maestro runners set a location.
for (const p of ['gps', 'network']) {
  try { adb('shell', 'cmd', 'location', 'providers', 'add-test-provider', p); } catch {}
  adb('shell', 'cmd', 'location', 'providers', 'set-test-provider-enabled', p, 'true');
  adb('shell', 'cmd', 'location', 'providers', 'set-test-provider-location', p, '--location', '45.5088,-73.5540');
}
