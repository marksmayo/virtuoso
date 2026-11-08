import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';
import { openScenario } from './open.ts';

test('Modal Flow', async ({ app, device, screen }) => {
  await openScenario({ app, screen }, 'Modal Flow');
  await screen.getByTestId('open-modal-button').tap();
  await screen.getByTestId('continue-button').tap();
  await screen.getByRole('button', /^confirm$/i).tap();
  await expect(screen.getByTestId('success-message')).toHaveText('Flow completed');
});
