// The Speedrun Bench app's flows (github.com/hubastard/speedrun, bench/app/flows) as Appium
// tests, step for step with the flow files: UiAutomator2 lookups, the W3C calls and the
// driver's `mobile:` commands an Appium user reaches for. On an iOS simulator
// (VIRTUOSO_BENCH_PLATFORM=ios) the same steps through the XCUITest driver. What Maestro does in
// one command (launchApp clearState, dark mode, launch arguments, permissions, media) is the
// driver's own command for it.
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { IOS, predicate, quoted, id, text, textMatches, hideKeyboard, mobile } from '../common.mjs';

const APP_ID = 'dev.speedrun.bench';
export const APP = { appPackage: APP_ID, appActivity: '.MainActivity', bundleId: APP_ID };
const KEYCODE = { HOME: 3, PASTE: 279 };
const MEDIA = new URL('../../../../speedrun/bench/app/flows/media/photo.png', import.meta.url);

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const run = (s, name, args = {}) => s.call('POST', '/execute/sync', { script: `mobile: ${name}`, args: [args] });
const xpath = (x) => ({ using: 'xpath', value: x });
const exact = (t) => (IOS ? predicate(`label == ${quoted(t)}`) : `new UiSelector().text(${JSON.stringify(t)})`);
const app = IOS ? { bundleId: APP_ID } : { appId: APP_ID };

// Every match, in the order the driver lists them.
async function all(s, selector) {
  const { using, value } = typeof selector === 'string' ? { using: '-android uiautomator', value: selector } : selector;
  return (await s.call('POST', '/elements', { using, value })).map((e) => Object.values(e)[0]);
}
// Until it shows (Maestro's extendedWaitUntil visible), or the time is up: lookups that do not
// wait, one after another (a toast is there for 2 s).
async function shows(s, selector, ms = 7000) {
  await s.call('POST', '/timeouts', { implicit: 0 });
  try {
    for (const end = Date.now() + ms; ; await sleep(100)) {
      if ((await all(s, selector)).length > 0) return true;
      if (Date.now() > end) return false;
    }
  } finally {
    await s.call('POST', '/timeouts', { implicit: 10000 });
  }
}
async function visible(s, selector, ms = 10000) {
  if (!(await shows(s, selector, ms))) throw new Error(`${JSON.stringify(selector)} is not visible`);
}
async function hidden(s, selector, ms = 7000) {
  for (const end = Date.now() + ms; await s.exists(selector); await sleep(200)) {
    if (Date.now() > end) throw new Error(`${JSON.stringify(selector)} is still visible`);
  }
}
async function attr(s, selector, name) {
  return s.call('GET', `/element/${await s.find(selector)}/attribute/${name}`);
}
// An element's state as Maestro asserts it. iOS has no checked: a switch's value is 1.
async function state(s, selector, name) {
  if (IOS && name === 'checked') {
    const value = await attr(s, selector, 'value');
    return value === '1' || String(await attr(s, selector, 'selected')) === 'true';
  }
  return String(await attr(s, selector, name)) === 'true';
}
async function expectState(s, selector, name, want) {
  for (let i = 0; i < 20; i++, await sleep(150)) if ((await state(s, selector, name)) === want) return;
  throw new Error(`${JSON.stringify(selector)}: ${name} is not ${want}`);
}
async function textOf(s, selector) {
  return s.call('GET', `/element/${await s.find(selector)}/text`);
}
async function expectText(s, selector, want) {
  for (let i = 0, got; i < 20; i++, await sleep(150)) if ((got = await textOf(s, selector)) === want) return;
  throw new Error(`${JSON.stringify(selector)}: text is not "${want}"`);
}
async function centre(s, selector) {
  const r = await s.rect(await s.find(selector));
  return { x: Math.round(r.x + r.width / 2), y: Math.round(r.y + r.height / 2), r };
}
// A finger down at one point and up at another (W3C actions: both drivers have them). `hold`
// keeps it still at the end: a scroll then stops where it is instead of coasting on.
async function drag(s, from, to, ms = 400, hold = 0) {
  const steps = Math.max(1, Math.round(ms / 50));
  await s.call('POST', '/actions', {
    actions: [{
      type: 'pointer', id: 'finger', parameters: { pointerType: 'touch' },
      actions: [
        { type: 'pointerMove', duration: 0, x: Math.round(from.x), y: Math.round(from.y) },
        { type: 'pointerDown', button: 0 },
        { type: 'pause', duration: 100 },
        // In steps of about 50 ms: a gesture handler follows the moves it is sent.
        ...Array.from({ length: steps }, (_, i) => ({
          type: 'pointerMove', duration: Math.round(ms / steps), x: Math.round(from.x + ((to.x - from.x) * (i + 1)) / steps), y: Math.round(from.y + ((to.y - from.y) * (i + 1)) / steps),
        })),
        ...(hold ? [{ type: 'pause', duration: hold }] : []),
        { type: 'pointerUp', button: 0 },
      ],
    }],
  });
  await s.call('DELETE', '/actions').catch(() => {});
}
async function tapPoint(s, x, y) {
  await mobile(s, 'clickGesture', { x: Math.round(x), y: Math.round(y) });
}
// Maestro's swipe from an element: a quick drag from its centre a long way in that direction.
async function swipeFrom(s, selector, direction, dist = 0.4, { ms = 250, hold = 0 } = {}) {
  const { width, height } = await s.call('GET', '/window/rect');
  const { x, y } = await centre(s, selector);
  const d = { left: [-width * dist, 0], right: [width * dist, 0], up: [0, -height * dist], down: [0, height * dist] }[direction];
  await drag(s, { x, y }, { x: Math.max(1, Math.min(width - 1, x + d[0])), y: Math.max(1, Math.min(height - 1, y + d[1])) }, ms, hold);
}
// scrollUntilVisible: drags across the screen (or across `around`) until it shows. The
// direction is the content's: down brings what is below into view.
async function scrollTo(s, selector, direction = 'down', { attempts = 40, around } = {}) {
  const { width, height } = await s.call('GET', '/window/rect');
  const y = around ? (await centre(s, around)).y : height / 2;
  const moves = {
    down: [{ x: width / 2, y: height * 0.7 }, { x: width / 2, y: height * 0.35 }],
    up: [{ x: width / 2, y: height * 0.35 }, { x: width / 2, y: height * 0.7 }],
    right: [{ x: width * 0.8, y }, { x: width * 0.2, y }],
    left: [{ x: width * 0.2, y }, { x: width * 0.8, y }],
  }[direction];
  // A screen just opened may not have drawn its list yet.
  if (await shows(s, selector, 1500)) return inView(s, selector);
  for (let i = 0; i < attempts; i++) {
    if (await s.exists(selector)) return inView(s, selector);
    await drag(s, moves[0], moves[1], 300, 300);
  }
  await visible(s, selector, 1000);
}
// Still: a popup slides in, a list coasts on, and a tap on where it was misses.
async function still(s, selector) {
  const at = async () => JSON.stringify(await s.rect(await s.find(selector)).catch(() => null));
  for (let i = 0, was = await at(), now; i < 20 && (now = await at()) !== was; i++) was = now;
  return selector;
}
async function tapStill(s, selector) {
  await s.tap(await still(s, selector));
}
// Still, and clear of the tab bar, before a tap.
async function inView(s, selector) {
  const { height } = await s.call('GET', '/window/rect');
  await still(s, selector);
  const { y } = await centre(s, selector);
  if (y > height * 0.82) await drag(s, { x: 10, y: height * 0.6 }, { x: 10, y: height * 0.4 }, 300, 300);
  return selector;
}
// inputText: typed into the field, after what it has (both drivers' set value add to it). A
// field that formats the number as it comes (phone, code) gets it key by key on Android (W3C
// key actions): UiAutomator2 sets the whole text at once, which such a field gets wrong.
async function typeInto(s, selector, value, { keys = false } = {}) {
  const el = await s.find(selector);
  await s.call('POST', `/element/${el}/click`, {});
  if (keys && !IOS) return typeKeys(s, value);
  await s.call('POST', `/element/${el}/value`, { text: value });
}
async function typeFocused(s, value) {
  const el = Object.values(await s.call('GET', '/element/active'))[0];
  await s.call('POST', `/element/${el}/value`, { text: value });
}
async function typeKeys(s, value) {
  const keys = [...value].flatMap((c) => [{ type: 'keyDown', value: c }, { type: 'keyUp', value: c }]);
  await s.call('POST', '/actions', { actions: [{ type: 'key', id: 'keyboard', actions: keys }] });
}
// Enter and Backspace. On Android Enter is the field's editor action (as the keyboard's
// return key sends it) and Backspace W3C's key: the key codes do not reach the field.
async function key(s, name, times = 1) {
  for (let i = 0; i < times; i++) {
    if (IOS) await typeFocused(s, { ENTER: '\n', DEL: '\b' }[name]);
    else if (name === 'ENTER') await run(s, 'performEditorAction', { action: 'done' });
    else if (name === 'DEL') await typeKeys(s, '\uE003');
    else await s.call('POST', '/appium/device/press_keycode', { keycode: KEYCODE[name] });
  }
}

