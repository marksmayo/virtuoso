// Selenium's ports of bench/web-flows, step for step with each flow: one function per flow,
// given the driver and `open(slug)`, which opens the scenario (the flow's launchApp). Each step
// waits explicitly for its element, visible, as a Selenium suite's page helpers do.
import { By, until } from 'selenium-webdriver';

const TIMEOUT = 17000;
// An XPath string literal of t: XPath 1 has no escapes, so a text with both quotes is a concat().
const q = (t) => (!t.includes('"') ? `"${t}"` : !t.includes("'") ? `'${t}'` : `concat(${t.split('"').map((p) => `"${p}"`).join(`, '"', `)})`);
// The innermost element whose whole text is t (whitespace normalized), however many text nodes
// it is made of.
const text = (t) => By.xpath(`//*[normalize-space(.)=${q(t)}][not(*[normalize-space(.)=${q(t)}])]`);
// The innermost element whose text contains t.
const containing = (t) => By.xpath(`//*[contains(normalize-space(.),${q(t)})][not(*[contains(normalize-space(.),${q(t)})])]`);

async function visible(driver, by, timeout = TIMEOUT) {
  const el = await driver.wait(until.elementLocated(by), timeout);
  await driver.wait(until.elementIsVisible(el), timeout);
  return el;
}
// A click, found again when the page rebuilt the element between finding and clicking (the
// stale-element retry every Selenium suite carries).
async function tap(driver, by) {
  for (let attempt = 0; ; attempt++) {
    try {
      return await (await visible(driver, by)).click();
    } catch (e) {
      if (e.name !== 'StaleElementReferenceError' || attempt >= 5) throw e;
    }
  }
}
// Once the element with data-testid `id` reads exactly `value`.
const testIdReads = (driver, id, value) =>
  driver.wait(async () => (await (await visible(driver, By.css(`[data-testid="${id}"]`))).getText()).trim() === value, TIMEOUT, `${id} is not ${value}`);
// Once no element `by` finds is on screen.
const gone = (driver, by) =>
  driver.wait(async () => {
    for (const el of await driver.findElements(by)) if (await el.isDisplayed().catch(() => false)) return false;
    return true;
  }, TIMEOUT, 'still visible');
// The element with the focus, once it is the one with data-testid `id`.
const focused = (driver, id) => driver.wait(async () => (await driver.switchTo().activeElement().getAttribute('data-testid')) === id, TIMEOUT, `${id} not focused`);
async function type(driver, by, value) {
  const el = await visible(driver, by);
  await el.clear();
  await el.sendKeys(value);
}
// The text of the innermost element containing `fragment`, once it matches `re`.
async function textMatching(driver, fragment, re) {
  return driver.wait(async () => {
    for (const el of await driver.findElements(containing(fragment))) {
      const t = (await el.getText()).trim();
      if (re.test(t)) return t;
    }
    return false;
  }, TIMEOUT);
}
async function frame(driver, title) {
  await driver.switchTo().defaultContent();
  await driver.switchTo().frame(await visible(driver, By.css(`iframe[title="${title}"]`)));
}

// Closed shadow roots are out of every locator's reach; the usual answer is to have the page
// open them, before it loads, here through Chrome's DevTools (virtuoso reads them through its own
// hook in the page the same way).
const openShadows = (driver) =>
  driver.sendDevToolsCommand('Page.addScriptToEvaluateOnNewDocument', {
    source: `{ const attach = Element.prototype.attachShadow;
      Element.prototype.attachShadow = function (init) { return attach.call(this, { ...init, mode: 'open' }); }; }`,
  });

// The browser's own dialog, once it is open: its message checked, answered, accepted.
async function answerDialog(driver, message, answer) {
  const alert = await driver.wait(until.alertIsPresent(), TIMEOUT);
  const got = await alert.getText();
  if (got !== message) throw new Error(`dialog "${got}", expected "${message}"`);
  if (answer !== undefined) await alert.sendKeys(answer);
  await alert.accept();
}

