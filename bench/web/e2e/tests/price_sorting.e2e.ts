import { test } from '@e2e-dev/web';
import { expect } from 'e2e';

test('price_sorting', async ({ browser, screen }) => {
  await browser.goto('/e/price-sorting');
  for (const name of ['Trail Backpack 24L', 'Steel Water Bottle', 'Merino Hiking Socks', 'Camp Lantern', 'Titanium Mug', '$120.00', '$49.00']) {
    await expect(screen.getByText(name, { exact: true })).toBeVisible();
  }
});
