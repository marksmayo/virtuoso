// A photo into the gallery, the newest there.
import { execFileSync } from 'node:child_process';

const devices = execFileSync('adb', ['devices'], { encoding: 'utf8' });
const serial = process.env.VIRTUOSO_BENCH_DEVICE || (devices.match(/^(emulator-\d+)\tdevice/m) ?? [])[1];
const adb = (...args) => execFileSync('adb', serial ? ['-s', serial, ...args] : args, { encoding: 'utf8' });
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
const photo = join(dirname(fileURLToPath(import.meta.url)), '..', '..', '..', '..', '..', 'speedrun', 'bench', 'app', 'flows', 'media', 'photo.png');
const name = `photo-${Date.now()}.png`;
adb('push', photo, `/sdcard/Pictures/${name}`);
adb('shell', 'am', 'broadcast', '-a', 'android.intent.action.MEDIA_SCANNER_SCAN_FILE', '-d', `file:///sdcard/Pictures/${name}`);
