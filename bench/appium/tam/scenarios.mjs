// Tester Army's mobile benchmark (github.com/tester-army/e2e, apps/mobile-benchmark) as Appium
// tests, step for step with the bench's flows (bench/tam-flows): UiAutomator2, UiSelector
// lookups, the W3C calls and UiAutomator2's `mobile:` gestures an Appium user reaches for. On an
// iOS simulator (VIRTUOSO_BENCH_PLATFORM=ios) the same steps through the XCUITest driver: test
// ids as accessibility ids, text by predicate, its own `mobile:` gestures.
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

export const APP = { appPackage: 'dev.e2e.benchmark', appActivity: '.MainActivity', bundleId: 'dev.e2e.benchmark' };

const IOS = process.env.VIRTUOSO_BENCH_PLATFORM === 'ios';
const predicate = (p) => ({ using: '-ios predicate string', value: p });
const quoted = (t) => JSON.stringify(t);
const id = (v) => (IOS ? { using: 'accessibility id', value: v } : `new UiSelector().resourceId(${JSON.stringify(v)})`);
const text = (t) => (IOS ? predicate(`label == ${quoted(t)} OR value == ${quoted(t)}`) : `new UiSelector().text(${JSON.stringify(t)})`);
const textMatches = (re) => (IOS ? predicate(`label MATCHES ${quoted(re)} OR value MATCHES ${quoted(re)}`) : `new UiSelector().textMatches(${JSON.stringify(re)})`);
const startsWith = (t) => (IOS ? predicate(`label BEGINSWITH ${quoted(t)}`) : `new UiSelector().descriptionStartsWith(${JSON.stringify(t)})`);
const field = (i) =>
  IOS ? { using: '-ios class chain', value: `**/XCUIElementTypeTextField[${i + 1}]` } : `new UiSelector().className("android.widget.EditText").instance(${i})`;
// Into view down the screen's list, as far as it takes: UiScrollable's scrollIntoView took the
// screen's outer scroll view, so a drag up the middle of the screen until it shows.
async function into(s, selector) {
  const { width, height } = await s.call('GET', '/window/rect');
  for (let i = 0; i < 200 && !(await s.exists(selector)); i++) {
    await mobile(s, 'scrollGesture', { left: width * 0.1, top: height * 0.3, width: width * 0.8, height: height * 0.5, direction: 'down', percent: 0.9 });
  }
  // Still, before a tap: the list coasts on after the drag, and a tap on where it was misses.
  const at = async () => JSON.stringify(await s.call('GET', `/element/${await s.find(selector)}/rect`));
  for (let i = 0, was = await at(), now; i < 20 && (now = await at()) !== was; i++) was = now;
  return selector;
}

// Opens a scenario from the home list (its row's test id is the scenario's name).
async function open(s, name) {
  await s.tap(id(name));
}

// Focus, then type into what has focus (WebDriver's active element): a field can leave the
// tree as the keyboard comes up (Sequential Onboarding's).
async function type(s, selector, value) {
  // A touch on it, as a finger's: an element click left Sequential Onboarding's field unfocused.
  await mobile(s, 'clickGesture', { elementId: await s.find(selector) });
  // Focus lands a moment after the touch; when the focused field has left the tree (Sequential
  // Onboarding's last name, under the keyboard), UiAutomator2's own typing into what has focus.
  for (let i = 0; i < 10; i++) {
    const active = await s.call('GET', '/element/active').then((e) => Object.values(e)[0], () => null);
    if (active) return s.call('POST', `/element/${active}/value`, { text: value });
    await new Promise((r) => setTimeout(r, 100));
  }
  await mobile(s, 'type', { text: value });
}

