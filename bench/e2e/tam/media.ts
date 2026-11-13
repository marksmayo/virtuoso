import { execFileSync } from 'node:child_process';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

/** Whether the benchmark runs on an iOS simulator (VIRTUOSO_BENCH_PLATFORM=ios). */
export const IOS = process.env.VIRTUOSO_BENCH_PLATFORM === 'ios';

/**
 * Puts the benchmark's receipt photo in the device's gallery, the newest picture there (on iOS,
 * the simulator's photo library).
 */
export function addReceipt(): void {
  const serial = process.env.VIRTUOSO_BENCH_DEVICE;
  const photo = join(dirname(fileURLToPath(import.meta.url)), '..', '..', 'tam-flows', 'media', 'receipt.png');
  if (IOS) {
    execFileSync('xcrun', ['simctl', 'addmedia', serial ?? 'booted', photo]);
    return;
  }
  const adb = (...args: string[]) => execFileSync('adb', serial ? ['-s', serial, ...args] : args);
  const name = `receipt-${Date.now()}.png`;
  adb('push', photo, `/sdcard/Pictures/${name}`);
  adb('shell', 'am', 'broadcast', '-a', 'android.intent.action.MEDIA_SCANNER_SCAN_FILE', '-d', `file:///sdcard/Pictures/${name}`);
}
