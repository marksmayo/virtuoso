import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';
import { openScenario } from './open.ts';

test('Choice Controls', async ({ app, device, screen }) => {
  await openScenario({ app, screen }, 'Choice Controls');
  await expect(screen.getByTestId('place-order')).toBeDisabled();
  await screen.getByTestId('size-medium').tap();
  await screen.getByTestId('topping-cheese').tap();
  await screen.getByTestId('topping-olives').tap();
  await screen.getByTestId('rush-delivery').tap();
  await screen.getByTestId('place-order').tap();
  await expect(screen.getByTestId('success-message')).toHaveText('Order placed: Medium with Cheese, Olives (rush)');
});
