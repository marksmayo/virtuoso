// Screen recording on, on the device itself: Argent's own recording needs ffmpeg on the host.
import { execFileSync, spawn } from 'node:child_process';

const devices = execFileSync('adb', ['devices'], { encoding: 'utf8' });
const serial = process.env.VIRTUOSO_BENCH_DEVICE || (devices.match(/^(emulator-\d+)\tdevice/m) ?? [])[1];
const args = ['shell', 'screenrecord', '/sdcard/argent-bench.mp4'];
spawn('adb', serial ? ['-s', serial, ...args] : args, { detached: true, stdio: 'ignore' }).unref();
// Until it has started, a stop finds nothing to stop.
for (let i = 0; i < 20; i++) {
  if (execFileSync('adb', [...(serial ? ['-s', serial] : []), 'shell', 'pidof screenrecord || true'], { encoding: 'utf8' }).trim()) break;
  await new Promise((r) => setTimeout(r, 100));
}
