// Puppeteer's ports of bench/web-flows, step for step with each flow: one function per flow,
// given the page and `open(slug)`, which opens the scenario (the flow's launchApp). A check waits
// for the element the flow asserts, visible.
const text = (t) => `::-p-xpath(//*[normalize-space(text())=${JSON.stringify(t)}])`;
const see = (page, t) => page.waitForSelector(text(t), { visible: true });

export const SCENARIOS = {
  async login_form(page, open) {
    await open('login-form');
    await page.locator('[placeholder="Email"]').fill('tester@tester.army');
    await page.locator('[placeholder="Password"]').fill('benchmark123');
    await page.locator(text('Log in')).click();
    await see(page, 'Logged in successfully');
    await page.locator(text('Log out')).click();
    await see(page, 'Log in');
  },
};
