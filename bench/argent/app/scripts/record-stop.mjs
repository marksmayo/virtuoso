// Screen recording off: the device's recorder stopped, its video saved next to the system's temp files.
import { execFileSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const devices = execFileSync('adb', ['devices'], { encoding: 'utf8' });
const serial = process.env.VIRTUOSO_BENCH_DEVICE || (devices.match(/^(emulator-\d+)\tdevice/m) ?? [])[1];
const adb = (...args) => execFileSync('adb', serial ? ['-s', serial, ...args] : args, { encoding: 'utf8' });
adb('shell', 'pkill -INT screenrecord || true');
// It finishes the file as it stops.
for (let i = 0; i < 30 && adb('shell', 'pidof screenrecord || true').trim(); i++) await new Promise((r) => setTimeout(r, 100));
adb('pull', '/sdcard/argent-bench.mp4', join(tmpdir(), 'argent-bench.mp4'));
adb('shell', 'rm', '-f', '/sdcard/argent-bench.mp4');
