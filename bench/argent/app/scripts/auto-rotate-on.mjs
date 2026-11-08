// Auto-rotate on: Argent turns the emulator by its sensors, which the system follows only so.
import { execFileSync } from 'node:child_process';

const devices = execFileSync('adb', ['devices'], { encoding: 'utf8' });
const serial = process.env.VIRTUOSO_BENCH_DEVICE || (devices.match(/^(emulator-\d+)\tdevice/m) ?? [])[1];
const adb = (...args) => execFileSync('adb', serial ? ['-s', serial, ...args] : args, { encoding: 'utf8' });
adb('shell', 'settings', 'put', 'system', 'accelerometer_rotation', '1');
