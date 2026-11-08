import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';
import { openScenario } from './open.ts';

test('WebView Accessibility', async ({ app, device, screen }) => {
  await openScenario({ app, screen }, 'WebView Accessibility');
  // The page renders after the native screen.
  await expect(screen.getByText('TA-BENCH-50')).toBeVisible({ timeout: 20_000 });
  await screen.getByTestId('coupon-input').fill('TA-BENCH-50');
  await device.dismissKeyboard();
  await screen.getByTestId('terms').tap();
  await screen.getByTestId('apply').tap();
  await expect(screen.getByTestId('success-message')).toHaveText('Coupon applied');
});
