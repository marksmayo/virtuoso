import { test } from '@e2e-dev/web';
import { expect } from 'e2e';

test('saved_addresses', async ({ browser, screen }) => {
  await browser.goto('/e/saved-addresses');
  await screen.getByTestId('delete-home').tap();
  await expect(screen.getByText('Address deleted', { exact: true })).toBeVisible();
  await expect(screen.getByTestId('address-home')).toBeVisible();
  await screen.getByTestId('delete-home').tap();
  await expect(screen.getByText('Home', { exact: true })).toBeVisible();
  await expect(screen.getByText('12 Rosewood Lane, Portland', { exact: false })).toBeVisible();
});
