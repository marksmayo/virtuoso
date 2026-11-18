import { test } from '@e2e-dev/web';
import { expect } from 'e2e';

test('product_reviews', async ({ browser, screen }) => {
  await browser.goto('/e/product-reviews');
  await expect(screen.getByText('Peak Trail Running Shoes', { exact: true })).toBeVisible();
  await expect(screen.getByText('$139.00 - Sizes 6 to 13 - Ships in 2 days', { exact: true })).toBeVisible();
  for (const part of ['4.7 out of 5', 'Marta', 'Deon', 'Priya', 'Grinds beans perfectly evenly']) {
    await expect(screen.getByText(part, { exact: false }).first()).toBeVisible();
  }
});
