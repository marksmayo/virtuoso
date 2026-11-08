import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';
import { addToCart, checkout, dismissOrderPopups, signIn } from './shared.ts';

const pick = (a) => a[Math.floor(Math.random() * a.length)];
const NAMES = ['Ava Martin', 'Noah Tremblay', 'Mia Roy', 'Liam Gagnon', 'Zoe Côté'];
const CITIES = ['Quebec City', 'Laval', 'Ottawa', 'Toronto', 'Vancouver'];

test('shop checkout', async ({ app, device, screen }) => {
  const ctx = { app, device, screen };
  await signIn(ctx, '5550101234', '123456', false);
  await screen.getByTestId('tab-shop').tap();
  await screen.getByTestId('search-input').fill('sneaker');
  await screen.getByTestId('search-input').press('Enter');
  await expect(screen.getByText('Searched for "sneaker"')).toBeVisible();
  await expect(screen.getByText('6 products')).toBeVisible({ timeout: 5_000 });
  await screen.getByText('City Sneaker').tap();
  await expect(screen.getByTestId('product-name')).toHaveText('City Sneaker');
  await expect(screen.getByTestId('add-to-cart')).toBeDisabled();
  await screen.getByTestId('size-M').tap();
  await expect(screen.getByTestId('size-M')).toBeSelected();
  await screen.getByTestId('color-Blue').tap();
  await expect(screen.getByTestId('color-Blue')).toBeChecked();
  for (let i = 0; i < 2; i++) {
    await screen.getByTestId('qty-plus').tap();
    await new Promise((r) => setTimeout(r, 300));
  }
  await expect(screen.getByText('Qty 3')).toBeVisible();
  const gift = screen.getByTestId('gift-wrap');
  await screen.scrollUntilVisible(gift, { direction: 'down' });
  await gift.tap();
  await expect(gift).toBeChecked();
  await screen.getByTestId('add-to-cart').tap();
  await expect(screen.getByText('Added to cart')).toBeVisible();
  await expect(screen.getByText('Cart (3)')).toBeVisible();
  await screen.getByTestId('go-to-cart').tap();
  await expect(screen.getByText('Size M · Blue · Gift wrap')).toBeVisible();
  await screen.getByTestId('promo-input').fill('SAVE10');
  // The promo service fails now and then: again, up to twice more.
  for (let attempt = 0; ; attempt++) {
    await screen.getByTestId('apply-promo').tap();
    const ok = await screen.getByTestId('promo-applied').waitFor({ state: 'visible', timeout: 2_000 }).then(() => true, () => false);
    if (ok) break;
    if (attempt === 2) throw new Error('the promo never applied');
  }
  await device.dismissKeyboard();
  await screen.getByTestId('checkout').tap();
  await screen.getByTestId('name-input').fill(pick(NAMES));
  await screen.getByTestId('email-input').fill('alex@');
  await screen.getByTestId('street-input').tap();
  await expect(screen.getByText('Enter a valid email')).toBeVisible();
  await screen.getByTestId('street-input').pressSequentially('1 Main Street');
  await screen.getByTestId('email-input').tap();
  await screen.getByTestId('email-input').pressSequentially('example.com');
  await screen.getByTestId('city-input').fill(pick(CITIES));
  await screen.getByTestId('postal-input').fill('H2X 1Y4');
  await device.dismissKeyboard();
  await screen.getByTestId('ship-express').tap();
  await screen.getByTestId('place-order').tap();
  await expect(screen.getByTestId('order-confirmed')).toBeVisible({ timeout: 10_000 });
  await expect(screen.getByTestId('order-number')).toHaveText('Order #10001');
  await screen.getByTestId('done').tap();
  await dismissOrderPopups(screen);
  await addToCart(screen, '31', 'L');
  await screen.getByTestId('cart-button').tap();
  await checkout(ctx, '10002');
  await screen.getByTestId('done').tap();
  await dismissOrderPopups(screen);
  await screen.getByTestId('tab-profile').tap();
  await expect(screen.getByTestId('profile-orders')).toHaveText('2');
});
