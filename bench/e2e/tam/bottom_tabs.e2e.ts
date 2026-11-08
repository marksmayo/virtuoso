import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';
import { openScenario } from './open.ts';

test('Bottom Tabs', async ({ app, device, screen }) => {
  await openScenario({ app, screen }, 'Bottom Tabs');
  await expect(screen.getByTestId('home-tab-content')).toBeVisible();
  await screen.getByTestId('tab-actions').tap();
  await screen.getByTestId('complete-action-button').tap();
  await expect(screen.getByTestId('success-message')).toHaveText('Action completed');
});
