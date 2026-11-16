import { test } from '@e2e-dev/web';
import { expect } from 'e2e';

test('checkout_review', async ({ browser, screen }) => {
  await browser.goto('/e/checkout-review');
  await expect(screen.getByText('Thanks, {{firstName}}! Review your order below.', { exact: true })).toBeVisible();
  // Each value shares its line with a bold label ("Delivery:", "Ship to:"): found by its text.
  await expect(screen.getByText('Arrives by undefined', { exact: false })).toBeVisible();
  await expect(screen.getByText('221B Baker Street, London', { exact: false })).toBeVisible();
  await screen.getByText('Place order - $123.00', { exact: true }).tap();
  await expect(screen.getByText('Order placed successfully', { exact: true })).toBeVisible();
  await expect(screen.getByText(/Order #10482/)).toBeVisible();
});
