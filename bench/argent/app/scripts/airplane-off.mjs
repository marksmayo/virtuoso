// Airplane mode off.
import { execFileSync } from 'node:child_process';

const devices = execFileSync('adb', ['devices'], { encoding: 'utf8' });
const serial = process.env.VIRTUOSO_BENCH_DEVICE || (devices.match(/^(emulator-\d+)\tdevice/m) ?? [])[1];
const adb = (...args) => execFileSync('adb', serial ? ['-s', serial, ...args] : args, { encoding: 'utf8' });
adb('shell', 'cmd', 'connectivity', 'airplane-mode', 'disable');
