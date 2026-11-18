import { test } from '@e2e-dev/web';
import { expect } from 'e2e';

test('order_history', async ({ browser, screen }) => {
  await browser.goto('/e/order-history');
  await screen.getByText('Orders', { exact: true }).tap();
  await expect(screen.getByText('Loading your orders...', { exact: true })).toBeVisible();
  await screen.getByText('Settings', { exact: true }).tap();
  await expect(screen.getByTestId('settings-panel')).toBeVisible();
  await screen.getByText('Orders', { exact: true }).tap();
  await expect(screen.getByText('Loading your orders...', { exact: true })).toBeVisible();
});
