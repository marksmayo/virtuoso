import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';
import { openScenario } from './open.ts';

test('Async States', async ({ app, device, screen }) => {
  await openScenario({ app, screen }, 'Async States');
  await expect(screen.getByText('Pull down to refresh and reveal your reward')).toBeVisible();
  const list = screen.getByTestId('reward-scroll');
  const claim = screen.getByTestId('claim-button');
  await expect
    .poll(async () => {
      if (await claim.isVisible()) return true;
      await list.swipe({ direction: 'up', momentum: 'fast' });
      return false;
    }, { timeout: 20_000 })
    .toBe(true);
  await claim.tap();
  await expect(screen.getByTestId('success-message')).toHaveText('Reward claimed');
});