// Down, and checked: when Appium's hide does not take it down, Back does (on iOS, where Back is
// the previous screen, the keyboard's own Done or Return key).
async function hideKeyboard(s) {
  const shown = () => s.call('GET', '/appium/device/is_keyboard_shown');
  if (!(await shown())) return;
  await s.call('POST', '/appium/device/hide_keyboard', IOS ? { keys: ['Done', 'Return', 'return'] } : {}).catch(() => {});
  if ((await shown()) && !IOS) await s.back();
}
// UiAutomator2's gestures; on iOS the XCUITest driver's of the same kind.
async function mobile(s, script, args) {
  const run = (name, a) => s.call('POST', '/execute/sync', { script: `mobile: ${name}`, args: [a] });
  if (!IOS) return run(script, args);
  const at = args.elementId ? { elementId: args.elementId } : { x: args.x, y: args.y };
  switch (script) {
    // XCUITest's tap takes a point only: an element is clicked.
    case 'clickGesture':
      return args.elementId ? s.call('POST', `/element/${args.elementId}/click`, {}) : run('tap', at);
    // Two taps the app counts as a double tap: presses within 300 ms of each other, the second
    // after React Native's 130 ms minimum press of the first.
    case 'doubleClickGesture': {
      const r = args.elementId ? await s.rect(args.elementId) : { x: args.x, y: args.y, width: 0, height: 0 };
      const x = Math.round(r.x + r.width / 2), y = Math.round(r.y + r.height / 2);
      const tap = [{ type: 'pointerDown', button: 0 }, { type: 'pause', duration: 100 }, { type: 'pointerUp', button: 0 }];
      return s.call('POST', '/actions', {
        actions: [{ type: 'pointer', id: 'finger', parameters: { pointerType: 'touch' },
          actions: [{ type: 'pointerMove', duration: 0, x, y }, ...tap, { type: 'pause', duration: 80 }, ...tap] }],
      });
    }
    case 'longClickGesture':
      return run('touchAndHold', { ...at, duration: (args.duration ?? 1000) / 1000 });
    case 'swipeGesture':
      return run('swipe', { ...(args.elementId ? { elementId: args.elementId } : {}), direction: args.direction });
    // UiAutomator2 scrolls the content in its direction: so does XCUITest's scroll.
    case 'scrollGesture':
      return run('scroll', { direction: args.direction });
    // Typing into what has focus.
    case 'type': {
      const active = Object.values(await s.call('GET', '/element/active'))[0];
      return s.call('POST', `/element/${active}/value`, { text: args.text });
    }
    default:
      throw new Error(`no iOS gesture for ${script}`);
  }
}

// A tap at a point on the screen, as a fraction of its size.
async function tapAt(s, fx, fy) {
  const { width, height } = await s.call('GET', '/window/rect');
  await mobile(s, 'clickGesture', { x: Math.round(width * fx), y: Math.round(height * fy) });
}

async function expectText(s, selector, want) {
  const el = await s.find(selector);
  const got = await s.call('GET', `/element/${el}/text`);
  if (!(want instanceof RegExp ? want.test(got) : got === want)) throw new Error(`expected ${want}, got "${got}"`);
}

