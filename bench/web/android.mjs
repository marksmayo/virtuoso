// Chrome on the Android device VIRTUOSO_BENCH_ANDROID names (a serial), set up as virtuoso's
// --device android sets it up: Chrome started fresh on a blank page with its first-run screens
// off, its DevTools socket forwarded to a port of this machine, and the app's port reversed so
// the device reaches http://127.0.0.1:4280 as the flows name it. Each tool then reaches that
// Chrome its own way.
import { execFileSync } from 'node:child_process';

export function androidSerial() {
  return process.env.VIRTUOSO_BENCH_ANDROID || null;
}

const adb = (serial, ...args) => execFileSync('adb', ['-s', serial, ...args], { encoding: 'utf8' }).trim();

/** Chrome started fresh on `serial`; its DevTools at http://127.0.0.1:<port>. */
export async function androidChrome(serial, { appPort = 4280, fresh = true } = {}) {
  adb(serial, 'shell', "echo '_ --disable-fre --no-default-browser-check --no-first-run' > /data/local/tmp/chrome-command-line");
  adb(serial, 'shell', 'am set-debug-app --persistent com.android.chrome');
  if (fresh) {
    adb(serial, 'shell', 'am force-stop com.android.chrome');
    adb(serial, 'shell', 'am start -n com.android.chrome/com.google.android.apps.chrome.Main -d about:blank');
  }
  const deadline = Date.now() + 30000;
  while (!adb(serial, 'shell', 'cat /proc/net/unix').includes('@chrome_devtools_remote')) {
    if (Date.now() > deadline) throw new Error(`Chrome on ${serial} did not open its DevTools within 30 s`);
    await new Promise((r) => setTimeout(r, 200));
  }
  const port = Number(adb(serial, 'forward', 'tcp:0', 'localabstract:chrome_devtools_remote'));
  adb(serial, 'reverse', `tcp:${appPort}`, `tcp:${appPort}`);
  return {
    port,
    http: `http://127.0.0.1:${port}`,
    ws: `ws://127.0.0.1:${port}/devtools/browser`,
    close() {
      try { adb(serial, 'forward', '--remove', `tcp:${port}`); } catch {}
    },
  };
}

/** The app's port reversed, for a tool that starts Chrome itself (Playwright, chromedriver). */
export function reverseApp(serial, appPort = 4280) {
  adb(serial, 'reverse', `tcp:${appPort}`, `tcp:${appPort}`);
}

/** Chrome's first-run screens off, for a tool that starts Chrome itself. */
export function quietChrome(serial) {
  adb(serial, 'shell', "echo '_ --disable-fre --no-default-browser-check --no-first-run' > /data/local/tmp/chrome-command-line");
  adb(serial, 'shell', 'am set-debug-app --persistent com.android.chrome');
}

/** The device's Chrome version, major only (`133`): what chromedriver for it must match. */
export function deviceChromeVersion(serial) {
  const out = adb(serial, 'shell', 'dumpsys package com.android.chrome');
  const m = out.match(/versionName=(\d+)\./);
  if (!m) throw new Error(`no Chrome version on ${serial}`);
  return m[1];
}