// launchApp clearState (and clearKeychain): the app's data gone, then opened. Maestro's launch
// grants every permission: so do these.
async function launch(s, { clear = false, args } = {}) {
  // The app never stops animating: UiAutomator2's wait for an idle screen before each lookup
  // and tap would run out every time (10 s), so it is off.
  if (!IOS) await s.call('POST', '/appium/settings', { settings: { waitForIdleTimeout: 0 } });
  await run(s, 'terminateApp', app).catch(() => {});
  if (clear) {
    await run(s, 'clearApp', app);
    if (IOS) await run(s, 'clearKeychains').catch(() => {});
  }
  if (IOS) {
    if (clear) await run(s, 'setPermission', { bundleId: APP_ID, access: { location: 'always', camera: 'yes', photos: 'yes', notifications: 'yes' } }).catch(() => {});
    await run(s, 'launchApp', { bundleId: APP_ID, arguments: args ? Object.entries(args).flatMap(([k, v]) => [`-${k}`, v]) : [] });
  } else {
    if (clear) await run(s, 'changePermissions', { permissions: 'all', appPackage: APP_ID, action: 'grant' }).catch(() => {});
    if (args) {
      await run(s, 'startActivity', { component: `${APP_ID}/.MainActivity`, extras: Object.entries(args).map(([k, v]) => ['s', k, v]), wait: true });
    } else {
      await run(s, 'activateApp', app);
    }
  }
}
async function guest(s) {
  await launch(s, { clear: true });
  await s.tap(id('onboarding-skip'));
  await s.tap(id('guest'));
  await visible(s, id('tab-home'));
}
async function lab(s, screen) {
  await s.tap(id('tab-lab'));
  await s.tap(id(`lab-${screen}`));
}
// Opens a scene from the list. A tap as the list slides back in from the last scene is lost:
// again when nothing opened, as Maestro's tapOn does.
async function openScene(s, name) {
  for (let i = 0; i < 3; i++) {
    await s.tap(await scrollTo(s, id(`scene-${name}`)));
    if (await shows(s, id('lab-tap'), 3000)) return;
  }
  throw new Error(`scene-${name} did not open`);
}
async function closeScene(s) {
  await s.tap(id('header-back'));
  await hidden(s, id('lab-tap'));
}
async function scene(s, name) {
  await openScene(s, name);
  for (let i = 0; i < 3; i++) await s.tap(id('lab-tap'));
  await visible(s, text('Taps: 3'));
  await closeScene(s);
}
async function dismissNotify(s) {
  if (await shows(s, id('notify-title'))) await tapStill(s, id('notify-later'));
}
async function signIn(s, phone, code, remember) {
  await launch(s, { clear: true });
  await s.tap(id('onboarding-skip'));
  await typeInto(s, id('phone-input'), phone, { keys: true });
  if (remember) await s.tap(id('remember-me'));
  await s.tap(id('next'));
  await visible(s, id('code-input'));
  await typeInto(s, id('code-input'), code, { keys: true });
  await home(s);
  await dismissNotify(s);
  await visible(s, textMatches('Hi, .*'));
}
// Home, or the notification prompt over it: either means the sign in is done.
async function home(s) {
  for (const end = Date.now() + 15000; !(await s.exists(id('tab-home'))) && !(await s.exists(id('notify-title'))); await sleep(200)) {
    if (Date.now() > end) throw new Error('no home screen after the sign in');
  }
}
async function addToCart(s, product, size) {
  await s.tap(id('tab-shop'));
  await s.tap(await scrollTo(s, id(`product-${product}`)));
  await s.tap(id(`size-${size}`));
  await s.tap(id('add-to-cart'));
  await visible(s, text('Added to cart'));
  await s.tap(id('header-back'));
}
async function checkout(s, order) {
  await s.tap(id('checkout'));
  for (const [f, v] of [['name-input', 'Alex Rivera'], ['email-input', 'alex@example.com'], ['street-input', '1 Main Street'], ['city-input', 'Montreal'], ['postal-input', 'H2X 1Y4']]) await typeInto(s, id(f), v);
  // The keyboard covers the sticky footer.
  await hideKeyboard(s);
  await s.tap(id('place-order'));
  await visible(s, text(`Order #${order}`));
}
async function dismissOrderPopups(s) {
  if (await shows(s, id('rate-title'))) await tapStill(s, id('rate-later'));
  if (await shows(s, id('badge-title'))) await tapStill(s, id('badge-close'));
  if (await shows(s, id('survey-title'))) {
    await tapStill(s, id('nps-9'));
    await visible(s, text('Your score: 9'));
    await s.tap(id('nps-submit'));
  }
}
// iOS may ask "Open in “Speedrun Bench”?" for a link from outside the app.
async function confirmLink(s) {
  if (IOS && (await shows(s, textMatches('Open in .*'), 2000))) await s.tap(exact('Open'));
}
const pick = (a) => a[Math.floor(Math.random() * a.length)];
const letters = (n) => Array.from({ length: n }, () => 'abcdefghijklmnopqrstuvwxyz'[Math.floor(Math.random() * 26)]).join('');
const count = async (s, field, prefix) => parseInt((await textOf(s, id(field))).replace(prefix, ''), 10);

