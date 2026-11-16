// Selenium's ports of bench/web-flows, step for step with each flow: one function per flow,
// given the driver and `open(slug)`, which opens the scenario (the flow's launchApp). Each step
// waits explicitly for its element, visible, as a Selenium suite's page helpers do.
import { By, until } from 'selenium-webdriver';

const TIMEOUT = 17000;
const text = (t) => By.xpath(`//*[normalize-space(text())=${JSON.stringify(t)}]`);
async function visible(driver, by) {
  const el = await driver.wait(until.elementLocated(by), TIMEOUT);
  await driver.wait(until.elementIsVisible(el), TIMEOUT);
  return el;
}
const tap = async (driver, by) => (await visible(driver, by)).click();
async function type(driver, by, value) {
  const el = await visible(driver, by);
  await el.clear();
  await el.sendKeys(value);
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
};
