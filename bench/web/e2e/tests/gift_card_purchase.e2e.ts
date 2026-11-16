import { test } from '@e2e-dev/web';
import { expect } from 'e2e';

test('gift_card_purchase', async ({ browser, screen }) => {
  await browser.goto('/e/gift-card-purchase');
  await expect(screen.getByTestId('total-amount')).toHaveText('$0.00');
  await screen.getByTestId('amount-25').tap();
  await expect(screen.getByTestId('total-amount')).toHaveText('$25.00');
  await screen.getByPlaceholder('friend@example.com').fill('friend@example.com');
  await screen.getByTestId('buy-button').tap();
  await expect(screen.getByText('Gift card sent to friend@example.com', { exact: true })).toBeVisible();
  await expect(screen.getByText('A $25.00 gift card is on its way.', { exact: true })).toBeVisible();
});
