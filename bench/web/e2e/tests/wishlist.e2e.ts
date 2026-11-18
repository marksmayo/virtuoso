import { test } from '@e2e-dev/web';
import { expect } from 'e2e';

test('wishlist', async ({ browser, screen }) => {
  await browser.goto('/e/wishlist');
  await screen.getByTestId('save-planner').tap();
  await screen.getByTestId('save-notebook').tap();
  await screen.getByText('Wishlist (2)', { exact: true }).tap();
  await expect(screen.getByText('No saved items yet.', { exact: true })).toBeVisible();
  await expect(screen.getByText('Weekly Desk Planner', { exact: false })).toBeHidden();
  await screen.getByText('Shop', { exact: true }).tap();
  await expect(screen.getByTestId('save-planner')).toHaveText('♥ Saved');
  await expect(screen.getByTestId('save-pen')).toHaveText('♡ Save');
});