// The mouse wheel over `over` (a CSS selector), a screenful at a time, until `by` finds an
// element on screen: what a user does with a feed or table that renders rows as they come.
async function scrollUntilThere(driver, by, { over, timeout = TIMEOUT } = {}) {
  const box = await visible(driver, By.css(over));
  const { height } = await box.getRect();
  const step = Math.round(Math.min(height, 800) * 0.8);
  const end = Date.now() + timeout;
  for (;;) {
    for (const el of await driver.findElements(by)) if (await el.isDisplayed().catch(() => false)) return el;
    if (Date.now() > end) throw new Error(`not found after scrolling for ${timeout} ms`);
    await driver.actions().scroll(0, 0, 0, step, box).perform();
  }
}

export const SCENARIOS = {
  async login_form(driver, open) {
    await open('login-form');
    await type(driver, By.css('[placeholder="Email"]'), 'tester@tester.army');
    await type(driver, By.css('[placeholder="Password"]'), 'benchmark123');
    await tap(driver, text('Log in'));
    await visible(driver, text('Logged in successfully'));
    await tap(driver, text('Log out'));
    await visible(driver, text('Log in'));
  },

  async shadow_dom_form(driver, open) {
    await openShadows(driver);
    await open('shadow-dom-form');
    // The form is two shadow roots down; inside one, Selenium finds by CSS only.
    const form = await driver.wait(async () => {
      for (const host of await driver.findElements(By.css('main div'))) {
        const outer = await host.getShadowRoot().catch(() => null);
        if (!outer) continue;
        for (const inner of await outer.findElements(By.css('div'))) {
          const root = await inner.getShadowRoot().catch(() => null);
          if (root && (await root.findElements(By.css('input'))).length) return root;
        }
      }
      return false;
    }, TIMEOUT);
    const inForm = async (css, wanted) => {
      return driver.wait(async () => {
        for (const el of await form.findElements(By.css(css))) {
          const t = (await el.getText()).trim();
          if ((await el.isDisplayed()) && wanted.test(t)) return el;
        }
        return false;
      }, TIMEOUT);
    };
    const hint = await (await inForm('p', /^Access code hint:.*/)).getText();
    const input = await form.findElement(By.css('input[placeholder="Access code"]'));
    await input.clear();
    await input.sendKeys(hint.match(/SHADOW-\d+/)[0]);
    await (await inForm('button', /^Submit$/)).click();
    // The page takes the form down and says so itself.
    await visible(driver, text('Access granted'));
  },

  async div_soup(driver, open) {
    await open('div-soup');
    // The keypad comes after the code's slots, which show the digits typed: its tile is the last.
    for (const glyph of ['7', '2', '8', '‹', '9', '1', '→']) await tap(driver, By.xpath(`(//*[normalize-space(.)=${q(glyph)}][not(*)])[last()]`));
    await visible(driver, text('Access granted'));
  },

  async iframe_form(driver, open) {
    await open('iframe-form');
    await frame(driver, 'coupon-frame');
    const code = await textMatching(driver, '-', /^[A-Z]+-\d+$/);
    await frame(driver, 'checkout-frame');
    await type(driver, By.css('[placeholder="Coupon code"]'), code);
    await tap(driver, text('Apply'));
    // The page takes the frames down and says so itself.
    await driver.switchTo().defaultContent();
    await visible(driver, text('Coupon applied'));
  },

  async async_states(driver, open) {
    await open('async-states');
    await driver.wait(async () => (await driver.findElements(By.css('[data-testid="initial-loading"]'))).length === 0, 20000);
    const verify = await visible(driver, text('Verify'));
    if (await verify.isEnabled()) throw new Error('Verify is enabled with no code typed');
    await tap(driver, text('Send code'));
    const toast = await textMatching(driver, 'Your code is', /^Your code is \d{4}$/);
    await type(driver, By.css('[placeholder="Verification code"]'), toast.match(/\d{4}/)[0]);
    await driver.wait(until.elementIsEnabled(verify), TIMEOUT);
    await verify.click();
    await visible(driver, text('Code verified successfully'));
  },

  async debounced_search(driver, open) {
    await open('debounced-search');
    await type(driver, By.css('[data-testid="search-input"]'), 'trail mix');
    await tap(driver, text('Trail Mix 500 g'));
    await visible(driver, text('Added Trail Mix 500 g to the cart'));
  },

  async infinite_scroll(driver, open) {
    await open('infinite-scroll');
    const claim = await scrollUntilThere(driver, text('Claim'), { over: '[data-testid="feed"]' });
    await visible(driver, text('Golden Ticket'));
    await claim.click();
    await visible(driver, text('Golden Ticket claimed'));
  },

  async virtualized_table(driver, open) {
    await open('virtualized-table');
    await visible(driver, text('Row 1'));
    await scrollUntilThere(driver, text('Claim'), { over: '[data-testid="table-viewport"]', timeout: 140000 });
    await textMatching(driver, 'Row 4322', /^Row 4322.*/);
    await tap(driver, text('Claim'));
    await visible(driver, text('Golden Row claimed'));
  },

  async overlay_trap(driver, open) {
    await open('overlay-trap');
    await tap(driver, text('Accept cookies'));
    await tap(driver, containing('close overlay'));
    await tap(driver, text('Continue'));
    await visible(driver, text('You made it past the overlay'));
  },

  async sticky_chrome(driver, open) {
    await open('sticky-chrome');
    await visible(driver, text('Terms of Service'));
    await driver.wait(async () => {
      const found = await driver.findElements(By.css('[data-testid="success-message"]'));
      return found.length === 0 || !(await found[0].isDisplayed());
    }, TIMEOUT);
    // Selenium scrolls the button just into view, where the sticky footer covers it: it is
    // brought to the middle of the screen first.
    const accept = await driver.wait(until.elementLocated(text('Accept terms')), TIMEOUT);
    await driver.executeScript('arguments[0].scrollIntoView({ block: "center" })', accept);
    await accept.click();
    await visible(driver, text('Terms accepted'));
  },

  async deferred_dialog(driver, open) {
    await open('deferred-dialog');
    await driver.actions().doubleClick(await visible(driver, text('Submit report'))).perform();
    await answerDialog(driver, 'Really submit the report?');
    await visible(driver, text('Report submitted after deferred confirmation'));
    const again = await driver.switchTo().alert().then(() => true, () => false);
    if (again) throw new Error('a second confirm opened');
  },

  async new_tab_flow(driver, open) {
    await open('new-tab-flow');
    const home = await driver.getWindowHandle();
    await tap(driver, text('Open verification tab'));
    const tab = await driver.wait(async () => (await driver.getAllWindowHandles()).find((h) => h !== home), TIMEOUT);
    await driver.switchTo().window(tab);
    const code = await textMatching(driver, 'TAB-', /^TAB-\d+$/);
    await driver.close();
    await driver.switchTo().window(home);
    await type(driver, By.css('[placeholder="Verification code"]'), code);
    await tap(driver, text('Verify'));
    await visible(driver, text('Verified successfully'));
  },

  async stale_dom(driver, open) {
    await open('stale-dom');
    await visible(driver, text('Progress: 0 / 3'));
    for (const n of [1, 2]) {
      await tap(driver, text('Tap me'));
      await visible(driver, text(`Progress: ${n} / 3`));
    }
    await tap(driver, text('Tap me'));
    await visible(driver, text('Target clicked 3 times'));
    if ((await driver.findElements(text('Tap me'))).length) throw new Error('Tap me is still there');
  },

  async otp_auto_advance(driver, open) {
    await open('otp-auto-advance');
    await tap(driver, By.css('[data-testid="otp-input-0"]'));
    await driver.actions().sendKeys('4').perform();
    await focused(driver, 'otp-input-1');
    await driver.actions().sendKeys('9302').perform();
    await focused(driver, 'otp-input-5');
    await driver.actions().sendKeys('7').perform();
    await visible(driver, text('Code verified'));
  },

  async filter_deep_link(driver, open) {
    await open('filter-deep-link');
    await type(driver, By.css('[placeholder="Filter products"]'), 'mug');
    await visible(driver, text('Titanium Mug'));
  },

  async date_picker(driver, open) {
    await open('date-picker');
    await tap(driver, By.css('[placeholder="Select date"]'));
    await visible(driver, text('June 2026'));
    for (let i = 0; i < 3; i++) await tap(driver, text('›'));
    await visible(driver, text('September 2026'));
    await tap(driver, text('17'));
    await gone(driver, text('September 2026'));
    await tap(driver, text('Confirm booking'));
    await visible(driver, text('Appointment booked for September 17, 2026'));
  },

  async playbook_cleanup(driver, open) {
    await open('playbook-cleanup');
    await visible(driver, text('Launch Approval Draft'));
    await tap(driver, By.xpath('(//button[normalize-space(.)="⋯"])[2]'));
    await tap(driver, text('Delete'));
    await visible(driver, text('Are you sure you want to remove "Launch Approval Draft"?'));
    await tap(driver, text('Remove'));
    await gone(driver, text('Launch Approval Draft'));
    await type(driver, By.css('[placeholder="Search"]'), 'Launch Approval Draft');
    await visible(driver, text('No playbooks match "Launch Approval Draft".'));
  },

  async price_sorting(driver, open) {
    await open('price-sorting');
    for (const name of ['Trail Backpack 24L', 'Steel Water Bottle', 'Merino Hiking Socks', 'Camp Lantern', 'Titanium Mug', '$120.00', '$49.00']) {
      await visible(driver, text(name));
    }
  },

  async newsletter_signup(driver, open) {
    await open('newsletter-signup');
    const email = By.css('[placeholder="you@example.com"]');
    await type(driver, email, 'maria.novak@example.com');
    await tap(driver, text('Subscribe'));
    await gone(driver, By.css('[data-testid="success-message"]'));
    await gone(driver, By.css('[data-testid="error-message"]'));
    const value = await (await visible(driver, email)).getAttribute('value');
    if (value !== 'maria.novak@example.com') throw new Error('the email field lost its value');
  },

  async aria_hidden_flow(driver, open) {
    await open('aria-hidden-flow');
    await visible(driver, text('Choose the Pro plan'));
    await tap(driver, text('Pro'));
    await visible(driver, text('Pro plan - $29/mo'));
    await tap(driver, text('Confirm Pro plan'));
    await visible(driver, text('Plan activated'));
  },

  async lying_labels(driver, open) {
    await open('lying-labels');
    // Every accessible name lies: the button whose own text is Pay now, the field beside Amount.
    await tap(driver, By.xpath('//button[text()[normalize-space()="Pay now"]]'));
    await type(driver, By.xpath('//span[normalize-space()="Amount"]/following-sibling::input'), '42.50');
    await tap(driver, text('Submit payment'));
    await visible(driver, text('Payment sent'));
  },

  async context_menu_trap(driver, open) {
    await open('context-menu-trap');
    await tap(driver, By.xpath('(//button[normalize-space(.)="⋯"])[2]'));
    await tap(driver, text('Rename'));
    await type(driver, By.css('[aria-label="New file name"]'), 'summary.pdf');
    await tap(driver, text('Save'));
    await visible(driver, text('File renamed to summary.pdf'));
    await visible(driver, text('quarterly-report.pdf'));
    await gone(driver, text('report.pdf'));
  },

  async cart_totals(driver, open) {
    await open('cart-totals');
    const reads = (id, value) =>
      driver.wait(async () => (await (await visible(driver, By.css(`[data-testid="${id}"]`))).getText()).trim() === value, TIMEOUT, `${id} is not ${value}`);
    await reads('summary-total', '$54.00');
    await tap(driver, By.css('[data-testid="increase-tee"]'));
    await tap(driver, By.css('[data-testid="increase-tee"]'));
    await reads('quantity-tee', '3');
    await reads('line-total-tee', '$60.00');
    await reads('summary-subtotal', '$49.00');
    await reads('summary-total', '$54.00');
  },

  async checkout_review(driver, open) {
    await open('checkout-review');
    await visible(driver, text('Thanks, {{firstName}}! Review your order below.'));
    // Each value shares its line with a bold label ("Delivery:", "Ship to:"): found by its text.
    await visible(driver, containing('Arrives by undefined'));
    await visible(driver, containing('221B Baker Street, London'));
    await tap(driver, text('Place order - $123.00'));
    await visible(driver, text('Order placed successfully'));
    await textMatching(driver, 'Order #10482', /Order #10482/);
  },

  async saved_addresses(driver, open) {
    await open('saved-addresses');
    await tap(driver, By.css('[data-testid="delete-home"]'));
    await visible(driver, text('Address deleted'));
    await visible(driver, By.css('[data-testid="address-home"]'));
    await tap(driver, By.css('[data-testid="delete-home"]'));
    await visible(driver, text('Home'));
    await visible(driver, containing('12 Rosewood Lane, Portland'));
  },

  async order_history(driver, open) {
    await open('order-history');
    await tap(driver, text('Orders'));
    await visible(driver, text('Loading your orders...'));
    await tap(driver, text('Settings'));
    await visible(driver, By.css('[data-testid="settings-panel"]'));
    await tap(driver, text('Orders'));
    await visible(driver, text('Loading your orders...'));
  },

  async product_reviews(driver, open) {
    await open('product-reviews');
    await visible(driver, text('Peak Trail Running Shoes'));
    await visible(driver, text('$139.00 - Sizes 6 to 13 - Ships in 2 days'));
    for (const part of ['4.7 out of 5', 'Marta', 'Deon', 'Priya', 'Grinds beans perfectly evenly']) await visible(driver, containing(part));
  },

  async wishlist(driver, open) {
    await open('wishlist');
    await tap(driver, By.css('[data-testid="save-planner"]'));
    await tap(driver, By.css('[data-testid="save-notebook"]'));
    await tap(driver, text('Wishlist (2)'));
    await visible(driver, text('No saved items yet.'));
    await gone(driver, containing('Weekly Desk Planner'));
    await tap(driver, text('Shop'));
    await testIdReads(driver, 'save-planner', '♥ Saved');
    await testIdReads(driver, 'save-pen', '♡ Save');
  },

  async promo_storefront(driver, open) {
    await open('promo-storefront');
    await tap(driver, text('Accept'));
    await gone(driver, By.css('[data-testid="cookie-banner"]'));
    await tap(driver, text('💬'));
    await visible(driver, text('Hi! How can we help?'));
    await tap(driver, text('Close'));
    await gone(driver, By.css('[data-testid="chat-panel"]'));
    await visible(driver, containing('Great bass'));
    await visible(driver, text('50% OFF'));
    await tap(driver, text('Buy now'));
    await visible(driver, text('Order confirmed'));
  },

  async gift_card_purchase(driver, open) {
    await open('gift-card-purchase');
    await testIdReads(driver, 'total-amount', '$0.00');
    await tap(driver, By.css('[data-testid="amount-25"]'));
    await testIdReads(driver, 'total-amount', '$25.00');
    await type(driver, By.css('[placeholder="friend@example.com"]'), 'friend@example.com');
    await tap(driver, By.css('[data-testid="buy-button"]'));
    await visible(driver, text('Gift card sent to friend@example.com'));
    await visible(driver, text('A $25.00 gift card is on its way.'));
  },

  async kanban_board(driver, open) {
    await open('kanban-board');
    // A pointer-event board: the mouse pressed on the card, moved to the column, released.
    async function drag(card, column) {
      const from = await visible(driver, By.css(`[data-testid="${card}"]`));
      const to = await visible(driver, By.css(`[data-testid="${column}"]`));
      await driver.actions().move({ origin: from }).press().move({ origin: from, x: 20, y: 0 }).move({ origin: to, duration: 200 }).release().perform();
    }
    await drag('card-fix-payment-bug', 'column-done');
    await visible(driver, By.css('[data-testid="column-done"] [data-testid="card-fix-payment-bug"]'));
    await drag('card-refactor-onboarding', 'column-in-progress');
    await visible(driver, text('Update pricing page'));
    await tap(driver, text('Submit board'));
    await visible(driver, text('Board matches the goal'));
  },

  async control_inventory(driver, open) {
    await open('control-inventory');
    await tap(driver, text('Agree to terms'));
    await tap(driver, text('Medium'));
    await visible(driver, text('terms agreed, size Medium'));
    await tap(driver, text('Agree to terms'));
    await visible(driver, text('terms not agreed, size Medium'));
    await tap(driver, text('Express'));
    await visible(driver, text('Express delivery selected'));
    await tap(driver, text('Hold me'));
    await visible(driver, text('tapped'));
    // A long press: the mouse held on the button past its 500 ms.
    await driver.actions().move({ origin: await visible(driver, text('Hold me')) }).press().pause(800).release().perform();
    await visible(driver, text('long-pressed'));
    await tap(driver, text('Double-click me'));
    await visible(driver, text('clicked once'));
    await driver.actions().doubleClick(await visible(driver, text('Double-click me'))).perform();
    await visible(driver, text('double-clicked'));
    await tap(driver, text('Open details'));
    await visible(driver, text('on details'));
    await driver.navigate().back();
    await visible(driver, text('back on the inventory'));
  },

  async onboarding_wizard(driver, open) {
    await open('onboarding-wizard');
    // The fields have no label, id or placeholder of their own: each by the div that names it,
    // just before it; a toggle is the div after its label, in its row.
    const after = (label, tag = 'input') => By.xpath(`//div[normalize-space()="${label}"]/following-sibling::${tag}[1]`);
    const fill = (label, value) => type(driver, after(label), value);
    const see = (t) => visible(driver, text(t));
    const press = (t) => tap(driver, text(t));
    await see('Step 1 of 5');
    await fill('Full name', 'Maria Novak');
    await fill('Email', 'maria.novak@example.com');
    await fill('Phone', '+48 601 222 333');
    await press('Next');
    await see('Step 2 of 5');
    await fill('Street', 'Krucza 12');
    await fill('City', 'Warsaw');
    await fill('Postal code', '00-585');
    await press('Choose');
    await press('Poland');
    await press('Next');
    await see('Step 3 of 5');
    await see('June 2026');
    for (let i = 0; i < 3; i++) await press('«');
    for (let i = 0; i < 6; i++) await press('‹');
    for (let i = 0; i < 3; i++) await press('‹ month');
    await see('March 1990');
    await press('14');
    await see('Selected: March 14, 1990');
    await press('Choose');
    await press('Product designer');
    for (let i = 0; i < 7; i++) await press('+');
    await press('Next');
    await see('Step 4 of 5');
    for (const label of ['Product updates', 'SMS alerts', 'Weekly digest']) await tap(driver, after(label, 'div'));
    await press('Studio');
    await press('Next');
    await see('Step 5 of 5');
    await see('Maria Novak · maria.novak@example.com · +48 601 222 333');
    await see('Krucza 12, 00-585 Warsaw, Poland');
    await see('Born March 14, 1990 · Product designer · 7 years');
    await see('Plan: Studio');
    await fill('Support PIN from the brief', '8412');
    await tap(driver, after('I confirm the details are correct', 'div'));
    await press('Finish');
    await see('Onboarding complete');
    await see('Welcome aboard, Maria Novak');
  },

  async native_dialogs(driver, open) {
    await open('native-dialogs');
    await tap(driver, text('Place order'));
    await answerDialog(driver, 'Place this order?');
    await answerDialog(driver, 'Enter your gift code', 'GIFT-7');
    await answerDialog(driver, 'Order placed. Thank you!');
    await visible(driver, text('Order placed with code GIFT-7'));
  },
};
