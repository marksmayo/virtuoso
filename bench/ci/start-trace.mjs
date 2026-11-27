// What Chrome did before a flow's first page committed (round 4's NW-03), read from the startup
// traces a traced virtuoso leaves (VIRTUOSO_CHROME_TRACE=<folder>: see trace_args in chrome.rs).
//
// usage: node bench/ci/start-trace.mjs <trace.json | folder>... [--min 5]
//
// For each trace, times from Chrome's entry point (Startup.BrowserMainEntryPoint, ms):
// - the browser's start-up phases (ChromeBrowserMainParts::*, the profile's creation);
// - the startup tab's commit (about:blank);
// - the first DevTools command (virtuoso connected);
// - the flow's page: its navigation asked for, its response started and its commit.
// Then the tasks that ran before the page's commit, on the browser's main and IO threads, the
// network service and the renderers, `--min` ms or longer (default 5). They are named by where
// they were posted from (the `toplevel` category's src_file:src_func) and summed over the traces:
// how many traces had each one, and the median of their totals.
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';

const argv = process.argv.slice(2);
const minAt = argv.indexOf('--min');
const MIN = minAt >= 0 ? +argv[minAt + 1] : 5;
const inputs = argv.filter((a, i) => !(minAt >= 0 && (i === minAt || i === minAt + 1)));
const files = inputs.flatMap((p) => (statSync(p).isDirectory() ? readdirSync(p).filter((f) => f.endsWith('.json')).map((f) => join(p, f)) : [p]));
if (!files.length) { console.error('start-trace: give trace files or folders'); process.exit(2); }

const median = (xs) => { const s = [...xs].sort((a, b) => a - b); const n = s.length; return n ? (n % 2 ? s[n >> 1] : (s[n / 2 - 1] + s[n / 2]) / 2) : NaN; };
const f = (x) => (Number.isFinite(x) ? x.toFixed(1) : '-');

const PHASES = [
  'ChromeBrowserMainParts::PreEarlyInitialization', 'BrowserMainLoop::CreateMainMessageLoop', 'ChromeBrowserMainParts::PreCreateThreads',
  'BrowserMainLoop::CreateThreads', 'ChromeBrowserMainParts::PreMainMessageLoopRun', 'ChromeBrowserMainParts::CreateProfile',
  'ChromeBrowserMainParts::PreBrowserStart', 'StartupBrowserCreator::Start',
];
const milestones = {};
const tasks = new Map();
const perTrace = [];
for (const file of files) {
  // Not all UTF-8 (Windows names): read as bytes, the JSON's structure is ASCII.
  const t = JSON.parse(readFileSync(file, 'latin1'));
  const ev = t.traceEvents || t;
  const proc = {}, thread = {};
  for (const e of ev) {
    if (e.ph !== 'M') continue;
    if (e.name === 'process_name') proc[e.pid] = e.args.name;
    if (e.name === 'thread_name') thread[`${e.pid}:${e.tid}`] = e.args.name;
  }
  const entry = ev.find((e) => e.name === 'Startup.BrowserMainEntryPoint') || ev.find((e) => e.name === 'BrowserMain');
  if (!entry) { console.error(`${file}: no browser start in it`); continue; }
  const t0 = entry.ts;
  const at = (e) => (e.ts - t0) / 1000;
  const m = {};
  for (const name of PHASES) {
    const e = ev.find((x) => x.name === name && x.ph === 'X');
    if (e) m[`${name} (dur)`] = e.dur / 1000;
  }
  const commits = ev.filter((e) => e.name === 'RenderFrameHostImpl::DidCommitNavigation');
  const urlOf = (e) => (JSON.stringify(e.args || {}).match(/"url":"(https?:[^"]+|about:blank)"/) || [])[1];
  const blank = commits.find((e) => urlOf(e) === 'about:blank');
  const page = commits.find((e) => (urlOf(e) || '').startsWith('http'));
  const devtools = ev.find((e) => e.name === 'DevToolsSession::HandleCommand in Browser');
  if (blank) m['startup tab committed'] = at(blank);
  if (devtools) m['first DevTools command'] = at(devtools);
  if (page) {
    const asked = ev.filter((e) => e.name === 'NavigationRequest::Create' && e.ts <= page.ts && (!devtools || e.ts >= devtools.ts)).pop();
    const response = ev.filter((e) => e.name === 'NavigationRequest::OnResponseStarted' && e.ts <= page.ts).pop();
    if (asked) m['page navigation asked'] = at(asked);
    if (response) m['page response started'] = at(response);
    m['page committed'] = at(page);
  }
  for (const [k, v] of Object.entries(m)) (milestones[k] ||= []).push(v);
  perTrace.push({ file, ...m });
  const end = page ? page.ts : Infinity;
  const totals = new Map();
  for (const e of ev) {
    if (e.ph !== 'X' || !/RunTask/.test(e.name) || e.ts < t0 || e.ts > end || e.dur / 1000 < MIN) continue;
    const th = thread[`${e.pid}:${e.tid}`] || `tid ${e.tid}`;
    const where = e.args && e.args.src_file ? `${e.args.src_file}:${e.args.src_func}` : e.name;
    const key = `${proc[e.pid] || e.pid} / ${th.replace(/[0-9]+$/, '')} / ${where}`;
    totals.set(key, (totals.get(key) || 0) + e.dur / 1000);
  }
  for (const [k, v] of totals) { if (!tasks.has(k)) tasks.set(k, []); tasks.get(k).push(v); }
}

const out = [];
out.push(`${perTrace.length} traces; times in ms from Chrome's entry point (medians, then each trace's)`, '');
out.push('| milestone | median | each |', '|---|---|---|');
for (const [k, v] of Object.entries(milestones)) out.push(`| ${k} | ${f(median(v))} | ${v.map(f).join(', ')} |`);
out.push('', `Tasks of ${MIN} ms or more before the page's commit (process / thread / posted from):`, '');
out.push('| task | traces | median total | max |', '|---|---|---|---|');
const rows = [...tasks.entries()].map(([k, v]) => [k, v.length, median(v), Math.max(...v)]).sort((a, b) => b[1] * b[2] - a[1] * a[2]).slice(0, 40);
for (const [k, n, med, max] of rows) out.push(`| ${k} | ${n} | ${f(med)} | ${f(max)} |`);
console.log(out.join('\n'));
