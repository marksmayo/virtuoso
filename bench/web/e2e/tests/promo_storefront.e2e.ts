import { test } from '@e2e-dev/web';
import { expect } from 'e2e';

test('promo_storefront', async ({ browser, screen }) => {
  await browser.goto('/e/promo-storefront');
  await screen.getByText('Accept', { exact: true }).tap();
  await expect(screen.getByTestId('cookie-banner')).toBeHidden();
  await screen.getByText('💬', { exact: true }).tap();
  await expect(screen.getByText('Hi! How can we help?', { exact: true })).toBeVisible();
  await screen.getByText('Close', { exact: true }).tap();
  await expect(screen.getByTestId('chat-panel')).toBeHidden();
  await expect(screen.getByText('Great bass', { exact: false }).first()).toBeVisible();
  await expect(screen.getByText('50% OFF', { exact: true })).toBeVisible();
  await screen.getByText('Buy now', { exact: true }).first().tap();
  await expect(screen.getByText('Order confirmed', { exact: true })).toBeVisible();
});
