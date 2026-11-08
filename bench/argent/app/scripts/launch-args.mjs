// Starts the app afresh with launch arguments.
import { execFileSync } from 'node:child_process';

const devices = execFileSync('adb', ['devices'], { encoding: 'utf8' });
const serial = process.env.VIRTUOSO_BENCH_DEVICE || (devices.match(/^(emulator-\d+)\tdevice/m) ?? [])[1];
const adb = (...args) => execFileSync('adb', serial ? ['-s', serial, ...args] : args, { encoding: 'utf8' });
adb('shell', 'pm', 'clear', 'dev.speedrun.bench');
// Granted, as a Maestro launch grants every permission: the clear took them back.
for (const p of ['ACCESS_FINE_LOCATION', 'ACCESS_COARSE_LOCATION', 'POST_NOTIFICATIONS', 'READ_MEDIA_IMAGES']) {
  adb('shell', 'pm', 'grant', 'dev.speedrun.bench', 'android.permission.' + p);
}
adb('shell', 'am', 'start', '-W', '-n', 'dev.speedrun.bench/.MainActivity', '--es', 'greeting', 'hello', '--es', 'mode', 'bench');
