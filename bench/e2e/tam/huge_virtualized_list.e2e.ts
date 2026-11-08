import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';
import { openScenario } from './open.ts';

test('Huge Virtualized List', async ({ app, device, screen }) => {
  await openScenario({ app, screen }, 'Huge Virtualized List');
  await expect(screen.getByText('Row 0001')).toBeVisible();
  const target = screen.getByTestId('row-512');
  await screen.scrollUntilVisible(target, { direction: 'down', timeout: 240_000 });
  await target.tap();
  await expect(screen.getByTestId('success-message')).toHaveText('Found Row 0512');
});
