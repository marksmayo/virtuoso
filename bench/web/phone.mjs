// The phone a run emulates, if any: VIRTUOSO_BENCH_EMULATE names one in phones.json (the same
// values virtuoso's --emulate uses: its viewport, device pixel ratio and user agent).
import { readFileSync } from 'node:fs';

export function phone() {
  const name = process.env.VIRTUOSO_BENCH_EMULATE;
  if (!name) return null;
  const all = JSON.parse(readFileSync(new URL('./phones.json', import.meta.url), 'utf8'));
  const p = all[name];
  if (!p) throw new Error(`no phone called ${name} in phones.json: ${Object.keys(all).join(', ')}`);
  return p;
}
