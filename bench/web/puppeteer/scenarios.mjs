// Puppeteer's ports of bench/web-flows, step for step with each flow: one function per flow,
// given the page and `open(slug)`, which opens the scenario (the flow's launchApp). A check waits
// for the element the flow asserts, visible.

const xpathText = (t) => `//*[normalize-space(text())=${JSON.stringify(t)}]`;
const escape = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

// The first visible element whose own text (its text nodes, joined, trimmed) matches `pattern`,
// a regex source, shadow roots included, once there is one. Puppeteer's ::-p-text matches any
// text containing t (the instructions often quote the label), and an XPath text() test sees
// only an element's first text node and no shadow root.
function find(source) {
  const re = new RegExp(source);
  const own = (el) => [...el.childNodes].filter((n) => n.nodeType === 3).map((n) => n.textContent).join('').replace(/\s+/g, ' ').trim();
  const roots = [document];
  while (roots.length) {
    const root = roots.shift();
    for (const el of root.querySelectorAll('*')) {
      if (el.shadowRoot) roots.push(el.shadowRoot);
      if (re.test(own(el)) && el.checkVisibility()) return el;
    }
  }
  return null;
}
const element = (where, pattern) => where.waitForFunction(find, {}, pattern);
const exactly = (t) => `^${escape(t)}$`;
const present = async (where, t) => (await where.evaluateHandle(find, exactly(t))).asElement() !== null;
const see = (where, t) => element(where, exactly(t));
const tap = async (where, t) => (await see(where, t)).click();
const textMatching = async (where, pattern) => (await element(where, pattern)).evaluate((el) => el.textContent.trim());
// Typed a key at a time, as a user types: the app's input handlers see every key.
const type = async (where, selector, value) => (await where.waitForSelector(selector, { visible: true })).type(value);

// Closed shadow roots are out of every selector's reach; the usual answer is to have the page
// open them, before it loads (virtuoso reads them through its own hook in the page the same way).
const openShadows = (page) =>
  page.evaluateOnNewDocument(() => {
    const attach = Element.prototype.attachShadow;
    Element.prototype.attachShadow = function (init) {
      return attach.call(this, { ...init, mode: 'open' });
    };
  });

const frame = async (page, title) => (await page.waitForSelector(`iframe[title="${title}"]`)).contentFrame();

// The browser's own dialogs, answered in turn: each [message, answer] checked as it opens.
function answerDialogs(page, expected) {
  const problems = [];
  page.on('dialog', async (dialog) => {
    const [message, answer] = expected.shift() ?? ['(none)'];
    if (dialog.message() !== message) problems.push(`dialog "${dialog.message()}", expected "${message}"`);
    await dialog.accept(answer);
  });
  return () => {
    if (problems.length) throw new Error(problems.join('; '));
  };
}

// The mouse wheel over `over` (a selector), a screenful at a time, until the text `t` is in the
// page: what a user does with a feed or table that renders rows as they come.
async function scrollUntilThere(page, t, { over, timeout = 17000 } = {}) {
  const box = await (await page.waitForSelector(over)).boundingBox();
  const top = Math.max(box.y, 0);
  const bottom = Math.min(box.y + box.height, 800);
  await page.mouse.move(box.x + box.width / 2, (top + bottom) / 2);
  const end = Date.now() + timeout;
  while (!(await present(page, t))) {
    if (Date.now() > end) throw new Error(`${t} not found after scrolling for ${timeout} ms`);
    await page.mouse.wheel({ deltaY: (bottom - top) * 0.8 });
  }
}

