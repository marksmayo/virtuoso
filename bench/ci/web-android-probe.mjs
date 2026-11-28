// A diagnostic for the emulator field (run-web-android.sh with BENCH_DIAGNOSE set): what a page
// on the device's browser is ready for, and when, after its load event. The browser is started
// as the tools start it; a tab is opened on the login form; then, every 50 ms for 8 s:
//   - whether the page is visible and focused (a hidden page is sent no input),
//   - whether React has attached its handlers (a button pressed before does nothing),
//   - whether the page has had a frame since (a rAF loop), and when its first contentful paint
//     was reported, after its load (input reaching a page before its first frame is shown can
//     be dropped),
//   - a mouse click on "Log in" through DevTools: whether the page saw a click event at all, and
//     whether it answered (the form's "Enter a valid email address").
// Each change is printed with its time since the load. Nothing is measured from it.
// Usage: node web-android-probe.mjs <serial> <package> [runs]
import { execFileSync } from 'node:child_process';

const [serial, pkg, runs = '3'] = process.argv.slice(2);
const url = 'http://127.0.0.1:4280/e/login-form';
const adb = (...a) => execFileSync('adb', ['-s', serial, ...a], { encoding: 'utf8' }).trim();
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const STATE = `(() => {
  const b = document.querySelector('[data-testid="login-button"]') || [...document.querySelectorAll('button')].find((e) => /log in/i.test(e.textContent));
  const r = b ? b.getBoundingClientRect() : null;
  if (window.__probeClicks === undefined) { window.__probeClicks = 0; addEventListener('click', () => { window.__probeClicks++; }, true); }
  // Frames the page has had since the probe first looked (a rAF loop), and when its first
  // contentful paint was reported (ms after its load; Chrome reports it once that frame is shown).
  if (window.__probeFrames === undefined) { window.__probeFrames = 0; const f = () => { window.__probeFrames++; requestAnimationFrame(f); }; requestAnimationFrame(f); }
  const nav = performance.getEntriesByType('navigation')[0], fcp = performance.getEntriesByName('first-contentful-paint')[0];
  return {
    frames: window.__probeFrames, fcp: fcp && nav ? Math.round(fcp.startTime - nav.loadEventStart) : null,
    visible: document.visibilityState, focus: document.hasFocus(),
    hydrated: !!b && Object.keys(b).some((k) => k.startsWith('__reactProps')),
    clicks: window.__probeClicks, answered: /Enter a valid email address/.test(document.body.innerText),
    x: r ? r.left + r.width / 2 : null, y: r ? r.top + r.height / 2 : null,
  };
})()`;

async function once(n) {
  const started = Date.now();
  adb('shell', "echo '_ --disable-fre --no-default-browser-check --no-first-run' > /data/local/tmp/chrome-command-line");
  adb('shell', `am set-debug-app --persistent ${pkg}`);
  adb('shell', `am force-stop ${pkg}`);
  adb('reverse', 'tcp:4280', 'tcp:4280');
  adb('shell', `am start -n ${pkg}/com.google.android.apps.chrome.Main -d about:blank`);
  while (!adb('shell', 'cat /proc/net/unix').includes('chrome_devtools_remote')) await sleep(100);
  const port = Number(adb('forward', 'tcp:0', 'localabstract:chrome_devtools_remote'));
  let ws;
  for (let i = 0; i < 200 && !ws; i++) {
    try {
      const v = await (await fetch(`http://127.0.0.1:${port}/json/version`)).json();
      const w = new WebSocket(v.webSocketDebuggerUrl);
      await new Promise((ok, no) => { w.onopen = ok; w.onerror = no; });
      ws = w;
    } catch { await sleep(100); }
  }
  if (!ws) throw new Error('no DevTools connection');
  let id = 0;
  const waiting = new Map();
  let loaded = null;
  let closing = false;
  ws.onclose = () => { if (!closing) console.log(`  the browser closed the DevTools connection ${Date.now() - started} ms after its start`); };
  // The form's load, not the blank page's the tab opened on: the first after the navigation.
  let navigated = false;
  ws.onmessage = (m) => {
    const d = JSON.parse(m.data);
    if (d.id && waiting.has(d.id)) { waiting.get(d.id)(d); waiting.delete(d.id); }
    else if (d.method === 'Page.frameNavigated' && d.params.frame.url.startsWith(url)) navigated = true;
    else if (d.method === 'Page.loadEventFired' && navigated && loaded === null) loaded = Date.now();
  };
  const call = (method, params = {}, sessionId) => new Promise((ok) => { const i = ++id; waiting.set(i, ok); ws.send(JSON.stringify({ id: i, method, params, sessionId })); });
  const t = (await call('Target.createTarget', { url: 'about:blank' })).result.targetId;
  const s = (await call('Target.attachToTarget', { targetId: t, flatten: true })).result.sessionId;
  await call('Page.enable', {}, s);
  await call('Page.navigate', { url }, s);
  while (loaded === null) {
    if (Date.now() - started > 60000) throw new Error('the form did not load within 60 s');
    await sleep(5);
  }
  console.log(`run ${n}: load ${loaded - started} ms after the browser was started`);
  let last = '';
  let sent = 0;
  while (Date.now() - loaded < 8000) {
    const r = await call('Runtime.evaluate', { expression: STATE, returnByValue: true }, s);
    const st = r.result?.result?.value;
    if (!st) {
      if (last !== 'none') { console.log(`  +${Date.now() - loaded} ms: no state: ${JSON.stringify(r).slice(0, 300)}`); last = 'none'; }
      await sleep(50);
      continue;
    }
    // Frames as had or not (not their count, or every look is a change), and the paint's time.
    const line = `visible=${st.visible} focus=${st.focus} hydrated=${st.hydrated} frames=${st.frames > 0 ? 'yes' : 'none'} fcp=${st.fcp === null ? 'none' : `${st.fcp} ms`} clicks sent=${sent} seen=${st.clicks} answered=${st.answered}`;
    if (line !== last) { console.log(`  +${Date.now() - loaded} ms: ${line}`); last = line; }
    if (st.answered) break;
    if (st.x !== null) {
      const m = (type, extra) => call('Input.dispatchMouseEvent', { type, x: st.x, y: st.y, button: 'left', clickCount: 1, ...extra }, s);
      // Not awaited past 300 ms: a press Chrome never answers must not stop the probe.
      await Promise.race([m('mousePressed', { buttons: 1 }).then(() => m('mouseReleased', {})), sleep(300)]);
      sent++;
    }
    await sleep(50);
  }
  closing = true;
  ws.close();
  try { adb('forward', '--remove', `tcp:${port}`); } catch {}
}

for (let n = 0; n < Number(runs); n++) {
  try { await once(n); } catch (e) { console.log(`run ${n}: ${e.message}`); }
}
adb('shell', `rm -f /data/local/tmp/chrome-command-line; am clear-debug-app; am force-stop ${pkg}`);
