// Selenium's ports of bench/web-flows, step for step with each flow: one function per flow,
// given the driver and `open(slug)`, which opens the scenario (the flow's launchApp). Each step
// waits explicitly for its element, visible, as a Selenium suite's page helpers do.
import { By, until } from 'selenium-webdriver';

const TIMEOUT = 17000;
const q = (t) => JSON.stringify(t);
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
const tap = async (driver, by) => (await visible(driver, by)).click();
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

  async native_dialogs(driver, open) {
    await open('native-dialogs');
    await tap(driver, text('Place order'));
    await answerDialog(driver, 'Place this order?');
    await answerDialog(driver, 'Enter your gift code', 'GIFT-7');
    await answerDialog(driver, 'Order placed. Thank you!');
    await visible(driver, text('Order placed with code GIFT-7'));
  },
};
