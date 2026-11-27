// Chrome's launch, shape by shape, with no flow around it (round 4's NW-71): how long from the
// spawn until a page of the benchmark app has loaded, for each way of starting Chrome and opening
// its first page. Raw CDP, no tool's library.
//
// The shapes cross four choices:
//   tab      tab: Chrome starts on about:blank (as virtuoso does) and the page uses that tab;
//            none: --no-startup-window (as Playwright does) and the page is a tab it creates
//   link     port: --remote-debugging-port=0, its DevToolsActivePort file, a WebSocket (virtuoso);
//            pipe: --remote-debugging-pipe, NUL-ended JSON on fds 3 and 4 (Playwright)
//   context  default: the profile's own browser context; new: Target.createBrowserContext first
//            (Playwright's newContext)
//   switches ours: virtuoso's launch_args (chrome.rs, origin/main f8893f9); pw: Playwright
//            1.65.1's chromiumSwitches and headless switches (it runs with --no-sandbox, as the
//            benchmark's Playwright does); pw-sandbox: the same, Chrome's sandbox kept;
//            ours-noproxy: ours with --no-proxy-server (no WPAD/DHCP proxy search)
//
// Each launch is timed from the spawn (ms): `connected` (the link is up: the WebSocket open, or at
// once for a pipe), `ready` (Chrome's first answer: its targets), `page` (the page's tab attached
// to and its events on), `answered` (Page.navigate answered: the response has begun), `loaded`
// (its load event). The shapes take turns, run by run, so a drift in the machine is shared.
//
// usage: node bench/ci/launch-shapes.mjs [--chrome PATH] [--url URL] [--runs 20]
//          [--switches ours,ours-noproxy,pw,pw-sandbox] [--shapes tab-port-default,none-pipe-new,...]
//          [--out shapes.json]
// Chrome: --chrome, else VIRTUOSO_CHROME. Prints a table of medians per shape and the effect of
// each choice (the median of the paired differences, the other choices held), and writes every
// launch to --out.
import { spawn } from 'node:child_process';
import { mkdirSync, readFileSync, existsSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const argv = process.argv.slice(2);
const opt = (k, d) => { const i = argv.indexOf(k); return i < 0 ? d : argv[i + 1]; };
const CHROME = opt('--chrome', process.env.VIRTUOSO_CHROME);
if (!CHROME) { console.error('launch-shapes: give --chrome or VIRTUOSO_CHROME'); process.exit(2); }
const URL_ = opt('--url', 'http://127.0.0.1:4280/');
const RUNS = +opt('--runs', 20);
const OUT = opt('--out', '');
const SWITCH_SETS = opt('--switches', 'ours,ours-noproxy,pw').split(',');
const windows = process.platform === 'win32';

// virtuoso's launch_args (crates/virtuoso/src/chrome.rs), headless, 1280 x 800, as on main.
const OURS = [
  '--headless=new', '--disable-gpu', ...(windows ? [] : ['--disable-frame-rate-limit']), '--window-size=1280,800',
  '--no-first-run', '--no-default-browser-check', '--disable-background-networking', '--disable-component-update',
  '--disable-default-apps', '--disable-sync', '--disable-component-extensions-with-background-pages',
  '--disable-field-trial-config', '--disable-breakpad', '--disable-client-side-phishing-detection', '--disable-extensions',
  '--disable-hang-monitor', '--no-service-autorun', '--disable-search-engine-choice-screen', '--mute-audio',
  '--disable-gpu-shader-disk-cache', '--disable-smooth-scrolling', '--metrics-recording-only', '--password-store=basic',
  '--use-mock-keychain',
  `--disable-features=WebUIOmniboxPopup,GpuPersistentCache,SkiaGraphiteUsePersistentCache,Translate,MediaRouter,DialMediaRouteProvider,GlobalMediaControls,OptimizationHints,LensOverlay,CertificateTransparencyComponentUpdater,DestroyProfileOnBrowserClose,PaintHolding,Prerender2,ShoppingList,SegmentationPlatform,HistoryEmbeddings,AutofillServerCommunication,PartitionAllocBackupRefPtr${windows ? ',CalculateNativeWinOcclusion' : ''}`,
  ...(process.platform === 'linux' ? ['--enable-features=NetworkServiceInProcess2'] : []),
];

// Playwright 1.65.1 (playwright-core's coreBundle.js: chromiumSwitches, then its headless ones).
const PW = [
  '--disable-field-trial-config', '--disable-background-networking', '--disable-background-timer-throttling',
  '--disable-backgrounding-occluded-windows', '--disable-back-forward-cache', '--disable-breakpad',
  '--disable-component-extensions-with-background-pages', '--disable-component-update', '--no-default-browser-check',
  '--disable-default-apps', '--disable-dev-shm-usage', '--disable-edgeupdater', '--disable-extensions',
  '--disable-features=AvoidUnnecessaryBeforeUnloadCheckSync,DestroyProfileOnBrowserClose,HttpsUpgrades,HttpsFirstBalancedModeAutoEnable,LensOverlay,MediaRouter,PaintHolding,ThirdPartyStoragePartitioning,AvoidCorsURLLoaderRestartOnRedirect,TcpPortRandomizationWin,OptimizationHints,NetworkTimeServiceQuerying,AimEnabled,PreconnectToSearch,msForceBrowserSignIn,msEdgeUpdateLaunchServicesPreferredVersion',
  '--enable-features=CDPScreenshotNewSurface,SkipListAccountsWithoutGaiaCookies',
  '--allow-pre-commit-input', '--disable-hang-monitor', '--disable-ipc-flooding-protection', '--disable-popup-blocking',
  '--disable-prompt-on-repost', '--disable-renderer-backgrounding', '--disable-updater-scheduler', '--force-color-profile=srgb',
  '--metrics-recording-only', '--no-first-run', '--password-store=basic', '--use-mock-keychain', '--no-service-autorun',
  '--disable-search-engine-choice-screen', '--unsafely-disable-devtools-self-xss-warnings', '--enable-unsafe-swiftshader',
  '--edge-skip-compat-layer-relaunch', '--disable-stack-profiler', '--disable-infobars', '--disable-sync', '--do-not-de-elevate',
  '--headless', '--hide-scrollbars', '--mute-audio',
  '--blink-settings=primaryHoverType=2,availableHoverTypes=2,primaryPointerType=4,availablePointerTypes=4',
];
// ours-noproxy: virtuoso's, with no proxy looked for (a WPAD/DHCP proxy search held a first
// request up to 2.3 s in a local trace).
const SWITCHES = { ours: OURS, 'ours-noproxy': [...OURS, '--no-proxy-server'], pw: [...PW, '--no-sandbox'], 'pw-sandbox': PW };
const leftovers = [];

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const now = () => performance.now();

// A CDP client over a send function: calls answered by id, events to listeners.
function client(send) {
  let id = 0;
  const pending = new Map(), listeners = new Set();
  const receive = (text) => {
    const d = JSON.parse(text);
    if (d.id && pending.has(d.id)) { const p = pending.get(d.id); pending.delete(d.id); d.error ? p.j(new Error(JSON.stringify(d.error))) : p.r(d.result); }
    else for (const l of listeners) l(d);
  };
  const call = (method, params = {}, sessionId) => new Promise((r, j) => { const i = ++id; pending.set(i, { r, j }); send(JSON.stringify({ id: i, method, params, ...(sessionId ? { sessionId } : {}) })); });
  const once = (pred) => new Promise((r) => { const l = (d) => { if (pred(d)) { listeners.delete(l); r(d); } }; listeners.add(l); });
  return { call, once, receive };
}

let seq = 0;
async function launchOnce(shape) {
  const [tab, link, context, switches] = shape.split('-').length === 4 ? shape.split('-') : [...shape.split('-').slice(0, 3), shape.split('-').slice(3).join('-')];
  const profile = join(tmpdir(), `launch-shape-${process.pid}-${++seq}`);
  mkdirSync(profile, { recursive: true });
  const args = [...SWITCHES[switches], link === 'port' ? '--remote-debugging-port=0' : '--remote-debugging-pipe', `--user-data-dir=${profile}`, tab === 'tab' ? 'about:blank' : '--no-startup-window'];
  const t = {};
  const t0 = now();
  const child = spawn(CHROME, args, { stdio: link === 'pipe' ? ['ignore', 'ignore', 'ignore', 'pipe', 'pipe'] : 'ignore' });
  let c, closeLink;
  try {
    if (link === 'port') {
      const pf = join(profile, 'DevToolsActivePort');
      const deadline = t0 + 30000;
      let lines = [];
      while (now() < deadline) {
        if (existsSync(pf)) { lines = readFileSync(pf, 'utf8').split('\n'); if (lines.length >= 2 && lines[1].startsWith('/')) break; }
        await sleep(1);
      }
      if (lines.length < 2) throw new Error('no DevToolsActivePort within 30 s');
      const ws = new WebSocket(`ws://127.0.0.1:${lines[0].trim()}${lines[1].trim()}`);
      c = client((s) => ws.send(s));
      ws.onmessage = (m) => c.receive(m.data);
      await new Promise((r, j) => { ws.onopen = r; ws.onerror = () => j(new Error('WebSocket failed')); });
      closeLink = () => { try { ws.close(); } catch { /* gone */ } };
    } else {
      const out = child.stdio[3], inp = child.stdio[4];
      c = client((s) => out.write(s + '\0'));
      let buf = '';
      inp.on('data', (chunk) => {
        buf += chunk.toString('utf8');
        let i;
        while ((i = buf.indexOf('\0')) >= 0) { const m = buf.slice(0, i); buf = buf.slice(i + 1); c.receive(m); }
      });
      inp.on('error', () => {});
      out.on('error', () => {});
      closeLink = () => { try { out.end(); } catch { /* gone */ } };
    }
    t.connected = now() - t0;
    const { targetInfos } = await c.call('Target.getTargets');
    t.ready = now() - t0;
    let targetId;
    const blank = targetInfos.find((x) => x.type === 'page' && x.url === 'about:blank');
    if (context === 'new') {
      const { browserContextId } = await c.call('Target.createBrowserContext', { disposeOnDetach: true });
      ({ targetId } = await c.call('Target.createTarget', { url: 'about:blank', browserContextId }));
    } else if (tab === 'tab' && blank) {
      targetId = blank.targetId;
    } else {
      ({ targetId } = await c.call('Target.createTarget', { url: 'about:blank' }));
    }
    const { sessionId } = await c.call('Target.attachToTarget', { targetId, flatten: true });
    await c.call('Page.enable', {}, sessionId);
    t.page = now() - t0;
    const loaded = c.once((d) => d.method === 'Page.loadEventFired' && d.sessionId === sessionId);
    const nav = await c.call('Page.navigate', { url: URL_ }, sessionId);
    t.answered = now() - t0;
    if (nav.errorText) throw new Error(`navigate: ${nav.errorText}`);
    await Promise.race([loaded, sleep(30000).then(() => { throw new Error('no load within 30 s'); })]);
    t.loaded = now() - t0;
    t.startTabs = targetInfos.filter((x) => x.type === 'page').length;
  } finally {
    closeLink?.();
    await stop(child);
    leftovers.push(profile);
  }
  return t;
}

// The Chrome this script started, ended at once with every process it started (by its pid, as
// virtuoso ends its own on Windows), its exit waited for (3 s at most). Its profile is removed at
// the end of the script, once Windows has let its files go.
async function stop(child) {
  if (child.exitCode !== null || child.signalCode !== null) return;
  const exited = new Promise((r) => child.once('exit', r));
  if (windows) spawn('taskkill', ['/PID', String(child.pid), '/T', '/F'], { stdio: 'ignore' });
  else child.kill('SIGKILL');
  await Promise.race([exited, sleep(3000)]);
  await sleep(200);
}

async function removeLeftovers() {
  for (const p of leftovers) for (let i = 0; i < 20; i++) { try { rmSync(p, { recursive: true, force: true }); break; } catch { await sleep(250); } }
}

const median = (xs) => { const s = [...xs].sort((a, b) => a - b); const n = s.length; return n ? (n % 2 ? s[n >> 1] : (s[n / 2 - 1] + s[n / 2]) / 2) : NaN; };
const quart = (xs, q) => { const s = [...xs].sort((a, b) => a - b); return s.length ? s[Math.min(s.length - 1, Math.floor(q * (s.length - 1) + 0.5))] : NaN; };

const allShapes = [];
for (const sw of SWITCH_SETS) for (const tab of ['tab', 'none']) for (const link of ['port', 'pipe']) for (const ctx of ['default', 'new']) allShapes.push(`${tab}-${link}-${ctx}-${sw}`);
const shapes = opt('--shapes', '') ? opt('--shapes', '').split(',') : allShapes;

const results = Object.fromEntries(shapes.map((s) => [s, []]));
const failures = [];
// One unmeasured launch first: the binary and its DLLs into the file cache.
await launchOnce(shapes[0]).catch((e) => console.error(`warm-up: ${e.message}`));
for (let r = 0; r < RUNS; r++) {
  // Each run in a turned order: no shape always first after the last's teardown.
  const order = shapes.map((_, i) => shapes[(i + r) % shapes.length]);
  for (const s of order) {
    try { results[s].push(await launchOnce(s)); }
    catch (e) { failures.push({ shape: s, run: r, error: e.message }); console.error(`${s} run ${r}: ${e.message}`); }
  }
  console.error(`run ${r + 1}/${RUNS} done`);
}

await removeLeftovers();
const f = (x) => (Number.isFinite(x) ? x.toFixed(0) : '-');
const lines = [];
lines.push(`Chrome: ${CHROME}`, `URL: ${URL_}`, `runs per shape: ${RUNS} (${process.platform})`, '');
lines.push('| shape | n | connected | ready | page | answered | loaded (p25-p75) | start tabs |', '|---|---|---|---|---|---|---|---|');
for (const s of shapes) {
  const rs = results[s];
  const m = (k) => median(rs.map((x) => x[k]));
  lines.push(`| ${s} | ${rs.length} | ${f(m('connected'))} | ${f(m('ready'))} | ${f(m('page'))} | ${f(m('answered'))} | ${f(m('loaded'))} (${f(quart(rs.map((x) => x.loaded), 0.25))}-${f(quart(rs.map((x) => x.loaded), 0.75))}) | ${f(m('startTabs'))} |`);
}
// Each choice's effect on `loaded`: the median over the pairs of shapes that differ only in it of
// the difference of their medians (second option minus first).
const effect = (i, a, b) => {
  const d = [];
  for (const s of shapes) {
    const parts = s.split('-');
    const head = parts.slice(0, 3), sw = parts.slice(3).join('-');
    const all = [...head, sw];
    if (all[i] !== a) continue;
    const other = [...all]; other[i] = b;
    const o = `${other[0]}-${other[1]}-${other[2]}-${other[3]}`;
    if (!results[o] || !results[s].length || !results[o].length) continue;
    d.push(median(results[o].map((x) => x.loaded)) - median(results[s].map((x) => x.loaded)));
  }
  return d.length ? `${f(median(d))} ms (${d.length} pairs: ${d.map(f).join(', ')})` : 'n/a';
};
lines.push('', 'Effect on loaded (median of paired shape medians, the other choices held):');
lines.push(`- no startup tab (none - tab): ${effect(0, 'tab', 'none')}`);
lines.push(`- pipe (pipe - port): ${effect(1, 'port', 'pipe')}`);
lines.push(`- new context (new - default): ${effect(2, 'default', 'new')}`);
if (SWITCH_SETS.includes('ours-noproxy')) lines.push(`- no proxy search (ours-noproxy - ours): ${effect(3, 'ours', 'ours-noproxy')}`);
if (SWITCH_SETS.includes('pw')) lines.push(`- Playwright's switches (pw - ours): ${effect(3, 'ours', 'pw')}`);
if (SWITCH_SETS.includes('pw-sandbox')) lines.push(`- Playwright's, sandboxed (pw-sandbox - ours): ${effect(3, 'ours', 'pw-sandbox')}`);
if (failures.length) lines.push('', `${failures.length} failed launches: ${failures.map((x) => `${x.shape}#${x.run} ${x.error}`).join('; ')}`);
console.log(lines.join('\n'));
if (OUT) writeFileSync(OUT, JSON.stringify({ chrome: CHROME, url: URL_, runs: RUNS, platform: process.platform, results, failures }, null, 1));
