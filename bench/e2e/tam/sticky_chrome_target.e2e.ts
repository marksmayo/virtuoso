import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';
import { openScenario } from './open.ts';

test('Sticky Chrome Target', async ({ app, device, screen }) => {
  await openScenario({ app, screen }, 'Sticky Chrome Target');
  await screen.getByTestId('continue-button').tap();
  await expect(screen.getByText('Accept the terms at the bottom first')).toBeVisible();
  const accept = screen.getByTestId('accept-terms');
  await screen.scrollUntilVisible(accept, { direction: 'down' });
  await accept.tap();
  await expect(screen.getByText('Terms accepted ✓')).toBeVisible();
  await screen.getByTestId('continue-button').tap();
  await expect(screen.getByTestId('success-message')).toHaveText('Terms accepted');
});
