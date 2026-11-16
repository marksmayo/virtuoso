import { test } from '@e2e-dev/web';
import { expect } from 'e2e';

test('iframe_form', async ({ browser, screen }) => {
  await browser.goto('/e/iframe-form');
  const code = (await browser.frameLocator('iframe[title="coupon-frame"]').getByText(/^[A-Z]+-\d+$/).textContent()) ?? '';
  const checkout = browser.frameLocator('iframe[title="checkout-frame"]');
  await checkout.getByPlaceholder('Coupon code').fill(code.trim());
  await checkout.getByText('Apply', { exact: true }).tap();
  // The page takes the frames down and says so itself.
  await expect(screen.getByText('Coupon applied', { exact: true })).toBeVisible();
});
