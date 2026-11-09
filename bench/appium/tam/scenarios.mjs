// Tester Army's mobile benchmark (github.com/tester-army/e2e, apps/mobile-benchmark) as Appium
// tests, step for step with the bench's flows (bench/tam-flows): UiAutomator2, UiSelector
// lookups, the W3C calls and UiAutomator2's `mobile:` gestures an Appium user reaches for. On an
// iOS simulator (VIRTUOSO_BENCH_PLATFORM=ios) the same steps through the XCUITest driver: test
// ids as accessibility ids, text by predicate, its own `mobile:` gestures.
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { IOS, predicate, quoted, id, text, textMatches, startsWith, field, into, type, hideKeyboard, mobile, tapAt, expectText } from '../common.mjs';

export const APP = { appPackage: 'dev.e2e.benchmark', appActivity: '.MainActivity', bundleId: 'dev.e2e.benchmark' };

// Opens a scenario from the home list (its row's test id is the scenario's name).
async function open(s, name) {
  await s.tap(id(name));
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
