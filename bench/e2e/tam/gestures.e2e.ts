import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';
import { openScenario } from './open.ts';

test('Gestures', async ({ app, device, screen }) => {
  await openScenario({ app, screen }, 'Gestures');
  await screen.getByTestId('long-press-target').longPress({ duration: 1200 });
  await expect(screen.getByText('Step 2 of 3')).toBeVisible();
  await screen.getByTestId('swipe-target').swipe({ direction: 'right', momentum: 'fast' });
  await expect(screen.getByText('Step 3 of 3')).toBeVisible();
  await screen.getByTestId('double-tap-target').doubleTap();
  await expect(screen.getByTestId('success-message')).toHaveText('All gestures completed');
});
