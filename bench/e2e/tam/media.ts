import { execFileSync } from 'node:child_process';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

/** Puts the benchmark's receipt photo in the device's gallery, the newest picture there. */
export function addReceipt(): void {
  const serial = process.env.VIRTUOSO_BENCH_DEVICE;
  const adb = (...args: string[]) => execFileSync('adb', serial ? ['-s', serial, ...args] : args);
  const photo = join(dirname(fileURLToPath(import.meta.url)), '..', '..', 'tam-flows', 'media', 'receipt.png');
  const name = `receipt-${Date.now()}.png`;
  adb('push', photo, `/sdcard/Pictures/${name}`);
  adb('shell', 'am', 'broadcast', '-a', 'android.intent.action.MEDIA_SCANNER_SCAN_FILE', '-d', `file:///sdcard/Pictures/${name}`);
}
