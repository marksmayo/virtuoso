// Playwright's ports of bench/web-flows, step for step with each flow: one function per flow,
// given the page and `open(slug)`, which opens the scenario (the flow's launchApp). A check is a
// locator waited for in the state the flow asserts.

const exact = { exact: true };

// Closed shadow roots are out of every locator's reach; the usual answer is to have the page
// open them, before it loads (virtuoso reads them through its own hook in the page the same way).
const openShadows = (page) =>
  page.addInitScript(() => {
    const attach = Element.prototype.attachShadow;
    Element.prototype.attachShadow = function (init) {
      return attach.call(this, { ...init, mode: 'open' });
    };
  });

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

// The mouse wheel over `over` (a selector; the page when none), a screenful at a time, until
// `target` is in the page: what a user does with a feed or table that renders rows as they come.
async function scrollUntilThere(page, target, { over, timeout = 17000, up = false } = {}) {
  const box = over ? await page.locator(over).boundingBox() : { x: 0, y: 0, width: 1280, height: 800 };
  const top = Math.max(box.y, 0);
  const bottom = Math.min(box.y + box.height, 800);
  await page.mouse.move(box.x + box.width / 2, (top + bottom) / 2);
  const step = (bottom - top) * 0.8 * (up ? -1 : 1);
  const end = Date.now() + timeout;
  while (!(await target.isVisible())) {
    if (Date.now() > end) throw new Error(`not found after scrolling for ${timeout} ms`);
    await page.mouse.wheel(0, step);
  }
}

export const SCENARIOS = {
  async login_form(page, open) {
    await open('login-form');
    await page.getByPlaceholder('Email').fill('tester@tester.army');
    await page.getByPlaceholder('Password').fill('benchmark123');
    await page.getByText('Log in', exact).click();
    await page.getByText('Logged in successfully', exact).waitFor();
    await page.getByText('Log out', exact).click();
    await page.getByText('Log in', exact).waitFor();
  },

  async shadow_dom_form(page, open) {
    await openShadows(page);
    await open('shadow-dom-form');
    const hint = await page.getByText(/^Access code hint:.*/).textContent();
    await page.getByPlaceholder('Access code').fill(hint.match(/SHADOW-\d+/)[0]);
    await page.getByText('Submit', exact).click();
    await page.getByText('Access granted', exact).waitFor();
  },

  async div_soup(page, open) {
    await open('div-soup');
    // The keypad comes after the code's slots, which show the digits typed: its tile is the last.
    for (const glyph of ['7', '2', '8', '‹', '9', '1', '→']) await page.getByText(glyph, exact).last().click();
    await page.getByText('Access granted', exact).waitFor();
  },

  async iframe_form(page, open) {
    await open('iframe-form');
    const code = await page.frameLocator('iframe[title="coupon-frame"]').getByText(/^[A-Z]+-\d+$/).textContent();
    const checkout = page.frameLocator('iframe[title="checkout-frame"]');
    await checkout.getByPlaceholder('Coupon code').fill(code);
    await checkout.getByText('Apply', exact).click();
    // The page takes the frames down and says so itself.
    await page.getByText('Coupon applied', exact).waitFor();
  },

  async async_states(page, open) {
    await open('async-states');
    await page.getByTestId('initial-loading').waitFor({ state: 'hidden', timeout: 20000 });
    await page.getByRole('button', { name: 'Verify', exact: true, disabled: true }).waitFor();
    await page.getByText('Send code', exact).click();
    const toast = await page.getByText(/^Your code is \d{4}$/).textContent();
    await page.getByPlaceholder('Verification code').fill(toast.match(/\d{4}/)[0]);
    await page.getByRole('button', { name: 'Verify', exact: true, disabled: false }).click();
    await page.getByText('Code verified successfully', exact).waitFor();
  },

  async debounced_search(page, open) {
    await open('debounced-search');
    await page.getByTestId('search-input').fill('trail mix');
    await page.getByText('Trail Mix 500 g', exact).click();
    await page.getByText('Added Trail Mix 500 g to the cart', exact).waitFor();
  },

  async infinite_scroll(page, open) {
    await open('infinite-scroll');
    const claim = page.getByText('Claim', exact);
    await scrollUntilThere(page, claim, { over: '[data-testid="feed"]' });
    await page.getByText('Golden Ticket', exact).waitFor();
    await claim.click();
    await page.getByText('Golden Ticket claimed', exact).waitFor();
  },

  async virtualized_table(page, open) {
    await open('virtualized-table');
    await page.getByText('Row 1', exact).waitFor();
    const claim = page.getByText('Claim', exact);
    await scrollUntilThere(page, claim, { over: '[data-testid="table-viewport"]', timeout: 140000 });
    await page.getByText(/^Row 4322.*/).waitFor();
    await claim.click();
    await page.getByText('Golden Row claimed', exact).waitFor();
  },

  async overlay_trap(page, open) {
    await open('overlay-trap');
    await page.getByText('Accept cookies', exact).click();
    await page.getByText(/close overlay/i).click();
    await page.getByText('Continue', exact).click();
    await page.getByText('You made it past the overlay', exact).waitFor();
  },

  async sticky_chrome(page, open) {
    await open('sticky-chrome');
    await page.getByText('Terms of Service', exact).waitFor();
    await page.getByTestId('success-message').waitFor({ state: 'hidden' });
    // The click scrolls the button into view, and clear of the sticky footer, itself.
    await page.getByText('Accept terms', exact).click();
    await page.getByText('Terms accepted', exact).waitFor();
  },

  async deferred_dialog(page, open) {
    const check = answerDialogs(page, [['Really submit the report?']]);
    await open('deferred-dialog');
    await page.getByText('Submit report', exact).dblclick();
    await page.getByText('Report submitted after deferred confirmation', exact).waitFor();
    check();
  },

  async new_tab_flow(page, open) {
    await open('new-tab-flow');
    const [tab] = await Promise.all([page.waitForEvent('popup'), page.getByText('Open verification tab', exact).click()]);
    const code = await tab.getByText(/^TAB-\d+$/).textContent();
    await tab.close();
    await page.getByPlaceholder('Verification code').fill(code.trim());
    await page.getByText('Verify', exact).click();
    await page.getByText('Verified successfully', exact).waitFor();
  },

  async stale_dom(page, open) {
    await open('stale-dom');
    await page.getByText('Progress: 0 / 3', exact).waitFor();
    for (const n of [1, 2]) {
      await page.getByText('Tap me', exact).click();
      await page.getByText(`Progress: ${n} / 3`, exact).waitFor();
    }
    await page.getByText('Tap me', exact).click();
    await page.getByText('Target clicked 3 times', exact).waitFor();
    await page.getByText('Tap me', exact).waitFor({ state: 'hidden' });
  },

  async otp_auto_advance(page, open) {
    await open('otp-auto-advance');
    await page.getByTestId('otp-input-0').click();
    await page.keyboard.type('4');
    await page.locator('[data-testid="otp-input-1"]:focus').waitFor();
    await page.keyboard.type('9302');
    await page.locator('[data-testid="otp-input-5"]:focus').waitFor();
    await page.keyboard.type('7');
    await page.getByText('Code verified', exact).waitFor();
  },

  async native_dialogs(page, open) {
    const check = answerDialogs(page, [
      ['Place this order?'],
      ['Enter your gift code', 'GIFT-7'],
      ['Order placed. Thank you!'],
    ]);
    await open('native-dialogs');
    await page.getByText('Place order', exact).click();
    await page.getByText('Order placed with code GIFT-7', exact).waitFor();
    check();
  },
};