export const SCENARIOS = {
  async login_form(page, open) {
    await open('login-form');
    await type(page, '[placeholder="Email"]', 'tester@tester.army');
    await type(page, '[placeholder="Password"]', 'benchmark123');
    await tap(page, 'Log in');
    await see(page, 'Logged in successfully');
    await tap(page, 'Log out');
    await see(page, 'Log in');
  },

  async shadow_dom_form(page, open) {
    await openShadows(page);
    await open('shadow-dom-form');
    const hint = await textMatching(page, '^Access code hint:.*');
    // pierce/ reaches into (open) shadow roots.
    await type(page, 'pierce/input[placeholder="Access code"]', hint.match(/SHADOW-\d+/)[0]);
    await tap(page, 'Submit');
    await see(page, 'Access granted');
  },

  async div_soup(page, open) {
    await open('div-soup');
    // The keypad comes after the code's slots, which show the digits typed: its tile is the last.
    for (const glyph of ['7', '2', '8', '‹', '9', '1', '→']) await page.locator(`::-p-xpath((${xpathText(glyph)})[last()])`).click();
    await see(page, 'Access granted');
  },

  async iframe_form(page, open) {
    await open('iframe-form');
    const code = await textMatching(await frame(page, 'coupon-frame'), '^[A-Z]+-\\d+$');
    const checkout = await frame(page, 'checkout-frame');
    await type(checkout, '[placeholder="Coupon code"]', code);
    await tap(checkout, 'Apply');
    // The page takes the frames down and says so itself.
    await see(page, 'Coupon applied');
  },

  async async_states(page, open) {
    await open('async-states');
    await page.waitForSelector('[data-testid="initial-loading"]', { hidden: true, timeout: 20000 });
    await page.waitForSelector('::-p-xpath(//button[normalize-space(text())="Verify" and @disabled])', { visible: true });
    await tap(page, 'Send code');
    const toast = await textMatching(page, '^Your code is \\d{4}$');
    await type(page, '[placeholder="Verification code"]', toast.match(/\d{4}/)[0]);
    await page.waitForSelector('::-p-xpath(//button[normalize-space(text())="Verify" and not(@disabled)])', { visible: true });
    await tap(page, 'Verify');
    await see(page, 'Code verified successfully');
  },

  async debounced_search(page, open) {
    await open('debounced-search');
    await type(page, '[data-testid="search-input"]', 'trail mix');
    await tap(page, 'Trail Mix 500 g');
    await see(page, 'Added Trail Mix 500 g to the cart');
  },

  async infinite_scroll(page, open) {
    await open('infinite-scroll');
    await scrollUntilThere(page, 'Claim', { over: '[data-testid="feed"]' });
    await see(page, 'Golden Ticket');
    await tap(page, 'Claim');
    await see(page, 'Golden Ticket claimed');
  },

  async virtualized_table(page, open) {
    await open('virtualized-table');
    await see(page, 'Row 1');
    await scrollUntilThere(page, 'Claim', { over: '[data-testid="table-viewport"]', timeout: 140000 });
    await element(page, '^Row 4322.*');
    await tap(page, 'Claim');
    await see(page, 'Golden Row claimed');
  },

  async overlay_trap(page, open) {
    await open('overlay-trap');
    await tap(page, 'Accept cookies');
    await (await element(page, '(?:^|.*)close overlay')).click();
    await tap(page, 'Continue');
    await see(page, 'You made it past the overlay');
  },

  async sticky_chrome(page, open) {
    await open('sticky-chrome');
    await see(page, 'Terms of Service');
    await page.waitForSelector('[data-testid="success-message"]', { hidden: true });
    // The click scrolls the button into view itself, but not clear of the sticky footer: the
    // button is brought to the middle of the screen first.
    const accept = await see(page, 'Accept terms');
    await accept.evaluate((el) => el.scrollIntoView({ block: 'center' }));
    await accept.click();
    await see(page, 'Terms accepted');
  },

  async deferred_dialog(page, open) {
    const check = answerDialogs(page, [['Really submit the report?']]);
    await open('deferred-dialog');
    await (await see(page, 'Submit report')).click({ count: 2 });
    await see(page, 'Report submitted after deferred confirmation');
    check();
  },

  async new_tab_flow(page, open) {
    await open('new-tab-flow');
    const popup = new Promise((resolve) => page.once('popup', resolve));
    await tap(page, 'Open verification tab');
    const tab = await popup;
    const code = await textMatching(tab, '^TAB-\\d+$');
    await tab.close();
    await type(page, '[placeholder="Verification code"]', code);
    await tap(page, 'Verify');
    await see(page, 'Verified successfully');
  },

  async stale_dom(page, open) {
    await open('stale-dom');
    await see(page, 'Progress: 0 / 3');
    for (const n of [1, 2]) {
      await tap(page, 'Tap me');
      await see(page, `Progress: ${n} / 3`);
    }
    await tap(page, 'Tap me');
    await see(page, 'Target clicked 3 times');
    await page.waitForFunction(`!(${find})(${JSON.stringify(exactly('Tap me'))})`);
  },

  async otp_auto_advance(page, open) {
    await open('otp-auto-advance');
    await page.locator('[data-testid="otp-input-0"]').click();
    await page.keyboard.type('4');
    await page.waitForSelector('[data-testid="otp-input-1"]:focus');
    await page.keyboard.type('9302');
    await page.waitForSelector('[data-testid="otp-input-5"]:focus');
    await page.keyboard.type('7');
    await see(page, 'Code verified');
  },

  async native_dialogs(page, open) {
    const check = answerDialogs(page, [
      ['Place this order?'],
      ['Enter your gift code', 'GIFT-7'],
      ['Order placed. Thank you!'],
    ]);
    await open('native-dialogs');
    await tap(page, 'Place order');
    await see(page, 'Order placed with code GIFT-7');
    check();
  },
};
