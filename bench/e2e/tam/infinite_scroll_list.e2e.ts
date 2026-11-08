import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';
import { openScenario } from './open.ts';

test('Infinite Scroll List', async ({ app, device, screen }) => {
  await openScenario({ app, screen }, 'Infinite Scroll List');
  const target = screen.getByTestId('item-137');
  await screen.scrollUntilVisible(target, { direction: 'down', timeout: 120_000 });
  await target.tap();
  await expect(screen.getByTestId('success-message')).toHaveText('Found item 137');
});
