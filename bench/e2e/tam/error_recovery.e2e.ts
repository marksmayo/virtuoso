import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';
import { openScenario } from './open.ts';

test('Error Recovery', async ({ app, device, screen }) => {
  await openScenario({ app, screen }, 'Error Recovery');
  await expect(screen.getByTestId('error-banner')).toBeVisible();
  await screen.getByTestId('retry-button').tap();
  await screen.getByTestId('delete-draft').tap();
  await screen.getByTestId('confirm-delete').tap();
  await expect(screen.getByTestId('success-message')).toHaveText('Draft deleted');
});
