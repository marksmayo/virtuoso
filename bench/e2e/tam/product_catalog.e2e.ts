import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';
import { openScenario } from './open.ts';

test('Product Catalog', async ({ app, device, screen }) => {
  await openScenario({ app, screen }, 'Product Catalog');
  await screen.getByTestId('product-trail-mix-500').tap();
  await screen.getByTestId('quantity-increase').tap();
  await expect(screen.getByTestId('quantity-value')).toHaveText('2');
  await screen.getByTestId('add-to-cart').tap();
  await screen.getByTestId('open-cart').tap();
  await expect(screen.getByTestId('success-message')).toHaveText('Order ready: 2 × Trail Mix 500 g');
});
