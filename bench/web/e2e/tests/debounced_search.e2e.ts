import { test } from '@e2e-dev/web';
import { expect } from 'e2e';

test('debounced_search', async ({ browser, screen }) => {
  await browser.goto('/e/debounced-search');
  await screen.getByTestId('search-input').fill('trail mix');
  await screen.getByText('Trail Mix 500 g', { exact: true }).tap();
  await expect(screen.getByText('Added Trail Mix 500 g to the cart', { exact: true })).toBeVisible();
});
