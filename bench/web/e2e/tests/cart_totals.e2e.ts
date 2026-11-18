import { test } from '@e2e-dev/web';
import { expect } from 'e2e';

test('cart_totals', async ({ browser, screen }) => {
  await browser.goto('/e/cart-totals');
  await expect(screen.getByTestId('summary-total')).toHaveText('$54.00');
  await screen.getByTestId('increase-tee').tap();
  await screen.getByTestId('increase-tee').tap();
  await expect(screen.getByTestId('quantity-tee')).toHaveText('3');
  await expect(screen.getByTestId('line-total-tee')).toHaveText('$60.00');
  await expect(screen.getByTestId('summary-subtotal')).toHaveText('$49.00');
  await expect(screen.getByTestId('summary-total')).toHaveText('$54.00');
});