export const SCENARIOS = {
  async animations(s) {
    await guest(s);
    await lab(s, 'animations');
    for (const name of ['still', 'reanimated', 'native-driver', 'js-driver', 'lottie', 'spinner', 'ticker', 'counter', 'marquee', 'shimmer', 'carousel', 'progress']) await scene(s, name);
  },
  async appearance(s) {
    await guest(s);
    const dark = (on) => (IOS ? run(s, 'setAppearance', { style: on ? 'dark' : 'light' }) : run(s, 'setUiMode', { mode: 'night', value: on ? 'yes' : 'no' }));
    await dark(false);
    await s.call('POST', '/appium/start_recording_screen', {});
    await lab(s, 'device');
    await visible(s, text('Appearance: light'));
    await dark(true);
    await visible(s, text('Appearance: dark'), 5000);
    await dark(false);
    await visible(s, text('Appearance: light'), 5000);
    await s.call('POST', '/appium/stop_recording_screen', {});
    await s.tap(id('header-back'));
    await s.tap(id('tab-profile'));
    await s.tap(id('settings'));
    await s.call('GET', '/screenshot');
    await scrollTo(s, id('version'));
    await s.call('GET', `/element/${await s.find(id('version'))}/screenshot`);
  },
  async cart(s) {
    await guest(s);
    await addToCart(s, '5', 'S');
    await addToCart(s, '9', 'XL');
    await addToCart(s, '21', 'M');
    await visible(s, text('Cart (3)'));
    await s.tap(id('cart-button'));
    await visible(s, text('Trail Cap'));
    await visible(s, text('Classic Sneaker'));
    await s.tap(id('line-plus-0'));
    await s.tap(id('line-plus-0'));
    await expectText(s, id('line-qty-0'), '× 3');
    await s.tap(id('line-minus-0'));
    await expectText(s, id('line-qty-0'), '× 2');
    await swipeFrom(s, id('cart-line-1'), 'left');
    await s.tap(id('remove-1'));
    await hidden(s, text('Trail Notebook'));
    await hidden(s, id('cart-line-2'));
    await visible(s, textMatches('Total \\$.*'));
  },
  async chat(s) {
    await guest(s);
    await s.tap(id('tab-chats'));
    await s.tap(id('chat-bot'));
    await visible(s, text('Hi! Ask me anything.'));
    await expectState(s, id('send'), 'enabled', false);
    await typeInto(s, id('message-input'), 'Hello there');
    await s.tap(id('send'));
    await visible(s, text('You said: Hello there'));
    await mobile(s, 'doubleClickGesture', { elementId: await s.find(id('message-1')) });
    await visible(s, textMatches('.*❤️ Loved'));
    await mobile(s, 'longClickGesture', { elementId: await s.find(id('message-2')) });
    await s.tap(id('menu-copy'));
    await visible(s, text('Copied'));
    await mobile(s, 'longClickGesture', { elementId: await s.find(id('message-2')) });
    await visible(s, id('menu-delete'));
    const { width, height } = await s.call('GET', '/window/rect');
    await tapPoint(s, width * 0.5, height * 0.1);
    await hidden(s, id('menu-delete'));
    await mobile(s, 'longClickGesture', { elementId: await s.find(id('message-2')) });
    await s.tap(id('menu-delete'));
    await hidden(s, text('You said: Hello there'));
    // setClipboard, then a paste into the field: Android's paste key; iOS has no key for it,
    // so its text is typed.
    const content = Buffer.from('Pasted hello').toString('base64');
    await (IOS ? run(s, 'setPasteboard', { content, encoding: 'utf8' }) : run(s, 'setClipboard', { content, contentType: 'plaintext' }));
    await s.tap(id('message-input'));
    if (IOS) await typeFocused(s, 'Pasted hello');
    else await key(s, 'PASTE');
    await s.tap(id('send'));
    await visible(s, text('You said: Pasted hello'));
    await s.tap(id('quick-Track my order'));
    await visible(s, text('You said: Track my order'));
  },
  async deep_links(s) {
    await guest(s);
    const open = (url) => run(s, 'deepLink', IOS ? { url, bundleId: APP_ID } : { url, package: APP_ID });
    await open('speedrunbench://product/7');
    await confirmLink(s);
    await expectText(s, id('product-name'), 'Trail Bottle');
    await open('speedrunbench://lab/gestures');
    await confirmLink(s);
    await visible(s, text('Last swipe: none (0)'));
    await open('speedrunbench://cart');
    await confirmLink(s);
    await visible(s, text('Your cart is empty'));
  },
  async device(s) {
    await launch(s, { clear: true, args: { greeting: 'hello', mode: 'bench' } });
    await s.tap(id('onboarding-skip'));
    await s.tap(id('guest'));
    await lab(s, 'device');
    await visible(s, text('Arguments: greeting=hello, mode=bench'));
    await visible(s, text('Launches: 1'));
    // On Android the driver's setGeolocation (the emulator's geo fix) leaves the app waiting for
    // a fix: the GPS and network test providers set instead, as the Maestro runners do.
    const shell = (command, args) => run(s, 'shell', { command, args });
    const locate = async (latitude, longitude) => {
      if (IOS) return s.call('POST', '/location', { location: { latitude, longitude, altitude: 0 } });
      await shell('appops', ['set', 'com.android.shell', 'android:mock_location', 'allow']);
      for (const p of ['gps', 'network']) {
        await shell('cmd', ['location', 'providers', 'add-test-provider', p]).catch(() => {});
        await shell('cmd', ['location', 'providers', 'set-test-provider-enabled', p, 'true']);
        await shell('cmd', ['location', 'providers', 'set-test-provider-location', p, '--location', `${latitude},${longitude}`]);
      }
    };
    await locate(45.5017, -73.5673);
    await visible(s, text('Location: 45.5017, -73.5673'));
    // No travel: the route's end.
    await locate(45.5088, -73.554);
    await visible(s, text('Location: 45.5088, -73.5540'), 20000);
    const camera = (allow) =>
      IOS
        ? run(s, 'setPermission', { bundleId: APP_ID, access: { camera: allow ? 'yes' : 'no' } })
        : run(s, 'changePermissions', { permissions: ['android.permission.CAMERA'], appPackage: APP_ID, action: allow ? 'grant' : 'revoke' });
    await camera(false);
    await launch(s);
    await lab(s, 'device');
    await visible(s, textMatches('Camera: (denied|undetermined)'));
    await camera(true);
    await launch(s);
    await lab(s, 'device');
    await visible(s, text('Camera: granted'));
    if (await s.exists(id('allow-photos'))) {
      await s.tap(id('allow-photos'));
      await s.tap(textMatches('Allow Full Access|Allow Access to All Photos|Allow all|Allow'));
    }
    await visible(s, textMatches('Photos: \\d+'));
    const photos = await count(s, 'photo-count', 'Photos: ');
    // addMedia: the photo pushed to the gallery (the drivers add it to the media library).
    const data = readFileSync(fileURLToPath(MEDIA)).toString('base64');
    await s.call('POST', '/appium/device/push_file', { path: IOS ? 'photo.png' : `/sdcard/Pictures/photo-${Date.now()}.png`, data });
    await s.tap(id('refresh-device'));
    if (IOS) {
      for (let i = 0; i < 25 && (await count(s, 'photo-count', 'Photos: ')) !== photos + 1; i++) await sleep(200);
      if ((await count(s, 'photo-count', 'Photos: ')) !== photos + 1) throw new Error('the photo did not arrive');
    } else {
      await visible(s, textMatches('Photos: [1-9]\\d*'), 5000);
    }
    const resumed = await count(s, 'resumed', 'Back in the foreground: ');
    if (IOS) await run(s, 'pressButton', { name: 'home' });
    else await key(s, 'HOME');
    await run(s, 'activateApp', app);
    await visible(s, text(`Back in the foreground: ${resumed + 1}`));
    const launches = await count(s, 'launches', 'Launches: ');
    await launch(s);
    await lab(s, 'device');
    await expectText(s, id('launches'), `Launches: ${launches + 1}`);
    await s.call('POST', '/orientation', { orientation: 'LANDSCAPE' });
    await visible(s, text('Orientation: landscape'), 5000);
    await s.call('POST', '/orientation', { orientation: 'PORTRAIT' });
    await visible(s, text('Orientation: portrait'), 5000);
    if (!IOS) {
      await run(s, 'setConnectivity', { airplaneMode: true });
      await visible(s, text('Network: offline'));
      await run(s, 'setConnectivity', { airplaneMode: false });
      await visible(s, text('Network: online'), 15000);
    }
  },
  async drag(s) {
    await guest(s);
    await lab(s, 'drag');
    // From the handle across the track. The handle is drawn at the track's start until it is
    // first dragged (its frame, moved by its animated style, says otherwise), so the drag starts
    // there.
    const track = await s.rect(await s.find(await still(s, id('slider-track'))));
    const y = track.y + track.height / 2;
    await drag(s, { x: track.x + track.height / 2, y }, { x: track.x + track.width * 0.95, y }, 400);
    await visible(s, textMatches('Volume: (7\\d|8\\d|9\\d|100)'));
    await swipeFrom(s, id('drag-card'), 'down', 0.3);
    await visible(s, text('Dropped in the zone'));
    await scrollTo(s, text('Card 25'), 'right', { around: text('Card 1') });
    await s.tap(id('open-long-list'));
    await s.tap(await scrollTo(s, text('Row 120'), 'down', { attempts: 120 }));
    await visible(s, text('Picked row 120'));
    await s.tap(await scrollTo(s, text('Row 3'), 'up', { attempts: 120 }));
    await visible(s, text('Picked row 3'));
  },
  async feed(s) {
    await signIn(s, '5550101234', '123456', false);
    await visible(s, textMatches('Post 1: .*'));
    await visible(s, id('announcement'));
    await visible(s, id('promo-banner'), 5000);
    await s.tap(id('like-1'));
    await visible(s, textMatches('Liked · .*'));
    await mobile(s, 'doubleClickGesture', { elementId: await s.find(id('photo-2')) });
    await scrollTo(s, id('like-2'));
    await expectState(s, id('like-2'), 'selected', true);
    // The first page has 20 posts: the next ones load at the end of the list.
    await scrollTo(s, textMatches('Post 30: .*'), 'down', { attempts: 80 });
    await s.tap(await scrollTo(s, id('like-30')));
    await expectState(s, id('like-30'), 'selected', true);
    await scrollTo(s, id('like-1'), 'up', { attempts: 80 });
    // Pull to refresh, until it does.
    for (let i = 0; i < 3 && !(await s.exists(textMatches('Refreshed \\d+×'))); i++) await swipeFrom(s, id('like-1'), 'down');
    await visible(s, textMatches('Refreshed \\d+×'), 5000);
    const { width, height } = await s.call('GET', '/window/rect');
    await drag(s, { x: width / 2, y: height * 0.7 }, { x: width / 2, y: height * 0.35 }, 300);
  },
  async gestures(s) {
    await guest(s);
    await lab(s, 'gestures');
    for (const [n, dir] of [[1, 'left'], [2, 'right'], [3, 'up'], [4, 'down']]) {
      await swipeFrom(s, id('swipe-pad'), dir, 0.25);
      await visible(s, text(`Last swipe: ${dir.toUpperCase()} (${n})`));
    }
    const { r } = await centre(s, id('tap-pad'));
    await tapPoint(s, r.x + r.width * 0.15, r.y + r.height * 0.2);
    await visible(s, text('Tapped top left'));
    await tapPoint(s, r.x + r.width * 0.85, r.y + r.height * 0.8);
    await visible(s, text('Tapped bottom right'));
    await mobile(s, 'doubleClickGesture', { elementId: await s.find(id('double-tap-target')) });
    await visible(s, text('Single: 0 · Double: 1'));
    await s.tap(id('double-tap-target'));
    await visible(s, text('Single: 1 · Double: 1'));
    await mobile(s, 'longClickGesture', { elementId: await s.find(id('long-press-target')) });
    await visible(s, text('Long presses: 1'));
  },
  async inputs(s) {
    await guest(s);
    await lab(s, 'inputs');
    await visible(s, text('Keyboard: hidden'));
    await typeInto(s, id('note-input'), 'Buy milk');
    await hideKeyboard(s);
    await visible(s, text('Keyboard: hidden'));
    // The note survives a stop, and a kill.
    for (let i = 0; i < 2; i++) {
      await launch(s);
      await lab(s, 'inputs');
      await visible(s, text('Buy milk'));
    }
    await typeInto(s, id('key-input'), 'hello');
    await key(s, 'ENTER');
    await visible(s, text('Submitted: hello'));
    await s.tap(id('key-input'));
    await key(s, 'DEL', 2);
    await visible(s, text('Backspaces: 2'));
    await key(s, 'DEL');
    await key(s, 'ENTER');
    await visible(s, text('Submitted: he'));
    // copyTextFrom, pasteText: the text read, then typed.
    await typeInto(s, id('paste-input'), await textOf(s, id('copy-source')));
    await visible(s, text('Pasted the code'));
    await hideKeyboard(s);
    // inputRandom...: values of each kind, made here.
    const values = {
      'rand-text': letters(8),
      'rand-number': String(Math.floor(10000 + Math.random() * 90000)),
      'rand-email': `${letters(6)}@example.com`,
      'rand-name': pick(['Ava Martin', 'Noah Tremblay', 'Mia Roy']),
      'rand-city': pick(['Laval', 'Ottawa', 'Toronto']),
      'rand-country': pick(['Canada', 'France', 'Japan']),
      'rand-color': pick(['teal', 'maroon', 'olive']),
    };
    for (const [f, v] of Object.entries(values)) {
      await typeInto(s, await scrollTo(s, id(f)), v);
      await hideKeyboard(s);
    }
    await scrollTo(s, id('rand-check'));
    await visible(s, textMatches('text \\d+ chars · number digits · email valid · name \\d+ words · city set · country set · color set'));
    if (!IOS) {
      await s.tap(await scrollTo(s, id('catch-back')));
      await s.back();
      await visible(s, text('Back presses: 1'));
      await s.tap(id('catch-back'));
      await s.back();
      await hidden(s, id('back-presses'));
    }
    await launch(s, { clear: true });
    await visible(s, text('Discover'));
  },
  async native(s) {
    await guest(s);
    await lab(s, 'native');
    await s.tap(exact('Add'));
    await s.tap(exact('Add'));
    await s.tap(id('native-add'));
    await visible(s, text('Count: 3'));
    await s.tap(exact('Details'));
    await visible(s, id('native-details'));
    await s.tap(exact('Details'));
    await hidden(s, id('native-details'));
    for (let i = 0; i < 2; i++) {
      await s.tap(exact('Load'));
      await visible(s, id('native-bar'), 5000);
      await hidden(s, id('native-bar'), 10000);
    }
    await visible(s, text('Loads finished: 2'));
    await s.tap(exact('Card'));
    await s.tap(exact('Card button'));
    await visible(s, text('Card taps: 1'));
    // A loading overlay takes the taps for 1.5 s: the tap after it waits for it to go.
    await s.tap(exact('Lock'));
    await shows(s, id('native-overlay'), 1000);
    await hidden(s, id('native-overlay'), 5000);
    await s.tap(exact('Add'));
    await visible(s, text('Count: 4'));
    await s.tap(exact('List'));
    await s.tap(await scrollTo(s, exact('Item 80')));
    await visible(s, text('Picked: Item 80'));
  },
  async onboarding(s) {
    await launch(s, { clear: true });
    await visible(s, text('Discover'));
    await visible(s, text('Page 1 of 3'));
    const { width, height } = await s.call('GET', '/window/rect');
    const across = (from, to) => drag(s, { x: width * from, y: height * 0.5 }, { x: width * to, y: height * 0.5 }, 250);
    await across(0.8, 0.2);
    await visible(s, text('Page 2 of 3'));
    await across(0.9, 0.1);
    await visible(s, text('Page 3 of 3'));
    await visible(s, text('Share'));
    await scrollTo(s, text('Discover'), 'left');
    await visible(s, text('Page 1 of 3'));
    await scrollTo(s, text('Share'), 'right');
    await visible(s, text('Page 3 of 3'));
    await s.tap(text('Get started'));
    await visible(s, text("Let's start with your phone number"));
    await launch(s);
    await visible(s, text("Let's start with your phone number"));
    await hidden(s, text('Discover'), 1000);
  },
  async popups(s) {
    await guest(s);
    await s.tap(id('tab-lab'));
    await s.tap(id('lab-popups-first'));
    await dismissOrderPopups(s);
    await hidden(s, id('badge-title'), 1000);
    await s.tap(id('lab-popups-second'));
    await dismissOrderPopups(s);
    await hidden(s, id('survey-title'), 1000);
    await visible(s, id('lab-device'));
    // Checks for popups that do not show share one wait (7 s, as Maestro's), not 7 s each.
    const from = Date.now();
    const left = () => Math.max(0, 7000 - (Date.now() - from));
    if (await shows(s, text('Try Pro free for a month'), left())) await s.tap(text('Not now'));
    for (let i = 0; i < 2; i++) if (await shows(s, id('notify-title'), left())) await tapStill(s, id('notify-later'));
    if (Date.now() - from >= 12000) throw new Error('the popup checks took 12 s or more');
  },
  async profile(s) {
    await signIn(s, '5550101234', '123456', false);
    await s.tap(id('tab-profile'));
    await expectText(s, id('profile-name'), 'Alex');
    await s.tap(id('edit-profile'));
    // eraseText, then the new name.
    await s.call('POST', `/element/${await s.find(id('profile-name-input'))}/clear`, {});
    await typeInto(s, id('profile-name-input'), 'Jordan');
    await typeInto(s, id('profile-email-input'), 'jordan');
    await s.tap(id('bio-input'));
    await visible(s, text('Enter a valid email'));
    await typeFocused(s, 'Runner and coffee lover.');
    await typeInto(s, id('profile-email-input'), '@example.com');
    await hideKeyboard(s);
    await s.tap(id('units-imperial'));
    await expectState(s, id('units-imperial'), 'checked', true);
    await expectState(s, id('units-metric'), 'checked', false);
    await s.tap(id('public-profile'));
    await expectState(s, id('public-profile'), 'checked', true);
    await s.tap(id('save-profile'));
    await visible(s, text('Profile saved'));
    await expectText(s, id('profile-name'), 'Jordan');
    await s.tap(id('edit-profile'));
    await visible(s, text('Saved: Imperial, public, jordan@example.com'));
    await s.tap(id('header-back'));
    await s.tap(id('settings'));
    await expectState(s, id('switch-push'), 'checked', true);
    await s.tap(id('switch-push'));
    await expectState(s, id('switch-push'), 'checked', false);
    await s.tap(id('switch-emails'));
    await expectState(s, id('switch-emails'), 'checked', true);
    await s.tap(id('clear-cache'));
    await s.tap(textMatches('Clear|CLEAR'));
    await visible(s, text('Cache cleared'));
    await scrollTo(s, id('version'));
    await visible(s, text('Version 1.0.0'));
    await s.tap(id('header-back'));
    await s.tap(id('security'));
    // The screen loads under an overlay that takes the taps for 1.5 s from when it opens; its
    // spinner stays in the tree, faded out, so there is nothing to wait for but the time.
    await visible(s, text('Create a PIN'));
    await sleep(1500);
    const keys = async (k) => { for (const c of k) await s.tap(id(c === '<' ? 'key-del' : `key-${c}`)); };
    await keys('1234');
    await visible(s, text('Confirm your PIN'));
    await keys('1235');
    await visible(s, text("PINs don't match"));
    await keys('4<1234');
    await visible(s, text('Confirm your PIN'));
    await keys('1234');
    await visible(s, text('PIN set'));
    await s.tap(id('sign-out'));
    await s.tap(textMatches('Yes, sign out|YES, SIGN OUT'));
    await visible(s, text("Let's start with your phone number"));
  },
  async remember_me(s) {
    await signIn(s, '5550109999', '123456', true);
    // clearState without clearKeychain: iOS keeps the remembered account in its keychain.
    await run(s, 'terminateApp', app).catch(() => {});
    await run(s, 'clearApp', app);
    await (IOS ? run(s, 'launchApp', { bundleId: APP_ID }) : run(s, 'activateApp', app));
    await s.tap(id('onboarding-skip'));
    if (IOS) {
      await visible(s, text('Welcome back, Alex!'));
      await s.tap(id('continue-remembered'));
      await home(s);
      await dismissNotify(s);
      await visible(s, text('Hi, Alex'));
    } else {
      await visible(s, text("Let's start with your phone number"));
    }
    await launch(s, { clear: true });
    await s.tap(id('onboarding-skip'));
    await visible(s, text("Let's start with your phone number"));
    if (IOS) await run(s, 'clearKeychains').catch(() => {});
  },
  async scripting(s) {
    // The flow's scripting, as test code: its env, an output object, a script's values back.
    const GREETING = 'Hello';
    const output = { started: true };
    await guest(s);
    if (output.started !== true) throw new Error('onFlowStart did not run');
    const plan = (PLAN) => ({ plan: PLAN, greeting: GREETING + ', ' + PLAN.toLowerCase() });
    Object.assign(output, plan('Pro'));
    console.log('picked ' + output.plan);
    if (output.plan !== 'Pro' || output.greeting !== 'Hello, pro') throw new Error('the script handed back the wrong values');
    await lab(s, 'selectors');
    let taps = 0;
    for (let i = 0; i < 3; i++) {
      await s.call('POST', `/element/${(await all(s, exact('Item')))[taps]}/click`, {});
      taps++;
    }
    await visible(s, text('Picked item 3'));
    if (taps !== 3) throw new Error('repeat ran the wrong number of times');
    while (!(await s.exists(text(`Chose ${output.plan}`)))) await s.tap(choose(`plan-${output.plan.toLowerCase()}`));
    if (GREETING === 'Hello') await s.tap(id('open-button'));
    await visible(s, text('Open was tapped'));
    if (GREETING === 'Bye') await s.tap(id('hide-me'));
    await expectState(s, id('hide-me'), 'checked', false);
    const COUNT = '7';
    if (COUNT !== '7') throw new Error('the env did not reach the commands');
    let tries = 0;
    for (let attempt = 0; attempt < 3; attempt++) {
      tries++;
      if (tries >= 2) break;
    }
    if (tries !== 2) throw new Error('retry ran the wrong number of times');
    await hidden(s, text('Nothing like this'), 1000);
    Object.assign(output, { finished: true });
  },
  async selectors(s) {
    await guest(s);
    await lab(s, 'selectors');
    const chooses = () => all(s, exact('Choose'));
    const rectOf = (el) => s.rect(el);
    const click = (el) => s.call('POST', `/element/${el}/click`, {});
    await click((await chooses())[1]);
    await visible(s, text('Chose Plus'));
    await visible(s, textMatches('(C)(h)(o)(s)(e) (P)(l)(u)(s)(.)?(.)?'));
    await visible(s, textMatches([...Array(4000).keys()].map((i) => 'Plan ' + i).join('|') + '|Chose Plus'));
    // No relative selectors: compared by the elements' frames. "Choose" below "$10".
    const ten = await rectOf(await s.find(exact('$10')));
    const below = [];
    for (const el of await chooses()) {
      const r = await rectOf(el);
      if (r.y >= ten.y + ten.height) below.push({ el, y: r.y });
    }
    await click(below.sort((a, b) => a.y - b.y)[0].el);
    await visible(s, text('Chose Pro'));
    await s.tap(choose('plan-basic'));
    await visible(s, text('Chose Basic'));
    // "Basic" left of the first "Choose".
    const first = await rectOf((await chooses())[0]);
    const basic = await rectOf(await s.find(exact('Basic')));
    if (basic.x + basic.width > first.x) throw new Error('Basic is not left of the first Choose');
    // An element with a "Choose" child, the third (Android's tree has them).
    if (!IOS && (await all(s, xpath("//*[*[@text='Choose']]"))).length < 3) throw new Error('no third element with a Choose child');
    // One with both "Plus" and "$10" in it.
    const plus = await s.find(id('plan-plus'));
    for (const t of ['Plus', '$10']) {
      const { using, value } = typeof exact(t) === 'string' ? { using: '-android uiautomator', value: exact(t) } : exact(t);
      await s.call('POST', `/element/${plus}/element`, { using, value });
    }
    const items = () => all(s, exact('Item'));
    await click((await items())[3]);
    await visible(s, text('Picked item 4'));
    // "Item" above the index result: the first of them.
    const result = await rectOf(await s.find(id('index-result')));
    for (const el of await items()) {
      const r = await rectOf(el);
      if (r.y + r.height <= result.y) {
        await click(el);
        break;
      }
    }
    await visible(s, text('Picked item 1'));
    // "Open" below "States".
    const states = await rectOf(await s.find(exact('States')));
    for (const el of await all(s, exact('Open'))) {
      if ((await rectOf(el)).y >= states.y + states.height) {
        await click(el);
        break;
      }
    }
    await visible(s, text('Open was tapped'));
    await expectState(s, id('locked'), 'enabled', false);
    // A tap on it, disabled as it is: at its centre.
    const locked = await centre(s, id('locked'));
    await tapPoint(s, locked.x, locked.y);
    await hidden(s, text('Locked was tapped'), 1000);
    await expectState(s, id('period-Week'), 'selected', true);
    await expectState(s, id('period-Day'), 'selected', false);
    // The period right of Week: the nearest.
    const week = await rectOf(await s.find(id('period-Week')));
    let right;
    for (const p of ['Day', 'Week', 'Month', 'Year']) {
      if (!(await s.exists(id(`period-${p}`)))) continue;
      const r = await rectOf(await s.find(id(`period-${p}`)));
      if (r.x >= week.x + week.width && (!right || r.x < right.x)) right = { p, x: r.x };
    }
    await s.tap(id(`period-${right.p}`));
    await expectState(s, id('period-Month'), 'selected', true);
    await visible(s, text('Showing: Month'));
    await expectState(s, id('notify-me'), 'checked', true);
    await expectState(s, id('hide-me'), 'checked', false);
    await s.tap(id('hide-me'));
    await expectState(s, id('hide-me'), 'checked', true);
    await s.tap(await scrollTo(s, id('focus-field'), 'up'));
    if (IOS) {
      if (!(await s.call('GET', '/appium/device/is_keyboard_shown'))) throw new Error('focus-field is not focused');
    } else {
      await expectState(s, id('focus-field'), 'focused', true);
    }
    await hideKeyboard(s);
    await s.tap(await scrollTo(s, id('slow-button')));
    await visible(s, text('Slow button: done'), 5000);
    // An optional tap on what is not there.
    if (await s.exists(text('Not on this screen'))) await s.tap(text('Not on this screen'));
    await visible(s, id('box-square'));
  },
  async shop_checkout(s) {
    await signIn(s, '5550101234', '123456', false);
    await s.tap(id('tab-shop'));
    await typeInto(s, id('search-input'), 'sneaker');
    await key(s, 'ENTER');
    await visible(s, text('Searched for "sneaker"'));
    await visible(s, text('6 products'), 5000);
    await s.tap(text('City Sneaker'));
    await expectText(s, id('product-name'), 'City Sneaker');
    await expectState(s, id('add-to-cart'), 'enabled', false);
    await s.tap(id('size-M'));
    await expectState(s, id('size-M'), 'selected', true);
    await s.tap(id('color-Blue'));
    await expectState(s, id('color-Blue'), 'checked', true);
    for (let i = 0; i < 2; i++) {
      await s.tap(id('qty-plus'));
      await sleep(300);
    }
    await visible(s, text('Qty 3'));
    await s.tap(await scrollTo(s, id('gift-wrap')));
    await expectState(s, id('gift-wrap'), 'checked', true);
    await s.tap(id('add-to-cart'));
    await visible(s, text('Added to cart'));
    await visible(s, text('Cart (3)'));
    await s.tap(id('go-to-cart'));
    await visible(s, text('Size M · Blue · Gift wrap'));
    await typeInto(s, id('promo-input'), 'SAVE10');
    // The promo service fails now and then: again, up to twice more.
    for (let attempt = 0; ; attempt++) {
      await s.tap(id('apply-promo'));
      if (await shows(s, id('promo-applied'), 2000)) break;
      if (attempt === 2) throw new Error('the promo never applied');
    }
    await hideKeyboard(s);
    await s.tap(id('checkout'));
    await typeInto(s, id('name-input'), pick(['Ava Martin', 'Noah Tremblay', 'Mia Roy']));
    await typeInto(s, id('email-input'), 'alex@');
    await s.tap(id('street-input'));
    await visible(s, text('Enter a valid email'));
    await typeFocused(s, '1 Main Street');
    await typeInto(s, id('email-input'), 'example.com');
    await typeInto(s, id('city-input'), pick(['Laval', 'Ottawa', 'Toronto']));
    await typeInto(s, id('postal-input'), 'H2X 1Y4');
    await hideKeyboard(s);
    await s.tap(id('ship-express'));
    await s.tap(id('place-order'));
    await visible(s, id('order-confirmed'));
    await expectText(s, id('order-number'), 'Order #10001');
    await s.tap(id('done'));
    await dismissOrderPopups(s);
    await addToCart(s, '31', 'L');
    await s.tap(id('cart-button'));
    await checkout(s, '10002');
    await s.tap(id('done'));
    await dismissOrderPopups(s);
    await s.tap(id('tab-profile'));
    await expectText(s, id('profile-orders'), '2');
  },
  async sign_in(s) {
    await launch(s, { clear: true });
    await s.tap(id('onboarding-skip'));
    await expectState(s, id('next'), 'enabled', false);
    await typeInto(s, id('phone-input'), '5550101234', { keys: true });
    await visible(s, text('(555) 010-1234'));
    await expectState(s, id('next'), 'enabled', true);
    await key(s, 'DEL', 4);
    await visible(s, text('(555) 010'));
    await typeInto(s, id('phone-input'), '1234', { keys: true });
    await s.tap(id('next'));
    await visible(s, text('Sent to (555) 010-1234'));
    await typeInto(s, id('code-input'), '111111', { keys: true });
    await visible(s, text('Wrong code, try again'), 5000);
    await typeInto(s, id('code-input'), '123456', { keys: true });
    await home(s);
    if (await s.exists(id('notify-title'))) {
      await s.tap(id('notify-on'));
      if (IOS && (await shows(s, exact('Allow'), 3000))) await s.tap(exact('Allow'));
    }
    await visible(s, text('Hi, Alex'));
  },
  async timing(s) {
    await guest(s);
    await lab(s, 'animations');
    await scene(s, 'confetti');
    await openScene(s, 'delayed');
    await expectState(s, id('lab-tap'), 'enabled', false);
    for (const end = Date.now() + 5000; !(await state(s, id('lab-tap'), 'enabled')); await sleep(100)) {
      if (Date.now() > end) throw new Error('lab-tap never turned on');
    }
    await s.tap(id('lab-tap'));
    await visible(s, text('Taps: 1'));
    await closeScene(s);
    await scene(s, 'shift');
    await openScene(s, 'busy');
    await s.tap(id('lab-tap'));
    await visible(s, text('Taps: 1'));
    await s.tap(id('lab-tap'));
    await visible(s, text('Taps: 2'));
  },
};

// "Choose" inside a plan's card (Maestro's childOf).
function choose(card) {
  return IOS
    ? { using: '-ios class chain', value: `**/*[\`name == ${quoted(card)}\`]/**/*[\`label == "Choose"\`]` }
    : xpath(`//*[@resource-id=${quoted(card)}]//*[@text='Choose']`);
}
