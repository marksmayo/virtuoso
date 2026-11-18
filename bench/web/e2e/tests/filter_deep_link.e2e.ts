import { test } from '@e2e-dev/web';
import { expect } from 'e2e';

test('filter_deep_link', async ({ browser, screen }) => {
  await browser.goto('/e/filter-deep-link');
  await screen.getByPlaceholder('Filter products').fill('mug');
  await expect(screen.getByText('Titanium Mug', { exact: true })).toBeVisible();
});
