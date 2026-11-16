// Playwright's ports of bench/web-flows, step for step with each flow: one function per flow,
// given the page and `open(slug)`, which opens the scenario (the flow's launchApp). A check is a
// locator waited for in the state the flow asserts.
export const SCENARIOS = {
  async login_form(page, open) {
    await open('login-form');
    await page.getByPlaceholder('Email').fill('tester@tester.army');
    await page.getByPlaceholder('Password').fill('benchmark123');
    await page.getByText('Log in', { exact: true }).click();
    await page.getByText('Logged in successfully', { exact: true }).waitFor();
    await page.getByText('Log out', { exact: true }).click();
    await page.getByText('Log in', { exact: true }).waitFor();
  },
};
