// The benchmark's Settings scenarios (bench/flows/*.yaml) as Appium tests, written the way an
// Appium user writes them: a UiAutomator2 session, UiSelector lookups with an implicit wait,
// and the W3C WebDriver calls a client library would make. The Appium server runs in this
// process and starts and stops with each run, as the other tools' agents do, so one process
// holds everything the run needs on the host.
//
//   node run.mjs <scenario>.yaml     the scenario, on VIRTUOSO_BENCH_DEVICE (emulator-5554); run
//                                    from tam/, Tester Army's mobile benchmark's (tam/scenarios.mjs)
//   node run.mjs --version
import { main as appium } from 'appium';
import { readFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { basename, join } from 'node:path';

const PORT = 4723;
const BASE = `http://127.0.0.1:${PORT}`;
const SETTINGS = 'com.android.settings';
const WAIT_MS = 10000;
const KEYCODE_DEL = 67;

const arg = process.argv[2] ?? '';
if (arg === '--version') {
  const home = process.env.APPIUM_HOME ?? join(homedir(), '.appium');
  const version = (p) => JSON.parse(readFileSync(p, 'utf8')).version;
  const server = version(new URL('./node_modules/appium/package.json', import.meta.url));
  // The driver of the platform: XCUITest's on an iOS simulator, UiAutomator2's on Android.
  const ios = process.env.VIRTUOSO_BENCH_PLATFORM === 'ios';
  const driver = version(join(home, 'node_modules', ios ? 'appium-xcuitest-driver' : 'appium-uiautomator2-driver', 'package.json'));
  console.log(`appium ${server} (${ios ? 'xcuitest' : 'uiautomator2'} ${driver})`);
  process.exit(0);
}

const text = (t) => `new UiSelector().text(${JSON.stringify(t)})`;

// One function per scenario, step for step with its flow file.
const SCENARIOS = {
  async launch(s) {
    await s.find(text('Network & internet'));
  },
  async navigate(s) {
    for (const [screen, check] of [['Network & internet', 'Internet'], ['Connected devices', 'Pair new device'], ['Apps', 'Recently opened apps']]) {
      await s.tap(text(screen));
      await s.find(text(check));
      await s.back();
    }
    await s.find(text('Notifications'));
  },
  async scroll(s) {
    const into = (t) => `new UiScrollable(new UiSelector().scrollable(true)).scrollIntoView(${text(t)})`;
    await s.find(into('About emulated device'));
    await s.tap(text('About emulated device'));
    await s.find(text('Device name'));
    await s.back();
    await s.find(into('Network & internet'));
  },
  async input(s) {
    await s.tap(text('Search Settings'));
    const field = await s.find('new UiSelector().focused(true)');
    await s.call('POST', `/element/${field}/value`, { text: 'brightness level' });
    for (let i = 0; i < 20; i++) await s.call('POST', '/appium/device/press_keycode', { keycode: KEYCODE_DEL });
    await s.call('POST', `/element/${field}/value`, { text: 'dark theme' });
    await s.call('POST', '/appium/device/hide_keyboard', {});
    await s.back();
  },
  async assertions(s) {
    for (const t of ['Network & internet', 'Connected devices', 'Apps', 'Notifications', 'Sound & vibration', 'Display & touch', 'Search Settings']) {
      await s.find(text(t));
    }
    await s.find(`new UiSelector().resourceId("${SETTINGS}:id/search_bar_title")`);
    await s.find('new UiSelector().textMatches("Mobile.*hotspot")');
    // No relative selectors in UiAutomator: compare the two elements' frames.
    const apps = await s.rect(await s.find(text('Apps')));
    const devices = await s.rect(await s.find(text('Connected devices')));
    if (apps.y < devices.y + devices.height) throw new Error('Apps is not below Connected devices');
    if (await s.exists(text('This text is not on screen'))) throw new Error('"This text is not on screen" is on screen');
    for (const t of ['Network & internet', 'Connected devices', 'Apps', 'Notifications']) await s.find(text(t));
  },
  async taps(s) {
    for (let i = 0; i < 5; i++) {
      await s.tap(text('Network & internet'));
      await s.find(text('Internet'));
      await s.back();
    }
  },
  async trivial(s) {
    await s.call('POST', '/appium/device/press_keycode', { keycode: 3 });
  },
};

// On an iOS simulator (VIRTUOSO_BENCH_PLATFORM=ios): the XCUITest driver, by bundle id.
const IOS = process.env.VIRTUOSO_BENCH_PLATFORM === 'ios';

class Session {
  async start(serial, app) {
    const caps = IOS
      ? {
          platformName: 'iOS',
          'appium:automationName': 'XCUITest',
          'appium:udid': serial,
          'appium:bundleId': app.bundleId,
          // WebDriverAgent builds on the first session: the warm-up run takes it.
          'appium:wdaLaunchTimeout': 300000,
        }
      : {
          platformName: 'Android',
          'appium:automationName': 'UiAutomator2',
          'appium:udid': serial,
          'appium:appPackage': app.appPackage,
          'appium:appActivity': app.appActivity,
        };
    const value = await request('POST', '/session', {
      capabilities: {
        alwaysMatch: {
          ...caps,
          // The harness resets the device before each run.
          'appium:noReset': true,
          'appium:newCommandTimeout': 60,
        },
      },
    });
    this.id = value.sessionId;
    await this.call('POST', '/timeouts', { implicit: WAIT_MS });
  }
  call(method, path, body) {
    return request(method, `/session/${this.id}${path}`, body);
  }
  // A selector string is UiAutomator's (Android); one with its own strategy is { using, value }.
  static locate(selector) {
    return typeof selector === 'string' ? { using: '-android uiautomator', value: selector } : selector;
  }
  async find(selector) {
    try {
      const el = await this.call('POST', '/element', Session.locate(selector));
      return Object.values(el)[0];
    } catch (e) {
      throw new Error(`${JSON.stringify(selector)}: ${e.message}`);
    }
  }
  async exists(selector) {
    await this.call('POST', '/timeouts', { implicit: 0 });
    const found = await this.call('POST', '/elements', Session.locate(selector));
    await this.call('POST', '/timeouts', { implicit: WAIT_MS });
    return found.length > 0;
  }
  async tap(selector) {
    await this.call('POST', `/element/${await this.find(selector)}/click`, {});
  }
  rect(id) {
    return this.call('GET', `/element/${id}/rect`);
  }
  back() {
    return this.call('POST', '/back', {});
  }
  stop() {
    return this.id && this.call('DELETE', '');
  }
}

async function request(method, path, body) {
  const res = await fetch(BASE + path, {
    method,
    headers: { 'content-type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const json = await res.json();
  if (json.value?.error) throw new Error(`${method} ${path}: ${json.value.error}: ${json.value.message.split('\n')[0]}`);
  return json.value;
}

const name = arg.replace(/\\/g, '/').split('/').pop().replace(/\.yaml$/, '');
// The suite is the folder the harness runs it from: tam/ for the mobile benchmark, else Settings.
const suite = basename(process.cwd()) === 'tam' ? await import('./tam/scenarios.mjs') : { APP: { appPackage: SETTINGS, appActivity: '.Settings' }, SCENARIOS };
const scenario = suite.SCENARIOS[name];
if (!scenario) {
  console.error(`no Appium test for "${arg}" (scenarios: ${Object.keys(suite.SCENARIOS).join(', ')})`);
  process.exit(2);
}
const server = await appium({ port: PORT, address: '127.0.0.1', loglevel: 'error' });
const session = new Session();
let code = 0;
try {
  await session.start(process.env.VIRTUOSO_BENCH_DEVICE ?? 'emulator-5554', suite.APP);
  await scenario(session);
  console.log(`${name}: passed`);
} catch (e) {
  console.error(`${name}: FAILED: ${e.message}`);
  code = 1;
}
try {
  await session.stop();
} catch {}
await server.close();
process.exit(code);
