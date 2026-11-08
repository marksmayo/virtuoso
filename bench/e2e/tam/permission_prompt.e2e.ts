import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';
import { openScenario } from './open.ts';

test('Permission Prompt', async ({ app, device, screen }) => {
  // The prompt shows only for a permission not yet decided.
  await device.setPermission('microphone', 'reset');
  await openScenario({ app, screen }, 'Permission Prompt');
  await screen.getByTestId('request-permission').tap();
  await device.alert('accept');
  await expect(screen.getByTestId('success-message')).toHaveText('Microphone enabled');
});