export const SCENARIOS = {
  async login_form(s) {
    await open(s, 'Login Form');
    await type(s, id('email-input'), 'tester@tester.army');
    await type(s, id('password-input'), 'benchmark123');
    await hideKeyboard(s);
    await s.tap(id('login-button'));
    await expectText(s, id('success-message'), 'Logged in successfully');
    await s.tap(id('logout-button'));
    await s.find(id('login-button'));
  },
  async infinite_scroll_list(s) {
    await open(s, 'Infinite Scroll List');
    await s.tap(await into(s, id('item-137')));
    await s.find(text('Found item 137'));
  },
  async modal_flow(s) {
    await open(s, 'Modal Flow');
    await s.tap(id('open-modal-button'));
    await s.tap(id('continue-button'));
    // The native alert's button, which Android spells in capitals.
    await s.tap(textMatches('(?i)confirm'));
    await s.find(text('Flow completed'));
  },
  async bottom_tabs(s) {
    await open(s, 'Bottom Tabs');
    await s.find(id('home-tab-content'));
    await s.tap(id('tab-actions'));
    await s.tap(id('complete-action-button'));
    await s.find(text('Action completed'));
  },
  async text_input_variations(s) {
    await open(s, 'Text Input Variations');
    await type(s, id('username-input'), 'tester');
    await type(s, id('pin-input'), '1234');
    await type(s, id('notes-input'), 'Notes long enough to count');
    await hideKeyboard(s);
    await s.tap(id('submit-button'));
    await s.find(text('Form submitted'));
  },
  async gestures(s) {
    await open(s, 'Gestures');
    await mobile(s, 'longClickGesture', { elementId: await s.find(id('long-press-target')), duration: 1200 });
    await s.find(text('Step 2 of 3'));
    // A swipe the app counts: the whole width of the target, quickly.
    await mobile(s, 'swipeGesture', { elementId: await s.find(id('swipe-target')), direction: 'left', percent: 1, speed: 5000 });
    await s.find(text('Step 3 of 3'));
    await mobile(s, 'doubleClickGesture', { elementId: await s.find(id('double-tap-target')) });
    await s.find(text('All gestures completed'));
  },
  async async_states(s) {
    await open(s, 'Async States');
    await s.find(text('Pull down to refresh and reveal your reward'));
    // Pull to refresh, again until the reward shows.
    for (let i = 0; i < 5 && !(await s.exists(id('claim-button'))); i++) {
      await mobile(s, 'swipeGesture', { elementId: await s.find(id('reward-scroll')), direction: 'down', percent: 0.8 });
    }
    await s.tap(id('claim-button'));
    await s.find(text('Reward claimed'));
  },
  async debounced_search(s) {
    await open(s, 'Debounced Search');
    await type(s, id('search-input'), 'benchmark target');
    await hideKeyboard(s);
    await s.tap(id('result-Benchmark Target'));
    await s.find(text('Selected Benchmark Target'));
  },
  async huge_virtualized_list(s) {
    await open(s, 'Huge Virtualized List');
    await s.find(text('Row 0001'));
    await s.tap(await into(s, id('row-512')));
    await s.find(text('Found Row 0512'));
  },
  async sticky_chrome_target(s) {
    await open(s, 'Sticky Chrome Target');
    await s.tap(id('continue-button'));
    await s.find(text('Accept the terms at the bottom first'));
    await into(s, id('accept-terms'));
    // In the tree is not in the clear: the checkbox can sit under the sticky footer, which takes
    // a tap at its centre. A short drag more while the footer covers any of it.
    const { width, height } = await s.call('GET', '/window/rect');
    const footer = await s.rect(await s.find(id('continue-button')));
    for (let i = 0; i < 10; i++) {
      const box = await s.rect(await s.find(id('accept-terms')));
      if (box.y + box.height <= footer.y) break;
      await mobile(s, 'scrollGesture', { left: width * 0.1, top: height * 0.3, width: width * 0.8, height: height * 0.4, direction: 'down', percent: 0.3 });
    }
    await s.tap(await into(s, id('accept-terms')));
    await s.find(text('Terms accepted ✓'));
    await s.tap(id('continue-button'));
    await expectText(s, id('success-message'), 'Terms accepted');
  },
  async permission_prompt(s) {
    await open(s, 'Permission Prompt');
    await s.tap(id('request-permission'));
    // The system dialog, when the app has to ask: whichever shows first.
    const either = await s.find(textMatches('While using the app|Microphone enabled'));
    if ((await s.call('GET', `/element/${either}/text`)) !== 'Microphone enabled') await s.tap(text('While using the app'));
    await s.find(text('Microphone enabled'));
  },
  async product_catalog(s) {
    await open(s, 'Product Catalog');
    await s.tap(id('product-trail-mix-500'));
    await s.tap(id('quantity-increase'));
    await expectText(s, id('quantity-value'), '2');
    await s.tap(id('add-to-cart'));
    await s.tap(id('open-cart'));
    await s.find(text('Order ready: 2 × Trail Mix 500 g'));
  },
  async error_recovery(s) {
    await open(s, 'Error Recovery');
    await s.find(id('error-banner'));
    await s.tap(id('retry-button'));
    await s.tap(id('delete-draft'));
    await s.tap(id('confirm-delete'));
    await s.find(text('Draft deleted'));
  },
  async choice_controls(s) {
    await open(s, 'Choice Controls');
    for (const t of ['size-medium', 'topping-cheese', 'topping-olives', 'rush-delivery', 'place-order']) await s.tap(id(t));
    await s.find(text('Order placed: Medium with Cheese, Olives (rush)'));
  },
  async sequential_onboarding(s) {
    await open(s, 'Sequential Onboarding');
    await type(s, id('email-input'), 'jane@example.com');
    await hideKeyboard(s);
    await s.tap(id('continue-button'));
    await type(s, id('first-name-input'), 'Jane');
    await hideKeyboard(s);
    await type(s, await into(s, id('last-name-input')), 'Merchant');
    await hideKeyboard(s);
    await s.tap(id('continue-button'));
    await type(s, id('phone-input'), '0612435678');
    await hideKeyboard(s);
    await s.tap(id('continue-button'));
    await s.find(text('Onboarding complete'));
    for (const t of ['jane@example.com', 'Jane Merchant', '0612435678']) await s.find(text(t));
  },
  async broken_accessibility(s) {
    await open(s, 'Broken Accessibility');
    await s.find(text('Tap the green circle to continue'));
    // The shapes carry no label: the green circle by where it is on screen.
    await tapAt(s, 0.587, 0.58);
    await type(s, field(0), '7291');
    await s.tap(text('→'));
    await s.find(text('Access granted'));
  },
  async vision_only(s) {
    await open(s, 'Vision Only');
    await s.find(text('Progress: 0/3'));
    // The shapes have no labels: square, circle and diamond by where they are.
    for (const fx of [0.694, 0.306, 0.5]) await tapAt(s, fx, 0.6075);
    await s.find(text('Sequence complete'));
  },
  async flattened_registration_form(s) {
    await open(s, 'Flattened Registration Form');
    // The fields have no label or id of their own: in order, under their captions.
    for (const [i, v] of ['Jane', 'Merchant', 'jane@example.com', 'benchmark123'].entries()) await type(s, field(i), v);
    await hideKeyboard(s);
    await s.tap(text('CREATE ACCOUNT'));
    await s.find(text('Account created'));
  },
  async flattened_login(s) {
    await open(s, 'Flattened Login');
    await type(s, field(0), 'tester@tester.army');
    await type(s, field(1), 'benchmark123');
    await hideKeyboard(s);
    await s.tap(text('SIGN IN'));
    await s.find(text('Logged in successfully'));
  },
  async webview_accessibility(s) {
    await open(s, 'WebView Accessibility');
    // The page renders after the native screen; its coupon code is read from it.
    const code = await s.call('GET', `/element/${await s.find(id('coupon'))}/attribute/content-desc`);
    await type(s, id('coupon-input'), code || 'TA-BENCH-50');
    await hideKeyboard(s);
    await s.tap(id('terms'));
    await s.tap(id('apply'));
    await s.find(text('Coupon applied'));
  },
  async photo_picker(s) {
    // The receipt to pick, newest in the gallery.
    const serial = process.env.VIRTUOSO_BENCH_DEVICE ?? 'emulator-5554';
    const photo = fileURLToPath(new URL('../../tam-flows/media/receipt.png', import.meta.url));
    const name = `receipt-${Date.now()}.png`;
    execFileSync('adb', ['-s', serial, 'push', photo, `/sdcard/Pictures/${name}`]);
    execFileSync('adb', ['-s', serial, 'shell', 'am', 'broadcast', '-a', 'android.intent.action.MEDIA_SCANNER_SCAN_FILE', '-d', `file:///sdcard/Pictures/${name}`]);
    await open(s, 'Photo Picker');
    await s.tap(id('choose-photo'));
    // The system photo picker, newest first.
    await s.tap(startsWith('Photo taken on'));
    await expectText(s, id('success-message'), /^Receipt attached: /);
  },
  async control_inventory(s) {
    await open(s, 'Control Inventory');
    // One exercise per verb, a tab of controls at a time, each checked by its status line.
    const name = await s.find(id('name-input'));
    await s.call('POST', `/element/${name}/clear`, {});
    await s.call('POST', `/element/${name}/value`, { text: 'Grace Hopper' });
    await hideKeyboard(s);
    await expectText(s, id('name-echo'), 'name: Grace Hopper');
    await s.tap(id('tab-toggles'));
    await s.tap(id('wifi-switch'));
    await expectText(s, id('wifi-status'), 'wifi: on');
    await s.tap(id('newsletter-checkbox'));
    await expectText(s, id('newsletter-status'), 'newsletter: on');
    await s.tap(id('size-large'));
    await expectText(s, id('size-status'), 'size: Large');
    await s.find(await into(s, id('delayed-button')));
    await s.find(`${id('delayed-button')}.enabled(true)`);
    await s.tap(id('delayed-button'));
    await expectText(s, id('delayed-status'), 'delayed: pressed');
    await s.tap(id('tab-gestures'));
    await mobile(s, 'longClickGesture', { elementId: await s.find(id('long-press-target')) });
    await expectText(s, id('gesture-status'), 'gesture: long press');
    await mobile(s, 'doubleClickGesture', { elementId: await s.find(id('double-tap-target')) });
    await expectText(s, id('tap-count'), /^2 taps/);
    await s.tap(await into(s, id('tap-pad')));
    await expectText(s, id('pad-status'), /^pad: (?!none)/);
    await mobile(s, 'swipeGesture', { elementId: await s.find(await into(s, id('swipe-pad'))), direction: 'right', percent: 0.8 });
    await expectText(s, id('swipe-status'), /^swipe: .* to /);
  },
};
