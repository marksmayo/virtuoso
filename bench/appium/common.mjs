// What the Appium tests share: lookups and gestures on Android (UiAutomator2) and, on an iOS
// simulator (VIRTUOSO_BENCH_PLATFORM=ios), through the XCUITest driver: test ids as
// accessibility ids, text by predicate, its own `mobile:` gestures.

export const IOS = process.env.VIRTUOSO_BENCH_PLATFORM === 'ios';
export const predicate = (p) => ({ using: '-ios predicate string', value: p });
export const quoted = (t) => JSON.stringify(t);
export const id = (v) => (IOS ? { using: 'accessibility id', value: v } : `new UiSelector().resourceId(${JSON.stringify(v)})`);
export const text = (t) => (IOS ? predicate(`label == ${quoted(t)} OR value == ${quoted(t)}`) : `new UiSelector().text(${JSON.stringify(t)})`);
// The pattern as it is: UiAutomator2's selector parser keeps a backslash escaped in its string
// (JSON.stringify's \\d stays two characters).
export const textMatches = (re) => (IOS ? predicate(`label MATCHES ${quoted(re)} OR value MATCHES ${quoted(re)}`) : `new UiSelector().textMatches("${re.replace(/"/g, '\\"')}")`);
export const startsWith = (t) => (IOS ? predicate(`label BEGINSWITH ${quoted(t)}`) : `new UiSelector().descriptionStartsWith(${JSON.stringify(t)})`);
export const field = (i) =>
  IOS ? { using: '-ios class chain', value: `**/XCUIElementTypeTextField[${i + 1}]` } : `new UiSelector().className("android.widget.EditText").instance(${i})`;
// Into view down the screen's list, as far as it takes: UiScrollable's scrollIntoView took the
// screen's outer scroll view, so a drag up the middle of the screen until it shows.
export async function into(s, selector) {
  const { width, height } = await s.call('GET', '/window/rect');
  for (let i = 0; i < 200 && !(await s.exists(selector)); i++) {
    await mobile(s, 'scrollGesture', { left: width * 0.1, top: height * 0.3, width: width * 0.8, height: height * 0.5, direction: 'down', percent: 0.9 });
  }
  // Still, before a tap: the list coasts on after the drag, and a tap on where it was misses.
  const at = async () => JSON.stringify(await s.call('GET', `/element/${await s.find(selector)}/rect`));
  for (let i = 0, was = await at(), now; i < 20 && (now = await at()) !== was; i++) was = now;
  return selector;
}

// Focus, then type into what has focus (WebDriver's active element): a field can leave the
// tree as the keyboard comes up (Sequential Onboarding's).
export async function type(s, selector, value) {
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
export async function hideKeyboard(s) {
  const shown = () => s.call('GET', '/appium/device/is_keyboard_shown');
  if (!(await shown())) return;
  await s.call('POST', '/appium/device/hide_keyboard', IOS ? { keys: ['Done', 'Return', 'return'] } : {}).catch(() => {});
  if ((await shown()) && !IOS) await s.back();
}
// UiAutomator2's gestures; on iOS the XCUITest driver's of the same kind.
export async function mobile(s, script, args) {
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
export async function tapAt(s, fx, fy) {
  const { width, height } = await s.call('GET', '/window/rect');
  await mobile(s, 'clickGesture', { x: Math.round(width * fx), y: Math.round(height * fy) });
}

export async function expectText(s, selector, want) {
  const el = await s.find(selector);
  const got = await s.call('GET', `/element/${el}/text`);
  if (!(want instanceof RegExp ? want.test(got) : got === want)) throw new Error(`expected ${want}, got "${got}"`);
}
