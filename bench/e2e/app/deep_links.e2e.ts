import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';
import { guest } from './shared.ts';

test('deep links', async ({ app, device, screen }) => {
  const ctx = { app, device, screen };
  await guest(ctx);
  await device.openLink('speedrunbench://product/7');
  await expect(screen.getByTestId('product-name')).toHaveText('Trail Bottle');
  await device.openLink('speedrunbench://lab/gestures');
  await expect(screen.getByText('Last swipe: none (0)')).toBeVisible();
  await device.openLink('speedrunbench://cart');
  await expect(screen.getByText('Your cart is empty')).toBeVisible();
});
