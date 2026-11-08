// The flow's runScript (scripts/plan.js) and its checks, as one script: no variables cross into the steps.
import { execFileSync } from 'node:child_process';

const devices = execFileSync('adb', ['devices'], { encoding: 'utf8' });
const serial = process.env.VIRTUOSO_BENCH_DEVICE || (devices.match(/^(emulator-\d+)\tdevice/m) ?? [])[1];
const adb = (...args) => execFileSync('adb', serial ? ['-s', serial, ...args] : args, { encoding: 'utf8' });
const GREETING = 'Hello';
const plan = 'Pro';
const greeting = GREETING + ', ' + plan.toLowerCase();
console.log('picked ' + plan);
if (plan !== 'Pro' || greeting !== 'Hello, pro') throw new Error('the plan script handed back the wrong values');
void adb;
