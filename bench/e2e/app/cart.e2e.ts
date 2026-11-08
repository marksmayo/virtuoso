import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';
import { addToCart, guest } from './shared.ts';

test('cart', async ({ app, device, screen }) => {
  const ctx = { app, device, screen };
  await guest(ctx);
  await addToCart(screen, '5', 'S');
  await addToCart(screen, '9', 'XL');
  await addToCart(screen, '21', 'M');
  await expect(screen.getByText('Cart (3)')).toBeVisible();
  await screen.getByTestId('cart-button').tap();
  await expect(screen.getByText('Trail Cap')).toBeVisible();
  await expect(screen.getByText('Classic Sneaker')).toBeVisible();
  await screen.getByTestId('line-plus-0').tap();
  await screen.getByTestId('line-plus-0').tap();
  await expect(screen.getByTestId('line-qty-0')).toHaveText('× 3');
  await screen.getByTestId('line-minus-0').tap();
  await expect(screen.getByTestId('line-qty-0')).toHaveText('× 2');
  // Swipe to remove: the finger left.
  await screen.getByTestId('cart-line-1').swipe({ direction: 'right' });
  await screen.getByTestId('remove-1').tap();
  await expect(screen.getByText('Trail Notebook')).toBeHidden();
  await expect(screen.getByTestId('cart-line-2')).toBeHidden();
  await expect(screen.getByText(/^Total \$/)).toBeVisible();
});
