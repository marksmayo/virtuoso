import { test } from '@e2e-dev/web';
import { expect } from 'e2e';

test('lying_labels', async ({ browser, screen }) => {
  await browser.goto('/e/lying-labels');
  // Every accessible name lies: the button whose own text is Pay now, the field beside Amount.
  await browser.locator('xpath=//button[text()[normalize-space()="Pay now"]]').tap();
  await browser.locator('xpath=//span[normalize-space()="Amount"]/following-sibling::input').fill('42.50');
  await screen.getByText('Submit payment', { exact: true }).tap();
  await expect(screen.getByText('Payment sent', { exact: true })).toBeVisible();
});
